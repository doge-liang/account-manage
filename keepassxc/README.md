# KeePassXC 个人 API Key 管理方案

与上级目录的 **AI账号资产盘点表.xlsx** 配套使用：

| 放哪里 | 存什么 |
|--------|--------|
| **KeePassXC**（本目录） | 完整 API Key（加密） |
| **Excel 盘点表** | 别名、末四位、用途、月费、账号状态 |

真实密钥**只**进 `.kdbx`，不要写进 Excel / Git / 聊天记录。

---

## 1. 安装

### 1.1 KeePassXC 桌面版（日常查看、编辑）

- 官网：https://keepassxc.org/download/
- Windows 推荐用官方安装包或 `winget install KeePassXC.KeePassXC`

### 1.2 Python 助手（本仓库脚本）

```powershell
cd D:\ai-account-inventory\keepassxc
pip install -r requirements.txt
```

---

## 2. 初始化数据库（只需一次）

```powershell
cd D:\ai-account-inventory\keepassxc
python init_vault.py
```

会生成：

```text
keepassxc/vaults/ai-keys.kdbx
```

- 设置**主密码**（务必记住；丢了无法恢复）
- 自动建好 `AI Keys/<厂商>/` 与 `Account Logins/`
- 放入与盘点表示例对应的 3 条占位条目（Password 为空）

用 KeePassXC 打开该文件，或继续用下面的 CLI。

> `vaults/` 与 `*.kdbx` 已在仓库 `.gitignore` 中，**不要**提交到 Git。

---

## 3. 日常命令

```powershell
cd D:\ai-account-inventory\keepassxc

# 列表
python key_helper.py list
python key_helper.py list --vendor OpenAI
python key_helper.py list --status active

# 搜索
python key_helper.py search prod

# 看元数据（不显示完整 key）
python key_helper.py show openai-server-prod

# 填入 / 更新真 key（交互输入，不进历史）
python key_helper.py set-password openai-server-prod

# 新增
python key_helper.py add --vendor OpenAI --title openai-lab --key-id KEY-010 --account-id ACC-001 --purpose "实验"

# 取出 key（管道用）
python key_helper.py get openai-server-prod

# 生成环境变量语句（PowerShell）
python key_helper.py export-env openai-server-prod
# 执行示例：
Invoke-Expression (python key_helper.py export-env openai-server-prod --print-only)
```

自定义库路径：

```powershell
$env:AI_KEYS_KDBX = "D:\secure\ai-keys.kdbx"
python key_helper.py list
```

---

## 4. 与 Excel 怎么对齐

1. KeePass **Title** = Excel **密钥别名**（完全一致）
2. 自定义属性 `key_id` / `last4` / `account_id` 与表内列一致
3. Excel **密钥存放处** 写成：

```text
KeePassXC: AI Keys/OpenAI/openai-server-prod
```

`key_helper.py show` / `add` / `set-password` 会打印这行，可直接粘贴。

字段与分组细则见 [STRUCTURE.md](./STRUCTURE.md)。

---

## 5. 推荐工作流

### 新申请一把 Key

1. 厂商控制台创建 key  
2. `python key_helper.py add ...` 或 KeePassXC 图形界面新建  
3. `set-password` 写入完整 key  
4. Excel「API密钥」加一行：别名、末四位、存放处  
5. 部署到服务器时用平台 Secrets，**不要**把 kdbx 拷到公网机器当唯一存储  

### 使用 Key（本机脚本）

```powershell
Invoke-Expression (python key_helper.py export-env openai-server-prod --print-only)
python your_script.py
```

或：

```powershell
$env:OPENAI_API_KEY = (python key_helper.py get openai-server-prod)
```

### 轮换

1. 控制台生成新 key → `set-password` 更新  
2. 改 Excel 末四位与计划轮换日  
3. 各环境替换后，**吊销旧 key**  
4. 旧条目可移到分组 `_Revoked/`  

---

## 6. 备份与安全

| 项 | 建议 |
|----|------|
| 备份什么 | `ai-keys.kdbx` 文件 |
| 备份到哪 | 加密 U 盘 / 网盘私密目录 / 第二台电脑 |
| 主密码 | 不要和 kdbx 放同一处；可用另一密码管理器只记这一条 |
| 双因素（可选） | KeePassXC → 数据库设置 → 增加密钥文件 `.keyx`，文件单独存 |
| 同步 | 可用网盘同步**加密后的** kdbx；避免多设备同时写导致冲突 |
| 锁屏 | KeePassXC 设置较短自动锁定时间 |

---

## 7. 文件一览

```text
keepassxc/
  README.md           # 本说明
  STRUCTURE.md        # 分组与字段规范
  init_vault.py       # 初始化空库
  key_helper.py       # 列表/搜索/取用/写入
  requirements.txt
  vaults/             # 本地数据库（不入库）
    ai-keys.kdbx
```

---

## 8. 常见问题

**Q: 和 KeePassXC 官方 CLI 什么关系？**  
A: 本脚本用 `pykeepass` 直接读写 `.kdbx`，与 KeePassXC 桌面版兼容。你也可以只用图形界面，规范见 STRUCTURE.md。

**Q: 手机上能看吗？**  
A: 把 kdbx 放到网盘后，用 KeePassDX（Android）/ Strongbox 或 KeePassium（iOS）打开同一文件。注意冲突与主密码保护。

**Q: 忘了主密码？**  
A: 无法解密。只能重新建库并从各厂商控制台重新签发 key。

**Q: 能否把 Excel 里的别名批量导入？**  
A: 当前以 `add` 单条为主；若条目很多，可再说，我可以加 `import-from-xlsx`（只导元数据，仍需你粘贴真 key）。
