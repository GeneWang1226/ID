# ID Web

查看和编辑各身份的非机密个人信息。本地 SQLite 数据库，Docker 部署。

## 快速启动

```bash
# 在项目根目录（~/ID）
docker compose up -d
```

打开 http://localhost:3000，输入访问密码（默认 `changeme`，可在 `docker-compose.yml` 或环境变量改）。

## 数据库

SQLite 文件在 `./data/id.db`，随 Syncthing 同步。首次启动时自动从 `./template/data/schema.sql` 初始化表结构。

## 本地开发（不用 Docker）

```bash
cd web
npm install
npm run dev
```

数据库路径默认 `../data/id.db`（相对 `web/` 目录）。

## 环境变量

| 变量 | 默认 | 说明 |
|------|------|------|
| `WEB_PASSWORD` | `changeme` | 访问密码 |
| `DB_PATH` | `/app/data/id.db` | SQLite 文件路径（Docker 内） |
| `SCHEMA_PATH` | `/app/template/data/schema.sql` | 表结构文件路径（Docker 内） |
