# 账号管家 AccountHub

本地账号/密码管理 + AI 服务用量追踪，Tauri 2 桌面应用（Rust 后端 + WebView 前端）。

## 运行

### 桌面版（推荐）

```bash
cd src-tauri
cargo tauri dev      # 开发
cargo build --release   # portable exe → src-tauri/target/release/account-hub.exe
```

- **portable 模式**：`account-hub.exe` 单文件可直接运行，数据目录在 exe 旁的 `app/data/`
- **开发模式**：数据目录在仓库的 `app/data/`（与旧版共用）
- 依赖：Windows 10/11（WebView2 系统自带）；开发时需要 Rust toolchain

### 旧 Python 版（浏览器）

```bash
python app/server.py   # http://127.0.0.1:8756/
```

纯标准库，无 pip 依赖。与桌面版共用同一份前端（`app/static/`）和数据格式。

## 数据

```
app/data/
├── accounts.json      # 账号 + 关系 + 用量配置（原子写，含 oauth_tokens）
├── backups/           # 自动备份（保留 10 份）
└── usage_cache.json   # 用量缓存
```

- 数据含真实密钥，**不入 git**（已 gitignore），通过 **Syncthing** 双机同步
- keepassxc/vaults/ai-keys.kdbx — KeePassXC 密钥库

## 功能

- **账号台账**：11 类账号（AI 会员/API/邮箱/手机/微信等），厂商品牌图标
- **用量追踪**：9 个内置 provider（ChatGPT Codex / Claude Code / Grok Build×多账号 / GLM / Kimi / MiniMax / DeepSeek / Gemini / 阿里百炼）+ 自定义中转站（URL+JSONPath）
- **OAuth**：Grok RFC 8628 设备码多账号登录；Codex/Claude 读本地 CLI 凭证或配置内 token，自动刷新
- **KeePassXC**：vault 信息/下载/备份/替换（KDBX 魔数校验）
- **双机同步**：数据走 Syncthing（文件夹已开 versioning keep=5）

## 架构

```
src-tauri/src/
├── main.rs        # Tauri 入口 + 命令注册
├── commands.rs    # 25 个 Tauri commands（对齐旧 HTTP API）
├── store.rs       # 数据层：原子写/备份轮换/损坏恢复
├── jsonpath.rs    # mini JSONPath 求值器（17 用例对照测试）
├── providers/     # 9 provider + OAuth + 缓存 + 调度
├── vault.rs       # KeePassXC 文件操作
└── scheduler.rs   # tokio 后台调度（30s 扫描，interval_min 到期抓取）

app/static/        # 前端（vanilla JS，无构建步骤）
└── app.js         # API 双通道适配层：Tauri invoke / HTTP fetch 自动切换
```

## 测试

```bash
cd src-tauri && cargo test     # 17 jsonpath 用例
node --check app/static/app.js # 前端语法
```
