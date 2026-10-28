import { createClient } from "@libsql/client";
import { readFileSync, readdirSync } from "fs";
import { join } from "path";

const identitiesDir = process.env.IDENTITIES_DIR || "../identities";

type Env = {
  TURSO_URL: string;
  TURSO_AUTH_TOKEN: string;
};

function parseEnv(content: string): Env | null {
  const lines = content.split("\n");
  let url = "";
  let token = "";
  for (const line of lines) {
    if (line.startsWith("TURSO_URL=")) url = line.slice("TURSO_URL=".length).trim();
    if (line.startsWith("TURSO_AUTH_TOKEN=")) token = line.slice("TURSO_AUTH_TOKEN=".length).trim();
  }
  return url && token ? { TURSO_URL: url, TURSO_AUTH_TOKEN: token } : null;
}

export function getIdentities(): string[] {
  try {
    return readdirSync(identitiesDir, { withFileTypes: true })
      .filter((d) => d.isDirectory())
      .map((d) => d.name)
      .filter((name) => {
        try {
          const envPath = join(identitiesDir, name, ".turso.env");
          const content = readFileSync(envPath, "utf-8");
          return parseEnv(content) !== null;
        } catch {
          return false;
        }
      })
      .sort();
  } catch {
    return [];
  }
}

export function getDb(identity: string) {
  const envPath = join(identitiesDir, identity, ".turso.env");
  const content = readFileSync(envPath, "utf-8");
  const env = parseEnv(content);
  if (!env) throw new Error(`No valid .turso.env for ${identity}`);
  return createClient({ url: env.TURSO_URL, authToken: env.TURSO_AUTH_TOKEN });
}
