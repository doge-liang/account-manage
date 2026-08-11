# -*- coding: utf-8 -*-
"""
KeePassXC AI Keys 助手：列表 / 搜索 / 取 key / 写入 / 导出环境变量。

依赖: pip install -r requirements.txt
默认库: keepassxc/vaults/ai-keys.kdbx

示例:
  python key_helper.py list
  python key_helper.py search openai
  python key_helper.py get openai-server-prod
  python key_helper.py export-env openai-server-prod
  python key_helper.py add --vendor OpenAI --title my-new-key
  python key_helper.py set-password anthropic-dev
  python key_helper.py show openai-admin-readonly
"""

from __future__ import annotations

import argparse
import getpass
import os
import sys
from pathlib import Path

from pykeepass import PyKeePass
from pykeepass.exceptions import CredentialsError

ROOT = Path(__file__).resolve().parent
DEFAULT_VAULT = Path(
    os.environ.get("AI_KEYS_KDBX", str(ROOT / "vaults" / "ai-keys.kdbx"))
)

META_TITLES = {"_README"}


def open_db(path: Path, password: str | None = None) -> PyKeePass:
    if not path.exists():
        raise FileNotFoundError(
            f"数据库不存在: {path}\n请先运行: python init_vault.py"
        )
    if password is None:
        password = getpass.getpass("主密码: ")
    try:
        return PyKeePass(str(path), password=password)
    except CredentialsError as e:
        raise SystemExit("主密码错误") from e


def _ai_root(kp: PyKeePass):
    g = kp.find_groups(name="AI Keys", first=True)
    if g is None:
        raise SystemExit("库中缺少分组「AI Keys」，请用 init_vault.py 重建或手动创建")
    return g


def _vendor_group(kp: PyKeePass, vendor: str, create: bool = False):
    root = _ai_root(kp)
    g = kp.find_groups(name=vendor, group=root, first=True)
    if g is None and create:
        g = kp.add_group(root, vendor)
    return g


def _iter_key_entries(kp: PyKeePass):
    root = _ai_root(kp)
    for e in root.entries:
        if e.title not in META_TITLES:
            yield e
    for g in root.subgroups:
        for e in kp.find_entries(group=g, recursive=True):
            if e.title not in META_TITLES:
                yield e


def _find_by_title(kp: PyKeePass, title: str):
    matches = [e for e in _iter_key_entries(kp) if e.title == title]
    if not matches:
        # 也允许用 key_id
        matches = [
            e
            for e in _iter_key_entries(kp)
            if (e.custom_properties or {}).get("key_id") == title
        ]
    if not matches:
        raise SystemExit(f"未找到条目: {title}")
    if len(matches) > 1:
        paths = [_path_str(e) for e in matches]
        raise SystemExit(f"标题重复，请改名保证唯一: {paths}")
    return matches[0]


def _prop(entry, key: str, default: str = "") -> str:
    return (entry.custom_properties or {}).get(key, default) or default


def _path_str(obj) -> str:
    """pykeepass 的 path 可能是 list 或 str。"""
    path = getattr(obj, "path", obj)
    if isinstance(path, (list, tuple)):
        return "/".join(str(p) for p in path)
    return str(path) if path else ""


def _group_name(entry) -> str:
    # entry.group.path like ["AI Keys", "OpenAI"]
    path = getattr(entry.group, "path", None) if entry.group else None
    if isinstance(path, (list, tuple)):
        parts = [str(p) for p in path if p]
    else:
        parts = [p for p in str(path or "").split("/") if p]
    return parts[-1] if parts else ""


def cmd_list(kp: PyKeePass, args) -> int:
    rows = []
    for e in _iter_key_entries(kp):
        if args.status and _prop(e, "status", "active") != args.status:
            continue
        if args.vendor and _group_name(e).lower() != args.vendor.lower():
            continue
        rows.append(
            {
                "title": e.title or "",
                "key_id": _prop(e, "key_id"),
                "vendor": _group_name(e),
                "last4": _prop(e, "last4"),
                "status": _prop(e, "status", "active"),
                "purpose": _prop(e, "purpose") or (e.notes or "")[:24],
                "has_secret": "yes" if (e.password or "").strip() else "NO",
            }
        )
    rows.sort(key=lambda r: (r["vendor"], r["title"]))

    if not rows:
        print("(无条目)")
        return 0

    headers = ["title", "key_id", "vendor", "last4", "status", "has_secret", "purpose"]
    widths = {h: len(h) for h in headers}
    for r in rows:
        for h in headers:
            widths[h] = max(widths[h], len(str(r[h])))

    def fmt(r):
        return "  ".join(str(r[h]).ljust(widths[h]) for h in headers)

    print(fmt({h: h for h in headers}))
    print(fmt({h: "-" * widths[h] for h in headers}))
    for r in rows:
        print(fmt(r))
    print(f"\n共 {len(rows)} 条")
    return 0


def cmd_search(kp: PyKeePass, args) -> int:
    q = args.query.lower()
    hits = []
    for e in _iter_key_entries(kp):
        blob = " ".join(
            [
                e.title or "",
                e.username or "",
                e.url or "",
                e.notes or "",
                _group_name(e),
                " ".join(e.tags or []),
                " ".join(f"{k}={v}" for k, v in (e.custom_properties or {}).items()),
            ]
        ).lower()
        if q in blob:
            hits.append(e)
    if not hits:
        print("无匹配")
        return 0
    for e in hits:
        print(
            f"{_path_str(e)}\t last4={_prop(e,'last4')}\t status={_prop(e,'status','active')}"
        )
    return 0


def cmd_show(kp: PyKeePass, args) -> int:
    e = _find_by_title(kp, args.title)
    print(f"路径:     {_path_str(e)}")
    print(f"Title:    {e.title}")
    print(f"Username: {e.username}")
    print(f"URL:      {e.url}")
    print(f"Tags:     {', '.join(e.tags or [])}")
    print(f"Password: {'******' if (e.password or '').strip() else '(空)'}")
    print("自定义属性:")
    for k, v in sorted((e.custom_properties or {}).items()):
        print(f"  {k} = {v}")
    if e.notes:
        print("Notes:")
        print(e.notes)
    print()
    print(f"台账引用: KeePassXC: {_path_str(e)}")
    return 0


def cmd_get(kp: PyKeePass, args) -> int:
    e = _find_by_title(kp, args.title)
    secret = e.password or ""
    if not secret.strip():
        raise SystemExit(f"条目「{e.title}」Password 为空，请先 set-password 或在 KeePassXC 中填写")
    # 仅输出密钥本身，便于管道: $env:KEY = python key_helper.py get xxx
    sys.stdout.write(secret)
    if sys.stdout.isatty():
        sys.stdout.write("\n")
    return 0


def cmd_export_env(kp: PyKeePass, args) -> int:
    e = _find_by_title(kp, args.title)
    secret = e.password or ""
    if not secret.strip():
        raise SystemExit(f"条目「{e.title}」Password 为空")
    var = args.var or _prop(e, "env_var") or _guess_env_var(e.title or "")
    shell = args.shell or _detect_shell()

    # 转义
    if shell == "powershell":
        # 单引号字符串，内部 ' → ''
        esc = secret.replace("'", "''")
        line = f"$env:{var} = '{esc}'"
    elif shell == "cmd":
        line = f"set {var}={secret}"
    else:
        # bash: 用单引号
        esc = secret.replace("'", "'\"'\"'")
        line = f"export {var}='{esc}'"

    if args.print_only:
        print(line)
    else:
        print(line)
        print(f"# 已生成（未自动执行）。PowerShell 可: Invoke-Expression (python key_helper.py export-env {e.title})", file=sys.stderr)
    return 0


def _guess_env_var(title: str) -> str:
    t = title.upper().replace("-", "_")
    if "OPENAI" in t:
        return "OPENAI_API_KEY"
    if "ANTHROPIC" in t or "CLAUDE" in t:
        return "ANTHROPIC_API_KEY"
    if "GEMINI" in t or "GOOGLE" in t:
        return "GOOGLE_API_KEY"
    if "DEEPSEEK" in t:
        return "DEEPSEEK_API_KEY"
    return "API_KEY"


def _detect_shell() -> str:
    if os.environ.get("PSModulePath") and sys.platform == "win32":
        return "powershell"
    if sys.platform == "win32":
        return "powershell"
    return "bash"


def cmd_set_password(kp: PyKeePass, args) -> int:
    e = _find_by_title(kp, args.title)
    if args.password:
        secret = args.password
    else:
        secret = getpass.getpass(f"新 API Key ({e.title}): ")
        again = getpass.getpass("再输一次: ")
        if secret != again:
            raise SystemExit("两次不一致")
    if not secret.strip():
        raise SystemExit("不能为空")
    e.password = secret
    last4 = secret[-4:] if len(secret) >= 4 else secret
    e.set_custom_property("last4", last4)
    if args.status:
        e.set_custom_property("status", args.status)
    kp.save()
    print(f"已更新 Password，last4={last4}")
    print(f"请同步 Excel「末四位」= {last4}")
    print(f"密钥存放处: KeePassXC: {_path_str(e)}")
    return 0


def cmd_add(kp: PyKeePass, args) -> int:
    vendor = args.vendor
    title = args.title
    group = _vendor_group(kp, vendor, create=True)
    existing = kp.find_entries(title=title, group=_ai_root(kp), recursive=True, first=True)
    if existing:
        raise SystemExit(f"已存在同名条目: {_path_str(existing)}")

    if args.password:
        secret = args.password
    else:
        secret = getpass.getpass("API Key（可回车稍后填）: ")

    username = args.account_id or ""
    url = args.url or ""
    notes = args.notes or ""
    tags = [t.strip() for t in (args.tags or "").split(",") if t.strip()]

    entry = kp.add_entry(
        group,
        title=title,
        username=username,
        password=secret or "",
        url=url,
        notes=notes,
        tags=tags,
    )

    last4 = ""
    if secret and len(secret) >= 4:
        last4 = secret[-4:]
    elif args.last4:
        last4 = args.last4

    props = {
        "key_id": args.key_id or "",
        "account_id": args.account_id or "",
        "last4": last4,
        "permission": args.permission or "full",
        "can_query_usage": args.can_query_usage or "未知",
        "purpose": args.purpose or "",
        "deploy": args.deploy or "",
        "created": args.created or "",
        "rotate_by": args.rotate_by or "",
        "status": args.status or "active",
        "env_var": args.env_var or _guess_env_var(title),
    }
    for k, v in props.items():
        if v:
            entry.set_custom_property(k, v)

    kp.save()
    print(f"已添加: {_path_str(entry)}")
    print(f"Excel 密钥存放处: KeePassXC: {_path_str(entry)}")
    if last4:
        print(f"末四位: {last4}")
    if not secret:
        print("Password 为空，稍后: python key_helper.py set-password " + title)
    return 0


def cmd_path(args) -> int:
    print(DEFAULT_VAULT.resolve())
    print("存在" if DEFAULT_VAULT.exists() else "不存在（请 init_vault.py）")
    return 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description="AI Keys KeePass 助手")
    p.add_argument(
        "--vault",
        type=Path,
        default=DEFAULT_VAULT,
        help="kdbx 路径，或设环境变量 AI_KEYS_KDBX",
    )
    sub = p.add_subparsers(dest="cmd", required=True)

    s = sub.add_parser("list", help="列出所有 key 条目")
    s.add_argument("--vendor", help="按厂商分组过滤，如 OpenAI")
    s.add_argument("--status", help="按 status 过滤，如 active")
    s.set_defaults(func=lambda kp, a: cmd_list(kp, a))

    s = sub.add_parser("search", help="全文搜索")
    s.add_argument("query")
    s.set_defaults(func=lambda kp, a: cmd_search(kp, a))

    s = sub.add_parser("show", help="显示元数据（不打印完整 key）")
    s.add_argument("title", help="密钥别名或 key_id")
    s.set_defaults(func=lambda kp, a: cmd_show(kp, a))

    s = sub.add_parser("get", help="输出完整 Password（供管道使用）")
    s.add_argument("title")
    s.set_defaults(func=lambda kp, a: cmd_get(kp, a))

    s = sub.add_parser("export-env", help="生成设置环境变量的语句")
    s.add_argument("title")
    s.add_argument("--var", help="环境变量名，默认读 env_var 属性")
    s.add_argument(
        "--shell",
        choices=["powershell", "bash", "cmd"],
        help="默认按系统猜测",
    )
    s.add_argument("--print-only", action="store_true")
    s.set_defaults(func=lambda kp, a: cmd_export_env(kp, a))

    s = sub.add_parser("set-password", help="设置/更新 API Key")
    s.add_argument("title")
    s.add_argument("--password", help="不推荐：会进 shell 历史；默认交互输入")
    s.add_argument("--status", help="同时改 status")
    s.set_defaults(func=lambda kp, a: cmd_set_password(kp, a))

    s = sub.add_parser("add", help="新增条目")
    s.add_argument("--vendor", required=True, help="厂商分组，如 OpenAI")
    s.add_argument("--title", required=True, help="密钥别名，与 Excel 一致")
    s.add_argument("--key-id", dest="key_id")
    s.add_argument("--account-id", dest="account_id")
    s.add_argument("--url")
    s.add_argument("--notes")
    s.add_argument("--tags", help="逗号分隔")
    s.add_argument("--permission", default="full")
    s.add_argument("--can-query-usage", dest="can_query_usage", default="未知")
    s.add_argument("--purpose")
    s.add_argument("--deploy")
    s.add_argument("--created")
    s.add_argument("--rotate-by", dest="rotate_by")
    s.add_argument("--status", default="active")
    s.add_argument("--env-var", dest="env_var")
    s.add_argument("--last4")
    s.add_argument("--password", help="不推荐写在命令行")
    s.set_defaults(func=lambda kp, a: cmd_add(kp, a))

    s = sub.add_parser("path", help="显示当前数据库路径")
    s.set_defaults(func=None)

    return p


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    vault = args.vault.resolve()

    if args.cmd == "path":
        # 临时改全局展示
        global DEFAULT_VAULT
        DEFAULT_VAULT = vault
        return cmd_path(args)

    kp = open_db(vault)
    return args.func(kp, args)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        print("\n已取消", file=sys.stderr)
        raise SystemExit(130)
