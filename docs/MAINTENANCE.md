# 维护、备份与同步

## 安装与检查

安装 Rust 1.89+、Make、KeePassXC CLI（当前测试版本 2.7.10）。Linux 支持完整编辑。无需 Python 运行时、Node、Docker 或数据库服务；Python 仅用于终端集成测试。

Debian 先安装基础依赖，再按 [Rust 官方安装说明](https://rust-lang.org/tools/install/) 安装当前 stable 工具链：

```sh
sudo apt install build-essential curl make keepassxc
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
cargo --version
rustc --version
```

Make 优先使用 PATH 中的 Cargo，也会检查 `${CARGO_HOME:-$HOME/.cargo}/bin/cargo`；自定义工具链可用 `make CARGO=/完整路径/cargo build`。缺少 Cargo 时给出安装提示。源码更新需要重新编译，Rust 工具链应安装在持久目录中，不能只放在 `/tmp`。

```sh
make build
make test
make check
```

项目设置在 `config/config.toml`，每次操作读取最新配置；API key 在 `config/secrets/.secrets.env`，权限应为 600。用 `make settings` 修改。新建身份不再读取 `KDBX_INITIAL_PASSWORD`，主密码由用户当场输入。Docker 的旧本机 `.env` 不会被程序读取，原 SQLite Web 服务已退役。

## 新建、改密与授权

```sh
make new id=example
make passwd id=example
make manage id=example
make tag id=example title=GitHub tags=ai-deny
make grant id=example title=GitHub
make grants
make revoke all=1
```

新建时设置并确认独立主密码，同时创建 `ID_info/<身份名>` 原生基础信息条目，填入“身份名”；主密码仅用于加密，不会作为条目字段或配置保存。完整初始化并验证后才发布 `identities/<身份名>.kdbx`，不覆盖已有库，也不再复制旧模板的子目录、档案或指引。旧版本创建的库仍使用当时的密码，需用 `make passwd` 提供旧密码后更换。条目 Tags：`ai-deny` 禁止 AI 授权，`ai-auto` 限时多次，无标签默认一次性。临时凭据放本机运行时目录，不入 Git 或 Syncthing。

受保护自定义字段通过管理界面的 `c` 或 `make pass field=字段名` 在本地取用，每次重新验证主密码。复制沿用本机 KeePassXC 或 SSH OSC 52 与十秒清理；查看沿用 `terminal.allow_plaintext`，打开的 TUI 字段窗口也会响应撤销权限并清屏。内容不送给模型、不写日志；清屏无法撤销终端录制或已粘贴的内容。

`make use token=... cmd='...'` 使用令牌代执行，秘密只通过子进程环境变量 `ID_SECRET` 注入，不要把秘密拼进命令行参数。Make 中命令里的 `$` 写成 `$$`。不要读取或打印临时凭据文件。

## 旧文件与表格导入

当前程序只使用 KDBX，不提供 SQLite 读取或迁移命令。未整理到 KDBX 的旧资料不会出现在管理界面。旧模板保留只读，程序不再复制。

身份目录采用一身份一文件：`identities/gene.kdbx`。程序兼容旧的 `identities/gene/passwords/gene.kdbx`；同一身份的新旧路径同时存在时拒绝操作，避免静默选择旧库。整理目录时先停止该库的编辑并等待同步完成，将加密库复制到新路径并校验字节完全一致，再把整个旧身份目录移入 `backups/legacy-identities/<整理时间>/`，保留原库、档案、密钥、旧 schema 等全部文件。归档不是全部导入的证明，旧档案可能还有未迁移内容；归档目录可能含明文资料，应继续保管，不进 Git 或 Syncthing。Syncthing 冲突副本保留，不作为新身份列出。

`id import 表格.xlsx --owner 原使用者=example` 仍支持旧七列账号表。账号、地区与服务商入原生字段，备注、密码提示和详细数据作为受保护字段；各身份分别解锁和提交。多身份导入是逐库完成，重复导入会跳过已完成行。原表不会再另存明文备份，也不会删除用户提供的文件。

`make organize id=example` 将现有基础信息统一为原生 `ID_info/example`，同时整理带项目标记的旧根条目，无需再次导入旧资料。基础信息可来自根目录或旧项目群组：复用群组和条目的 UUID，保留字段、保护标记、密码、附件和历史；缺少基础信息时仅创建身份名。若旧项目身份群组被移动，其他内容仍保留在原群组节点中，不删除原有群组或文件。解锁后预检并要求确认，保存前留加密备份；同名目标、重复基础信息或群组标记冲突时中止，保留原库。其他用户群组不搬动。

邮箱条目使用 `ID.AccountType=email`，仍是 `ID.Kind=account`；原生 `ID_emails` 群组使用 `ID.Group=email` 和邮箱图标。`make organize` 也整理明确的邮箱导入记录、邮箱群组及带已知邮箱服务商的项目账号；其他网站与未识别条目不自动迁移，可手动确认“归为邮箱”。邮箱字段归一化仅补充已知别名，保留原值、原字段及保护状态；恢复类属性始终受保护。未知备注和源记录不自动解释或删除。

历史沿用 KeePassXC 原生合并规则；时间戳相同的历史版本可能被合并，保存前的加密备份保留原始库。

## 备份与恢复

`make backup` 复制所有身份的加密 KDBX 到 `backups/vaults/<身份>/`。写入资料、密码或标签前也会备份。备份权限为 600；不会自动清理，定期转存到独立介质并检查磁盘空间。

恢复前退出编辑界面、暂停该身份的同步，保留当前 KDBX，再用 KeePassXC 核对备份并恢复。备份可能使用旧主密码，需妥善保管。删除条目保留加密备份，不代表历史副本中的秘密已经销毁。

## 移除身份

`make remove id=example` 用于移除某个身份，未指定时先选择。需在终端确认并重新验证主密码；取消或验证失败不会移动文件。程序取得该身份写入锁，撤销其本机临时凭证，再将 `identities/example.kdbx` 原样移入 `backups/removed-identities/example-<随机后缀>/`。旧布局则将整个 `identities/example/` 移入归档，连同旧档案、密钥及未迁移文件一起保留。其他身份、模板、已有备份和导出文件不清理。归档可能含旧明文，继续按敏感资料保管。

操作前退出 KeePassXC 等其他编辑器并等待同步完成；ID 的写入锁无法阻止其他客户端修改文件。`identities/` 下的移除会由 Syncthing 传播，`backups/` 归档仅在执行设备保留。恢复时暂停同步，核对归档后将其 KDBX 放回 `identities/example.kdbx`，不要覆盖同名新库或让新旧路径同时存在；恢复不会重新启用已撤销的凭证。该命令是可恢复的移除，不提供彻底销毁或自动清理归档。

## Syncthing

### 目录检查与异常处理

运行 `make doctor` 检查 `identities/`，或 `make doctor id=example` 只检查当前身份。检查只读取文件名和元数据，不解锁密码库；兼容旧 `example/passwords/example.kdbx` 布局，不读取其中档案。发现冲突库、遗留备份、临时文件、锁、空库、新旧库并存、链接、目录权限异常或未知文件时列出处理建议并返回非零状态，便于定期只读检查。`make status` 会提示异常数量，`make check` 也会报错。

`make doctor id=example fix=1` 进入本地选择界面，可以归档全部或单个已知加密副本：`*.sync-conflict-*.kdbx`、`*.kdbx.bak`、`*.kdbx.old`、`*.old.kdbx` 和 `*.kdbx~`。默认取消。请先退出相关编辑器并等同步完成；归档取得 ID 写入锁，核对正式库存在且非空、没有临时文件或外部锁、源副本未变化、路径没有链接，并确认 `/backups` 已排除同步。随后把原文件完整移到 `backups/identity-cleanup/<身份>-<随机后缀>/`，校验加密文件指纹，正式库不变，已有归档不覆盖。不同文件系统间不采用“复制后删除”的回退方式。

归档只移出当前身份目录，不表示冲突已合并；可能含独有条目，应在 KeePassXC 人工核对，必要时将归档合并进正式库。`backups/` 不参与 Syncthing，仅保存在运行清理的设备，需单独备份。CLI 在修改前及资料保存替换前检查冲突副本，即使同步交接关闭也会中止写入；它无法阻止外部客户端在检查之后继续编辑。

`.syncthing.*.tmp`、`*.kdbx.tmp` 和 `*.kdbx.lock` 可能正在使用，不按年龄删除；空库、新旧库并存、未知资料和符号链接也只提示。NAS 的 ACL 可能显示为 Unix `000`，不自动 `chmod`、`chown` 或改 Syncthing 配置；应结合 NAS 权限与同步错误判断。检查和清理仅作用于本机路径，不通过 NFS 路径或 SSH 自动修改其他设备。

AI 助手可调用只读 `doctor` 工具，只接收当前身份的异常类型与数量，不接收文件名、路径或内容。请求整理目录时，AI 可生成固定的 `directory-repair` 本地操作，由用户在本地选择具体归档；模型不能指定其他身份、文件路径、执行命令或直接删除文件。对话中 `/doctor` 和 `/repair` 可直接检查或进入归档，不经过 AI 服务，也不要求重新解锁。只有加密文件操作，不导出密码或恢复码。

### 设备交接

多端轮流使用时，退出程序并不等于更新已到达 NAS。可在每台运行 ID 的设备用 `make settings` →“Syncthing 同步交接”开启检查：接口连接当前设备的 Syncthing，填入它的 API key，再从列表选择包含密码库的同步文件夹和交接设备（通常是 NAS）。返回主设置后选择“保存并退出”。API key 只保存到本机 `config/secrets/.secrets.env`；普通参数保存到 `config/config.toml`，不修改 Syncthing 的共享或网络配置。默认关闭，未接入时沿用原有操作。

开启后，管理界面、AI、密码取用/录入、普通资料与事件写入、整理、修改主密码和标签、授权入口先确认本机与所选设备同步；有修改时，在退出前触发当前 KDBX 的扫描并等交接设备持有完整的同版本副本。检查连接、错误、待同步数量、文件版本与可用性，临时下载副本不算完成。等待期间可取消；超时或错误时可以重试，退出会明确标记“未确认同步”，不会声称可以切换设备。已保存的数据和加密备份保留。默认等待 60 秒，可用 `syncthing.timeout_seconds` 设置 1–300 秒。

Debian 与 NAS 分别运行 Syncthing 时，各自操作各自的文件；通过 NFS/SMB 查看 NAS 不影响此流程。配置中的同步文件夹路径应对应当前设备的实际目录。出现此身份的 `.sync-conflict-*.kdbx` 会阻止交接，需在 KeePassXC 合并并人工核对，或明确选择保留当前正式库、把另一版本归档待核对。`make new`、`make remove` 和批量导入仍按下述维护流程手动确认同步；直接用 KeePassXC 或其他客户端编辑也不受 ID 控制。这是基于 Syncthing 已公布索引的交接检查，不提供跨设备互斥，无法确认未上线设备尚未公布的修改。

继续把项目目录作为同步目录，在各设备同步根安装同样的 `.stignore`；该文件本身不会由 Syncthing 自动分发。主要同步 `identities/` 内的加密 KDBX；源码由 Git 部署，`config/`、`data/`、`exports/`、`backups/`、日志和 `.id-*` 过程文件不参与同步。

编辑前等待该身份同步完成，尽量一次只在一台设备编辑。CLI 在 `config/.cache/vault-locks/` 对自身写入加锁，并在保存前检查原文件是否变化；它不能阻止 KeePassXC GUI 或另一台设备同时修改。目录整理涉及路径变化，各设备应先部署兼容新路径的版本，再继续编辑；不能将另一个设备残留的旧路径覆盖到新库。检测到更新时刷新后重试；发现 `.sync-conflict-*.kdbx` 时保留两份，在 KeePassXC 用“合并数据库”处理，或用上述确认归档保留副本。不要直接删冲突副本。

## 实现边界

KDBX 的加解密、附件格式、合并由 KeePassXC 完成。Rust 读取 KeePassXC 导出的 XML 到内存，写入通过 Linux memfd、导入加密变更库、合并至原库加密副本，校验后原子替换。没有明文临时文件回退。标准图标编号参考 [KeePassXC 的格式说明](https://github.com/keepassxreboot/keepassxc-specs/blob/master/kdbx-xml/rfc.md#standard-icons)。

群组使用原生 Group、UUID、图标与时间字段；项目用途记录在群组 CustomData 的 `ID.Group` 中，条目类型使用 `ID.Kind`。这是项目约定，不是 KeePass 的保留名称；[KeePass 官方格式说明](https://keepass.info/help/kb/kdbx.html)建议将应用扩展数据放在 XML 的 CustomData 中。已有群组不自动搬动或删除；发现同名路径或重复用途标记会拒绝写入。保存后核对条目路径和项目群组标记，防止导入合并时丢失群组关系。

单次导出最多 64 MiB（包括附件的编码与历史），大型附件可能需要用 KeePassXC GUI 管理。当前只支持密码解锁，尚无 keyfile、YubiKey 或自定义图片终端渲染入口。内存清零不等于抵御同用户恶意程序、交换文件、核心转储或系统被接管；请按实际威胁模型使用。
