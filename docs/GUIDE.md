# 操作指引

日常使用看这一篇就够。安装依赖、故障排查等见 [MAINTENANCE.md](MAINTENANCE.md)。

## 命令速查

全部在 `~/ID` 下执行，输入 `make help` 可随时查看。参数统一用 `id=` 指身份；**任何参数都可以不给，缺的会逐个询问**——比如 `make pass-add id=gene` 会先问标题、用户名，再提示输主密码。

| 命令 | 作用 |
| --- | --- |
| `make new id=gene` | 新增身份 |
| `make passwd id=gene` | 修改某身份 kdbx 主密码 |
| `make pass id=gene title="..."` | 复制密码到剪贴板（10 秒后自动清空） |
| `make pass-show id=gene title="..."` | 显示整条记录（无桌面/剪贴板时用） |
| `make pass-list id=gene` | 列出该身份 kdbx 里所有条目 |
| `make pass-add id=gene` | 新增条目并自动生成强密码（逐个询问标题/用户名） |
| `make pass-add-manual id=gene` | 新增条目，手动输入密码 |
| `make web` / `make web-stop` / `make web-logs` | 启停 Web 界面、看日志 |
| `make show id=gene` | 打印该身份的全部条目和事件 |
| `make db` | 打开 SQLite 交互查询（退出用 `.quit`） |
| `make item id=gene` | 写/改条目（逐个询问分类/名称/内容/备注） |
| `make event id=gene` | 记事件（逐个询问标题/详情） |
| `make ai` / `make ai file=资料.txt` | AI 整理一段资料，确认后写入（先在 `config` 填 `ai_api_key`） |
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
2. `make passwd id=gene` 把空白库改成该身份自己的主密码（第一条提示输 `config` 里的初始密码）
3. 有身份专属规则就补 `identities/gene/AGENTS.md`

同一身份要开多个「分身」（多个手机号、多套银行卡）：`make new id=gene_2`，在 `profile.md` 写明用途。

## 场景二：刚注册了一个账号

三步，密码和信息分开存：

1. **密码 → kdbx（自己操作）**：`make pass-add id=gene`，按提示输标题和用户名，自动生成 20 位强密码，把它填到注册页面；已有密码用 `make pass-add-manual`
2. **账号信息 → SQLite（可让 AI 代劳）**：`make item id=gene`，按提示逐项填
3. **记一笔事件**：`make event id=gene`

> 注意：`make item` / `make event` 的值里别用单引号，用双引号包整个值。

### 用 AI 整理（`make ai`）

在 `config` 里填 `ai_api_key` 后，把杂乱资料粘贴进去（或 `make ai file=资料.txt`），AI 会整理成条目/事件草稿，**确认后才写库**；识别到的密码、授权码、助记词只列提醒，由你手动 `make pass-add-manual` 存 kdbx。

- 云端 API（默认阿里云百炼 `qwen-plus`）：整理质量稳，但**只发非机密资料，绝不粘贴密码/授权码/助记词**
- 本地模型：`config` 里换 `ai_base_url=http://localhost:11434/v1`、`ai_model=qwen3:8b`（需先装 Ollama），数据不出机，但小服务器上较慢

## 场景三：查看和修改信息

- 不想敲命令：`make web` 后开 http://localhost:3000 （默认密码 `changeme`，在 `docker-compose.yml` 改）
- 命令行：`make db` 进 SQLite 交互模式，查询示例见 `template/data/schema.sql` 末尾注释

## 场景四：换一台新设备

1. 装 KeePassXC、Docker、Syncthing（见 [MAINTENANCE.md](MAINTENANCE.md)）
2. `git clone <私有仓库> ~/ID` 拿到脚本、模板和文档
3. Syncthing 把 `~/ID` 加为同步文件夹，等 `identities/`、`config`、`data/id.db` 同步过来
4. 确认 `config` 权限是 `600`，`make status` 检查一遍

## 场景五：删除一个身份

先确认，再执行：

```bash
rm -rf identities/<身份名>
sqlite3 data/id.db "DELETE FROM items WHERE identity='<身份名>'; DELETE FROM events WHERE identity='<身份名>';"
```
