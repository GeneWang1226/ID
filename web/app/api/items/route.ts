import { NextResponse } from "next/server";
import { db } from "@/lib/db";

export async function POST(request: Request) {
  try {
    const { identity, category, key, value, note } = await request.json();
    if (!identity || !category || !key || !value) {
      return NextResponse.json({ error: "missing fields" }, { status: 400 });
    }
    await db.execute({
      sql: "INSERT INTO items (identity, category, key, value, note) VALUES (?, ?, ?, ?, ?)",
      args: [identity, category, key, value, note ?? ""],
    });
    return NextResponse.json({ ok: true });
  } catch (e) {
    return NextResponse.json({ error: String(e) }, { status: 500 });
  }
}
