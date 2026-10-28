# ID Web

查看和编辑各身份的非机密个人信息（Turso 库）。

## 本地开发

```bash
npm install
# 建 .env.local：
#   TURSO_DATABASE_URL=libsql://...
#   TURSO_AUTH_TOKEN=...
#   WEB_PASSWORD=...
npm run dev
```

打开 http://localhost:3000

## 部署到 Vercel

1. 在 Vercel 新建项目，导入包含本目录的 GitHub 仓库，把根目录设为 `web`。
2. 在 Vercel 项目环境变量里加：
   - `TURSO_DATABASE_URL`
   - `TURSO_AUTH_TOKEN`
   - `WEB_PASSWORD`（访问网站时要输的密码）
3. 部署即可。

Turso 建库和初始化表结构：

```bash
turso auth login
turso db create id
turso db shell id < ../template/data/schema.sql
```
