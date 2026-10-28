import { NextResponse } from "next/server";
import { db } from "@/lib/db";

export async function PATCH(
  request: Request,
  { params }: { params: Promise<{ id: string }> }
) {
  try {
    const { id } = await params;
    const { value } = await request.json();
    if (value === undefined) {
      return NextResponse.json({ error: "missing value" }, { status: 400 });
    }
    await db.execute({
      sql: "UPDATE items SET value = ?, updated_at = datetime('now') WHERE id = ?",
      args: [value, id],
    });
    return NextResponse.json({ ok: true });
  } catch (e) {
    return NextResponse.json({ error: String(e) }, { status: 500 });
  }
}

export async function DELETE(
  _request: Request,
  { params }: { params: Promise<{ id: string }> }
) {
  try {
    const { id } = await params;
    await db.execute({ sql: "DELETE FROM items WHERE id = ?", args: [id] });
    return NextResponse.json({ ok: true });
  } catch (e) {
    return NextResponse.json({ error: String(e) }, { status: 500 });
  }
}
