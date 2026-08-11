# KeePassXC 库结构与字段规范

与 `AI账号资产盘点表.xlsx` 的「API密钥」表一一对应。

## 分组（Group）

```
AI Keys/                    # 所有可调用凭证（Coding Plan API_KEY + 官方 API）
├── OpenAI/                 # chatgpt-max-20x
├── Anthropic/              # claude-max-20x
├── xAI/                    # grok-super-1, grok-super-2
├── Google/                 # gemini-pro
├── DeepSeek/               # deepseek-api（按量）
├── Moonshot/               # kimi-allegretto
├── Zhipu/                  # glm-max
├── MiniMax/                # minimax-max
├── Alibaba/
├── ByteDance/
├── Other/
└── _Revoked/

Account Logins/             # 网页/控制台「登录密码」（与 API Key 分开）
├── OpenAI/
├── Anthropic/
└── ...
```

规则：
- **Coding Plan 也有 API_KEY**：凭证进 `AI Keys/`，登录密码进 `Account Logins/`，两条都要建
- 按**厂商**分子组，不按设备分（设备写在自定义字段 `deploy`）
- 吊销的 key 移到 `_Revoked/`，不要直接删
- 登录密码与 API Key **不要混在同一条目**

## 条目命名（Title）

使用盘点表里的 **密钥别名**，全局唯一、小写、连字符：

| 好 | 差 |
|----|-----|
| `openai-admin-readonly` | `我的key` |
| `openai-server-prod` | `OpenAI API Key 1` |
| `anthropic-dev` | `sk-xxx...`（别名不要用真 key） |

与 Excel「密钥别名」列**完全一致**，方便 `key_helper get` 和台账对照。

## 标准字段映射

| KeePassXC 字段 | Excel 列 | 填什么 |
|----------------|----------|--------|
| **Title** | 密钥别名 | `openai-server-prod` |
| **Username** | account_id | `ACC-001`（或登录邮箱） |
| **Password** | （不进表） | **完整 API Key** |
| **URL** | 控制台URL | `https://platform.openai.com` |
| **Notes** | 备注 + 用途摘要 | 自由文本 |
| **Tags** | — | 如 `api`, `prod`, `readonly` |

## 自定义属性（Advanced → Additional attributes）

| 属性名 | Excel 列 | 示例 |
|--------|----------|------|
| `key_id` | key_id | `KEY-001` |
| `account_id` | account_id | `ACC-001` |
| `last4` | 末四位 | `a1b2` |
| `permission` | 权限范围 | `full` / `readonly` / `admin_readonly` |
| `can_query_usage` | 能否查用量 | `是` / `否` |
| `purpose` | 用途 | `生产服务` |
| `deploy` | 部署位置 | `服务器A,B` |
| `created` | 创建日期 | `2026-03-01` |
| `rotate_by` | 计划轮换日 | `2026-09-01` |
| `status` | 状态 | `active` / `revoked` / `expired` |
| `env_var` | — | `OPENAI_API_KEY`（给 `export-env` 用） |

`init_vault.py` 和 `key_helper.py add` 会自动写入上述自定义属性。

## 台账「密钥存放处」写法

Excel 中统一写成：

```text
KeePassXC: AI Keys/<厂商>/<密钥别名>
```

示例：

```text
KeePassXC: AI Keys/OpenAI/openai-server-prod
KeePassXC: AI Keys/Anthropic/anthropic-dev
```

## 与盘点表的分工

| 内容 | 放哪里 |
|------|--------|
| 完整 API Key | **仅** KeePassXC Password |
| 别名、末四位、用途、轮换日 | Excel + KeePass 自定义属性（两边同步） |
| 月费、账单、账号状态 | **仅** Excel 账号台账 |
| 控制台登录密码 | KeePass `Account Logins/`（可选） |

## 备份约定

1. 数据库文件：`keepassxc/vaults/ai-keys.kdbx`（已在 `.gitignore`）
2. 备份到：加密 U 盘 / 网盘「仅自己可见」文件夹 / 另一台电脑
3. 备份频率：每次批量增删 key 后；至少每月一次
4. **主密码**只存在你脑子里或另一套离线记录，不要和 `.kdbx` 放同一处
5. 可选：再设一个 **密钥文件（.keyx）** 双因素，密钥文件单独存

## 轮换流程

1. 厂商控制台创建新 key  
2. KeePassXC 更新该条目 Password，改 `last4`、`created`、`rotate_by`  
3. Excel「API密钥」同步末四位与日期  
4. 各部署位置替换环境变量  
5. 旧 key 在控制台吊销；条目可移到 `_Revoked/` 并改 `status=revoked`
