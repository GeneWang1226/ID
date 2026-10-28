import { NextResponse } from "next/server";
import { db } from "@/lib/db";

export async function POST(request: Request) {
  try {
    const { identity, title, detail } = await request.json();
    if (!identity || !title) {
      return NextResponse.json({ error: "missing fields" }, { status: 400 });
    }
    await db.execute({
      sql: "INSERT INTO events (identity, title, detail) VALUES (?, ?, ?)",
      args: [identity, title, detail ?? ""],
    });
    return NextResponse.json({ ok: true });
  } catch (e) {
    return NextResponse.json({ error: String(e) }, { status: 500 });
  }
}
