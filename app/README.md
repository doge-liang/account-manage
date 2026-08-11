# 账号管家 (AccountHub)

本地浏览器应用，统一管理你的全部账号：AI 会员 / API Key 元数据、邮箱、手机号、微信/QQ/公众号、Z-library、Apple ID，以及账号之间的关联关系。

- **纯 Python 标准库**后端（零依赖安装）+ 原生 JS 前端（零外部 CDN，离线可用）
- 数据存 **`app/data/accounts.json`**（原子写入 + 自动备份），随 Syncthing 同步
- 完整 API Key **不进本系统**，继续留在 KeePassXC（本应用只记别名/末四位/存放处）

## 快速开始

双击 `run.bat`，或命令行：

```powershell
cd D:\account-manage\app
python server.py            # 默认 http://127.0.0.1:8756/ ，自动开浏览器
python server.py --port 9000 --no-browser
```

> 要求：任意 Python 3.9+（`server.py` 只用标准库；导入脚本才需要 `openpyxl`，用 `D:\Program\Python\3.13.0\python.exe` 即可，已装好）。

## 首次使用

1. 启动后打开仪表盘，确认 9 个 AI 账号已自动导入（来自 Excel 盘点表）
2. **设置 → 查询平台链接**：添加手机话费查询、AI 用量查询等跳转链接（如 `https://wap.10086.cn`）
3. **账号 → 新增账号**：录入邮箱、手机号、微信等；在账号详情里建「关联」（如：Gmail 是哪些 AI 会员的登录邮箱）
4. **密钥库**页：查看/下载/上传 `ai-keys.kdbx`（上传会自动备份旧文件）

## 功能

| 模块 | 说明 |
|---|---|
| 仪表盘 | 账号统计、AI 月费合计（USD/CNY 分开）、7 天内扣费/重置提醒、余额/用量预警、按厂商月费汇总 |
| 账号 | 搜索/类别筛选/状态筛选；AI 会员、按量 API、邮箱、手机号、微信、公众号、QQ、Z-library、Apple ID、其他 共 10 类，按类别渲染专属字段 |
| 关联 | 账号间双向关联（登录邮箱 / 绑定手机 / 同一主体…），删除账号自动清理关联 |
| 密钥库 | kdbx 文件信息（大小/SHA-256/有效性校验）、下载、上传替换（自动备份到 `vaults/backups/`）、备份列表下载 |
| 设置 | 应用名、密钥库路径、查询链接管理、数据导出/导入 JSON、清空数据（有备份） |

AI 会员专属字段：套餐、月费、币种、额度类型（订阅/用量/按量）、账单/扣费日、用量重置日、余额、剩余用量、最近核对日、Key 别名、Key 末四位、KeePass 存放处。
手机号专属字段：地区（境内/境外）、运营商、账单日、话费余额、话费查询链接。

## 数据与备份

- 数据文件：`app/data/accounts.json`（每次变更自动备份最近 10 份到 `app/data/backups/`）
- 完整导出：设置页「导出 JSON」，或 `GET /api/export`
- 还原：设置页「导入 JSON」（保留原账号 id，关联不断）
- 同步：整个 `account-manage` 目录由 Syncthing 同步；`ai-keys.kdbx` 也在其中，但**文件本身有主密码加密**，可放心分发；你决定哪些设备可信
- 密钥库备份：上传替换时旧文件自动存入 `keepassxc/vaults/backups/`（保留 10 份）

## 从 Excel 导入

`AI账号资产盘点表-已填.xlsx` 的 9 账号 + 9 Key 元数据已导入。需要重新导入时：

```powershell
cd D:\account-manage\app
D:\Program\Python\3.13.0\python.exe scripts/import_from_xlsx.py
```

服务未启动时可用 `--out 文件.json` 生成数据，再到设置页导入。
脚本会警告模板遗留的示例末四位（a1b2/c3d4/e5f6）——这些不是真实末四位，请在确认真 key 后更正。

## API 速查

```
GET  /api/data                    全部数据
POST /api/accounts                新增账号
PUT  /api/accounts/<id>           更新
DELETE /api/accounts/<id>         删除（级联清理关联）
POST /api/relations               新增关联
DELETE /api/relations/<id>        解除关联
GET/POST /api/query-links ...     查询链接 CRUD
GET  /api/vault/info              密钥库信息
GET  /api/vault/download          下载密钥库
POST /api/vault/upload            上传替换（校验 KDBX 魔数，自动备份）
GET  /api/vault/backups           备份列表
GET  /api/export                  导出全量 JSON
```

## 安全说明

- 服务只绑定 `127.0.0.1`，仅本机可访问
- 完整 API Key 和登录密码只存在 KeePassXC（主密码加密），本系统不存任何明文密钥
- 本表/本库可以进 Git 私库或网盘，但**主密码不要**和 `ai-keys.kdbx` 放同一处

## 目录结构

```text
app/
  server.py               # 后端（纯标准库）
  run.bat                 # Windows 启动脚本
  static/                 # 前端（index.html / app.js / style.css）
  scripts/import_from_xlsx.py   # Excel → 应用数据 一次性导入
  data/
    accounts.json         # 账号数据（Syncthing 同步）
    backups/              # 数据自动备份
```
