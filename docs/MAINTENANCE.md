# 维护、备份与同步

## 安装与检查

安装 Rust 1.89+、Make、KeePassXC CLI（当前测试版本 2.7.10）。Linux 支持完整编辑。无需 Python 运行时、Node、Docker 或数据库服务；Python 仅用于终端集成测试。

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

新建时设置并确认独立主密码，同时创建 `ID_info/<身份名>` 原生基础信息条目，填入“身份名”；主密码仅用于加密，不会作为条目字段或配置保存。完整初始化并验证后才发布 KDBX，不覆盖已有库。旧版本创建的库仍使用当时的密码，需用 `make passwd` 提供旧密码后更换。模板不会自动覆盖已有身份。条目 Tags：`ai-deny` 禁止 AI 授权，`ai-auto` 限时多次，无标签默认一次性。临时凭据放本机运行时目录，不入 Git 或 Syncthing。

`make use token=... cmd='...'` 使用令牌代执行，秘密只通过子进程环境变量 `ID_SECRET` 注入，不要把秘密拼进命令行参数。Make 中命令里的 `$` 写成 `$$`。不要读取或打印临时凭据文件。

## 旧文件与表格导入

当前程序只使用 KDBX，不提供 SQLite 读取或迁移命令。已有旧资料文件保持原样，仍由忽略规则保护，不会自动删除、同步或转存；未整理到 KDBX 的旧资料不会出现在管理界面。模板保留只读，新建身份跳过其中的旧 `data/` 目录。

`id import 表格.xlsx --owner 原使用者=example` 仍支持旧七列账号表。账号、地区与服务商入原生字段，备注、密码提示和详细数据作为受保护字段；各身份分别解锁和提交。多身份导入是逐库完成，重复导入会跳过已完成行。原表不会再另存明文备份，也不会删除用户提供的文件。

`make organize id=example` 将现有基础信息统一为原生 `ID_info/example`，同时整理带项目标记的旧根条目，无需再次导入旧资料。基础信息可来自根目录或旧项目群组：复用群组和条目的 UUID，保留字段、保护标记、密码、附件和历史；缺少基础信息时仅创建身份名。若旧项目身份群组被移动，其他内容仍保留在原群组节点中，不删除原有群组或文件。解锁后预检并要求确认，保存前留加密备份；同名目标、重复基础信息或群组标记冲突时中止，保留原库。其他用户群组不搬动。

## 备份与恢复

`make backup` 复制所有身份的加密 KDBX 到 `backups/vaults/<身份>/`。写入资料、密码或标签前也会备份。备份权限为 600；不会自动清理，定期转存到独立介质并检查磁盘空间。

恢复前退出编辑界面、暂停该身份的同步，保留当前 KDBX，再用 KeePassXC 核对备份并恢复。备份可能使用旧主密码，需妥善保管。删除条目保留加密备份，不代表历史副本中的秘密已经销毁。

## Syncthing

继续把项目目录作为同步目录，在各设备同步根安装同样的 `.stignore`；该文件本身不会由 Syncthing 自动分发。主要同步 `identities/` 内的加密 KDBX；源码由 Git 部署，`config/`、`data/`、`exports/`、`backups/`、日志和 `.id-*` 过程文件不参与同步。

编辑前等待该身份同步完成，尽量一次只在一台设备编辑。CLI 对自身写入加锁，并在保存前检查原文件是否变化；它不能阻止 KeePassXC GUI 或另一台设备同时修改。检测到更新时刷新后重试；发现 `.sync-conflict-*.kdbx` 时保留两份，在 KeePassXC 用“合并数据库”处理并核对后继续同步。不要直接删冲突副本。

## 实现边界

KDBX 的加解密、附件格式、合并由 KeePassXC 完成。Rust 读取 KeePassXC 导出的 XML 到内存，写入通过 Linux memfd、导入加密变更库、合并至原库加密副本，校验后原子替换。没有明文临时文件回退。标准图标编号参考 [KeePassXC 的格式说明](https://github.com/keepassxreboot/keepassxc-specs/blob/master/kdbx-xml/rfc.md#standard-icons)。

群组使用原生 Group、UUID、图标与时间字段；项目用途记录在群组 CustomData 的 `ID.Group` 中，条目类型使用 `ID.Kind`。这是项目约定，不是 KeePass 的保留名称；[KeePass 官方格式说明](https://keepass.info/help/kb/kdbx.html)建议将应用扩展数据放在 XML 的 CustomData 中。已有群组不自动搬动或删除；发现同名路径或重复用途标记会拒绝写入。保存后核对条目路径和项目群组标记，防止导入合并时丢失群组关系。

单次导出最多 64 MiB（包括附件的编码与历史），大型附件可能需要用 KeePassXC GUI 管理。当前只支持密码解锁，尚无 keyfile、YubiKey 或自定义图片终端渲染入口。内存清零不等于抵御同用户恶意程序、交换文件、核心转储或系统被接管；请按实际威胁模型使用。
