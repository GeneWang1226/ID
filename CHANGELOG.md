# Changelog

本文件记录 **可被 git 跟踪的项目变更**（源码、模板和文档）。身份数据在 `identities/` 的 KDBX 中，靠 Syncthing，不写入此日志。

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号与 git tag 对应。

## 未发布

- `make organize` 可将旧根目录或旧项目群组中的基础信息统一为原生 `ID_info/<身份名>`，保留群组与条目 UUID、字段保护、密码、附件及历史；保存前本地确认并备份，冲突时保留原库。
- 新建身份时直接设置并确认独立主密码，自动创建原生 `ID_info/<身份名>` 基础信息条目；取消或密码不一致不创建身份，初始化验证后才发布加密库，不再使用配置中的共享初始密码。
- 新建内容使用 KDBX 原生 `ID/身份信息`、`ID/账号`、`ID/事件` 群组，TUI 按实际路径分组；原生 UUID 与 CustomData 保持群组改名后的识别。新增 `make organize`，确认后整理带项目标记的旧根条目，保留 UUID、密码、附件和历史，其他原有条目及群组不移动。
- 项目转为 Rust 命令行与全屏 TUI；保留 Make 入口，移除 SQLite 运行依赖、数据库命令和旧 Web 服务。
- 所有身份资料归入各自 KDBX：账号与密码使用同一原生条目，个人资料集中为“基础信息”的原生自定义字段，事件使用事件条目。
- TUI 读取原生图标编号，支持图标修改、普通与受保护字段、维度备注、附件导入导出，以及加密备份后删除。
- KeePassXC 负责加解密与合并；XML 只在 Linux 匿名内存中处理，保存校验字段、保护标记、图标和附件内容，检查外部更新后原子替换。
- 移除 SQLite 读取及迁移入口；测试不再依赖 SQLite，旧数据库文件不自动删除。Syncthing 不再同步旧数据库和明文导出目录。
- AI 查询仅获取普通资料和原生密码字段是否为空，密码与受保护值留在本地。普通设置仍在 TOML，密钥保留在本机秘密配置。
- 主密码空闲五分钟主动清除，高权限操作重新验证；保留 SSH 剪贴板自测、全局明文开关与临时凭证。

## [0.1.1] — 2026-10-29

相对 v0.1.0 的 dev 进度快照（tag 指向含 Web AI 的提交；`main` 合入见 PR #3）。

### Added

- **配置**：根目录 `config` 改为 `.env`（权限 600，Syncthing 同步）；入库模板 `.env.example`（`kdbx_initial_password`、`WEB_PASSWORD`、AI 相关键）
- **安全**：`make check` 自检（ignore、git 追踪、权限、追踪文件明文扫描）
- **终端**：Makefile 与主要脚本彩色输出，非 TTY 自动无色
- **AI 整理**：`make ai id=<身份>`（必选身份或交互询问）；展示「AI 说明」与「模型原始 JSON」；空 `identity` 禁止写 SQLite
- **AI 用密码**：`make grant` / `make use` / `make grants` / `make revoke`；`grant-secret.sh`、`secret-exec.sh`、`revoke-grant.sh`；kdbx 条目 Tags 分级（`ai-auto` / 无标签 / `ai-deny`）
- **kdbx 标签**：`scripts/tag-entry.sh`、`make tag`（CLI 写 Tags，补 keepassxc-cli 只读限制）
- **Web**：`/ai` 对话助手（会话存库、草稿勾选确认写 SQLite、机密只提醒、可展开模型原始 JSON）；Compose 从 `.env` 注入 `AI_*`；`web/package-lock.json`、`next-env.d.ts` 供 Docker `npm ci` 与本地构建
- **文档**：GUIDE 场景四（分级授权）、Web AI 说明；MAINTENANCE 临时凭证与 tag 维护；README「keepassxc-cli 局限」

### Changed

- `scripts/add-identity.sh`、`scripts/ai-organize.py`、security-check 等改为读取 `.env`
- SQLite schema 增加 `ai_sessions` / `ai_messages`（Web AI 历史）

## [0.1.0] — 2026-10-29

首个阶段性 tag（`main` 稳定线基准）。

### Added

- 身份管理：`scripts/add-identity.sh`、`make new`；kdbx 初始密码来自根目录 `config`
- 密码：`make pass` / `pass-add` / `pass-list` 等，机密只进 `.kdbx`
- 非机密信息：共用 SQLite `data/id.db`，`make item` / `event` / `show`
- Web：Next.js + Docker（`make web`），编辑 SQLite 非机密条目
- 部署：`main` / `dev` 分支；`.gitignore` / `.stignore` 与 Syncthing 错开
- 文档：`README`、`docs/GUIDE.md`、`docs/MAINTENANCE.md`

[Unreleased]: https://github.com/GeneWang1226/ID/compare/v0.1.1...dev
[0.1.1]: https://github.com/GeneWang1226/ID/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/GeneWang1226/ID/releases/tag/v0.1.0
