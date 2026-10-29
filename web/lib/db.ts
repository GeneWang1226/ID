import Database from "better-sqlite3";
import { readFileSync, existsSync, mkdirSync } from "fs";
import { join, dirname } from "path";

const dbPath = process.env.DB_PATH || join(process.cwd(), "..", "data", "id.db");
const schemaPath = process.env.SCHEMA_PATH || join(process.cwd(), "..", "template", "data", "schema.sql");

let db: Database.Database | null = null;

function initDb(): Database.Database {
  if (db) return db;

  const dir = dirname(dbPath);
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
  }

  db = new Database(dbPath);

  // 表结构全是 IF NOT EXISTS，幂等，每次启动都跑一遍（新加的表在老库上也能补出来）
  if (existsSync(schemaPath)) {
    const schema = readFileSync(schemaPath, "utf-8");
    db.exec(schema);
  }

  return db;
}

export function getDb(): Database.Database {
  return initDb();
}

export function getIdentities(): string[] {
  const db = getDb();
  const items = db.prepare("SELECT DISTINCT identity FROM items ORDER BY identity").all() as { identity: string }[];
  const events = db.prepare("SELECT DISTINCT identity FROM events ORDER BY identity").all() as { identity: string }[];
  const set = new Set<string>();
  items.forEach((r) => set.add(r.identity));
  events.forEach((r) => set.add(r.identity));
  return Array.from(set).sort();
}
