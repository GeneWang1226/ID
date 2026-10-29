import { NextResponse } from "next/server";
import { getDb } from "@/lib/db";

// POST /api/ai/apply { message_id, items, events } — 把确认过的草稿写入库，身份以对话为准
export async function POST(request: Request) {
  try {
    const { message_id, items, events } = await request.json();
    if (!message_id) return NextResponse.json({ error: "missing message_id" }, { status: 400 });
    const db = getDb();
    const row = db
      .prepare(
        `SELECT m.id, m.status, s.identity FROM ai_messages m
         JOIN ai_sessions s ON s.id = m.session_id WHERE m.id = ? AND m.role = 'assistant'`
      )
      .get(message_id) as { id: number; status: string; identity: string } | undefined;
    if (!row) return NextResponse.json({ error: "message not found" }, { status: 404 });
    if (row.status === "applied") return NextResponse.json({ error: "已写入过，不能重复写入" }, { status: 409 });

    const safeItems = (Array.isArray(items) ? items : []).filter(
      (it) => it && it.category && it.key && it.value
    );
    const safeEvents = (Array.isArray(events) ? events : []).filter((ev) => ev && ev.title);

    const upsertItem = db.prepare(
      `INSERT INTO items (identity, category, key, value, note) VALUES (?, ?, ?, ?, ?)
       ON CONFLICT(identity, category, key) DO UPDATE SET value = excluded.value, note = excluded.note, updated_at = datetime('now')`
    );
    const insertEvent = db.prepare("INSERT INTO events (identity, title, detail) VALUES (?, ?, ?)");

    db.transaction(() => {
      for (const it of safeItems) {
        upsertItem.run(row.identity, String(it.category), String(it.key), String(it.value), String(it.note ?? ""));
      }
      for (const ev of safeEvents) {
        insertEvent.run(row.identity, String(ev.title), String(ev.detail ?? ""));
      }
      db.prepare("UPDATE ai_messages SET status = 'applied' WHERE id = ?").run(message_id);
    })();

    return NextResponse.json({ ok: true, items: safeItems.length, events: safeEvents.length });
  } catch (e) {
    return NextResponse.json({ error: String(e) }, { status: 500 });
  }
}
