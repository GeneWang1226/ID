# Changelog

本文件记录 **可被 git 跟踪的项目变更**（脚本、模板、文档、Web）。身份数据在 `identities/`、`.env`、`data/id.db`，靠 Syncthing，不写入此日志。

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号与 git tag 对应。

## [Unreleased]

- **配置**：新增可同步的 `config/config.toml` 与 `make settings`；敏感配置迁移到本机 `config/secrets/.secrets.env`，Docker 配置集中在 `docker/`。

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
