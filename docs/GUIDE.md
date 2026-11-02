# 使用说明

## 身份与解锁

运行 `make manage` 后，用方向键选择身份，Enter 解锁。也可用 `make manage id=example` 直接打开。各身份使用独立 KDBX；同一进程只缓存当前库的主密码，切换库、退出、锁定或空闲五分钟会清除缓存。

管理界面、AI 助手和设置会一直保持全屏。主密码、条目密码、受保护字段与 API key 在界面中央输入，已输入时显示固定圆点，不显示内容或长度；Enter 确认、Backspace 删除、Ctrl+U 清空、Esc / Ctrl+C 取消。窗口缩放后会重新排版，退出后恢复原终端。独立 CLI 命令保留原来的隐藏输入提示。

`make new id=example` 会让你设置并确认主密码，完成后自动在原生 `ID_info` 群组创建 `example` 基础信息条目，填入“身份名”。没有共享初始密码；输入为空、不一致或取消时不创建身份。已有库不会因再次执行 `make new` 被覆盖或重置。

身份保存在 `identities/example.kdbx`，不再需要 `passwords`、`notes`、`documents` 等子目录。程序兼容未整理的旧路径；新旧两处同时有库时会提示冲突，需核对后归档旧目录。旧资料归档在 `backups/legacy-identities/`，不代表其中的内容已全部导入 KDBX。

## TUI

| 按键 | 操作 |
| --- | --- |
| ↑ / ↓ | 选择条目 |
| ← / → | 翻页 |
| PgUp / PgDn | 翻页；展开时滚动完整详情 |
| Enter | 展开或收起详情 |
| 1 / 2 / 3 / 4 | 全部 / 账号 / 已填密码 / 基础信息 |
| n | 新建账号、基础信息字段或事件 |
| d | 基础信息按维度筛选 |
| o | 备注、字段、密码、图标与附件操作 |
| e / f / x | 编辑备注 / 删除字段 / 删除整条 |
| r / l / q | 刷新 / 锁定 / 退出 |

账号显示登录名和密码状态，不需要再关联另一个“密码记录”。`type=password` 仅列出密码已填的账号；未填密码的账号在全部和账号视图查看。

群组标题显示 KDBX 中的实际路径，展开详情可查看所属群组。新身份的基础信息在 `ID_info/<身份名>`，后续字段继续加入同一条目；兼容旧库的 `ID/身份信息/基础信息`。账号和事件默认进入 `ID/账号`、`ID/事件`，按需创建；已有条目在原群组直接编辑。群组可在 KeePassXC 中改名，项目仍按 UUID 和原生 CustomData 识别，不会仅因改名重复建组。新建账号也可指定已有群组的完整路径，例如 `make item id=example cat=account key='工作/GitHub' value=example-user`；目标群组须已存在。

运行 `make organize id=example` 可将已有基础信息统一为 `ID_info/example`，包括已放在旧项目群组里的条目；同时整理带项目标记的旧根条目。保留原群组及条目 UUID、字段保护、密码、附件与历史，本地解锁、确认后才会保存，并留加密备份。其他用户群组保持原位置；同名或多份基础信息冲突时中止，重复执行不会重复建条目。没有基础信息时仅填入身份名，不自动读取旧 SQLite 或档案。

除身份信息外，原生群组可按用途组织。以下是按需使用的命名建议，不会自动创建空群组或按内容猜测分类：

| 群组建议 | 适合存放 |
| --- | --- |
| `ID_accounts` | 邮箱、网站、应用账号；用户名、密码、恢复码、应急邮箱及恢复 PDF 集中在所属条目 |
| `ID_keys` | 独立 API 凭据、SSH 密钥、证书；密钥使用受保护字段或附件 |
| `ID_documents` | 证件、合同、恢复文档等独立资料；原文件作为原生附件 |
| `ID_events` | 重要变更、到期或续费等事件 |

这些都是普通 KeePass 群组，名称可自定；当前账号与事件默认仍使用已有的 `ID/账号`、`ID/事件`。普通字段、受保护属性、附件都属于条目，群组只是分类，不提供额外的访问隔离。相关数据能力见 [KeePassXC 用户指南](https://keepassxc.org/docs/KeePassXC_UserGuide#_advanced_entry_handling)。

个人信息保存在“基础信息”的自定义字段。Enter 展开后能看到完整值、维度与备注；“编辑字段”修改已有值，“添加字段”增加属性。恢复码、密钥等选择“受保护内容”，输入不回显，详情中只显示掩码。需要查看这类受保护自定义字段时使用 KeePassXC；本版 TUI 不提供它们的明文查看或复制入口。

图标选择与 KeePass 标准编号对应；其他客户端改变标准图标后刷新即可显示。自定义图片原样保留，在 TUI 中以原生备用图标和“自定义图标”说明表示。

## 密码与剪贴板

```sh
make pass id=example title=GitHub
make pass-add-manual id=example title=GitHub
make pass-edit id=example title=GitHub
make pass-add id=example title=GitHub
make clipboard-check
```

录入密码时，已有空密码账号直接补填，不新增重复条目；替换已有密码会重新验证。`pass-add` 在本地生成 32 位随机密码。

SSH 下使用 OSC 52 适配本地终端，并提供人工粘贴自测。终端支持不能仅靠环境变量可靠判断。无法复制时不会声称成功；可在 `make settings` 允许明文显示，并随时撤销。明文显示会进入终端滚屏记录。

## 附件

展开条目，按 `o` 选择导入附件，再选择“浏览文件”或“输入 / 粘贴路径”。浏览器用 ↑↓ 选择、Enter 进入目录或导入文件、← / Backspace 返回上级、PgUp / PgDn 翻页、h 切换隐藏文件、p 输入路径、q / Esc 取消。路径支持 `~/`、空格、单/双引号及反斜线转义；Tab 补全，多个匹配可用方向键选择。取消路径输入返回浏览，Ctrl+C 取消整个文件选择。选好文件后才再次验证主密码；原文件保留，附件加密存入 KDBX，不送给 AI。PDF 等文件不必先提取成文字。

SSH 下明确显示“服务器文件”，只能选择服务器能访问的路径。电脑上的文件须先经 SFTP 或配置好的 trzsz 等工具上传，再在 TUI 导入；普通 SSH 中拖入本地路径不会自动上传。也可在文件所在设备用 TUI 或 KeePassXC 添加附件，再通过 Syncthing 同步加密 KDBX。上传或导入不会自动删除明文原文件。

导出附件需要再次验证和确认，写到项目的 `exports/<身份>/`，不会自动同步。

## AI

`make ai` 打开全屏助手。空行提交，`/lock` 锁定，`/quit` 退出；模型细节默认折叠，可按 F2 查看。

AI 可按请求查询普通资料和账号是否已填密码；姓名、电话等写入基础信息，具体服务账号写入独立条目。草稿确认后才保存。没有提供登录名时，会提供本地账号表单；密码在本地录入。受保护字段、附件内容及密码不送给模型，不要在聊天中输入它们。

AI 代用密码仍通过 `make grant` / `make use` 临时授权，凭据不放命令参数。详见维护文档。

## 命令写入普通资料

```sh
make item id=example cat=id-info key=姓名 value='Example Name' note='常用姓名'
make item id=example cat=contact key=电话号码 value='5550100' note='工作号码'
make item id=example cat=account key=GitHub value=example-user note='开发账号'
make event id=example title='更新恢复方式' detail='已在本地核对'
make show id=example
```

这些命令会在本地解锁 KDBX，只用于普通资料。机密字段请使用管理界面的受保护输入。`show` 不显示密码或受保护字段的值。
