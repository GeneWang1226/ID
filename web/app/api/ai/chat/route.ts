import { NextResponse } from "next/server";
import { getDb } from "@/lib/db";
import { aiChat } from "@/lib/ai";

// POST /api/ai/chat { session_id, text } — 发消息，调 AI，存双方消息，返回 AI 回复
export async function POST(request: Request) {
  try {
    const { session_id, text } = await request.json();
    if (!session_id || !text || typeof text !== "string" || !text.trim()) {
      return NextResponse.json({ error: "missing fields" }, { status: 400 });
    }
    const db = getDb();
    const session = db.prepare("SELECT id, identity, title FROM ai_sessions WHERE id = ?").get(session_id) as
      | { id: number; identity: string; title: string }
      | undefined;
    if (!session) return NextResponse.json({ error: "session not found" }, { status: 404 });

    // 先取历史（不含本条），再插入本条用户消息
    const history = db
      .prepare("SELECT role, content FROM ai_messages WHERE session_id = ? ORDER BY id DESC LIMIT 20")
      .all(session_id) as { role: "user" | "assistant"; content: string }[];
    history.reverse();

    db.prepare("INSERT INTO ai_messages (session_id, role, content) VALUES (?, 'user', ?)").run(session_id, text);

    // 首条消息生成对话标题
    if (!session.title) {
      db.prepare("UPDATE ai_sessions SET title = ? WHERE id = ?").run(text.trim().slice(0, 20), session_id);
    }

    const draft = await aiChat(session.identity, history, text);

    const r = db
      .prepare("INSERT INTO ai_messages (session_id, role, content, draft_json) VALUES (?, 'assistant', ?, ?)")
      .run(session_id, draft.reply, JSON.stringify({ items: draft.items, events: draft.events, secrets: draft.secrets }));

    return NextResponse.json({
      id: Number(r.lastInsertRowid),
      role: "assistant",
      content: draft.reply,
      draft: { items: draft.items, events: draft.events, secrets: draft.secrets },
      status: "",
    });
  } catch (e) {
    return NextResponse.json({ error: String(e) }, { status: 500 });
  }
}
