import { cookies } from "next/headers";

const COOKIE_NAME = "id_web_auth";

export async function isAuthed(): Promise<boolean> {
  const store = await cookies();
  const cookie = store.get(COOKIE_NAME);
  return cookie?.value === process.env.WEB_PASSWORD;
}

export async function setAuthCookie(): Promise<void> {
  const store = await cookies();
  store.set(COOKIE_NAME, process.env.WEB_PASSWORD!, {
    httpOnly: true,
    secure: true,
    sameSite: "lax",
    maxAge: 60 * 60 * 24 * 7, // 7 days
    path: "/",
  });
}

export async function clearAuthCookie(): Promise<void> {
  const store = await cookies();
  store.delete(COOKIE_NAME);
}
