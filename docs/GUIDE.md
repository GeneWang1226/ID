# 操作指引

日常使用看这一篇就够。安装依赖、故障排查等见 [MAINTENANCE.md](MAINTENANCE.md)。

## 命令速查

全部在 `~/ID` 下执行，输入 `make help` 可随时查看。参数统一用 `id=` 指身份；**任何参数都可以不给，缺的会逐个询问**。选择已有身份时可用 ↑/↓ 高亮、Enter 确认；比如 `make pass-add` 会先选身份，再问标题、用户名和主密码。

> 提交代码或开启同步前，先跑 `make check` 自检：ignore 规则、git 追踪、权限、明文机密一次查完。

| 命令 | 作用 |
| --- | --- |
| `make new id=gene` | 新增身份 |
| `make passwd id=gene` | 修改某身份 kdbx 主密码 |
| `make pass id=gene title="..."` | 复制密码；SSH 复制无效时输入 `v`、Enter 回退明文显示 |
| `make pass-show id=gene title="..."` | 在当前交互终端明文显示密码，会留在终端记录 |
| `make clipboard-check` | 重新测试当前 SSH 连接的本地剪贴板，只使用测试文字 |
| `make pass-list id=gene` | 列出该身份 kdbx 里所有条目 |
| `make pass-add id=gene` | 新增条目并自动生成强密码（逐个询问标题/用户名） |
| `make pass-add-manual id=gene title="..."` | 新增条目，手动输入密码 |
| `make grant id=gene title="..."` | 签发临时凭证，让 AI 不见明文地用密码 |
| `make use token=... cmd='...'` | 用凭证代执行命令（密码经 `$ID_SECRET` 注入） |
| `make grants` / `make revoke` | 列出活跃凭证 / 撤销（`revoke all=1` 全撤） |
| `make web` / `make web-stop` / `make web-logs` | 启停 Web 界面、看日志 |
| `make show id=gene` | 打印该身份的全部条目和事件 |
| `make db` | 打开 SQLite 交互查询（退出用 `.quit`） |
| `make item id=gene` | 写/改条目（逐个询问分类/名称/内容/备注） |
| `make event id=gene` | 记事件（逐个询问标题/详情） |
| `make settings` | 修改 AI 设置、API key，以及全局“明文显示密码”权限 |
| `make ai id=jacob` / `make ai id=jacob file=资料.txt` | AI 整理资料（**先指定身份**；交互模式空行提交，显示 `AI ›` 回复与草稿，确认后才写 SQLite；不写 kdbx，不涉及主密码） |
| `make check` | 安全自检（提交/同步前必跑） |
| `make status` | 身份、条目数、Web 状态总览 |
| `make backup` | 备份 SQLite 库到 `backups/` |

也支持全部参数一次给全（适合脚本或让 AI 代跑）：
`make item id=gene cat=account key=proton-email value="gene@proton.me" note="主用邮箱"`

## 场景一：新增一个身份

```bash
make new id=gene
```

然后做三件事：

1. 填 `identities/gene/profile.md`（档案与喜好）
2. `make passwd id=gene` 把空白库改成该身份自己的主密码（第一条提示输 `config/secrets/.secrets.env` 里的初始密码）
3. 有身份专属规则就补 `identities/gene/AGENTS.md`

同一身份要开多个「分身」（多个手机号、多套银行卡）：`make new id=gene_2`，在 `profile.md` 写明用途。

## 场景二：刚注册了一个账号

三步，密码和信息分开存：

1. **密码 → kdbx（自己操作）**：`make pass-add id=gene`，按提示输标题和用户名，自动生成 20 位强密码，把它填到注册页面；已有密码用 `make pass-add-manual`
2. **账号信息 → SQLite（可让 AI 代劳）**：`make item id=gene`，按提示逐项填
3. **记一笔事件**：`make event id=gene`

> 注意：`make item` / `make event` 的值里别用单引号，用双引号包整个值。

### 用 AI 整理（`make ai`）

先运行 `make settings` 设置模型、接口地址、思考模式和 API key。非敏感设置写入可同步的 `config/config.toml`，API key 写入权限为 600 的 `config/secrets/.secrets.env`。然后运行 `make ai`，用方向键选择身份，粘贴杂乱资料并输入空行提交；也可以用 `make ai id=gene file=资料.txt`。AI 会先打印 `AI ›` 回复和条目/事件草稿，**确认后才写库**；要保存新机密时会给出带预填标题的 `make pass-add-manual` 命令，按回车可直接进入密码录入；要查看已有机密时会给出 `make pass` 命令，将密码复制到剪贴板。密码内容仍由你在终端输入或粘贴。

新设备先执行 `cp config/secrets/.secrets.env.example config/secrets/.secrets.env && chmod 600 config/secrets/.secrets.env`，再运行 `make settings` 输入 API key。旧 `docker/.env` 中的 `ai_api_key` 和 `kdbx_initial_password` 迁移到 `config/secrets/.secrets.env` 后即可删除旧值；兼容读取只用于过渡。

首次在 SSH 连接中运行 `make ai` 或复制密码时，会做一次剪贴板自测：发送无敏感的测试文字，请你粘贴到隐藏输入框并按 Enter；不能粘贴就直接 Enter，`q` 可跳过。测试会覆盖当前剪贴板。未通过或跳过时，程序先显示全局提示；未通过时不再向该连接尝试发送密码，AI 菜单改为提供明文查看（须已允许）。自测不解锁密码库，不查询本地剪贴板内容，也不会保存粘贴的文字。仅缓存本次连接的检测结果，换一个 SSH 连接会重新检测；可随时运行 `make clipboard-check` 重测。

保存密码后，下一步可选择“复制刚保存的密码到剪贴板”，等价于执行 `make pass id=gene title="GitHub 账户密码"`。SSH 下会发送 OSC 52 复制请求，请在 10 秒内粘贴，之后发送清空请求。复制无效时，在同一提示中输入 `v` 并按 Enter 即可明文显示，不需要退出 SSH 或另开本地管道。直接 Enter、超时或 Ctrl+C 都不会显示明文。自测成功只确认当时可用，不能保证每次复制均成功。

AI 菜单也可直接选择“明文显示密码”，等价于 `make pass-show id=gene title="GitHub 账户密码"`。明文只显示在当前终端，不回传给模型，不写入 AI 对话或项目日志，但可能保留在终端滚屏或外部会话录制中。两种显示入口均拒绝非交互式终端和输出重定向。

**随时撤销明文权限**：`make settings` → “明文显示密码”切换为“禁止” → 保存并退出。设置写入 `config/config.toml` 的 `[terminal].allow_plaintext`；AI 菜单、`make pass-show` 和 `v` 回退都受这个开关控制。显示前会重新读取配置，因此已打开的会话也会遵守撤销。关闭开关不能清除已经显示过的终端记录；配置缺失或无效时默认禁止明文显示。

再次运行 `make settings` 即可热加载修改后的 `config/config.toml`，无需重启 CLI；Web 使用同一份非敏感配置。

- 云端 API（默认阿里云百炼 `qwen-plus`）：整理质量稳，但**只发非机密资料，绝不粘贴密码/授权码/助记词**
- 本地模型：运行 `make settings` 把接口改为 `http://localhost:11434/v1`、模型改为 `qwen3:8b`（需先装 Ollama），数据不出机，但小服务器上较慢

## 场景三：查看和修改信息

- 不想敲命令：`make web` 后开 http://localhost:3000 （访问密码在 `docker/.env` 的 `WEB_PASSWORD`）
- **AI 助手**：Web 左侧「✦ AI 助手」进对话页——粘贴资料或附加文本文件，AI 整理成条目/事件草稿，**勾选确认后才写库**；机密只列提醒。对话和草稿都存库可回看，每条回复可展开「模型原始输出」。API key 来自 `config/secrets/.secrets.env`，接口和模型来自 `config/config.toml`。
- 命令行：`make db` 进 SQLite 交互模式，查询示例见 `template/data/schema.sql` 末尾注释

## 场景四：让 AI 用密码干活（分级授权）

原则：**AI 永远看不到明文密码**，只能拿到一个临时凭证 token，由 broker 脚本把密码注入子进程执行，输出自动打码。

1. AI 说需要某个账号的密码时，你在终端签发凭证：

```bash
make grant id=jacob title="126 授权码 jacob_ng"     # 输一次主密码
# 输出一个 token 和过期时间，把 token 发给 AI
```

2. AI 用凭证代执行（密码经 `$ID_SECRET` 环境变量传给命令，不进参数、不显示）：

```bash
make use token=<token> cmd='curl -u "user:$$ID_SECRET" https://example.com'
# 注意：cmd 里用双引号、别用单引号；经 make 传参时 $ 要写成 $$
```

3. 随时 `make grants` 看活跃凭证，`make revoke token=...` 或 `make revoke all=1` 撤销。

**分级**（给条目打 Tag：`make tag id=jacob title="..." tags=ai-deny`，或在 KeePassXC 图形客户端打）：

| Tag | 行为 | 适合 |
| --- | --- | --- |
| （无标签） | 一次性凭证，用一次即销毁（默认 5 分钟有效） | 大多数账号 |
| `ai-auto` | 限时凭证，TTL 内可多次使用（默认 10 分钟，`ttl=` 可调，上限 1 小时） | 低价值、AI 常用的账号 |
| `ai-deny` | 拒绝签发，只能 `make pass` 复制剪贴板或手动输入 | 主邮箱、金融、恢复码 |

凭证放在用户级 tmpfs（`/run/user/<uid>/id-grants/`），**重启即清空**；日志只记「什么时间授权了哪个条目」，绝不记密码。

> 网页表单等代执行够不到的场景：AI 会请你 `make pass` 复制到剪贴板（10 秒自动清空）后手动粘贴。

## 场景五：换一台新设备

1. 装 KeePassXC、Docker、Syncthing（见 [MAINTENANCE.md](MAINTENANCE.md)）
2. `git clone <私有仓库> ~/ID` 拿到脚本、模板和文档
3. Syncthing 把 `~/ID` 加为同步文件夹，等 `identities/`、`data/id.db` 同步过来；`config/config.toml` 由 git 部署
4. 在新设备复制 `config/secrets/.secrets.env.example` 为 `config/secrets/.secrets.env`，填写 secrets 并确认权限是 `600`；复制 `docker/.env.example` 为 Docker 配置后也设为 `600`

## 场景六：删除一个身份

先确认，再执行：

```bash
rm -rf identities/<身份名>
sqlite3 data/id.db "DELETE FROM items WHERE identity='<身份名>'; DELETE FROM events WHERE identity='<身份名>';"
```
