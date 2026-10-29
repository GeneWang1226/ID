# AGENTS.md — 给 AI 助手的全局指引

这个目录（`~/ID`）存放**多个身份的敏感信息**。任何 AI 助手在这里工作时，必须先读完本文件再动手。

## 这里是什么

- `template/` — 身份模板，由用户维护。**改模板只影响新建身份**，不要动它，除非用户明确要求。
- `identities/<name>/` — 一个具体身份。进入某个身份目录前，先读里面的 `AGENTS.md` 和 `profile.md`。
- `scripts/` — 工具脚本。新增身份**必须用** `scripts/add-identity.sh <name>`，不要手工复制目录。

## 铁律（不可违反）

1. **不泄露**：身份目录的内容（密码库、密钥、恢复码、档案）不得发送到任何外部服务，不得写入本目录之外的文件，不得在回复中明文展示秘密内容。
2. **不解密**：不要尝试破解、绕过或直接解析 `.kdbx` 文件。密码相关操作引导用户使用 KeePass(XC)。
3. **不擅自删除**：删除身份或身份内的任何文件前，必须得到用户明确确认。
4. **不动模板**：除非用户明确要求，否则 `template/` 只读。
5. **最小访问**：只读与当前任务直接相关的文件，不要漫无目的地翻身份目录。
6. **日志红线**：`logs/scripts.log` 只记「什么时间做了什么操作」，绝不写入密码、token、秘密内容。身份的大事记写进 SQLite 库 `data/id.db` 的 `events` 表，不写日志文件。
7. **git 只跟踪项目，不跟踪秘密**：仓库用来把脚本、模板和文档部署到新设备。`identities/`、`.env`、`*.kdbx`、`.turso.env`、`logs/` 永不入库（模板 `.env.example` 反而要入库），禁止 `git add -f`。除非用户明确要求，不要 `git commit` 或推远程。不要改 git config。身份文件的历史靠 Syncthing，不靠 git。

## 常见任务怎么做

| 任务 | 做法 |
| --- | --- |
| 新增身份 | 运行 `scripts/add-identity.sh <name>`。新 kdbx 的初始密码来自根目录 `.env` 的 `kdbx_initial_password`（不得写入日志或在回复中展示）。然后提醒用户填 `profile.md`、**改成该身份自己的主密码** |
| 记录身份偏好 | 写入该身份的 `identities/<name>/profile.md`，而不是记在别处 |
| 找某个身份的信息 | 先读该身份的 `profile.md`，再按它指向去找 |
| 存/查非机密个人信息 | 写 SQLite 库 `data/id.db`（表结构见 `template/data/schema.sql`，`identity` 列区分身份），机密永远进 kdbx |
| 改脚本 | 可以改 `scripts/` 下的脚本，改完用临时身份名测试后清理 |
| 安装依赖、改主密码等维护操作 | 按 `docs/MAINTENANCE.md` 执行 |

## 用户习惯

- 用户说中文（简体），回复也用中文。
- 密码管理用 KeePass（`.kdbx`）；非机密个人信息用本地 SQLite 库 `data/id.db`（所有身份共用，`identity` 列区分，表结构见 `template/data/schema.sql`）。
- 维护方法都记录在 `docs/MAINTENANCE.md`，别凭记忆瞎猜命令。
- 后续计划接入 Syncthing 同步和定期备份，相关脚本放 `scripts/`。
