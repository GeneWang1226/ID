import { NextResponse } from "next/server";
import type { NextRequest } from "next/server";
import { isAuthed } from "@/lib/auth";

export async function middleware(request: NextRequest) {
  const { pathname } = request.nextUrl;

  if (pathname.startsWith("/api/login")) {
    return NextResponse.next();
  }

  const authed = await isAuthed();

  if (pathname === "/login") {
    if (authed) {
      return NextResponse.redirect(new URL("/", request.url));
    }
    return NextResponse.next();
  }

  if (!authed) {
    return NextResponse.redirect(new URL("/login", request.url));
  }

  return NextResponse.next();
}

export const config = {
  matcher: ["/((?!_next/static|_next/image|favicon.ico).*)"],
};
