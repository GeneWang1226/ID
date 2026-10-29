import { NextResponse } from "next/server";
import { getDb } from "@/lib/db";

// POST /api/ai/dismiss { message_id } — 忽略某条草稿
export async function POST(request: Request) {
  try {
    const { message_id } = await request.json();
    if (!message_id) return NextResponse.json({ error: "missing message_id" }, { status: 400 });
    const db = getDb();
    db.prepare("UPDATE ai_messages SET status = 'dismissed' WHERE id = ? AND role = 'assistant'").run(message_id);
    return NextResponse.json({ ok: true });
  } catch (e) {
    return NextResponse.json({ error: String(e) }, { status: 500 });
  }
}
