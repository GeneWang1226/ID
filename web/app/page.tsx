import { getIdentities, getDb } from "@/lib/db";
import IdentityView from "./IdentityView";

export const dynamic = "force-dynamic";

export default async function Home({
  searchParams,
}: {
  searchParams: Promise<{ identity?: string }>;
}) {
  const identities = getIdentities();
  const params = await searchParams;
  const identity = params.identity ?? identities[0] ?? "";

  let items: { id: number; category: string; key: string; value: string; note: string; updated_at: string }[] = [];
  let events: { id: number; happened_at: string; title: string; detail: string }[] = [];

  if (identity && identities.includes(identity)) {
    try {
      const db = getDb();
      items = db.prepare("SELECT id, category, key, value, note, updated_at FROM items WHERE identity = ? ORDER BY category, key").all(identity) as typeof items;
      events = db.prepare("SELECT id, happened_at, title, detail FROM events WHERE identity = ? ORDER BY happened_at DESC").all(identity) as typeof events;
    } catch (e) {
      console.error(`Failed to load ${identity}:`, e);
    }
  }

  return (
    <div style={{ display: "flex", minHeight: "100vh" }}>
      <aside
        style={{
          width: "200px",
          borderRight: "1px solid #334155",
          padding: "1rem",
          flexShrink: 0,
        }}
      >
        <h1 style={{ fontSize: "1rem", marginTop: 0 }}>身份</h1>
        <a
          href="/ai"
          style={{
            display: "block",
            marginBottom: "0.75rem",
            padding: "0.375rem 0.5rem",
            borderRadius: "0.375rem",
            textDecoration: "none",
            color: "#38bdf8",
            border: "1px solid #334155",
            fontSize: "0.875rem",
          }}
        >
          ✦ AI 助手
        </a>
        {identities.length === 0 && (
          <p style={{ color: "#64748b", fontSize: "0.875rem" }}>暂无身份数据</p>
        )}
        <nav style={{ display: "flex", flexDirection: "column", gap: "0.25rem" }}>
          {identities.map((name) => (
            <a
              key={name}
              href={`/?identity=${encodeURIComponent(name)}`}
              style={{
                padding: "0.375rem 0.5rem",
                borderRadius: "0.375rem",
                textDecoration: "none",
                color: name === identity ? "#fff" : "#94a3b8",
                background: name === identity ? "#3b82f6" : "transparent",
              }}
            >
              {name}
            </a>
          ))}
        </nav>
      </aside>
      <main style={{ flex: 1, padding: "1.5rem", overflow: "auto" }}>
        {identity && identities.includes(identity) ? (
          <IdentityView identity={identity} items={items} events={events} />
        ) : (
          <p style={{ color: "#64748b" }}>
            {identities.length === 0
              ? "请先添加身份和数据"
              : "身份不存在"}
          </p>
        )}
      </main>
    </div>
  );
}
