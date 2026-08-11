# -*- coding: utf-8 -*-
"""
初始化 AI Keys 的 KeePassXC 数据库（.kdbx）。

用法:
  python init_vault.py
  python init_vault.py --path vaults/ai-keys.kdbx
  python init_vault.py --force   # 覆盖已有库（危险）
"""

from __future__ import annotations

import argparse
import getpass
import os
import sys
from pathlib import Path

from pykeepass import create_database

ROOT = Path(__file__).resolve().parent
DEFAULT_VAULT = ROOT / "vaults" / "ai-keys.kdbx"

# 厂商分组（与当前个人资产对齐，可按需增删）
VENDOR_GROUPS = [
    "OpenAI",
    "Anthropic",
    "xAI",
    "Google",
    "DeepSeek",
    "Moonshot",
    "Zhipu",
    "MiniMax",
    "Alibaba",
    "ByteDance",
    "Other",
    "_Revoked",
]

# 与盘点表 KEY-001…KEY-009 对齐的占位条目（Password 为空，需 set-password 填入）
SAMPLE_ENTRIES = [
    {
        "group": "OpenAI",
        "title": "chatgpt-max-20x",
        "username": "ACC-001",
        "password": "",
        "url": "https://chatgpt.com",
        "notes": "ChatGPT Max 20x Coding Plan API_KEY\n台账: KEY-001",
        "tags": ["coding_plan", "api"],
        "props": {
            "key_id": "KEY-001",
            "account_id": "ACC-001",
            "permission": "plan",
            "purpose": "ChatGPT Max 20x",
            "status": "active",
            "env_var": "OPENAI_API_KEY",
        },
    },
    {
        "group": "Anthropic",
        "title": "claude-max-20x",
        "username": "ACC-002",
        "password": "",
        "url": "https://claude.ai",
        "notes": "Claude Max 20x Coding Plan API_KEY\n台账: KEY-002",
        "tags": ["coding_plan", "api"],
        "props": {
            "key_id": "KEY-002",
            "account_id": "ACC-002",
            "permission": "plan",
            "purpose": "Claude Max 20x",
            "status": "active",
            "env_var": "ANTHROPIC_API_KEY",
        },
    },
    {
        "group": "xAI",
        "title": "grok-super-1",
        "username": "ACC-003",
        "password": "",
        "url": "https://grok.com",
        "notes": "Grok Super #1 Coding Plan API_KEY\n台账: KEY-003",
        "tags": ["coding_plan", "api"],
        "props": {
            "key_id": "KEY-003",
            "account_id": "ACC-003",
            "permission": "plan",
            "purpose": "Grok Super #1",
            "status": "active",
            "env_var": "XAI_API_KEY",
        },
    },
    {
        "group": "xAI",
        "title": "grok-super-2",
        "username": "ACC-004",
        "password": "",
        "url": "https://grok.com",
        "notes": "Grok Super #2 Coding Plan API_KEY\n台账: KEY-004",
        "tags": ["coding_plan", "api"],
        "props": {
            "key_id": "KEY-004",
            "account_id": "ACC-004",
            "permission": "plan",
            "purpose": "Grok Super #2",
            "status": "active",
            "env_var": "XAI_API_KEY_2",
        },
    },
    {
        "group": "Zhipu",
        "title": "glm-max",
        "username": "ACC-005",
        "password": "",
        "url": "https://open.bigmodel.cn",
        "notes": "GLM Max Coding Plan API_KEY\n台账: KEY-005",
        "tags": ["coding_plan", "api"],
        "props": {
            "key_id": "KEY-005",
            "account_id": "ACC-005",
            "permission": "plan",
            "purpose": "GLM Max",
            "status": "active",
            "env_var": "ZHIPU_API_KEY",
        },
    },
    {
        "group": "Moonshot",
        "title": "kimi-allegretto",
        "username": "ACC-006",
        "password": "",
        "url": "https://www.kimi.com",
        "notes": "Kimi Allegretto Coding Plan API_KEY\n台账: KEY-006",
        "tags": ["coding_plan", "api"],
        "props": {
            "key_id": "KEY-006",
            "account_id": "ACC-006",
            "permission": "plan",
            "purpose": "Kimi Allegretto",
            "status": "active",
            "env_var": "MOONSHOT_API_KEY",
        },
    },
    {
        "group": "MiniMax",
        "title": "minimax-max",
        "username": "ACC-007",
        "password": "",
        "url": "https://platform.minimaxi.com",
        "notes": "MiniMax Max Coding Plan API_KEY\n台账: KEY-007",
        "tags": ["coding_plan", "api"],
        "props": {
            "key_id": "KEY-007",
            "account_id": "ACC-007",
            "permission": "plan",
            "purpose": "MiniMax Max",
            "status": "active",
            "env_var": "MINIMAX_API_KEY",
        },
    },
    {
        "group": "Google",
        "title": "gemini-pro",
        "username": "ACC-008",
        "password": "",
        "url": "https://aistudio.google.com",
        "notes": "Gemini Pro / AI Pro 调用凭证\n台账: KEY-008",
        "tags": ["coding_plan", "api"],
        "props": {
            "key_id": "KEY-008",
            "account_id": "ACC-008",
            "permission": "plan",
            "purpose": "Gemini Pro",
            "status": "active",
            "env_var": "GOOGLE_API_KEY",
        },
    },
    {
        "group": "DeepSeek",
        "title": "deepseek-api",
        "username": "ACC-009",
        "password": "",
        "url": "https://platform.deepseek.com",
        "notes": "DeepSeek 官方按量 API\n台账: KEY-009",
        "tags": ["api", "payg"],
        "props": {
            "key_id": "KEY-009",
            "account_id": "ACC-009",
            "permission": "full",
            "purpose": "DeepSeek 官方 API",
            "status": "active",
            "env_var": "DEEPSEEK_API_KEY",
        },
    },
]


def _set_props(entry, props: dict) -> None:
    for k, v in props.items():
        if v is None or v == "":
            continue
        entry.set_custom_property(k, str(v))


def create_vault(path: Path, password: str, with_samples: bool = True) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    kp = create_database(str(path), password=password)

    ai_root = kp.add_group(kp.root_group, "AI Keys")
    for name in VENDOR_GROUPS:
        kp.add_group(ai_root, name)

    login_root = kp.add_group(kp.root_group, "Account Logins")
    for name in [g for g in VENDOR_GROUPS if not g.startswith("_")]:
        kp.add_group(login_root, name)

    guide = kp.add_entry(
        ai_root,
        title="_README",
        username="",
        password="",
        notes=(
            "本库存放 API Key / Coding Plan 调用凭证。\n"
            "1. Title = 密钥别名，与 Excel「API密钥」表一致\n"
            "2. Password = 完整 key；last4 写自定义属性\n"
            "3. 登录密码放 Account Logins/，不要和 API Key 混条目\n"
            "4. 台账引用: KeePassXC: AI Keys/<厂商>/<别名>\n"
            "5. 详见 STRUCTURE.md；用 key_helper.py 写入\n"
        ),
    )
    guide.set_custom_property("status", "meta")

    if with_samples:
        for sample in SAMPLE_ENTRIES:
            group = kp.find_groups(name=sample["group"], group=ai_root, first=True)
            if group is None:
                group = kp.add_group(ai_root, sample["group"])
            entry = kp.add_entry(
                group,
                title=sample["title"],
                username=sample["username"],
                password=sample["password"],
                url=sample.get("url") or "",
                notes=sample.get("notes") or "",
                tags=sample.get("tags") or [],
            )
            _set_props(entry, sample.get("props") or {})

    kp.save()


def main() -> int:
    parser = argparse.ArgumentParser(description="初始化 AI Keys KeePassXC 数据库")
    parser.add_argument(
        "--path",
        type=Path,
        default=DEFAULT_VAULT,
        help=f"数据库路径（默认: {DEFAULT_VAULT})",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="若文件已存在则覆盖（会毁掉旧库！）",
    )
    parser.add_argument(
        "--no-samples",
        action="store_true",
        help="不创建与盘点表对应的占位条目",
    )
    parser.add_argument(
        "--yes",
        action="store_true",
        help="与 --force 联用时跳过确认",
    )
    parser.add_argument(
        "--password-env",
        default="AI_KEYS_MASTER_PASSWORD",
        help="从该环境变量读取主密码（非交互安装用）；默认 AI_KEYS_MASTER_PASSWORD",
    )
    args = parser.parse_args()
    path: Path = args.path.resolve()

    if path.exists() and not args.force:
        print(f"已存在: {path}")
        print("如需重建请加 --force（会删除旧数据）。")
        return 1

    if path.exists() and args.force:
        print(f"警告: 将覆盖 {path}")
        if not args.yes:
            confirm = input("输入 yes 确认: ").strip()
            if confirm != "yes":
                print("已取消")
                return 1
        path.unlink()

    env_name = args.password_env
    pw1 = os.environ.get(env_name, "").strip()
    if pw1:
        print(f"已从环境变量 {env_name} 读取主密码")
        if len(pw1) < 8:
            print("主密码至少 8 位")
            return 1
    else:
        print("设置主密码（请牢记；丢失则库内密钥无法恢复）")
        if sys.stdin.isatty():
            pw1 = getpass.getpass("主密码: ")
            if len(pw1) < 8:
                print("主密码至少 8 位")
                return 1
            pw2 = getpass.getpass("再输一次: ")
            if pw1 != pw2:
                print("两次密码不一致")
                return 1
        else:
            print(
                f"非交互环境：请先设置环境变量 {env_name} 后再运行，例如：\n"
                f'  $env:{env_name} = "你的主密码"\n'
                f"  python init_vault.py\n"
                f"  Remove-Item Env:{env_name}",
                file=sys.stderr,
            )
            return 2

    try:
        create_vault(path, pw1, with_samples=not args.no_samples)
    except Exception as e:
        print(f"创建失败: {e}", file=sys.stderr)
        return 1

    print()
    print(f"已创建: {path}")
    print("分组: AI Keys/<厂商>/  与 Account Logins/")
    if not args.no_samples:
        print("已放入 9 条与盘点表对齐的占位条目（Password 为空）：")
        for s in SAMPLE_ENTRIES:
            print(f"  AI Keys/{s['group']}/{s['title']}")
        print("写入真 key: python key_helper.py set-password <别名>")
    print()
    print("下一步: python key_helper.py list")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
