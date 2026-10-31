# MAINTENANCE — 维护手册

本文件记录如何维护 `~/ID`：依赖安装、日常操作、同步与备份。

## 依赖安装

### Rust CLI

需要 Rust 1.85+（推荐稳定版），通过 [rustup](https://rustup.rs/) 安装。Debian/Ubuntu 的构建依赖：

```bash
sudo apt install build-essential make keepassxc python3
make build
```

SQLite 随 Rust 依赖编译，不需要系统开发库；`sqlite3` 只用于手工查询。Python 3 仅用于 `make test` 的终端集成测试，CLI 源码全部在 `src/`。首次构建下载锁定的依赖，后续构建可复用缓存。发布版可用 `cargo build --release --locked`。


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
make new id=<身份名>
```

CLI 会自动：复制模板 → 权限设为 700 → 用 `config/secrets/.secrets.env` 里的初始密码新建 `<身份名>.kdbx`。已有的密码库不会被覆盖或改写。

### 新建密码库的初始密码

初始密码写在 `~/ID/config/secrets/.secrets.env`，键名 `KDBX_INITIAL_PASSWORD`，文件权限必须是 `600`（模板见 `config/secrets/.secrets.env.example`，新设备复制后填写）。旧 Docker 环境中的初始密码应先迁移到这里；CLI 不再从 Docker 配置读取秘密。改这个值只影响之后新建的库。

```bash
chmod 600 ~/ID/config/secrets/.secrets.env
```

### ⚠️ 修改新密码库的主密码（必做）

新建身份后，把这份空白库改成该身份自己的主密码:

```bash
keepassxc-cli db-edit -p ~/ID/identities/<身份名>/passwords/<身份名>.kdbx
```

按提示先输入 `config/secrets/.secrets.env` 里的 `KDBX_INITIAL_PASSWORD`，再输两遍新密码。

> `template/passwords/template.kdbx` 不再参与新建。CLI 会当场 `db-create`。

### 临时凭证（AI 代执行密码）

交互式 `make ai` / `make manage` 的主密码缓存由 `src/vault.rs` 管理，仅在同一进程中复用，不跨独立命令或终端。切换库、退出会话和 AI `/lock` 会清除缓存；空闲 5 分钟即由后台计时清除，下次使用需解锁。明文显示和修改已有密码强制重新验证。该缓存不替代 `make grant` 的 AI 授权机制，主密码不会提供给模型。

`make grant` 把某条目的密码提取成临时凭证，AI 用 `make use` 代执行命令而不见明文（机制与分级 Tags 见 [GUIDE.md](GUIDE.md) 场景四）。维护要点：

- 凭证在 `${XDG_RUNTIME_DIR:-/run/user/<uid>}/id-grants/`，tmpfs，**重启即清空**；目录权限 700，文件 600
- 过期凭证在每次 `grant` / `grants` 时顺手清理，无需定时任务
- 条目的 `ai-auto` / `ai-deny` 标签用 `make tag id=<身份> title=<条目> tags=<标签>` 打（`tags=` 留空则清除）；也可在 KeePassXC 图形客户端打，标签随 kdbx 经 Syncthing 同步。`src/maintenance.rs` 在 Linux 匿名内存文件中通过 XML 导出/注入/导入实现，会重建数据库——条目和附件保留，但数据库级的自定义设置（如加密参数）如有改动需自行复查
- 日志只记授权、使用等操作和身份，不记条目内容、密码或 token，出现泄露疑虑时 `grep grant logs/scripts.log` 核对后 `make revoke all=1`

主密码通过匿名管道传入 KeePassXC，不进入命令参数、环境变量、模型请求或日志。主密码输入缓冲区固定容量，退格立即擦除移除的字节；缓存到期、手动锁定和正常退出使用 `zeroize` 清理。工程代码禁止 `unsafe`。

这不能保证清除 KeePassXC、操作系统或依赖库中的所有副本，也没有实现内存锁页或禁用系统转储。机器被入侵、同用户调试权限、交换分区、终端录屏等仍可能泄露。标签重建和原表附件目前需要 Linux；其他平台可用 KeePassXC 图形界面处理这两项。

### SSH 终端剪贴板与明文权限

SSH 连接首次使用 AI 或剪贴板时会要求粘贴测试文字；结果仅按当前连接缓存到 `config/.cache/clipboard/`，不进 git 或 Syncthing，不保存剪贴板内容。切换 SSH 客户端或 tmux 客户端会使用独立结果。运行 `make clipboard-check` 可强制重测。

`make settings` 中的“明文显示密码”是全局权限，关闭后禁止所有项目明文入口，显示前会重新读取设置。该开关不会清除已有终端滚屏或外部会话录制。

回归检查无需接触真实密码库：

```bash
make test
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
```

### 修改模板

直接编辑 `~/ID/template/` 下的目录和文件。**改动只影响之后新建的身份**，已有身份保持原样。

### SQLite 数据库（非机密个人信息）

所有身份共用一个 SQLite 库 `./data/id.db`，用 `identity` 列区分。首次启动 Web 界面时自动从 `template/data/schema.sql` 初始化。

`make manage` 的资料关联保存在同一库的 `password_links` 表，首次关联时自动创建。它只保存资料 ID 和本身份的 KDBX 条目路径，不存密码；新增关联不会改写 `items` 中的资料。关联随 SQLite 同步，KDBX 条目改名或移动后需在列表中重新关联。`make manage type=info` 只读 SQLite，无需解锁密码库。

`items.category=account` 独立显示为账号，其余分类作为个人信息维度；分类名可自定义，不需要迁移表结构。备注仍保存在 `items.note`；编辑普通备注直接更新文本，编辑导入账号的备注只更新 JSON 中的 `purpose`，其他结构化备注使用 `text` 字段，保留来源及关联元数据。

**启动 Web 界面：**

```bash
make web
```

打开 http://localhost:3000，输入访问密码（在 `~/ID/docker/.env` 的 `WEB_PASSWORD`，改后 `make web` 生效）。

AI 助手页（`/ai`）使用 `config/secrets/.secrets.env` 提供的 `AI_API_KEY` 和挂载的 `config/config.toml`。运行 `make settings` 修改非敏感设置后，Web 每次请求都会重新读取；API key 修改后运行 `make web` 重建容器。

**手动查库：**

```bash
sqlite3 ./data/id.db
# 或
docker compose -f docker/docker-compose.yml --env-file docker/.env exec web sqlite3 /app/data/id.db
```

常用 SQL 示例见 `template/data/schema.sql` 末尾注释。

**分工原则：**

| 数据 | 放哪 |
| --- | --- |
| 密码、密钥、恢复码（机密） | `passwords/*.kdbx` |
| 账号名、地址、偏好、事件（非机密个人信息） | SQLite 库 `./data/id.db`（所有身份共用） |
| 身份档案与喜好（给人和 AI 看） | `profile.md` |

## 同步与备份（待接入）

- [ ] **Syncthing**：添加文件夹时选 `~/ID`，不要只选 `identities/`，否则根目录的 `.stignore` 不生效。同步出去的是 `identities/` 和 `data/id.db`；`config/config.toml` 通过 git 同步，Docker 环境和 `config/secrets/.secrets.env` 留在各机器本地。`.stignore` 本身不会被同步，新设备要从 git 拿一份，或手动复制。kdbx 和 SQLite 库都可以同步，但**避免两端同时改同一个库**。**开启交错版本控制（staggered versioning）**。身份文件的历史靠它，不靠 git。
- [ ] **备份**：定期 `tar` 打包 + `gpg -c` 加密后存外部存储；SQLite 库直接复制 `./data/id.db` 或用 `sqlite3 ./data/id.db .dump > backup.sql` 导出。

## 故障排查

AI 请求失败时，CLI 只显示经过校验的错误码、请求编号和处理提示，不回显服务端原始错误正文，以免其中包含凭据或请求内容。失败菜单会保留本次输入，可修改 AI 设置后重试，或返回输入。

百炼返回 403 时，请结合具体错误码查看[官方错误说明](https://www.alibabacloud.com/help/zh/model-studio/error-code)：`AllocationQuota.FreeTierOnly` 通常涉及免费额度及用尽停机设置，`AccessDenied` 涉及模型访问权限，`AccessDenied.Unpurchased` 涉及服务开通状态。订阅套餐的 API key 和专用接口地址应配套使用；不要仅凭 HTTP 状态码修改 key 或启用付费。

| 问题 | 处理 |
| --- | --- |
| 忘记某身份 kdbx 主密码 | 无法找回，从备份恢复；恢复码在 `recovery/` |
| 凭证提示不存在/已过期 | tmpfs 重启清空或 TTL 到期，重新 `make grant`；`make grants` 看活跃凭证 |
| sqlite3 命令找不到 | `sudo apt install sqlite3` |
| 想删掉某身份 | 手动 `rm -rf identities/<身份名>`；SQLite 里对应的数据用 `DELETE FROM items WHERE identity='<身份名>'` 和 `DELETE FROM events WHERE identity='<身份名>'` 清理 |
