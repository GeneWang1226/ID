# ID — 多身份信息管理

本目录用于集中管理多个身份的密码、文件与信息。

## 目录结构

| 路径 | 说明 |
| --- | --- |
| `template/` | 身份模板目录，由我维护。新增身份时脚本会完整复制它 |
| `identities/` | 所有身份目录，每个身份一个子目录（如 `identities/work/`） |
| `scripts/` | 工具脚本，如 `add-identity.sh` |
| `config` | 新建密码库的初始主密码（`kdbx_initial_password`，权限 600） |
| `assets/` | 工具安装包（KeePassXC 各平台客户端等），只放安装包、不放身份数据 |
| `web/` | Web 界面（Next.js），查看/编辑 Turso 库里的非机密信息 |
| `logs/` | 脚本操作日志（`scripts.log`，只记操作、不记秘密，权限 700） |

## 身份目录内的文件

模板中的预设内容（每个身份都会有）：

| 路径 | 说明 |
| --- | --- |
| `profile.md` | 该身份的档案：基本信息、喜好、习惯、备忘 |
| `AGENTS.md` | 给 AI 的身份级指引 |
| `passwords/template.kdbx` | 旧的空白密码库壳。新建身份不再复制它，而是按 `config` 里的密码当场生成 |
| `data/schema.sql` | Turso 个人信息表结构（非机密数据，所有身份共用一个库） |
| `documents/` | 证件扫描件、重要文件 |
| `keys/` | SSH / GPG 密钥 |
| `notes/` | 文本笔记 |
| `recovery/` | 恢复码、助记词、备用验证方式 |

## 新增身份

```bash
~/ID/scripts/add-identity.sh <身份名>
```

身份名只允许小写字母、数字、`-`、`_`。如果身份已存在，脚本会进入**补齐模式**：只复制模板中缺失的内容（如忘带的 kdbx、`data/schema.sql`），绝不覆盖已有文件。

## 维护

安装依赖（KeePassXC、Turso）、修改主密码、开通 Turso 等操作见 [MAINTENANCE.md](MAINTENANCE.md)。

## git 与 Syncthing 错开

两类文件不叠在一起。身份数据不放进 `data/`，那个目录已经是每个身份的 Turso 表结构。

| 归谁 | 路径 |
| --- | --- |
| git（脚本和文档） | `scripts/`、`template/`、`assets/`、`AGENTS.md`、`README.md`、`MAINTENANCE.md`、`.gitignore`、`.stignore` |
| Syncthing（身份数据） | `identities/`、`config` |
| 两边都不要 | `logs/`、各身份的 `.turso.env`、`.git` |

`.stignore` 排除 git 那一侧和 `.git`，避免仓库被同步写坏。`.gitignore` 排除身份数据。本机仓库已初始化，跟踪上表里 git 那一行。新设备克隆私有远程来部署项目；远程还没建好时，仓库只在这台机器上。只开 Syncthing 的话，那边不会有 `scripts/` 和 `template/`。

## 安全规则

- 身份目录权限为 `700`，密钥类文件建议 `600`。
- 本目录内容属于敏感数据，不要提交到任何公开仓库。
- `.kdbx` 密码库始终用 KeePass(XC) 打开。各身份自己的主密码不要写进任何文件。
- 新建空白库用的初始密码只放在根目录 `config`（权限 600）。脚本不得把它打进日志或终端。

## 同步与备份（待接入）

- [ ] Syncthing：把 `~/ID` 加为同步文件夹（规则在 `.stignore`，实际同步 `identities/` 和 `config`）
- [ ] 备份：定期加密打包到外部存储
