# AGENTS.md — 给 AI 助手的全局指引

这个目录（`~/ID`）存放**多个身份的敏感信息**。任何 AI 助手在这里工作时，必须先读完本文件再动手。

## 这里是什么

- `template/` — 身份模板，由用户维护。**改模板只影响新建身份**，不要动它，除非用户明确要求。
- `identities/<name>/` — 一个具体身份。进入某个身份目录前，先读里面的 `AGENTS.md` 和 `profile.md`。
- `src/` — Rust CLI 源码。新增身份**必须用** `make new id=<name>`，不要手工复制目录。

## 铁律（不可违反）

1. **不泄露**：身份目录的内容（密码库、密钥、恢复码、档案）不得发送到任何外部服务，不得写入本目录之外的文件，不得在回复中明文展示秘密内容。
2. **不解密**：不要尝试破解、绕过或直接解析 `.kdbx` 文件。密码相关操作引导用户使用 KeePass(XC)。
3. **不擅自删除**：删除身份或身份内的任何文件前，必须得到用户明确确认。
4. **不动模板**：除非用户明确要求，否则 `template/` 只读。
5. **最小访问**：只读与当前任务直接相关的文件，不要漫无目的地翻身份目录。
6. **日志红线**：`logs/scripts.log` 只记「什么时间做了什么操作」，绝不写入密码、token、秘密内容。身份的大事记作为事件条目存入该身份 KDBX，不写日志文件。
7. **git 只跟踪项目，不跟踪秘密**：仓库用来把源码、模板和文档部署到新设备。`identities/`、`docker/.env`、`config/secrets/.secrets.env`、`*.kdbx`、`.turso.env`、`logs/`、`exports/` 永不入库（模板 `docker/.env.example`、`config/secrets/.secrets.env.example` 和非敏感 `config/config.toml` 要入库），禁止 `git add -f`。除非用户明确要求，不要 `git commit` 或推远程。不要改 git config。身份文件的历史靠 Syncthing，不靠 git。
8. **AI 不碰明文密码**：AI 需要密码时，只能请用户运行 `make grant id=<身份> title=<条目>` 签发临时凭证，然后用 `make use token=<token> cmd='...'` 代执行（密码经 `$ID_SECRET` 注入子进程）。禁止读 `/run/user/*/id-grants/` 下的任何文件，禁止打印/回显 `$ID_SECRET`，禁止把密码拼进命令行参数（argv 会进进程列表）。网页表单等代执行够不到的场景，请用户 `make pass` 复制剪贴板后手动粘贴。

当前存储约定覆盖旧身份模板中的 SQLite、Turso、档案和零散笔记写入指引；旧文件只保留供人工迁移，不再作为日常资料存储。

## 常见任务怎么做

| 任务 | 做法 |
| --- | --- |
| 新增身份 | 运行 `make new id=<name>`，由用户在终端设置并确认独立主密码；不再读取配置中的初始密码。自动创建原生 `ID_info/<身份名>` 基础信息条目，再用 `make manage` 补充字段。已有库不覆盖、不重置 |
| 记录身份偏好 | 请用户通过 `make manage` 写入该身份 KDBX 的“基础信息”条目 |
| 找某个身份的信息 | 先读该身份的 `AGENTS.md` 和 `profile.md`；KDBX 内容由用户本地解锁查询，不要由 AI 导出密码库 |
| 让 AI 用密码干活 | 请用户 `make grant` 签发临时凭证，AI 只拿 token 用 `make use` 代执行；条目 Tags 分级：`ai-auto` 限时多次、无标签一次性、`ai-deny` 禁止 |
| 存/查非机密个人信息 | 通过本地 CLI 存入当前身份 KDBX 的“基础信息”原生字段；账号为独立条目，机密字段必须受保护 |
| 改 CLI | 修改 `src/`，运行 `make test`；测试只使用临时构造的身份和密码库 |
| 安装依赖、改主密码等维护操作 | 按 `docs/MAINTENANCE.md` 执行 |

## 用户习惯

- 用户说中文（简体），回复也用中文。
- 所有身份资料统一保存在各自 KeePass（`.kdbx`）；新建身份在原生 `ID_info` 群组中创建以身份名命名的基础信息条目，账号与密码为同一条目。日常编辑兼容旧群组，通过 UUID 和 CustomData 识别。用户显式运行 `make organize` 并本地解锁、确认后，基础信息才统一为 `ID_info/<身份名>`，同时整理旧项目根条目；保留原 UUID、字段保护、密码、附件和历史，其他用户群组不搬动。SQLite 只允许由用户显式迁移，原文件不得擅自删除。
- 维护方法都记录在 `docs/MAINTENANCE.md`，别凭记忆瞎猜命令。
- 后续计划接入 Syncthing 同步和定期备份，相关实现放 `src/`。
