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

### sqlite3（查本地库）

```bash
sudo apt install sqlite3
```

### Docker（跑 Web 界面）

Debian/Ubuntu 装 Docker Engine 和 Compose 插件，官方文档：https://docs.docker.com/engine/install/debian/

验证:

```bash
docker --version
docker compose version
```

### make（命令入口）

```bash
sudo apt install make
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

### SQLite 数据库（非机密个人信息）

所有身份共用一个 SQLite 库 `./data/id.db`，用 `identity` 列区分。首次启动 Web 界面时自动从 `template/data/schema.sql` 初始化。

**启动 Web 界面：**

```bash
docker compose up -d
```

打开 http://localhost:3000，输入访问密码（默认 `changeme`，在 `docker-compose.yml` 改）。

**手动查库：**

```bash
sqlite3 ./data/id.db
# 或
docker compose exec web sqlite3 /app/data/id.db
```

常用 SQL 示例见 `template/data/schema.sql` 末尾注释。

**分工原则：**

| 数据 | 放哪 |
| --- | --- |
| 密码、密钥、恢复码（机密） | `passwords/*.kdbx` |
| 账号名、地址、偏好、事件（非机密个人信息） | SQLite 库 `./data/id.db`（所有身份共用） |
| 身份档案与喜好（给人和 AI 看） | `profile.md` |

## 同步与备份（待接入）

- [ ] **Syncthing**：添加文件夹时选 `~/ID`，不要只选 `identities/`，否则根目录的 `.stignore` 不生效。同步出去的是 `identities/`、`config` 和 `data/id.db`；脚本、模板、文档、`.git` 被忽略。`.stignore` 本身不会被同步，新设备要从 git 拿一份，或手动复制。`config` 到另一台机器后确认权限仍是 `600`。kdbx 和 SQLite 库都可以同步，但**避免两端同时改同一个库**。**开启交错版本控制（staggered versioning）**。身份文件的历史靠它，不靠 git。
- [ ] **备份**：定期 `tar` 打包 + `gpg -c` 加密后存外部存储；SQLite 库直接复制 `./data/id.db` 或用 `sqlite3 ./data/id.db .dump > backup.sql` 导出。

## 故障排查

| 问题 | 处理 |
| --- | --- |
| 忘记某身份 kdbx 主密码 | 无法找回，从备份恢复；恢复码在 `recovery/` |
| sqlite3 命令找不到 | `sudo apt install sqlite3` |
| 想删掉某身份 | 手动 `rm -rf identities/<身份名>`；SQLite 里对应的数据用 `DELETE FROM items WHERE identity='<身份名>'` 和 `DELETE FROM events WHERE identity='<身份名>'` 清理 |
