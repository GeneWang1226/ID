# ID

在命令行里管理多个身份的账号、密码与个人资料。Rust 编写，提供全屏 TUI；KeePassXC 负责密码库的加解密，数据统一保存在每个身份自己的 KDBX 中。

不运行 Web 服务，不依赖 SQLite。可以用 KeePassXC 等兼容客户端打开同一密码库，用 Syncthing 同步加密文件。

## 开始使用

需要 Linux、Rust 1.89+、Make 和 `keepassxc-cli`。Debian 可安装 `keepassxc` 与 `make`，Rust 使用 rustup 安装。

```sh
make build
make settings
make new id=example
make manage id=example
```

新建身份时输入并确认该身份的主密码，不再读取配置中的初始密码；同时创建原生 `ID_info/example` 基础信息条目，身份名自动填入。已有身份直接运行 `make manage`，方向键选身份并解锁；更换主密码使用 `make passwd`。详情见 [使用说明](docs/GUIDE.md)。

## 数据怎么放

| 内容 | KDBX 中的表示 |
| --- | --- |
| GitHub、邮箱、Telegram 等账号 | 原生条目；标题、登录名、密码、网址和备注放在同一条目 |
| 姓名、生日、电话、偏好等 | 一个“基础信息”条目，使用原生自定义字段，密码可以为空 |
| 恢复码、API key 等 | 对应条目里的受保护自定义字段 |
| PDF、应急说明等文件 | 对应条目的原生附件 |
| 大事记 | 事件条目 |
| 图标 | 原生 IconID；TUI 显示对应的文字图标，自定义图片保留在 KDBX |

普通字段可设维度与备注；维度、字段备注和条目类型使用 KeePass CustomData，不把所有资料塞进一段 JSON。其他客户端仍可直接编辑原生字段和附件。自定义图片在终端用备用图标表示，不要求特定字体或图片协议。

新身份创建时立即建立原生群组与基础信息条目；账号、事件按需建立群组：

```text
ID_info
└── example（基础信息，身份名 = example）
ID
├── 账号
│   └── GitHub、邮箱等条目
└── 事件
```

`ID`、`ID_info` 是本项目的命名约定，KDBX 不要求 `ID_` 前缀。旧库的 `ID/身份信息/基础信息` 日常编辑时继续复用；用户显式执行 `make organize` 才统一为 `ID_info/<身份名>`。分组及条目用原生 UUID 识别，分组用途在 CustomData 的 `ID.Group` 中记录；在 KeePassXC 中重命名群组后仍能复用。其他用户群组保留原位置，TUI 按实际群组展示；重名条目使用完整路径区分。项目是 KDBX 的命令行管理外壳，密码库可独立用其他兼容客户端管理。

旧库不会因为升级自动移动。运行 `make organize id=example`，本地解锁并确认后，基础信息统一为 `ID_info/example`，带项目标记的旧根条目归入相应用途的原生项目群组；保留 UUID、字段保护、密码、附件和历史，先留加密备份。其他用户群组不搬动，同名冲突则中止。更多群组的用途建议见 [使用说明](docs/GUIDE.md)。

## 常用入口

```sh
make manage                     # 全部资料，list 是兼容别名
make manage id=example type=info # 基础信息
make pass id=example             # 选择条目、复制密码
make pass-add-manual id=example  # 在本地录入密码
make ai id=example               # 全屏 AI 助手
make organize id=example         # 旧项目条目归入原生群组
make backup                     # 备份各身份加密密码库
make help
```

TUI 中 Enter 展开详情，`o` 操作，`n` 新增，`l` 锁定；方向键选择、翻页。普通资料只解锁一次会话；修改密码、删除内容、导出附件和明文查看等操作需再次验证主密码。空闲五分钟清除缓存主密码，退出即锁定。

AI 查询仅提供当前身份的普通资料及密码是否已填，不提供密码和受保护字段；本地密码操作不经过模型。AI 外部服务按需配置 API key，正常资料管理无需网络。不要把密码、主密码或恢复码输入聊天。

## 安全与平台范围

本版本的资料写入使用 Linux 匿名内存文件，把修改交给 KeePassXC 导入、合并到原库副本，再原子替换；明文 XML 不落磁盘。保存保留已有加密参数、密码、历史、附件和自定义图标，自动留加密备份；检测到外部更新会拒绝覆盖。

完整编辑目前支持 Linux，包括 SSH。其他平台尚未验证，也没有以明文临时文件回退的编辑路径。Rust 和清零缓冲区不能防止同用户调试、恶意软件、系统交换或系统被接管；本项目没有经过独立安全审计。KDBX 不提供跨设备事务锁，避免多端同时编辑同一身份。

## 项目目录

- `src/`：Rust CLI、TUI、KeePassXC 适配。
- `identities/<身份>.kdbx`：一身份一文件，唯一的日常资料存储，不进 Git；兼容旧的 `<身份>/passwords/<身份>.kdbx` 路径，两处同时存在时拒绝选择，避免写错库。
- `config/config.toml`：非敏感设置；`config/secrets/.secrets.env`：本机密钥。
- `backups/vaults/`：本机加密备份；`backups/legacy-identities/`：整理时保留的旧身份目录，可能包含旧明文档案，不进 Git 或 Syncthing。
- `exports/`：用户主动导出的明文附件，不进 Git 或 Syncthing。
- `template/`：保留的旧身份模板，程序不再复制其目录；新身份仅创建 KDBX。
- `tests/`：只使用临时身份和合成密码库的测试。

```sh
make test
make check
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```
