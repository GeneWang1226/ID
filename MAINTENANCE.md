# MAINTENANCE — 维护手册

本文件记录如何维护 `~/ID`：依赖安装、日常操作、同步与备份。

## 依赖安装

### KeePassXC（管理 .kdbx 密码库）

Debian/Ubuntu:

```bash
sudo apt install keepassxc
```

验证:

```bash
keepassxc-cli --version
```

### Turso CLI（云端 SQLite，存非机密个人信息）

```bash
curl -sSfL https://get.tur.so/install.sh | bash
turso auth signup   # 首次使用注册；已有账号用 turso auth login
```

验证:

```bash
turso --version
turso db list
```

## 日常操作

### 新增身份

```bash
~/ID/scripts/add-identity.sh <身份名>
```

脚本会自动：复制模板 → 权限设为 700 → 用 `config` 里的初始密码新建 `<身份名>.kdbx`。已有的密码库不会被覆盖或改写。

### 新建密码库的初始密码

初始密码写在 `~/ID/config`，键名 `kdbx_initial_password`，文件权限必须是 `600`。改这个值只影响之后新建的库。

```bash
chmod 600 ~/ID/config
```

### ⚠️ 修改新密码库的主密码（必做）

新建身份后，把这份空白库改成该身份自己的主密码:

```bash
keepassxc-cli db-edit -p ~/ID/identities/<身份名>/passwords/<身份名>.kdbx
```

按提示先输入 `config` 里的 `kdbx_initial_password`，再输两遍新密码。

> `template/passwords/template.kdbx` 不再参与新建。脚本会当场 `db-create`。

### 修改模板

直接编辑 `~/ID/template/` 下的目录和文件。**改动只影响之后新建的身份**，已有身份保持原样。

### 为身份开通 Turso 数据库

每个身份一个库 `id-<身份名>`：

```bash
~/ID/scripts/turso-setup.sh <身份名>
```

完成后连接信息写在 `identities/<身份名>/.turso.env`（权限 600）。用法:

```bash
turso db shell id-<身份名>          # 直接 SQL 操作
source identities/<身份名>/.turso.env  # 程序里用 TURSO_URL / TURSO_AUTH_TOKEN
```

常用 SQL 示例见 `template/data/schema.sql` 末尾注释。

**分工原则：**

| 数据 | 放哪 |
| --- | --- |
| 密码、密钥、恢复码（机密） | `passwords/*.kdbx` |
| 账号名、地址、偏好、事件（非机密个人信息） | 各身份自己的 Turso 库 `id-<身份名>` |
| 身份档案与喜好（给人和 AI 看） | `profile.md` |

## 同步与备份（待接入）

- [ ] **Syncthing**：添加文件夹时选 `~/ID`，不要只选 `identities/`，否则根目录的 `.stignore` 不生效。同步出去的是 `identities/` 和 `config`；脚本、模板、文档、`.git` 被忽略。`.stignore` 本身不会被同步，新设备要从 git 拿一份，或手动复制。`config` 到另一台机器后确认权限仍是 `600`。kdbx 可以同步，但**避免两端同时改同一个库**。`.turso.env` 含 token，已在 `.stignore` 里排除，每台机器自己开通。**开启交错版本控制（staggered versioning）**。身份文件的历史靠它，不靠 git。
- [ ] **备份**：定期 `tar` 打包 + `gpg -c` 加密后存外部存储；Turso 侧用 `turso db shell id-<身份名> .dump > backup.sql` 导出。

## 故障排查

| 问题 | 处理 |
| --- | --- |
| 忘记某身份 kdbx 主密码 | 无法找回，从备份恢复；恢复码在 `recovery/` |
| turso 命令找不到 | 重新跑安装脚本，或检查 `~/.turso` 是否在 PATH |
| 想删掉某身份 | 手动 `rm -rf identities/<身份名>`；有 Turso 库的话同时 `turso db destroy id-<身份名>` |
