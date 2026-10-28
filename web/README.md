# ID Web

查看和编辑各身份的非机密个人信息。每个身份有自己的 Turso 库，Web 界面读取各身份的 `.turso.env` 连接。

## 本地开发

```bash
npm install
# 建 .env.local：
#   IDENTITIES_DIR=../identities   # 身份目录路径
#   WEB_PASSWORD=...               # 访问密码
npm run dev
```

打开 http://localhost:3000

## 部署到 Vercel

1. 在 Vercel 新建项目，导入包含本目录的 GitHub 仓库，把根目录设为 `web`。
2. 在 Vercel 项目环境变量里加：
   - `IDENTITIES_DIR`（默认 `../identities`，如果仓库根目录是 `ID/` 则不用改）
   - `WEB_PASSWORD`（访问网站时要输的密码）
3. 部署即可。

**注意**：Vercel 是 Serverless，读不到本地文件系统。`identities/*/.turso.env` 需要同步到部署环境，或改用其他方式（见下）。

## 替代方案：环境变量存所有身份

如果 `.turso.env` 不方便同步到 Vercel，可以改成在 Vercel 环境变量里存所有身份的连接信息（格式：`IDENTITY_<name>_URL` 和 `IDENTITY_<name>_TOKEN`），`web/lib/db.ts` 从环境变量读。需要时告诉我，我改。

## 给身份开通数据库

```bash
~/ID/scripts/turso-setup.sh <身份名>
```

这会创建 `id-<身份名>` 库，并把连接信息写入 `identities/<身份名>/.turso.env`。
