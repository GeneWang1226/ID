# 操作指引

日常使用看这一篇就够。安装依赖、故障排查等见 [MAINTENANCE.md](MAINTENANCE.md)。

## 命令速查

全部在 `~/ID` 下执行，输入 `make help` 可随时查看：

| 命令 | 作用 |
| --- | --- |
| `make new name=gene` | 新增身份 |
| `make passwd name=gene` | 修改某身份 kdbx 主密码 |
| `make pass name=gene title="..."` | 复制密码到剪贴板（10 秒后自动清空） |
| `make pass-show name=gene title="..."` | 显示整条记录（无桌面/剪贴板时用） |
| `make pass-add name=gene title="..." user="..."` | 新增条目并自动生成强密码 |
| `make pass-add-manual name=gene title="..."` | 新增条目，手动输入密码 |
| `make web` / `make web-stop` / `make web-logs` | 启停 Web 界面、看日志 |
| `make show identity=gene` | 打印该身份的全部条目和事件 |
| `make db` | 打开 SQLite 交互查询（退出用 `.quit`） |
| `make item identity=gene cat=account key=proton-email value="gene@proton.me"` | 写/改条目 |
| `make event identity=gene title="注册 Proton 邮箱"` | 记事件 |
| `make status` | 身份、条目数、Web 状态总览 |
| `make backup` | 备份 SQLite 库到 `backups/` |

## 场景一：新增一个身份

```bash
make new name=gene
```

然后做三件事：

1. 填 `identities/gene/profile.md`（档案与喜好）
2. `make passwd name=gene` 把空白库改成该身份自己的主密码（第一条提示输 `config` 里的初始密码）
3. 有身份专属规则就补 `identities/gene/AGENTS.md`

同一身份要开多个「分身」（多个手机号、多套银行卡）：`make new name=gene_2`，在 `profile.md` 写明用途。

## 场景二：刚注册了一个账号

三步，密码和信息分开存：

1. **密码 → kdbx（自己操作）**：`make pass-add name=gene title="Proton 邮箱" user="gene@proton.me"` 会自动生成 20 位强密码，把它填到注册页面；已有密码用 `make pass-add-manual`
2. **账号信息 → SQLite（可让 AI 代劳）**：
   `make item identity=gene cat=account key=proton-email value="gene@proton.me" note="主用邮箱"`
3. **记一笔事件**：
   `make event identity=gene title="注册 Proton 邮箱" detail="密码在 kdbx"`

> 注意：`make item` / `make event` 的值里别用单引号，用双引号包整个值。

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
