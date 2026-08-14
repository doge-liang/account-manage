# -*- coding: utf-8 -*-
"""
账号管家 (AccountHub) — 本地账号管理系统 后端服务
===================================================
纯 Python 标准库实现，无需 pip install。

启动:
    python server.py                # 默认 127.0.0.1:8756，自动打开浏览器
    python server.py --port 9000    # 自定义端口
    python server.py --no-browser   # 不自动开浏览器

数据:
    data/accounts.json              # 账号/关系/配置（原子写入，Syncthing 友好）
    data/backups/                   # 数据自动备份（保留最近 N 份）
    密钥库文件默认位置: <项目根>/keepassxc/vaults/ai-keys.kdbx（可在设置中修改）
"""

from __future__ import annotations

import argparse
import hashlib
import json
import mimetypes
import os
import re
import ssl
import urllib.error
import urllib.request
import shutil
import sys
import threading
import time
import webbrowser
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

ROOT = Path(__file__).resolve().parent          # app/
PROJECT_ROOT = ROOT.parent                      # account-manage/
DATA_DIR = ROOT / "data"
DATA_FILE = DATA_DIR / "accounts.json"
BACKUP_DIR = DATA_DIR / "backups"
USAGE_CACHE_FILE = DATA_DIR / "usage_cache.json"
STATIC_DIR = ROOT / "static"
DEFAULT_VAULT = PROJECT_ROOT / "keepassxc" / "vaults" / "ai-keys.kdbx"

KDBX_MAGIC = bytes([0x03, 0xD9, 0xA2, 0x9A])     # KDBX 文件头魔数
DATA_BACKUP_KEEP = 10
VAULT_BACKUP_KEEP = 10

EMPTY_DATA = {
    "version": 1,
    "accounts": [],
    "relations": [],
    "query_links": [],
    "usage_configs": [],
    "settings": {"vault_path": str(DEFAULT_VAULT), "name": "账号管家"},
}

_lock = threading.Lock()


# ---------------------------------------------------------------------------
# 数据读写（原子写 + 自动备份）
# ---------------------------------------------------------------------------
def load_data() -> dict:
    if not DATA_FILE.exists():
        return json.loads(json.dumps(EMPTY_DATA))  # deep copy
    try:
        with open(DATA_FILE, "r", encoding="utf-8") as f:
            data = json.load(f)
    except (json.JSONDecodeError, OSError):
        # 损坏时尝试用最新备份恢复，否则返回空
        b = latest_backup(BACKUP_DIR)
        if b:
            with open(b, "r", encoding="utf-8") as f:
                return json.load(f)
        return json.loads(json.dumps(EMPTY_DATA))
    data.setdefault("version", 1)
    data.setdefault("accounts", [])
    data.setdefault("relations", [])
    data.setdefault("query_links", [])
    data.setdefault("usage_configs", [])
    data.setdefault("settings", {})
    data["settings"].setdefault("vault_path", str(DEFAULT_VAULT))
    data["settings"].setdefault("name", "账号管家")
    return data


def save_data(data: dict) -> None:
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    # 自动备份：仅在内容变化时备份（简单起见：每次保存前把旧文件转备份，限数量）
    if DATA_FILE.exists():
        try:
            with open(DATA_FILE, "r", encoding="utf-8") as f:
                old = f.read()
            new = json.dumps(data, ensure_ascii=False, indent=2)
            if old.strip() == new.strip():
                return  # 无变化，不写
        except OSError:
            pass
        _rotate_backup(DATA_FILE, BACKUP_DIR, DATA_BACKUP_KEEP)
    tmp = DATA_FILE.with_suffix(".json.tmp")
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=False, indent=2)
        f.flush()
        os.fsync(f.fileno())
    os.replace(tmp, DATA_FILE)


def _rotate_backup(src: Path, bdir: Path, keep: int) -> None:
    """把 src 复制成带时间戳的备份，清理超出 keep 的旧备份。"""
    bdir.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    dst = bdir / f"{src.stem}-{stamp}{src.suffix}"
    try:
        shutil.copy2(src, dst)
    except OSError:
        return
    backups = sorted(bdir.glob(f"{src.stem}-*{src.suffix}"))
    for old in backups[:-keep]:
        try:
            old.unlink()
        except OSError:
            pass


def latest_backup(bdir: Path) -> Path | None:
    if not bdir.exists():
        return None
    backups = sorted(bdir.glob("accounts-*.json"))
    return backups[-1] if backups else None


def vault_path(data: dict) -> Path:
    p = Path(data["settings"].get("vault_path") or str(DEFAULT_VAULT))
    if not p.is_absolute():
        p = PROJECT_ROOT / p
    return p


# ---------------------------------------------------------------------------
# REST API
# ---------------------------------------------------------------------------
class Handler(BaseHTTPRequestHandler):
    server_version = "AccountHub/1.0"

    # ---- helpers ----------------------------------------------------------
    def _send_json(self, obj, status=200):
        body = json.dumps(obj, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def _send_error_json(self, status, message):
        self._send_json({"error": message}, status)

    def _read_json(self) -> dict:
        length = int(self.headers.get("Content-Length") or 0)
        if length <= 0:
            return {}
        raw = self.rfile.read(length)
        try:
            return json.loads(raw.decode("utf-8"))
        except json.JSONDecodeError:
            raise ValueError("请求体不是合法 JSON")

    def _read_body(self) -> bytes:
        length = int(self.headers.get("Content-Length") or 0)
        return self.rfile.read(length) if length > 0 else b""

    # ---- routing ----------------------------------------------------------
    def do_GET(self):
        try:
            self._route_GET()
        except BrokenPipeError:
            pass
        except Exception as e:  # noqa: BLE001
            self._send_error_json(500, f"服务器错误: {e}")

    def do_POST(self):
        try:
            self._route_POST()
        except ValueError as e:
            self._send_error_json(400, str(e))
        except BrokenPipeError:
            pass
        except Exception as e:  # noqa: BLE001
            self._send_error_json(500, f"服务器错误: {e}")

    def do_PUT(self):
        try:
            self._route_PUT()
        except ValueError as e:
            self._send_error_json(400, str(e))
        except BrokenPipeError:
            pass
        except Exception as e:  # noqa: BLE001
            self._send_error_json(500, f"服务器错误: {e}")

    def do_DELETE(self):
        try:
            self._route_DELETE()
        except ValueError as e:
            self._send_error_json(400, str(e))
        except BrokenPipeError:
            pass
        except Exception as e:  # noqa: BLE001
            self._send_error_json(500, f"服务器错误: {e}")

    # ---- GET --------------------------------------------------------------
    def _route_GET(self):
        parsed = urlparse(self.path)
        path = parsed.path
        if path == "/api/health":
            return self._send_json({"ok": True, "time": datetime.now().isoformat()})
        if path == "/api/data":
            with _lock:
                return self._send_json(load_data())
        if path == "/api/export":
            with _lock:
                data = load_data()
            body = json.dumps(data, ensure_ascii=False, indent=2).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Content-Disposition", 'attachment; filename="accounts-backup.json"')
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if path == "/api/vault/info":
            return self._vault_info()
        if path == "/api/vault/download":
            return self._vault_download()
        if path == "/api/vault/backups":
            return self._vault_backups()
        if path.startswith("/api/vault/backup/"):
            return self._vault_restore(path)
        if path == "/api/usage":
            return self._usage_list()
        if path == "/api/usage/providers":
            return self._send_json({"providers": [{"key": k, **v} for k, v in USAGE_PROVIDERS.items()]})
        if path == "/api/usage/fetch":
            qs = parse_qs(parsed.query)
            cfg_id = (qs.get("id") or [""])[0]
            return self._usage_fetch(cfg_id)
        # 静态文件
        return self._serve_static(path)

    def _serve_static(self, path: str):
        if path in ("/", "/index.html"):
            rel = "index.html"
        else:
            rel = path.lstrip("/")
        target = (STATIC_DIR / rel).resolve()
        if not str(target).startswith(str(STATIC_DIR.resolve())) or not target.is_file():
            return self._send_error_json(404, "Not Found")
        ctype = mimetypes.guess_type(str(target))[0] or "application/octet-stream"
        body = target.read_bytes()
        self.send_response(200)
        self.send_header("Content-Type", f"{ctype}; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    # ---- POST -------------------------------------------------------------
    def _route_POST(self):
        parsed = urlparse(self.path)
        path = parsed.path
        if path == "/api/accounts":
            payload = self._read_json()
            with _lock:
                data = load_data()
                acc = self._validate_account(payload, data)
                acc["id"] = _new_id("acc", data["accounts"])
                acc["created_at"] = _now()
                acc["updated_at"] = acc["created_at"]
                data["accounts"].append(acc)
                save_data(data)
            return self._send_json({"ok": True, "account": acc}, 201)
        if path == "/api/relations":
            payload = self._read_json()
            with _lock:
                data = load_data()
                rel = self._validate_relation(payload, data)
                rel["id"] = _new_id("rel", data["relations"])
                rel["created_at"] = _now()
                data["relations"].append(rel)
                save_data(data)
            return self._send_json({"ok": True, "relation": rel}, 201)
        if path == "/api/query-links":
            payload = self._read_json()
            with _lock:
                data = load_data()
                q = self._validate_query_link(payload)
                q["id"] = _new_id("q", data["query_links"])
                data["query_links"].append(q)
                save_data(data)
            return self._send_json({"ok": True, "query_link": q}, 201)
        if path == "/api/import":
            payload = self._read_json()
            with _lock:
                data = load_data()
                accounts = payload.get("accounts") or []
                relations = payload.get("relations") or []
                links = payload.get("query_links") or []
                usage_cfgs = payload.get("usage_configs") or []
                n_acc = n_rel = n_ql = n_uc = 0
                for a in accounts:
                    acc = self._validate_account(a, data)
                    # 备份还原时保留原 id（避免关联断裂）；冲突时重新生成
                    if a.get("id") and a["id"] not in {x["id"] for x in data["accounts"]}:
                        acc["id"] = a["id"]
                    else:
                        acc["id"] = _new_id("acc", data["accounts"])
                    acc["created_at"] = a.get("created_at") or _now()
                    acc["updated_at"] = _now()
                    data["accounts"].append(acc)
                    n_acc += 1
                for r in relations:
                    rel = self._validate_relation(r, data)
                    rel["id"] = _new_id("rel", data["relations"])
                    data["relations"].append(rel)
                    n_rel += 1
                for q in links:
                    ql = self._validate_query_link(q)
                    ql["id"] = q.get("id") or _new_id("q", data["query_links"])
                    data["query_links"].append(ql)
                    n_ql += 1
                for uc in usage_cfgs:
                    cfg = self._validate_usage_config(uc, data)
                    cfg["id"] = uc.get("id") or _new_id("uc", data["usage_configs"])
                    cfg["last_run_at"] = uc.get("last_run_at") or ""
                    data["usage_configs"].append(cfg)
                    n_uc += 1
                save_data(data)
            return self._send_json(
                {"ok": True,
                 "imported": {"accounts": n_acc, "relations": n_rel, "query_links": n_ql, "usage_configs": n_uc}},
                201)
        if path == "/api/usage-configs":
            payload = self._read_json()
            with _lock:
                data = load_data()
                cfg = self._validate_usage_config(payload, data)
                cfg["id"] = _new_id("uc", data["usage_configs"])
                cfg["last_run_at"] = ""
                data["usage_configs"].append(cfg)
                save_data(data)
            return self._send_json({"ok": True, "config": cfg}, 201)
        if path == "/api/usage-configs/test":
            payload = self._read_json()
            res = _usage_test(payload)
            return self._send_json(res)
        if path == "/api/usage-configs/import-oauth":
            payload = self._read_json()
            provider = str(payload.get("provider") or "").strip()
            res = _oauth_import_from_cli(provider)
            return self._send_json(res)
        if path == "/api/oauth/grok/device-code":
            return self._send_json(_grok_device_code_start())
        if path == "/api/oauth/grok/poll":
            payload = self._read_json()
            return self._send_json(_grok_device_code_poll(payload))
        if path == "/api/vault/upload":
            return self._vault_upload()
        if path == "/api/data/reset":
            payload = self._read_json()
            confirm = payload.get("confirm")
            if confirm != "RESET":
                raise ValueError("需在请求体带 confirm=RESET 才可重置")
            with _lock:
                _rotate_backup(DATA_FILE, BACKUP_DIR, DATA_BACKUP_KEEP)
                save_data(json.loads(json.dumps(EMPTY_DATA)))
            return self._send_json({"ok": True})
        return self._send_error_json(404, "Not Found")

    # ---- PUT --------------------------------------------------------------
    def _route_PUT(self):
        parsed = urlparse(self.path)
        path = parsed.path
        parts = path.strip("/").split("/")
        if len(parts) == 3 and parts[0] == "api" and parts[1] == "accounts":
            acc_id = parts[2]
            payload = self._read_json()
            with _lock:
                data = load_data()
                idx = next((i for i, a in enumerate(data["accounts"]) if a["id"] == acc_id), None)
                if idx is None:
                    return self._send_error_json(404, f"账号 {acc_id} 不存在")
                updated = self._validate_account(payload, data)
                updated["id"] = acc_id
                updated["created_at"] = data["accounts"][idx].get("created_at", _now())
                updated["updated_at"] = _now()
                data["accounts"][idx] = updated
                save_data(data)
            return self._send_json({"ok": True, "account": updated})
        if len(parts) == 3 and parts[0] == "api" and parts[1] == "query-links":
            qid = parts[2]
            payload = self._read_json()
            with _lock:
                data = load_data()
                idx = next((i for i, q in enumerate(data["query_links"]) if q["id"] == qid), None)
                if idx is None:
                    return self._send_error_json(404, f"查询链接 {qid} 不存在")
                q = self._validate_query_link(payload)
                q["id"] = qid
                data["query_links"][idx] = q
                save_data(data)
            return self._send_json({"ok": True, "query_link": q})
        if len(parts) == 3 and parts[0] == "api" and parts[1] == "usage-configs":
            cfg_id = parts[2]
            payload = self._read_json()
            with _lock:
                data = load_data()
                idx = next((i for i, c in enumerate(data["usage_configs"]) if c["id"] == cfg_id), None)
                if idx is None:
                    return self._send_error_json(404, f"用量配置 {cfg_id} 不存在")
                updated = self._validate_usage_config(payload, data)
                updated["id"] = cfg_id
                updated["last_run_at"] = data["usage_configs"][idx].get("last_run_at", "")
                # 保留已有的 oauth_tokens（前端编辑时不一定回传）
                if not updated.get("oauth_tokens"):
                    updated["oauth_tokens"] = data["usage_configs"][idx].get("oauth_tokens", {})
                data["usage_configs"][idx] = updated
                save_data(data)
            return self._send_json({"ok": True, "config": updated})
        if len(parts) == 2 and parts[0] == "api" and parts[1] == "settings":
            payload = self._read_json()
            with _lock:
                data = load_data()
                if "vault_path" in payload and isinstance(payload["vault_path"], str):
                    data["settings"]["vault_path"] = payload["vault_path"].strip() or str(DEFAULT_VAULT)
                if "name" in payload and isinstance(payload["name"], str):
                    data["settings"]["name"] = payload["name"].strip() or "账号管家"
                save_data(data)
            return self._send_json({"ok": True, "settings": data["settings"]})
        return self._send_error_json(404, "Not Found")

    # ---- DELETE -----------------------------------------------------------
    def _route_DELETE(self):
        parsed = urlparse(self.path)
        parts = parsed.path.strip("/").split("/")
        if len(parts) == 3 and parts[0] == "api" and parts[1] == "accounts":
            acc_id = parts[2]
            with _lock:
                data = load_data()
                before = len(data["accounts"])
                data["accounts"] = [a for a in data["accounts"] if a["id"] != acc_id]
                removed_rels = len(data["relations"])
                data["relations"] = [r for r in data["relations"]
                                     if r["from"] != acc_id and r["to"] != acc_id]
                removed_rels -= len(data["relations"])
                if len(data["accounts"]) == before:
                    return self._send_error_json(404, f"账号 {acc_id} 不存在")
                save_data(data)
            return self._send_json({"ok": True, "removed_relations": removed_rels})
        if len(parts) == 3 and parts[0] == "api" and parts[1] == "relations":
            rel_id = parts[2]
            with _lock:
                data = load_data()
                before = len(data["relations"])
                data["relations"] = [r for r in data["relations"] if r["id"] != rel_id]
                if len(data["relations"]) == before:
                    return self._send_error_json(404, f"关联 {rel_id} 不存在")
                save_data(data)
            return self._send_json({"ok": True})
        if len(parts) == 3 and parts[0] == "api" and parts[1] == "query-links":
            qid = parts[2]
            with _lock:
                data = load_data()
                before = len(data["query_links"])
                data["query_links"] = [q for q in data["query_links"] if q["id"] != qid]
                if len(data["query_links"]) == before:
                    return self._send_error_json(404, f"查询链接 {qid} 不存在")
                save_data(data)
            return self._send_json({"ok": True})
        if len(parts) == 3 and parts[0] == "api" and parts[1] == "usage-configs":
            cfg_id = parts[2]
            with _lock:
                data = load_data()
                before = len(data["usage_configs"])
                data["usage_configs"] = [c for c in data["usage_configs"] if c["id"] != cfg_id]
                if len(data["usage_configs"]) == before:
                    return self._send_error_json(404, f"用量配置 {cfg_id} 不存在")
                # 清理缓存
                cache = _usage_load_cache()
                cache.pop(cfg_id, None)
                _usage_save_cache(cache)
                save_data(data)
            return self._send_json({"ok": True})
        return self._send_error_json(404, "Not Found")

    # ---- 校验 --------------------------------------------------------------
    @staticmethod
    def _validate_account(payload: dict, data: dict) -> dict:
        name = str(payload.get("name") or "").strip()
        if not name:
            raise ValueError("显示名称不能为空")
        category = str(payload.get("category") or "other").strip()
        known = {"ai_member", "api", "email", "phone", "wechat", "public_account",
                 "qq", "zlibrary", "apple", "other"}
        if category not in known:
            raise ValueError(f"未知类别: {category}")
        fields = payload.get("fields") or {}
        if not isinstance(fields, dict):
            raise ValueError("fields 必须是对象")
        return {
            "category": category,
            "name": name,
            "vendor": str(payload.get("vendor") or "").strip(),
            "username": str(payload.get("username") or "").strip(),
            "url": str(payload.get("url") or "").strip(),
            "status": str(payload.get("status") or "active").strip(),
            "notes": str(payload.get("notes") or "").strip(),
            "fields": fields,
        }

    @staticmethod
    def _validate_relation(payload: dict, data: dict) -> dict:
        frm = str(payload.get("from") or "").strip()
        to = str(payload.get("to") or "").strip()
        if not frm or not to:
            raise ValueError("关联的两端账号不能为空")
        ids = {a["id"] for a in data["accounts"]}
        if frm not in ids or to not in ids:
            raise ValueError("关联的账号不存在")
        if frm == to:
            raise ValueError("不能关联到自身")
        rtype = str(payload.get("type") or "其他").strip() or "其他"
        return {
            "from": frm,
            "to": to,
            "type": rtype,
            "note": str(payload.get("note") or "").strip(),
        }

    @staticmethod
    def _validate_query_link(payload: dict) -> dict:
        label = str(payload.get("label") or "").strip()
        url = str(payload.get("url") or "").strip()
        if not label or not url:
            raise ValueError("标签和链接不能为空")
        return {
            "category": str(payload.get("category") or "other").strip(),
            "vendor": str(payload.get("vendor") or "").strip(),
            "label": label,
            "url": url,
        }

    @staticmethod
    def _validate_usage_config(payload: dict, data: dict) -> dict:
        account_id = str(payload.get("account_id") or "").strip()
        ids = {a["id"] for a in data["accounts"]}
        if account_id not in ids:
            raise ValueError("account_id 不存在")
        provider = str(payload.get("provider") or "").strip()
        api_key = str(payload.get("api_key") or "").strip()
        url = str(payload.get("url") or "").strip()
        method = str(payload.get("method") or "GET").strip().upper()
        headers = payload.get("headers") or {}
        body = str(payload.get("body") or "")
        jp_used = str(payload.get("jsonpath_used") or "").strip()
        jp_total = str(payload.get("jsonpath_total") or "").strip()
        # 内置 provider 不需要手动填 URL / JSONPath
        if provider and provider in USAGE_PROVIDERS:
            p = USAGE_PROVIDERS[provider]
            if p.get("requires_api_key") and not api_key:
                raise ValueError("该 provider 需要 API Key")
            url = url or p.get("default_url", "")
            method = method or "GET"
            jp_used = jp_used or p.get("default_jsonpath_used", "")
            jp_total = jp_total or p.get("default_jsonpath_total", "")
        else:
            if not url.startswith("http"):
                raise ValueError("URL 必须是 http(s) 链接")
            if not jp_used and not jp_total:
                raise ValueError("至少填写 used 或 total 的取值路径")
        if method not in ("GET", "POST"):
            raise ValueError("method 只支持 GET / POST")
        if not isinstance(headers, dict):
            raise ValueError("headers 必须是对象")
        try:
            interval_min = int(payload.get("interval_min") or 60)
        except (TypeError, ValueError):
            interval_min = 60
        if interval_min < 1:
            interval_min = 1
        return {
            "account_id": account_id,
            "provider": provider,
            "api_key": api_key,
            "oauth_tokens": payload.get("oauth_tokens") or {},
            "url": url,
            "method": method,
            "headers": headers,
            "body": body,
            "jsonpath_used": jp_used,
            "jsonpath_total": jp_total,
            "unit": str(payload.get("unit") or "").strip(),
            "interval_min": interval_min,
            "enabled": bool(payload.get("enabled", True)),
        }

    # ---- 用量查询 ----------------------------------------------------------
    def _usage_list(self):
        data = load_data()
        configs = data.get("usage_configs", [])
        cache = _usage_load_cache()
        out = []
        for c in configs:
            entry = dict(c)
            entry["cache"] = cache.get(c["id"])
            out.append(entry)
        return self._send_json({"configs": out})

    def _usage_fetch(self, cfg_id: str):
        if not cfg_id:
            return self._send_error_json(400, "缺少 id")
        data = load_data()
        cfg = next((c for c in data.get("usage_configs", []) if c["id"] == cfg_id), None)
        if not cfg:
            return self._send_error_json(404, "配置不存在")
        res = _usage_do_fetch(cfg)
        if res.get("ok"):
            cache = _usage_load_cache()
            cache[cfg_id] = res["result"]
            _usage_save_cache(cache)
            with _lock:
                data = load_data()
                for c in data["usage_configs"]:
                    if c["id"] == cfg_id:
                        c["last_run_at"] = _now()
                        break
                save_data(data)
        return self._send_json(res)

    # ---- 密钥库 ------------------------------------------------------------
    def _vault_info(self):
        with _lock:
            data = load_data()
            vp = vault_path(data)
        info = {"path": str(vp), "exists": vp.exists()}
        if vp.exists():
            st = vp.stat()
            info["size"] = st.st_size
            info["mtime"] = datetime.fromtimestamp(st.st_mtime).isoformat()
            info["sha256"] = _sha256(vp)
            head = vp.read_bytes()[:4]
            info["valid_kdbx"] = head == KDBX_MAGIC
        return self._send_json(info)

    def _vault_download(self):
        with _lock:
            data = load_data()
            vp = vault_path(data)
        if not vp.exists():
            return self._send_error_json(404, "密钥库文件不存在")
        body = vp.read_bytes()
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Disposition", f'attachment; filename="{vp.name}"')
        self.send_header("Content-Length", str(len(body)))
        self.send_header("X-SHA256", _sha256(vp))
        self.end_headers()
        self.wfile.write(body)

    def _vault_upload(self):
        raw = self._read_body()
        if not raw:
            raise ValueError("上传内容为空")
        if len(raw) > 64 * 1024 * 1024:
            raise ValueError("文件超过 64MB 限制")
        if raw[:4] != KDBX_MAGIC:
            raise ValueError("不是有效的 KDBX 文件（文件头不匹配），已拒绝替换")
        with _lock:
            data = load_data()
            vp = vault_path(data)
            vp.parent.mkdir(parents=True, exist_ok=True)
            if vp.exists():
                _rotate_backup(vp, vp.parent / "backups", VAULT_BACKUP_KEEP)
            tmp = vp.with_suffix(".kdbx.tmp")
            with open(tmp, "wb") as f:
                f.write(raw)
                f.flush()
                os.fsync(f.fileno())
            os.replace(tmp, vp)
        return self._send_json({"ok": True, "path": str(vp), "sha256": _sha256(vp)})

    def _vault_backups(self):
        with _lock:
            data = load_data()
            vp = vault_path(data)
        bdir = vp.parent / "backups"
        items = []
        if bdir.exists():
            for b in sorted(bdir.glob(f"{vp.stem}-*{vp.suffix}"), reverse=True):
                items.append({
                    "name": b.name,
                    "size": b.stat().st_size,
                    "mtime": datetime.fromtimestamp(b.stat().st_mtime).isoformat(),
                    "sha256": _sha256(b),
                })
        return self._send_json({"backups": items, "dir": str(bdir)})

    def _vault_restore(self, path: str):
        """GET /api/vault/backup/<name> → 下载该备份"""
        name = path.rsplit("/", 1)[-1]
        with _lock:
            data = load_data()
            vp = vault_path(data)
        bdir = vp.parent / "backups"
        target = (bdir / name).resolve()
        if not str(target).startswith(str(bdir.resolve())) or not target.is_file():
            return self._send_error_json(404, "备份不存在")
        body = target.read_bytes()
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Disposition", f'attachment; filename="{target.name}"')
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt, *args):  # 静默访问日志
        pass


# ---------------------------------------------------------------------------
# 工具函数
# ---------------------------------------------------------------------------
def _now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def _new_id(prefix: str, items: list[dict]) -> str:
    used = {it.get("id", "") for it in items}
    n = 1
    while f"{prefix}-{n:04d}" in used:
        n += 1
    return f"{prefix}-{n:04d}"


def _sha256(p: Path) -> str:
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


# ---------------------------------------------------------------------------
# 用量查询（JSONPath 提取 + HTTP 抓取 + 定时调度）
# ---------------------------------------------------------------------------
_TOKEN_RE = re.compile(r"""
    \$(?:\.\w+|\['[^']+'\]|\["[^"]+"\])   # $.foo 或 $['foo'] 或 $["foo"]
""", re.VERBOSE)
_SSL_CTX = ssl.create_default_context()
_SSL_CTX.check_hostname = False
_SSL_CTX.verify_mode = ssl.CERT_NONE


def _jp_eval(path: str, obj):
    """极简 JSONPath 求值，仅支持 $.a.b.c / $['a']['b'] / $("a") 链式取值。
    缺失返回 None。支持点号键名本身含点的边缘情形（按 $['key'] 精确）。"""
    if not path:
        return None
    p = path.strip()
    if p.startswith("$"):
        p = p[1:]
    cur = obj
    i = 0
    n = len(p)
    while i < n:
        c = p[i]
        if c == ".":
            i += 1
            j = i
            while j < n and p[j] not in ".[":
                j += 1
            key = p[i:j]
            if key == "":
                return None
            cur = _jp_index(cur, key)
            i = j
        elif c == "[":
            j = p.find("]", i)
            if j == -1:
                return None
            inner = p[i + 1:j].strip()
            if (inner.startswith("'") and inner.endswith("'")) or (inner.startswith('"') and inner.endswith('"')):
                key = inner[1:-1]
            else:
                key = inner
            cur = _jp_index(cur, key)
            i = j + 1
        else:
            i += 1
    return cur


def _jp_index(cur, key):
    if isinstance(cur, list):
        try:
            return cur[int(key)]
        except (ValueError, IndexError):
            return None
    if isinstance(cur, dict):
        return cur.get(key)
    return None


def _parse_numeric(val):
    """尽力把 JSONPath 取出来的值转成 float，失败返回 None。"""
    if val is None:
        return None
    if isinstance(val, bool):
        return None
    if isinstance(val, (int, float)):
        return float(val)
    s = str(val).strip()
    if not s:
        return None
    # 提取第一个数字（支持 1,234.5 / 12.5% / "12.5 USD"）
    m = re.search(r"-?\d[\d,]*\.?\d*", s)
    if not m:
        return None
    try:
        return float(m.group(0).replace(",", ""))
    except ValueError:
        return None


# ---------------------------------------------------------------------------
# 内置用量 Provider 注册表
# ---------------------------------------------------------------------------
USAGE_PROVIDERS = {
    "chatgpt_codex": {
        "label": "ChatGPT Codex (OAuth)",
        "description": "自动读取 ~/.codex/auth.json，刷新 token，查询 codex 用量",
        "vendor_filter": "OpenAI",
        "default_url": "https://chatgpt.com/backend-api/codex/usage",
        "default_jsonpath_used": "$.rate_limit.primary_window.used_percent",
        "default_jsonpath_total": "",
        "default_unit": "%",
        "default_interval_min": 30,
        "base_url": "https://api.openai.com/v1",
        "docs_url": "https://platform.openai.com/docs/api-reference",
    },
    "claude_code": {
        "label": "Claude Code (OAuth)",
        "description": "自动读取 ~/.claude/.credentials.json，刷新 token，发送探测请求读取 rate-limit 响应头",
        "vendor_filter": "Anthropic",
        "default_url": "",
        "default_jsonpath_used": "",
        "default_jsonpath_total": "",
        "default_unit": "%",
        "default_interval_min": 30,
        "base_url": "https://api.anthropic.com",
        "docs_url": "https://docs.anthropic.com/en/api/getting-started",
    },
    "grok_build": {
        "label": "Grok Build (OAuth)",
        "description": "自动读取 ~/.grok/auth.json，刷新 token，查询 xAI 用量",
        "vendor_filter": "xAI",
        "default_url": "",
        "default_jsonpath_used": "",
        "default_jsonpath_total": "",
        "default_unit": "%",
        "default_interval_min": 30,
        "base_url": "https://api.x.ai/v1",
        "docs_url": "https://docs.x.ai/docs",
    },
    "glm_coding": {
        "label": "GLM Coding Plan (API Key)",
        "description": "通过 API Key 查询智谱 GLM Coding Plan 配额（open.bigmodel.cn）",
        "vendor_filter": "Zhipu",
        "requires_api_key": True,
        "default_unit": "%",
        "default_interval_min": 30,
        "base_url": "https://open.bigmodel.cn/api/paas/v4",
        "docs_url": "https://open.bigmodel.cn/dev/api",
    },
    "kimi_coding": {
        "label": "Kimi for Coding (API Key)",
        "description": "通过 API Key 查询 Kimi Coding Plan 周配额（api.kimi.com）",
        "vendor_filter": "Moonshot",
        "requires_api_key": True,
        "default_unit": "%",
        "default_interval_min": 30,
        "base_url": "https://api.moonshot.cn/v1",
        "docs_url": "https://platform.moonshot.cn/docs",
    },
    "minimax_coding": {
        "label": "MiniMax Coding Plan (API Key)",
        "description": "通过 API Key 查询 MiniMax Coding Plan 配额（api.minimaxi.com）",
        "vendor_filter": "MiniMax",
        "requires_api_key": True,
        "default_unit": "%",
        "default_interval_min": 30,
        "base_url": "https://api.minimaxi.com/v1",
        "docs_url": "https://platform.minimaxi.com/document",
    },
    "deepseek_balance": {
        "label": "DeepSeek 余额 (API Key)",
        "description": "通过 API Key 查询 DeepSeek 账户余额（api.deepseek.com/user/balance）",
        "vendor_filter": "DeepSeek",
        "requires_api_key": True,
        "default_unit": "",
        "default_interval_min": 60,
        "base_url": "https://api.deepseek.com/v1",
        "docs_url": "https://api-docs.deepseek.com/",
    },
    "gemini_models": {
        "label": "Gemini Key 验证 (API Key)",
        "description": "通过 API Key 调用 /v1beta/models 验证密钥有效性并列出可用模型（无用量配额 API）",
        "vendor_filter": "Google",
        "requires_api_key": True,
        "default_unit": "",
        "default_interval_min": 60,
        "base_url": "https://generativelanguage.googleapis.com/v1beta",
        "docs_url": "https://ai.google.dev/gemini-api/docs",
    },
    "dashscope_balance": {
        "label": "阿里百炼配额 (API Key)",
        "description": "通过 API Key 调用 /api/v1/quotas 验证密钥有效性并返回模型配额（dashscope.aliyuncs.com）",
        "vendor_filter": "阿里云",
        "requires_api_key": True,
        "default_unit": "",
        "default_interval_min": 60,
        "base_url": "https://dashscope.aliyuncs.com/compatible-mode/v1",
        "docs_url": "https://help.aliyun.com/zh/dashscope/",
    },
}

# Provider → fetch function 映射
PROVIDER_FETCHERS = {}  # 延迟填充，函数定义后注册


def _codex_auth_path() -> Path:
    """定位 ~/.codex/auth.json"""
    return Path(os.path.expanduser("~")) / ".codex" / "auth.json"


# ---------------------------------------------------------------------------
# OAuth token 获取：优先从 cfg["oauth_tokens"] 读取，fallback 到本地 CLI 文件
# ---------------------------------------------------------------------------

def _oauth_get_tokens(cfg: dict, local_path: Path, local_extractor):
    """统一的 OAuth token 获取入口。
    优先从 cfg["oauth_tokens"] 读取（支持多账号），没有则 fallback 到本地 CLI 文件。
    local_extractor: function(file_data) -> dict(access_token, refresh_token, **extra)
    返回 dict 或 None。
    """
    ot = cfg.get("oauth_tokens") or {}
    if ot.get("access_token") or ot.get("refresh_token"):
        return {
            "access_token": ot.get("access_token", ""),
            "refresh_token": ot.get("refresh_token", ""),
            "source": "config",
            "file_path": None,
            "file_data": None,
            "auth_data": ot,
            "extra": {k: v for k, v in ot.items() if k not in ("access_token", "refresh_token")},
        }
    if not local_path.exists():
        return None
    try:
        with open(local_path, "r", encoding="utf-8") as f:
            file_data = json.load(f)
    except (json.JSONDecodeError, OSError):
        return None
    extracted = local_extractor(file_data)
    if not extracted or (not extracted.get("access_token") and not extracted.get("refresh_token")):
        return None
    return {
        "access_token": extracted.get("access_token", ""),
        "refresh_token": extracted.get("refresh_token", ""),
        "source": "file",
        "file_path": local_path,
        "file_data": file_data,
        "auth_data": extracted,
        "extra": {k: v for k, v in extracted.items() if k not in ("access_token", "refresh_token")},
    }


def _oauth_save_tokens_to_cfg(cfg_id: str, tokens: dict):
    """将刷新后的 token 回写到 usage_config 的 oauth_tokens 字段。"""
    with _lock:
        data = load_data()
        idx = next((i for i, c in enumerate(data["usage_configs"]) if c["id"] == cfg_id), None)
        if idx is None:
            return
        data["usage_configs"][idx]["oauth_tokens"] = tokens
        save_data(data)


def _codex_refresh_and_fetch(cfg: dict) -> dict:
    """读取 auth.json → 如需则刷新 token → 请求 codex usage 端点。
    刷新成功后回写 auth.json（更新 tokens + last_refresh）。"""
    auth_path = _codex_auth_path()
    if not auth_path.exists():
        return {"ok": False, "error": f"未找到 {auth_path}（请先用 Codex CLI 登录）", "status": 0, "raw": ""}
    try:
        with open(auth_path, "r", encoding="utf-8") as f:
            auth = json.load(f)
    except (json.JSONDecodeError, OSError) as e:
        return {"ok": False, "error": f"读取 auth.json 失败: {e}", "status": 0, "raw": ""}

    tokens = auth.get("tokens") or {}
    access_token = tokens.get("access_token", "")
    refresh_token = tokens.get("refresh_token", "")
    account_id = tokens.get("account_id", "")
    client_id = "app_EMoamEEZ73f0CkXaXp7hrann"  # Codex CLI 固定 client_id

    # 尝试用当前 access_token 直接请求；失败则刷新后重试
    def do_request(tok: str) -> tuple[int, str, str]:
        url = cfg.get("url") or "https://chatgpt.com/backend-api/codex/usage"
        req = urllib.request.Request(url, method="GET", headers={
            "Authorization": f"Bearer {tok}",
            "ChatGPT-Account-Id": account_id,
            "User-Agent": "codex-cli/1.0",
            "Accept": "application/json",
            "originator": "codex_cli_rs",
        })
        try:
            # ChatGPT 端点必须用默认 SSL（带证书验证）——Cloudflare 会拦截禁用验证的连接
            with urllib.request.urlopen(req, timeout=15) as resp:
                return resp.status, resp.read().decode("utf-8", errors="replace"), ""
        except urllib.error.HTTPError as e:
            txt = ""
            try:
                txt = e.read().decode("utf-8", errors="replace")
            except Exception:
                pass
            return e.code, txt, f"HTTP {e.code}"
        except Exception as e:
            return 0, "", str(e)

    status, text, err = do_request(access_token)
    # 401/403 → 尝试刷新
    if status in (401, 403) and refresh_token:
        body = json.dumps({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": client_id,
        }).encode("utf-8")
        try:
            req = urllib.request.Request(
                "https://auth.openai.com/oauth/token",
                data=body,
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            with urllib.request.urlopen(req, timeout=15) as resp:
                refreshed = json.loads(resp.read().decode("utf-8"))
            access_token = refreshed["access_token"]
            new_refresh = refreshed.get("refresh_token", refresh_token)
            # 回写 auth.json
            auth["tokens"]["access_token"] = access_token
            auth["tokens"]["refresh_token"] = new_refresh
            auth["tokens"]["id_token"] = refreshed.get("id_token", auth["tokens"].get("id_token", ""))
            auth["last_refresh"] = _now()
            tmp = auth_path.with_suffix(".json.tmp")
            with open(tmp, "w", encoding="utf-8") as f:
                json.dump(auth, f, ensure_ascii=False, indent=2)
                f.flush()
                os.fsync(f.fileno())
            os.replace(tmp, auth_path)
            # 重新请求
            status, text, err = do_request(access_token)
        except Exception as e:
            return {"ok": False, "error": f"token 刷新失败: {e}", "status": status, "raw": text[:500]}

    if err:
        return {"ok": False, "error": err, "status": status, "raw": text[:500]}
    try:
        obj = json.loads(text)
    except json.JSONDecodeError:
        return {"ok": False, "error": "响应不是合法 JSON", "status": status, "raw": text[:500]}

    jp_used = cfg.get("jsonpath_used") or "$.rate_limit.primary_window.used_percent"
    jp_total = cfg.get("jsonpath_total") or ""
    used_raw = _jp_eval(jp_used, obj) if jp_used else None
    total_raw = _jp_eval(jp_total, obj) if jp_total else None
    used_val = _parse_numeric(used_raw)
    total_val = _parse_numeric(total_raw)
    extra = {}
    if "credits" in obj and isinstance(obj["credits"], dict):
        bal = obj["credits"].get("balance")
        if bal is not None:
            extra["credits_balance"] = str(bal)
    if "plan_type" in obj:
        extra["plan_type"] = obj["plan_type"]
    if "rate_limit" in obj and isinstance(obj["rate_limit"], dict):
        pw = obj["rate_limit"].get("primary_window") or {}
        if pw.get("reset_at"):
            extra["reset_at"] = str(pw["reset_at"])
    # ChatGPT Codex: used_percent 是「已用百分比」，直接作为进度条填充值
    percent_used = None
    if used_val is not None and total_val is not None and total_val > 0:
        # 有绝对值 → 算百分比
        percent_used = round(used_val / total_val * 100, 1)
    elif used_val is not None:
        # 只有百分比 → used_percent 本身就是「已用百分比」
        percent_used = round(used_val, 1)
    return {
        "ok": True,
        "status": status,
        "result": {
            "used": used_val if (used_val is not None and total_val is not None) else None,
            "total": total_val,
            "percent_used": percent_used,
            "percent_semantics": "used",
            "unit": cfg.get("unit") or "%",
            "fetched_at": _now(),
            "raw_used": used_raw,
            "raw_total": total_raw,
            **extra,
        },
    }


def _usage_http(cfg: dict) -> tuple[int, str, str]:
    """执行 HTTP 请求，返回 (status, body_text, error_msg)。"""
    url = cfg["url"]
    method = cfg.get("method", "GET").upper()
    headers = dict(cfg.get("headers") or {})
    body = cfg.get("body") or ""
    data = None
    if method == "POST" and body:
        data = body.encode("utf-8")
        headers.setdefault("Content-Type", "application/json")
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=15, context=_SSL_CTX) as resp:
            return resp.status, resp.read().decode("utf-8", errors="replace"), ""
    except urllib.error.HTTPError as e:
        try:
            txt = e.read().decode("utf-8", errors="replace")
        except Exception:
            txt = ""
        return e.code, txt, f"HTTP {e.code}"
    except Exception as e:  # noqa: BLE001
        return 0, "", str(e)


# ---------------------------------------------------------------------------
# Claude Code provider
# ---------------------------------------------------------------------------
def _claude_auth_path() -> Path:
    return Path(os.path.expanduser("~")) / ".claude" / ".credentials.json"


def _claude_refresh_and_fetch(cfg: dict) -> dict:
    """读取 ~/.claude/.credentials.json → 如需则刷新 token →
    调用 api.anthropic.com/api/oauth/usage 获取用量百分比。"""
    auth_path = _claude_auth_path()
    if not auth_path.exists():
        return {"ok": False, "error": f"未找到 {auth_path}（请先用 Claude Code 登录）", "status": 0, "raw": ""}
    try:
        with open(auth_path, "r", encoding="utf-8") as f:
            creds = json.load(f)
    except (json.JSONDecodeError, OSError) as e:
        return {"ok": False, "error": f"读取 credentials 失败: {e}", "status": 0, "raw": ""}

    auth = creds.get("claudeAiOauth") or {}
    access_token = auth.get("accessToken", "")
    refresh_token = auth.get("refreshToken", "")
    if not access_token and not refresh_token:
        return {"ok": False, "error": "credentials.json 中未找到 claudeAiOauth token", "status": 0, "raw": ""}

    # 判断 token 是否过期
    import time as _time
    expires_at = auth.get("expiresAt")
    token_expired = False
    if expires_at:
        token_expired = _time.time() > expires_at / 1000

    # 刷新 token（过期时或查询返回 401 时）
    def refresh():
        nonlocal access_token
        body = json.dumps({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": "https://claude.ai/oauth/claude-code-client-metadata",
        }).encode("utf-8")
        req = urllib.request.Request(
            "https://claude.ai/oauth/token",
            data=body,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(req, timeout=15) as resp:
            refreshed = json.loads(resp.read().decode("utf-8"))
        access_token = refreshed["access_token"]
        # 回写
        creds["claudeAiOauth"]["accessToken"] = access_token
        creds["claudeAiOauth"]["refreshToken"] = refreshed.get("refresh_token", refresh_token)
        creds["claudeAiOauth"]["expiresAt"] = int((refreshed.get("expires_in", 28800) + _time.time()) * 1000)
        tmp = auth_path.with_suffix(".json.tmp")
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(creds, f, ensure_ascii=False, indent=2)
            f.flush()
            os.fsync(f.fileno())
        os.replace(tmp, auth_path)

    if token_expired and refresh_token:
        try:
            refresh()
        except Exception as e:
            return {"ok": False, "error": f"token 刷新失败: {e}", "status": 0, "raw": ""}

    # 调用 /api/oauth/usage 获取用量（不消耗 quota，纯只读查询）
    def do_fetch(tok: str):
        req = urllib.request.Request(
            "https://api.anthropic.com/api/oauth/usage",
            method="GET",
            headers={
                "Authorization": f"Bearer {tok}",
                "Content-Type": "application/json",
            },
        )
        try:
            with urllib.request.urlopen(req, timeout=15) as resp:
                return resp.status, resp.read().decode("utf-8", errors="replace"), ""
        except urllib.error.HTTPError as e:
            txt = ""
            try:
                txt = e.read().decode("utf-8", errors="replace")
            except Exception:
                pass
            return e.code, txt, f"HTTP {e.code}"
        except Exception as e:
            return 0, "", str(e)

    status, text, err = do_fetch(access_token)
    # 401 → 刷新后重试
    if status == 401 and refresh_token:
        try:
            refresh()
        except Exception as e:
            return {"ok": False, "error": f"token 刷新失败: {e}", "status": status, "raw": ""}
        status, text, err = do_fetch(access_token)

    if err:
        return {"ok": False, "error": err, "status": status, "raw": text[:500]}

    try:
        usage = json.loads(text)
    except json.JSONDecodeError:
        return {"ok": False, "error": "usage 响应不是合法 JSON", "status": status, "raw": text[:500]}

    # usage 结构:
    # {five_hour: {utilization: 5.0, resets_at: "..."}, seven_day: {utilization: 49.0, resets_at: "..."}, ...}
    # utilization 已经是 0-100 的百分比
    seven_day = usage.get("seven_day") or {}
    five_hour = usage.get("five_hour") or {}

    # 优先 seven_day (weekly)，次选 five_hour (session)
    weekly_util = seven_day.get("utilization")
    session_util = five_hour.get("utilization")
    weekly_reset = seven_day.get("resets_at")

    percent_used = None
    window_label = ""
    reset_at = None
    if weekly_util is not None:
        percent_used = round(float(weekly_util), 1)
        window_label = "weekly"
        reset_at = weekly_reset
    elif session_util is not None:
        percent_used = round(float(session_util), 1)
        window_label = "session"
        reset_at = five_hour.get("resets_at")

    extra = {"subscription_type": auth.get("subscriptionType", "")}
    if window_label:
        extra["window"] = window_label
    if reset_at:
        extra["reset_at"] = reset_at
    # 额外：5h 百分比也带上（仪表盘可以选择展示）
    if session_util is not None:
        extra["session_percent"] = round(float(session_util), 1)

    return {
        "ok": True,
        "status": status,
        "result": {
            "used": None,
            "total": None,
            "percent_used": percent_used,
            "percent_semantics": "used",
            "unit": cfg.get("unit") or "%",
            "fetched_at": _now(),
            "raw_used": weekly_util if weekly_util is not None else session_util,
            "raw_total": None,
            **extra,
        },
    }


# ---------------------------------------------------------------------------
# Grok Build provider
# ---------------------------------------------------------------------------
def _grok_auth_path() -> Path:
    return Path(os.path.expanduser("~")) / ".grok" / "auth.json"


def _grok_refresh_and_fetch(cfg: dict) -> dict:
    """读取 token（优先 config，fallback ~/.grok/auth.json）→ 刷新 → 查询用量。"""
    def _grok_extractor(file_data):
        issuer_key = list(file_data.keys())[0] if file_data else None
        if not issuer_key:
            return {}
        ad = file_data[issuer_key]
        return {
            "access_token": ad.get("key", ""),
            "refresh_token": ad.get("refresh_token", ""),
            "oidc_client_id": ad.get("oidc_client_id", ""),
            "user_id": ad.get("user_id", ""),
        }

    tok_info = _oauth_get_tokens(cfg, _grok_auth_path(), _grok_extractor)
    if not tok_info:
        return {"ok": False, "error": f"未找到 OAuth token（请先从 CLI 导入或用 Grok CLI 登录）", "status": 0, "raw": ""}

    access_token = tok_info["access_token"]
    refresh_token = tok_info["refresh_token"]
    auth_data = tok_info["auth_data"]
    oidc_client_id = auth_data.get("oidc_client_id", "")
    user_id = auth_data.get("user_id", "")
    source = tok_info["source"]
    file_path = tok_info["file_path"]
    file_data = tok_info["file_data"]

    # 检查 token 过期
    import time as _time
    import base64 as _b64
    token_expired = False
    try:
        parts = access_token.split(".")
        if len(parts) >= 2:
            payload = json.loads(_b64.urlsafe_b64decode(parts[1] + "==="))
            token_expired = _time.time() > payload.get("exp", 0)
    except Exception:
        token_expired = True  # 无法解析 → 尝试刷新

    def refresh():
        nonlocal access_token
        from urllib.parse import urlencode as _ue
        body = _ue({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": oidc_client_id,
        }).encode("utf-8")
        req = urllib.request.Request(
            "https://auth.x.ai/oauth2/token",
            data=body,
            headers={"Content-Type": "application/x-www-form-urlencoded", "Accept": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(req, timeout=15) as resp:
            refreshed = json.loads(resp.read().decode("utf-8"))
        access_token = refreshed["access_token"]
        new_refresh = refreshed.get("refresh_token", refresh_token)
        expires_at = str(int((refreshed.get("expires_in", 21600) + _time.time())))
        # 回写：config 模式写回 config，file 模式写回本地文件
        if source == "config" and cfg.get("id"):
            _oauth_save_tokens_to_cfg(cfg["id"], {
                "access_token": access_token,
                "refresh_token": new_refresh,
                "expires_at": expires_at,
                "oidc_client_id": oidc_client_id,
                "user_id": user_id,
            })
        elif file_path and file_data:
            issuer_key = list(file_data.keys())[0]
            file_data[issuer_key]["key"] = access_token
            file_data[issuer_key]["refresh_token"] = new_refresh
            file_data[issuer_key]["expires_at"] = expires_at
            tmp = file_path.with_suffix(".json.tmp")
            with open(tmp, "w", encoding="utf-8") as f:
                json.dump(file_data, f, ensure_ascii=False, indent=2)
                f.flush()
                os.fsync(f.fileno())
            os.replace(tmp, file_path)

    if token_expired and refresh_token:
        try:
            refresh()
        except Exception as e:
            return {"ok": False, "error": f"token 刷新失败: {e}", "status": 0, "raw": ""}

    # 查询 billing 端点获取用量（cli-chat-proxy.grok.com/v1/billing?format=credits）
    # 需要 Cloudflare 兼容的请求方式，用 subprocess curl 绕过 TLS 指纹检测
    import subprocess as _sp

    def do_billing_request(tok: str):
        url = "https://cli-chat-proxy.grok.com/v1/billing?format=credits"
        try:
            proc = _sp.run(
                ["curl", "-s", "-w", "\n%{http_code}", url,
                 "-H", f"Authorization: Bearer {tok}",
                 "-H", f"x-userid: {user_id}",
                 "-H", "x-grok-client-mode: xai-grok-cli",
                 "-H", "Accept: application/json",
                 "--max-time", "15"],
                capture_output=True, text=True, timeout=20,
            )
            output = proc.stdout
            parts = output.rsplit("\n", 1)
            body_text = parts[0] if len(parts) > 1 else output
            code = int(parts[-1]) if len(parts) > 1 and parts[-1].isdigit() else 0
            return code, body_text, ""
        except Exception as e:
            return 0, "", str(e)

    status, text, err = do_billing_request(access_token)
    if status in (401, 403) and refresh_token:
        try:
            refresh()
        except Exception as e:
            return {"ok": False, "error": f"token 刷新失败: {e}", "status": status, "raw": text[:500]}
        status, text, err = do_billing_request(access_token)

    if err:
        return {"ok": False, "error": err, "status": status, "raw": text[:500]}

    try:
        billing = json.loads(text)
    except json.JSONDecodeError:
        return {"ok": False, "error": "billing 响应不是合法 JSON", "status": status, "raw": text[:500]}

    # billing 结构: {config: {creditUsagePercent, productUsage, prepaidBalance, billingPeriodStart, ...}}
    config = billing.get("config") or {}
    credit_pct = config.get("creditUsagePercent")
    # creditUsagePercent 缺失 = 本周期还没产生用量 = 0%
    percent_used = round(float(credit_pct), 1) if credit_pct is not None else 0.0

    # 产品级别用量
    products = []
    for pu in (config.get("productUsage") or []):
        products.append(f"{pu.get('product','')}: {pu.get('usagePercent','')}%")

    extra = {}
    prepaid = config.get("prepaidBalance")
    if prepaid and isinstance(prepaid, dict) and "val" in prepaid:
        extra["prepaid_balance"] = str(prepaid["val"])
    if config.get("billingPeriodEnd"):
        extra["billing_period_end"] = config["billingPeriodEnd"]
    if products:
        extra["products"] = "; ".join(products)

    return {
        "ok": True,
        "status": status,
        "result": {
            "used": None,
            "total": None,
            "percent_used": percent_used,
            "percent_semantics": "used",
            "unit": cfg.get("unit") or "%",
            "fetched_at": _now(),
            "raw_used": credit_pct,
            "raw_total": None,
            **extra,
        },
    }


# ---------------------------------------------------------------------------
# Grok OAuth Device Code Flow（RFC 8628）— 支持多账号独立登录
# ---------------------------------------------------------------------------
_GROK_OAUTH_CLIENT_ID = "b1a00492-073a-47ea-816f-4c329264a828"
_GROK_OAUTH_SCOPE = "openid profile email offline_access grok-cli:access api:access conversations:read conversations:write"
_GROK_DEVICE_CODE_URL = "https://auth.x.ai/oauth2/device/code"
_GROK_TOKEN_URL = "https://auth.x.ai/oauth2/token"


def _grok_device_code_start() -> dict:
    """发起 device code 流程：POST /oauth2/device/code，返回 user_code + verification_uri。"""
    from urllib.parse import urlencode as _ue
    body = _ue({"client_id": _GROK_OAUTH_CLIENT_ID, "scope": _GROK_OAUTH_SCOPE}).encode("utf-8")
    req = urllib.request.Request(
        _GROK_DEVICE_CODE_URL, data=body,
        headers={"Content-Type": "application/x-www-form-urlencoded", "Accept": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            obj = json.loads(resp.read().decode("utf-8"))
        return {"ok": True, "data": {
            "device_code": obj.get("device_code", ""),
            "user_code": obj.get("user_code", ""),
            "verification_uri": obj.get("verification_uri", ""),
            "verification_uri_complete": obj.get("verification_uri_complete", ""),
            "expires_in": obj.get("expires_in", 1800),
            "interval": obj.get("interval", 5),
        }}
    except urllib.error.HTTPError as e:
        txt = e.read().decode("utf-8", errors="replace")[:300]
        return {"ok": False, "error": f"HTTP {e.code}: {txt}"}
    except Exception as e:
        return {"ok": False, "error": str(e)}


def _grok_device_code_poll(payload: dict) -> dict:
    """轮询 token 端点，直到用户授权完成或超时。"""
    device_code = str(payload.get("device_code") or "").strip()
    if not device_code:
        return {"ok": False, "error": "缺少 device_code"}
    from urllib.parse import urlencode as _ue
    body = _ue({
        "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
        "client_id": _GROK_OAUTH_CLIENT_ID,
        "device_code": device_code,
    }).encode("utf-8")
    req = urllib.request.Request(
        _GROK_TOKEN_URL, data=body,
        headers={"Content-Type": "application/x-www-form-urlencoded", "Accept": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            obj = json.loads(resp.read().decode("utf-8"))
        if obj.get("access_token"):
            return {"ok": True, "status": "success", "tokens": {
                "access_token": obj["access_token"],
                "refresh_token": obj.get("refresh_token", ""),
                "expires_in": obj.get("expires_in", 3600),
                "id_token": obj.get("id_token", ""),
            }}
        err_code = obj.get("error", "")
        if err_code in ("authorization_pending", "slow_down"):
            return {"ok": True, "status": "pending", "error": err_code}
        return {"ok": False, "status": "error", "error": obj.get("error_description") or err_code}
    except urllib.error.HTTPError as e:
        try:
            txt = e.read().decode("utf-8", errors="replace")
            obj = json.loads(txt)
            err_code = obj.get("error", "")
            if err_code in ("authorization_pending", "slow_down"):
                return {"ok": True, "status": "pending", "error": err_code}
            return {"ok": False, "status": "error", "error": obj.get("error_description") or err_code}
        except (json.JSONDecodeError, Exception):
            return {"ok": False, "status": "error", "error": f"HTTP {e.code}"}
    except Exception as e:
        return {"ok": False, "status": "error", "error": str(e)}


# ---------------------------------------------------------------------------
# API Key 类 Provider：GLM / Kimi / MiniMax
# ---------------------------------------------------------------------------
def _apikey_http_get(url: str, api_key: str, auth_mode: str = "bearer") -> tuple[int, str, str]:
    """统一 GET 请求。auth_mode: 'bearer' → Authorization: Bearer {key}; 'raw' → Authorization: {key}"""
    auth_val = api_key if auth_mode == "raw" else f"Bearer {api_key}"
    req = urllib.request.Request(url, method="GET", headers={
        "Authorization": auth_val,
        "Accept": "application/json",
    })
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            return resp.status, resp.read().decode("utf-8", errors="replace"), ""
    except urllib.error.HTTPError as e:
        txt = ""
        try:
            txt = e.read().decode("utf-8", errors="replace")
        except Exception:
            pass
        return e.code, txt, f"HTTP {e.code}"
    except Exception as e:
        return 0, "", str(e)


def _glm_coding_fetch(cfg: dict) -> dict:
    """智谱 GLM Coding Plan: GET open.bigmodel.cn/api/monitor/usage/quota/limit
    认证：裸 API key（不加 Bearer）。返回 data.limits[] 含 TOKENS_LIMIT(5h/周) 百分比。"""
    api_key = cfg.get("api_key", "")
    if not api_key:
        return {"ok": False, "error": "缺少 API Key", "status": 0, "raw": ""}
    status, text, err = _apikey_http_get(
        "https://open.bigmodel.cn/api/monitor/usage/quota/limit", api_key, auth_mode="raw")
    if err:
        return {"ok": False, "error": err, "status": status, "raw": text[:500]}
    try:
        obj = json.loads(text)
    except json.JSONDecodeError:
        return {"ok": False, "error": "响应不是合法 JSON", "status": status, "raw": text[:500]}
    # 智谱错误响应：HTTP 200 但 body 含 {code:401, success:false}
    if not obj.get("success", True) or (isinstance(obj.get("code"), int) and obj.get("code") != 200):
        return {"ok": False, "error": obj.get("msg") or "API 返回错误", "status": status, "raw": text[:500]}
    # limits 数组，每项有 type / unit / percentage / nextResetTime
    # unit=3 → 5h token 窗口, unit=6 → 周配额
    # 注：智谱 API 实际字段为 "percentage" 和 "nextResetTime"
    limits = (obj.get("data") or {}).get("limits") or []
    level = (obj.get("data") or {}).get("level")
    weekly_pct = None
    session_pct = None
    reset_at = None
    for lim in limits:
        unit = lim.get("unit")
        pct = _parse_numeric(lim.get("percentage"))
        if unit == 6 and pct is not None:  # 周配额优先
            weekly_pct = pct
            reset_at = lim.get("nextResetTime") or lim.get("resetTime") or lim.get("reset_time")
        elif unit == 3 and pct is not None:  # 5h 窗口
            session_pct = pct
            if not reset_at:
                reset_at = lim.get("nextResetTime") or lim.get("resetTime") or lim.get("reset_time")
    percent_used = weekly_pct if weekly_pct is not None else session_pct
    extra = {}
    if level:
        extra["level"] = str(level)
    if session_pct is not None:
        extra["session_percent"] = session_pct
    if reset_at:
        extra["reset_at"] = str(reset_at)
    return {
        "ok": True, "status": status,
        "result": {
            "used": None, "total": None,
            "percent_used": percent_used, "percent_semantics": "used",
            "unit": cfg.get("unit") or "%", "fetched_at": _now(),
            "raw_used": None, "raw_total": None, **extra,
        },
    }


def _kimi_coding_fetch(cfg: dict) -> dict:
    """Kimi for Coding: GET api.kimi.com/coding/v1/usages
    认证：Bearer {key}。返回结构：
      - limits[]: 数组，每项含 detail.{limit, remaining, resetTime}（5h 滑动窗口）
      - usage: {limit, remaining, resetTime}（周配额）
    """
    api_key = cfg.get("api_key", "")
    if not api_key:
        return {"ok": False, "error": "缺少 API Key", "status": 0, "raw": ""}
    status, text, err = _apikey_http_get(
        "https://api.kimi.com/coding/v1/usages", api_key, auth_mode="bearer")
    if err:
        return {"ok": False, "error": err, "status": status, "raw": text[:500]}
    try:
        obj = json.loads(text)
    except json.JSONDecodeError:
        return {"ok": False, "error": "响应不是合法 JSON", "status": status, "raw": text[:500]}

    percent_used = None   # 周配额已用百分比（优先展示）
    session_pct = None    # 5h 窗口已用百分比
    reset_at = None
    used_val = None
    total_val = None

    # 5h 窗口：limits[].detail.{limit, remaining, resetTime}
    limits = obj.get("limits") if isinstance(obj.get("limits"), list) else []
    for item in limits:
        detail = item.get("detail") if isinstance(item, dict) else None
        if not isinstance(detail, dict):
            continue
        lim = _parse_numeric(detail.get("limit"))
        rem = _parse_numeric(detail.get("remaining"))
        if lim is not None and lim > 0 and rem is not None:
            session_pct = round((1 - rem / lim) * 100, 1)
            if not reset_at:
                reset_at = detail.get("resetTime") or detail.get("reset_time")
            break  # 取第一个有效窗口

    # 周配额：usage.{limit, remaining, resetTime}
    usage = obj.get("usage") if isinstance(obj.get("usage"), dict) else {}
    if usage:
        wlim = _parse_numeric(usage.get("limit"))
        wrem = _parse_numeric(usage.get("remaining"))
        if wlim is not None and wlim > 0 and wrem is not None:
            percent_used = round((1 - wrem / wlim) * 100, 1)
            used_val = round(wlim - wrem, 1)
            total_val = wlim
            reset_at = usage.get("resetTime") or usage.get("reset_time") or reset_at

    # 降级：无周配额则用 5h 窗口
    if percent_used is None:
        percent_used = session_pct

    extra = {}
    if session_pct is not None:
        extra["session_percent"] = session_pct
    if reset_at:
        extra["reset_at"] = str(reset_at)
    return {
        "ok": True, "status": status,
        "result": {
            "used": used_val, "total": total_val,
            "percent_used": percent_used, "percent_semantics": "used",
            "unit": cfg.get("unit") or "%", "fetched_at": _now(),
            "raw_used": used_val, "raw_total": total_val, **extra,
        },
    }


def _minimax_coding_fetch(cfg: dict) -> dict:
    """MiniMax Coding Plan: GET api.minimaxi.com/v1/api/openplatform/coding_plan/remains
    认证：Bearer {key}（Coding Plan 专用订阅 key，非普通按量 key）。
    返回 model_remains[]，取 model_name=="general" 条目：
      - current_interval_remaining_percent: 5h 窗口剩余百分比
      - current_weekly_remaining_percent / current_weekly_status: 周配额（status==1 才激活）
    接口给的是「剩余百分比」，需反转为「已用百分比」。
    """
    api_key = cfg.get("api_key", "")
    if not api_key:
        return {"ok": False, "error": "缺少 API Key", "status": 0, "raw": ""}
    status, text, err = _apikey_http_get(
        "https://api.minimaxi.com/v1/api/openplatform/coding_plan/remains", api_key, auth_mode="bearer")
    if err:
        return {"ok": False, "error": err, "status": status, "raw": text[:500]}
    try:
        obj = json.loads(text)
    except json.JSONDecodeError:
        return {"ok": False, "error": "响应不是合法 JSON", "status": status, "raw": text[:500]}
    # MiniMax 错误响应：HTTP 200 但 body 含 {base_resp: {status_code:1004, ...}}
    base_resp = obj.get("base_resp") if isinstance(obj, dict) else None
    if base_resp and isinstance(base_resp.get("status_code"), int) and base_resp.get("status_code") != 0:
        msg = base_resp.get("status_msg") or "API 返回错误"
        if base_resp.get("status_code") == 1004:
            msg = "认证失败/cookie 缺失（1004）：请确认使用的是 Coding Plan 专用订阅 key（非普通 API key）"
        return {"ok": False, "error": msg, "status": status, "raw": text[:500]}
    # 找到 model_name == "general" 的条目（跳过 video 等非编程模型）
    model_remains = obj.get("model_remains") if isinstance(obj.get("model_remains"), list) else []
    general = next((m for m in model_remains
                    if isinstance(m, dict) and m.get("model_name") == "general"), None)
    percent_used = None
    session_pct = None
    reset_at = None
    if general:
        # 5h 窗口
        remain5 = _parse_numeric(
            general.get("current_interval_remaining_percent")
            or general.get("currentIntervalRemainingPercent"))
        if remain5 is not None:
            session_pct = round(100 - remain5, 1)
            reset_at = general.get("end_time") or general.get("endTime")
        # 周配额（仅 status==1 时激活；status==3 表示该套餐无周限额，跳过）
        weekly_status = general.get("current_weekly_status") or general.get("currentWeeklyStatus")
        if weekly_status == 1:
            remain_w = _parse_numeric(
                general.get("current_weekly_remaining_percent")
                or general.get("currentWeeklyRemainingPercent"))
            if remain_w is not None:
                percent_used = round(100 - remain_w, 1)
                reset_at = general.get("weekly_end_time") or general.get("weeklyEndTime") or reset_at
    # 降级：无周配额则用 5h 窗口
    if percent_used is None:
        percent_used = session_pct
    extra = {}
    if session_pct is not None:
        extra["session_percent"] = session_pct
    if reset_at:
        extra["reset_at"] = str(reset_at)
    return {
        "ok": True, "status": status,
        "result": {
            "used": None, "total": None,
            "percent_used": percent_used, "percent_semantics": "used",
            "unit": cfg.get("unit") or "%", "fetched_at": _now(),
            "raw_used": None, "raw_total": None, **extra,
        },
    }


# 注册 provider fetchers
PROVIDER_FETCHERS["chatgpt_codex"] = _codex_refresh_and_fetch
PROVIDER_FETCHERS["claude_code"] = _claude_refresh_and_fetch
PROVIDER_FETCHERS["grok_build"] = _grok_refresh_and_fetch
PROVIDER_FETCHERS["glm_coding"] = _glm_coding_fetch
PROVIDER_FETCHERS["kimi_coding"] = _kimi_coding_fetch
PROVIDER_FETCHERS["minimax_coding"] = _minimax_coding_fetch


def _deepseek_balance_fetch(cfg: dict) -> dict:
    """DeepSeek: GET https://api.deepseek.com/user/balance
    认证：Bearer {key}。返回 is_available + balance_infos[]（total_balance 为字符串）。
    DeepSeek 是按量 API，没有用量配额百分比，只返回余额。
    """
    api_key = cfg.get("api_key", "")
    if not api_key:
        return {"ok": False, "error": "缺少 API Key", "status": 0, "raw": ""}
    status, text, err = _apikey_http_get(
        "https://api.deepseek.com/user/balance", api_key, auth_mode="bearer")
    if err:
        return {"ok": False, "error": err, "status": status, "raw": text[:500]}
    try:
        obj = json.loads(text)
    except json.JSONDecodeError:
        return {"ok": False, "error": "响应不是合法 JSON", "status": status, "raw": text[:500]}
    is_available = obj.get("is_available")
    infos = obj.get("balance_infos") or []
    # 取第一个有效条目
    balance_str = ""
    currency = ""
    granted_str = ""
    topped_up_str = ""
    if infos and isinstance(infos[0], dict):
        balance_str = infos[0].get("total_balance", "")
        currency = infos[0].get("currency", "")
        granted_str = infos[0].get("granted_balance", "")
        topped_up_str = infos[0].get("topped_up_balance", "")
    extra = {}
    if is_available is not None:
        extra["is_available"] = bool(is_available)
    if granted_str:
        extra["granted_balance"] = str(granted_str)
    if topped_up_str:
        extra["topped_up_balance"] = str(topped_up_str)
    # 余额作为 used/total 展示（语义上 total=余额，used=None）
    # 前端会显示 used / total，这里把余额放在 total
    balance_val = _parse_numeric(balance_str)
    return {
        "ok": True, "status": status,
        "result": {
            "used": None, "total": balance_val,
            "percent_used": None, "percent_semantics": "remaining",
            "unit": currency or cfg.get("unit") or "",
            "fetched_at": _now(),
            "raw_used": None, "raw_total": balance_str,
            **extra,
        },
    }


def _gemini_models_fetch(cfg: dict) -> dict:
    """Gemini: GET https://generativelanguage.googleapis.com/v1beta/models
    认证：x-goog-api-key header。验证 key 有效性并列出可用模型。
    Gemini 订阅没有公开的用量配额查询 API，只做 key 验证 + 模型列表。
    """
    api_key = cfg.get("api_key", "")
    if not api_key:
        return {"ok": False, "error": "缺少 API Key", "status": 0, "raw": ""}
    url = "https://generativelanguage.googleapis.com/v1beta/models"
    req = urllib.request.Request(url, method="GET", headers={
        "x-goog-api-key": api_key,
        "Accept": "application/json",
    })
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            status = resp.status
            text = resp.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as e:
        txt = ""
        try:
            txt = e.read().decode("utf-8", errors="replace")
        except Exception:
            pass
        # 解析 Google 错误格式
        err_msg = f"HTTP {e.code}"
        try:
            err_obj = json.loads(txt)
            if "error" in err_obj:
                err_msg = err_obj["error"].get("message", err_msg)
        except (json.JSONDecodeError, KeyError):
            pass
        return {"ok": False, "error": err_msg, "status": e.code, "raw": txt[:500]}
    except Exception as e:
        return {"ok": False, "error": str(e), "status": 0, "raw": ""}
    try:
        obj = json.loads(text)
    except json.JSONDecodeError:
        return {"ok": False, "error": "响应不是合法 JSON", "status": status, "raw": text[:500]}
    models = obj.get("models") or []
    model_names = []
    for m in models:
        name = m.get("name", "")
        # name 格式 "models/gemini-3.5-flash"，只取模型名
        if name.startswith("models/"):
            name = name[len("models/"):]
        if name:
            model_names.append(name)
    extra = {}
    if model_names:
        extra["model_count"] = len(model_names)
        extra["models"] = ", ".join(model_names[:10]) + ("..." if len(model_names) > 10 else "")
    return {
        "ok": True, "status": status,
        "result": {
            "used": None, "total": None,
            "percent_used": None, "percent_semantics": "remaining",
            "unit": cfg.get("unit") or "",
            "fetched_at": _now(),
            "raw_used": None, "raw_total": None,
            **extra,
        },
    }


# 注册 provider fetchers（追加）
PROVIDER_FETCHERS["deepseek_balance"] = _deepseek_balance_fetch
PROVIDER_FETCHERS["gemini_models"] = _gemini_models_fetch


def _dashscope_balance_fetch(cfg: dict) -> dict:
    """阿里百炼 DashScope: GET https://dashscope.aliyuncs.com/api/v1/quotas
    认证：Bearer {key}。返回模型配额列表（RPM/TPM 限制）。
    DashScope API key 无法查询账户余额（余额查询需阿里云 BSS OpenAPI + AccessKey），
    这里用 /api/v1/quotas 验证 key 有效性并返回配额摘要。
    """
    api_key = cfg.get("api_key", "")
    if not api_key:
        return {"ok": False, "error": "缺少 API Key", "status": 0, "raw": ""}
    status, text, err = _apikey_http_get(
        "https://dashscope.aliyuncs.com/api/v1/quotas",
        api_key, auth_mode="bearer")
    if err:
        return {"ok": False, "error": err, "status": status, "raw": text[:500]}
    try:
        obj = json.loads(text)
    except json.JSONDecodeError:
        return {"ok": False, "error": "响应不是合法 JSON", "status": status, "raw": text[:500]}
    # DashScope 错误响应：{code: "...", message: "...", success: false, request_id: "..."}
    if obj.get("success") is False:
        return {"ok": False, "error": obj.get("message") or "API 返回错误", "status": status, "raw": text[:500]}
    output = obj.get("output") or {}
    quotas = output.get("quotas") or []
    # 提取有 usage_limit 的模型配额摘要
    model_summary = []
    for q in quotas:
        model = q.get("model", "")
        ml = q.get("model_limit") or {}
        usage_limit = ml.get("usage_limit")
        if model and usage_limit:
            model_summary.append(f"{model}:{usage_limit}{ml.get('usage_limit_field','')}/{ml.get('usage_limit_period','')}d")
    extra = {}
    if model_summary:
        extra["model_count"] = len(model_summary)
        extra["models"] = ", ".join(model_summary[:8]) + ("..." if len(model_summary) > 8 else "")
    return {
        "ok": True, "status": status,
        "result": {
            "used": None, "total": None,
            "percent_used": None, "percent_semantics": "remaining",
            "unit": cfg.get("unit") or "",
            "fetched_at": _now(),
            "raw_used": None, "raw_total": None,
            **extra,
        },
    }


PROVIDER_FETCHERS["dashscope_balance"] = _dashscope_balance_fetch


def _usage_test(payload: dict) -> dict:
    """测试一次抓取但不写缓存。payload 可以是未保存的临时配置。"""
    provider = str(payload.get("provider") or "").strip()
    cfg = {
        "provider": provider,
        "api_key": str(payload.get("api_key") or "").strip(),
        "url": str(payload.get("url") or "").strip(),
        "method": str(payload.get("method") or "GET").strip().upper(),
        "headers": payload.get("headers") or {},
        "body": str(payload.get("body") or ""),
        "jsonpath_used": str(payload.get("jsonpath_used") or "").strip(),
        "jsonpath_total": str(payload.get("jsonpath_total") or "").strip(),
        "unit": str(payload.get("unit") or "").strip(),
    }
    return _usage_do_fetch(cfg)


def _usage_do_fetch(cfg: dict) -> dict:
    # 内置 provider 走专用路径
    provider = cfg.get("provider") or ""
    if provider and provider in PROVIDER_FETCHERS:
        return PROVIDER_FETCHERS[provider](cfg)
    status, text, err = _usage_http(cfg)
    if err:
        return {"ok": False, "error": err, "status": status, "raw": text[:500]}
    try:
        obj = json.loads(text)
    except json.JSONDecodeError:
        return {"ok": False, "error": "响应不是合法 JSON", "status": status, "raw": text[:500]}
    used_raw = _jp_eval(cfg.get("jsonpath_used", ""), obj) if cfg.get("jsonpath_used") else None
    total_raw = _jp_eval(cfg.get("jsonpath_total", ""), obj) if cfg.get("jsonpath_total") else None
    used = _parse_numeric(used_raw)
    total = _parse_numeric(total_raw)
    # 统一计算 percent_used（已用百分比 0-100）
    percent_used = None
    if used is not None and total is not None and total > 0:
        percent_used = round(used / total * 100, 1)
    elif used is not None:
        # 只有单个数值 → 假定它就是已用百分比
        percent_used = round(used, 1)
        used = None
    return {
        "ok": True,
        "status": status,
        "result": {
            "used": used,
            "total": total,
            "percent_used": percent_used,
            "percent_semantics": "used",
            "unit": cfg.get("unit", ""),
            "fetched_at": _now(),
            "raw_used": used_raw,
            "raw_total": total_raw,
        },
    }


def _usage_load_cache() -> dict:
    if not USAGE_CACHE_FILE.exists():
        return {}
    try:
        with open(USAGE_CACHE_FILE, "r", encoding="utf-8") as f:
            return json.load(f)
    except (json.JSONDecodeError, OSError):
        return {}


def _usage_save_cache(cache: dict) -> None:
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    tmp = USAGE_CACHE_FILE.with_suffix(".json.tmp")
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(cache, f, ensure_ascii=False)
        f.flush()
        os.fsync(f.fileno())
    os.replace(tmp, USAGE_CACHE_FILE)


_usage_stop = threading.Event()


def _usage_scheduler():
    """每 30s 扫一遍配置，按 interval_min 决定是否抓取。"""
    while not _usage_stop.is_set():
        try:
            data = load_data()
            cache = _usage_load_cache()
            now = datetime.now(timezone.utc)
            changed = False
            for c in data.get("usage_configs", []):
                if not c.get("enabled", True):
                    continue
                interval = max(int(c.get("interval_min") or 60), 1)
                last = cache.get(c["id"], {}).get("fetched_at") if cache.get(c["id"]) else None
                due = True
                if last:
                    try:
                        lt = datetime.fromisoformat(last)
                        if (now - lt).total_seconds() < interval * 60:
                            due = False
                    except ValueError:
                        pass
                if not due:
                    continue
                res = _usage_do_fetch(c)
                if res.get("ok"):
                    cache[c["id"]] = res["result"]
                    changed = True
                    with _lock:
                        d2 = load_data()
                        for cc in d2["usage_configs"]:
                            if cc["id"] == c["id"]:
                                cc["last_run_at"] = _now()
                                break
                        save_data(d2)
            if changed:
                _usage_save_cache(cache)
        except Exception:  # noqa: BLE001
            pass
        _usage_stop.wait(30)


def main():
    ap = argparse.ArgumentParser(description="账号管家本地服务")
    ap.add_argument("--port", type=int, default=8756)
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--no-browser", action="store_true")
    args = ap.parse_args()

    DATA_DIR.mkdir(parents=True, exist_ok=True)
    BACKUP_DIR.mkdir(parents=True, exist_ok=True)
    if not DATA_FILE.exists():
        save_data(json.loads(json.dumps(EMPTY_DATA)))

    srv = ThreadingHTTPServer((args.host, args.port), Handler)
    url = f"http://{args.host}:{args.port}/"
    # 启动用量定时抓取线程
    sched_thread = threading.Thread(target=_usage_scheduler, name="usage-scheduler", daemon=True)
    sched_thread.start()
    print(f"账号管家已启动: {url}")
    print(f"数据文件: {DATA_FILE}")
    print("Ctrl+C 停止服务")
    if not args.no_browser:
        threading.Timer(0.6, lambda: webbrowser.open(url)).start()
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        _usage_stop.set()
        print("\n已停止")


if __name__ == "__main__":
    main()
