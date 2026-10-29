import { NextResponse } from "next/server";
import { getDb } from "@/lib/db";

// GET /api/ai/sessions/<id> — 对话元信息 + 全部消息
export async function GET(_request: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const db = getDb();
  const session = db.prepare("SELECT id, identity, title, created_at FROM ai_sessions WHERE id = ?").get(id);
  if (!session) return NextResponse.json({ error: "not found" }, { status: 404 });
  const rows = db
    .prepare("SELECT id, role, content, draft_json, status, created_at FROM ai_messages WHERE session_id = ? ORDER BY id")
    .all(id) as { id: number; role: string; content: string; draft_json: string | null; status: string; created_at: string }[];
  const messages = rows.map((r) => ({
    id: r.id,
    role: r.role,
    content: r.content,
    draft: r.draft_json ? JSON.parse(r.draft_json) : null,
    status: r.status,
    created_at: r.created_at,
  }));
  return NextResponse.json({ session, messages });
}
