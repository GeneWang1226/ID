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

  // Initialize schema if database is empty
  const tables = db.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='items'").get();
  if (!tables && existsSync(schemaPath)) {
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
