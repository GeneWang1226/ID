import { NextResponse } from "next/server";
import { getDb } from "@/lib/db";

export async function POST(request: Request) {
  try {
    const { identity, category, key, value, note } = await request.json();
    if (!identity || !category || !key || !value) {
      return NextResponse.json({ error: "missing fields" }, { status: 400 });
    }
    const db = getDb(identity);
    await db.execute({
      sql: "INSERT INTO items (category, key, value, note) VALUES (?, ?, ?, ?)",
      args: [category, key, value, note ?? ""],
    });
    return NextResponse.json({ ok: true });
  } catch (e) {
    return NextResponse.json({ error: String(e) }, { status: 500 });
  }
}
