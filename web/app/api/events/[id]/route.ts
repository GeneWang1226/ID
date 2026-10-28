import { NextResponse } from "next/server";
import { getDb } from "@/lib/db";

export async function DELETE(
  request: Request,
  { params }: { params: Promise<{ id: string }> }
) {
  try {
    const { id } = await params;
    const { identity } = await request.json();
    if (!identity) {
      return NextResponse.json({ error: "missing identity" }, { status: 400 });
    }
    const db = getDb(identity);
    await db.execute({ sql: "DELETE FROM events WHERE id = ?", args: [id] });
    return NextResponse.json({ ok: true });
  } catch (e) {
    return NextResponse.json({ error: String(e) }, { status: 500 });
  }
}
