# ID — 多身份信息管理

本目录用于集中管理多个身份的密码、文件与信息。

- **日常使用：[docs/GUIDE.md — 操作指引](docs/GUIDE.md)**
- 安装、维护、故障排查：[docs/MAINTENANCE.md — 维护手册](docs/MAINTENANCE.md)
- 版本变更：[CHANGELOG.md](CHANGELOG.md)

## CLI 构建

安装 Rust 1.85 或更新版本、C 编译器、make 和 KeePassXC，然后运行：

```bash
make build
make help
make list
```

Make 入口调用 `target/debug/id`，源码变化后自动重新编译；也可以直接运行 `cargo run --locked -- list`。CLI 不再依赖 Python 或 Bash 脚本；`make test` 的终端测试需要 Python 3，临时凭证代执行仍使用系统 Bash。Web 保持 Next.js。

KDBX 仍由本机 KeePassXC 处理，SQLite 结构与现有资料兼容，无需转换资料。主密码使用可擦除缓冲区，空闲五分钟、锁定或退出时清除。Rust 的内存安全不等于防止系统内存转储，见维护手册。

## 目录结构

| 路径 | 说明 |
| --- | --- |
| `docs/` | 文档：操作指引、维护手册 |
| `template/` | 身份模板目录。新增身份时复制缺失文件，不覆盖已有内容 |
| `identities/` | 所有身份目录，每个身份一个子目录（如 `identities/gene/`） |
| `src/` | Rust CLI：会话、KeePassXC 调用、终端界面和资料管理 |
| `tests/` | 使用临时身份和密码库的集成测试 |
| `Cargo.toml` / `Cargo.lock` | Rust 依赖与锁定版本 |
| `Makefile` | 常用操作入口，`make help` 查看 |
| `config/config.toml` | 可同步的非敏感配置：AI 接口、模型、思考开关、全局明文权限 |
| `config/secrets/.secrets.env` / `config/secrets/.secrets.env.example` | 敏感配置：AI API key、KDBX 初始密码（真实文件权限 600，不进 git） |
| `docker/.env` / `docker/.env.example` | Docker 配置：Web 访问密码（真实文件权限 600，不进 git，模板入库） |
| `assets/` | 工具安装包（KeePassXC 各平台客户端等），只放安装包、不放身份数据 |
| `web/` | Web 界面（Next.js），查看/编辑 SQLite 库里的非机密信息，Docker 部署 |
| `data/` | SQLite 数据库文件（`id.db`），随 Syncthing 同步 |
| `logs/` | 脚本操作日志（`scripts.log`，只记操作、不记秘密，权限 700） |

## 身份目录内的文件

模板中的预设内容（每个身份都会有）：

| 路径 | 说明 |
| --- | --- |
| `profile.md` | 该身份的档案：基本信息、喜好、习惯、备忘 |
| `AGENTS.md` | 给 AI 的身份级指引 |
| `passwords/<身份名>.kdbx` | 密码库，新建身份时按 `config/secrets/.secrets.env` 里的初始密码当场生成 |
| `data/schema.sql` | SQLite 个人信息表结构（非机密数据，所有身份共用一个库） |
| `documents/` | 证件扫描件、重要文件 |
| `keys/` | SSH / GPG 密钥 |
| `notes/` | 文本笔记 |
| `recovery/` | 恢复码、助记词、备用验证方式 |

## git 与 Syncthing 错开

两类文件不叠在一起：

| 归谁 | 路径 |
| --- | --- |
| git（源码、配置和文档） | `src/`、`tests/`、`Cargo.toml`、`Cargo.lock`、`template/`、`assets/`、`docs/`、`web/`、`docker/`（不含 `.env`）、`config/`（不含 secrets）、`Makefile`、`AGENTS.md`、`README.md`、`.gitignore`、`.stignore` |
| Syncthing（身份数据） | `identities/`、`data/` |
| 两边都不要 | `logs/`、`backups/`、`.git` |

`.stignore` 排除 git 那一侧和 `.git`，避免仓库被同步写坏。`.gitignore` 排除身份数据。新设备克隆私有远程部署项目；只开 Syncthing 的话，那边不会有 `src/` 和 `template/`。

## keepassxc-cli 的局限（为什么打标签要绕道）

`keepassxc-cli` 只是 KeePassXC 的附属工具，定位是查询和常见自动化，不少 GUI 功能没做进去：

| 功能 | CLI | GUI |
| --- | --- | --- |
| 增删改条目、改主密码、生成密码、附件导入导出 | ✓ | ✓ |
| 读写 Tags | 只能 `show` 读 | ✓ 可写 |
| 自定义字段（TOTP 种子、额外属性） | 只能读 | ✓ 可写 |
| 条目历史、回收站 | ✗ | ✓ |
| Auto-Type 序列、浏览器集成配置 | ✗ | ✓ |
| 自定义图标 | ✗ | ✓ |

分级授权需要的 `ai-auto` / `ai-deny` 标签就是「只能读不能写」的那类。本项目的解法：

- **服务器端**：`make tag id=<身份> title=<条目> tags=ai-deny`（[src/maintenance.rs](src/maintenance.rs)，原理是 `keepassxc-cli` 导出 XML → 注入标签 → 导入新库 → 验证后原子替换；明文 XML 只放 Linux 匿名内存文件，主密码只经 stdin 传递）
- **桌面端**：也可以在 KeePassXC 图形客户端里打，标签随 kdbx 经 Syncthing 同步

## 安全规则

- 身份目录权限为 `700`，密钥类文件建议 `600`。
- 本目录内容属于敏感数据，不要提交到任何公开仓库。
- `.kdbx` 密码库始终用 KeePass(XC) 打开。各身份自己的主密码不要写进任何文件。
- 新建空白库用的初始密码只放在 `config/secrets/.secrets.env`（权限 600，模板见 `config/secrets/.secrets.env.example`）。CLI 不得把它打进日志或终端。
