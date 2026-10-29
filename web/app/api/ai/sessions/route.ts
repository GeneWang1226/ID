import { NextResponse } from "next/server";
import { getDb } from "@/lib/db";

// GET /api/ai/sessions?identity=xxx — 某身份的对话列表（新的在前）
export async function GET(request: Request) {
  const { searchParams } = new URL(request.url);
  const identity = searchParams.get("identity") || "";
  if (!identity) return NextResponse.json({ error: "missing identity" }, { status: 400 });
  const db = getDb();
  const sessions = db
    .prepare(
      `SELECT s.id, s.identity, s.title, s.created_at,
              (SELECT created_at FROM ai_messages WHERE session_id = s.id ORDER BY id DESC LIMIT 1) AS last_at
       FROM ai_sessions s WHERE s.identity = ? ORDER BY s.id DESC`
    )
    .all(identity);
  return NextResponse.json({ sessions });
}

// POST /api/ai/sessions { identity } — 新建对话
export async function POST(request: Request) {
  try {
    const { identity } = await request.json();
    if (!identity || typeof identity !== "string") {
      return NextResponse.json({ error: "missing identity" }, { status: 400 });
    }
    const db = getDb();
    const r = db.prepare("INSERT INTO ai_sessions (identity) VALUES (?)").run(identity);
    return NextResponse.json({ id: Number(r.lastInsertRowid) });
  } catch (e) {
    return NextResponse.json({ error: String(e) }, { status: 500 });
  }
}
