import { NextResponse } from "next/server";
import { setAuthCookie } from "@/lib/auth";

export async function POST(request: Request) {
  try {
    const { password } = await request.json();
    if (password === process.env.WEB_PASSWORD) {
      await setAuthCookie();
      return NextResponse.json({ ok: true });
    }
    return NextResponse.json({ error: "unauthorized" }, { status: 401 });
  } catch {
    return NextResponse.json({ error: "bad request" }, { status: 400 });
  }
}
