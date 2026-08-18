# AGENTS.md — account-manage 开发约定

Tauri 2 桌面应用（Rust 后端 + WebView 前端，vanilla JS 无构建步骤）。
数据含真实密钥，**永远不提交** `app/data/`（已 gitignore，走 Syncthing 同步）。

## 开发循环

```bash
cd src-tauri
cargo tauri dev        # 启动开发窗口，保持运行
```

改动生效方式（纯静态前端模式，`frontendDist: ../app/static`，无 devUrl/dev server）：

| 改什么 | 怎么生效 | 速度 |
|---|---|---|
| 前端 `app/static/`（html/css/js） | 应用窗口里 **F5 / Ctrl+R** 刷新——dev 模式 WebView 从磁盘读资源，不重新编译 | 即时 |
| Rust `src-tauri/src/` | 保存后 `tauri dev` 自动增量重编译并重启应用 | 小改动几秒 |

- 不要用浏览器直接开 `app/static/index.html` 预览——Python 后端已删，浏览器里只有空壳 UI。
  数据交互全走 Tauri invoke，必须在应用窗口里验证。
- 发布版 `cargo build --release` 是内嵌资源的，改动需重编译；开发期一律用 `cargo tauri dev`。

## 验证

```bash
cd src-tauri && cargo test          # 17 jsonpath 用例
cd src-tauri && cargo check         # Rust 快速检查
node --check app/static/app.js      # 前端语法
```

## 架构速览

```
src-tauri/src/
├── main.rs        # Tauri 入口 + 命令注册
├── commands.rs    # 25 个 Tauri commands
├── store.rs       # 数据层：原子写/备份轮换/损坏恢复
├── jsonpath.rs    # mini JSONPath 求值器
├── providers/     # 9 provider + OAuth + 缓存 + 调度
├── vault.rs       # KeePassXC 文件操作
└── scheduler.rs   # tokio 后台调度

app/static/        # 前端（vanilla JS）
└── app.js         # API 双通道适配层：Tauri invoke / HTTP fetch 自动切换
```

- 数据目录：dev 模式 = 仓库根 `app/data/`；portable exe = exe 旁 `app/data/`（`store.rs data_dir()`）
- 用量 provider 定义：`USAGE_PROVIDERS`（Python 时代遗留命名，Rust 中在 `providers/mod.rs`）
- `keepassxc/` 仅剩 `vaults/`（KeePassXC 库文件，gitignored 数据）。早期 Python 运维脚本（init_vault.py / key_helper.py）已随 Tauri 迁移清理；建库/改密直接用 KeePassXC 官方客户端，应用内「密钥库」页负责上传/下载/备份

## 双机同步

- 代码 → git（GitHub）；运行数据 → Syncthing（`app/data/`，文件夹已开 versioning keep=5）
- OAuth tokens 随 accounts.json 同步；refresh_token 失效需在 UI 重新设备码认证
