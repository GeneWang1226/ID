# 使用说明

## 身份与解锁

运行 `make manage` 后，用方向键选择身份，Enter 解锁。也可用 `make manage id=example` 直接打开。各身份使用独立 KDBX；同一进程只缓存当前库的主密码，切换库、退出、锁定或空闲五分钟会清除缓存。

管理界面、AI 助手和设置会一直保持全屏。主密码、条目密码、受保护字段与 API key 在界面中央输入，已输入时显示固定圆点，不显示内容或长度；Enter 确认、Backspace 删除、Ctrl+U 清空、Esc / Ctrl+C 取消。窗口缩放后会重新排版，退出后恢复原终端。独立 CLI 命令保留原来的隐藏输入提示。

`make new id=example` 会让你设置并确认主密码，完成后自动在原生 `ID_info` 群组创建 `example` 基础信息条目，填入“身份名”。没有共享初始密码；输入为空、不一致或取消时不创建身份。已有库不会因再次执行 `make new` 被覆盖或重置。

身份保存在 `identities/example.kdbx`，不再需要 `passwords`、`notes`、`documents` 等子目录。程序兼容未整理的旧路径；新旧两处同时有库时会提示冲突，需核对后归档旧目录。旧资料归档在 `backups/legacy-identities/`，不代表其中的内容已全部导入 KDBX。

移除身份用 `make remove id=example`，省略 `id` 可先选择。默认取消；确认后需重新验证主密码。成功后原文件完整移到本机 `backups/removed-identities/`，身份不再出现在列表中，临时凭证撤销；旧布局会连同旧档案、密钥等整个目录归档。取消、密码错误或其他 ID 写入占用时不移动文件。不会清理既有备份或导出文件。恢复与同步注意事项见维护文档。

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

群组标题显示 KDBX 中的实际路径，展开详情可查看所属群组。新身份的基础信息在 `ID_info/<身份名>`，后续字段继续加入同一条目；兼容旧库的 `ID/身份信息/基础信息`。账号和事件默认进入 `ID/账号`、`ID/事件`，按需创建；已有条目在原群组直接编辑。群组可在 KeePassXC 中改名，项目仍按 UUID 和原生 CustomData 识别，不会仅因改名重复建组。新建账号也可指定已有群组的完整路径，例如 `make item id=example cat=account key='工作/GitHub' value=example-user`；目标群组须已存在。已有同名账号的登录名不会被覆盖，修改请用 `make manage`。

运行 `make organize id=example` 可将已有基础信息统一为 `ID_info/example`，包括已放在旧项目群组里的条目；同时整理带项目标记的旧根条目。保留原群组及条目 UUID、字段保护、密码、附件与历史，本地解锁、确认后才会保存，并留加密备份。其他用户群组保持原位置；同名或多份基础信息冲突时中止，重复执行不会重复建条目。没有基础信息时仅填入身份名，不自动读取旧 SQLite 或档案。

除身份信息外，原生群组可按用途组织。以下是按需使用的命名建议，不会自动创建空群组或按内容猜测分类：

| 群组建议 | 适合存放 |
| --- | --- |
| `ID_accounts` | 网站、应用账号；用户名、密码及恢复资料集中在所属条目（当前默认 `ID/账号`） |
| `ID_emails` | 邮箱账号专用群组；邮箱地址放登录名，恢复资料和服务器设置放独立属性 |
| `ID_keys` | 独立 API 凭据、SSH 密钥、证书；密钥使用受保护字段或附件 |
| `ID_documents` | 证件、合同、恢复文档等独立资料；原文件作为原生附件 |
| `ID_events` | 重要变更、到期或续费等事件 |

这些都是普通 KeePass 群组，名称可自定；普通账号与事件默认使用 `ID/账号`、`ID/事件`，邮箱使用根群组 `ID_emails`。普通字段、受保护属性、附件都属于条目，群组只是分类，不提供额外的访问隔离。相关数据能力见 [KeePassXC 用户指南](https://keepassxc.org/docs/KeePassXC_UserGuide#_advanced_entry_handling)。

### 邮箱账号

在管理界面按 `n` →“邮箱”，填写名称、邮箱地址和备注；邮箱与其他账号共用“账号”和“密码”视图，按原生群组分开显示。也可用 `make item id=example cat=email key=Proton value=example@proton.me note='备用邮箱'`。已有普通账号可展开后按 `o` →“归为邮箱”，确认后原条目移入 `ID_emails`，保留 UUID、密码、属性和附件；网站用邮箱登录不会因此自动成为邮箱条目。

邮箱展开后按 `o` →“邮箱常用属性”，可直接选择服务商、地区、用途、应急邮箱、应急手机号、恢复码、恢复短语、授权码、应用密码、IMAP 服务器或 SMTP 服务器。只保存实际填写的属性，不建立空占位；额外属性仍可自定义。邮箱地址使用原生 `UserName`，密码使用 `Password`，网页入口使用 `URL`。恢复码、恢复短语、授权码和应用密码强制受保护，按 `c` 在本地复制或查看。

已导入的邮箱运行 `make organize id=example` 后归组。仅识别明确的邮箱类型、邮箱群组、邮箱导入标记，或项目账号的已知邮箱服务商属性，不靠登录名含 `@` 猜测其他账号类型。整理会把已知旧属性（例如 `proton_recovery_phrase`、`详细数据 · 授权码`）补充成“恢复短语”“授权码”等常用属性，保留原字段和完整源记录；目标已有不同值时不覆盖。含义不明的备注、JSON 或原始记录不自动拆解。群组改名后按 UUID 和用途标记继续识别；目标路径冲突时中止，不覆盖密码库。

个人信息保存在“基础信息”的自定义字段。Enter 展开后能看到完整值、维度与备注；“编辑字段”修改已有值，“添加字段”增加属性。恢复码、密钥等选择“受保护内容”，详情中只显示掩码。展开条目后按 `c`，或在 `o` 菜单选择“取用字段”，方向键选字段，再选择“复制字段”或“临时查看”。两种取用都重新验证主密码；临时查看还需设置允许明文。查看支持多行、↑↓ / PgUp / PgDn 滚动，Enter / Esc 隐藏并返回；设置撤销明文权限后会立即隐藏。受保护字段的内容不进入普通列表、操作日志或 AI 对话。

图标选择与 KeePass 标准编号对应；其他客户端改变标准图标后刷新即可显示。自定义图片原样保留，在 TUI 中以原生备用图标和“自定义图标”说明表示。

## 密码与剪贴板

```sh
make pass id=example title=GitHub
make pass id=example title=GitHub show=1
make pass id=example title=Proton field='恢复短语'
make pass id=example title=Proton field='恢复短语' show=1
make pass-set id=example title=GitHub
make pass-set id=example title=GitHub generate=1
make pass-set id=example title=Cloudflare field=恢复码
make clipboard-check
```

`pass-set` 同时支持新增和修改：已有账号直接补填或替换，不新增重复条目；替换已有密码会重新验证。`generate=1` 在本地生成 32 位随机密码。不指定标题时，可逐个选择账号或新增条目。

`pass-set field=恢复码` 等自定义属性录入只操作已有条目，不会新建密码条目或替换账号密码。省略标题时选择已有账号，重新验证后隐藏输入，支持终端多行粘贴并保留前导零；按 Enter 保存，空内容或取消不修改。替换已有字段需确认，保存沿用加密备份并保留字段备注。读取使用 `make pass id=example title=Cloudflare field=恢复码`。

`pass field=字段名` 取用对应的自定义字段，省略标题时选择包含该字段的条目，包括基础信息条目。字段名须与库中一致；不存在时报告错误，不回退到密码。原生 CLI 使用 `id pass --id example --title Proton --field 恢复短语`，加 `--show` 明文查看。独立 CLI 的明文输出可能留在终端滚屏中；TUI 临时查看返回后清屏，但终端录制仍可能保留内容。

SSH 下使用 OSC 52 适配本地终端，并提供人工粘贴自测。终端支持不能仅靠环境变量可靠判断。无法复制时不会声称成功；可在 `make settings` 允许明文显示，并随时撤销。明文显示会进入终端滚屏记录。

## 附件

展开条目，按 `o` 选择导入附件，再选择“浏览文件”或“输入 / 粘贴路径”。浏览器用 ↑↓ 选择、Enter 进入目录或导入文件、← / Backspace 返回上级、PgUp / PgDn 翻页、h 切换隐藏文件、p 输入路径、q / Esc 取消。路径支持 `~/`、空格、单/双引号及反斜线转义；Tab 补全，多个匹配可用方向键选择。取消路径输入返回浏览，Ctrl+C 取消整个文件选择。选好文件后才再次验证主密码；原文件保留，附件加密存入 KDBX，不送给 AI。PDF 等文件不必先提取成文字。

SSH 下明确显示“服务器文件”，只能选择服务器能访问的路径。电脑上的文件须先经 SFTP 或配置好的 trzsz 等工具上传，再在 TUI 导入；普通 SSH 中拖入本地路径不会自动上传。也可在文件所在设备用 TUI 或 KeePassXC 添加附件，再通过 Syncthing 同步加密 KDBX。上传或导入不会自动删除明文原文件。

导出附件需要再次验证和确认，写到项目的 `exports/<身份>/`，不会自动同步。

## AI

`make ai` 打开全屏助手。空行提交，`/lock` 锁定，`/quit` 退出；模型细节默认折叠，可按 F2 查看。

AI 可按请求查询普通资料和账号是否已填密码；姓名、电话等写入基础信息，具体服务账号写入独立条目。草稿确认后才保存。没有提供登录名时，会提供本地账号表单；密码在本地录入。受保护字段、附件内容及密码不送给模型，不要在聊天中输入它们。

接着账号操作说“保存双因素身份验证恢复代码”时，AI 会提供所属账号的 `field=恢复码` 本地操作。模型把字段拼进标题时，本地会尝试匹配已有账号；无法唯一确定则让你选择，不会为恢复资料新建账号。

AI 代用密码仍通过 `make grant` / `make use` 临时授权，凭据不放命令参数。详见维护文档。

## 多端切换

Debian 和 NAS 各自运行 Syncthing 时，用 `make settings` →“Syncthing 同步交接”配置本机接口与 API key，选择同步文件夹和 NAS，开启后保存。之后打开身份前自动等待同步，修改后退出时等待 NAS 收到完整更新；看到“可以切换设备”再去另一端打开。NFS/SMB 仍可用来查看 NAS。超时、离线或冲突会明确提示，不会自动忽略；详见[维护文档](MAINTENANCE.md#syncthing)。

## 命令写入普通资料

```sh
make item id=example cat=id-info key=姓名 value='Example Name' note='常用姓名'
make item id=example cat=contact key=电话号码 value='5550100' note='工作号码'
make item id=example cat=account key=GitHub value=example-user note='开发账号'
make event id=example title='更新恢复方式' detail='已在本地核对'
make manage id=example type=info
```

## 命令整理

`make help` 按用途分组；日常录入、查看与附件操作集中在 `manage`，`item`、`event` 与授权命令供自动化使用。以下重复命令及对应 Rust CLI 子命令已移除：

| 旧 Make 命令 | 现在使用 |
| --- | --- |
| `list`、`show` | `manage` |
| `pass-list` | `manage type=password` |
| `pass-add-manual`、`pass-edit` | `pass-set` |
| `pass-add` | `pass-set generate=1` |
| `pass-show` | `pass show=1` |

直接调用 Rust CLI 时，对应参数为 `id pass-set --generate`、`id pass --show`；所有身份移除都必须本地确认，没有免确认删除选项。

`item`、`event` 在本地解锁 KDBX 后写入普通资料。机密字段请使用管理界面的受保护输入。
