import { db } from "@/lib/db";
import IdentityView from "./IdentityView";

export const dynamic = "force-dynamic";

async function getIdentities(): Promise<string[]> {
  const items = await db.execute(
    "SELECT DISTINCT identity FROM items ORDER BY identity"
  );
  const events = await db.execute(
    "SELECT DISTINCT identity FROM events ORDER BY identity"
  );
  const set = new Set<string>();
  for (const row of items.rows) set.add(String(row.identity));
  for (const row of events.rows) set.add(String(row.identity));
  return Array.from(set).sort();
}

export default async function Home({
  searchParams,
}: {
  searchParams: Promise<{ identity?: string }>;
}) {
  const identities = await getIdentities();
  const params = await searchParams;
  const identity = params.identity ?? identities[0] ?? "";

  let items: { id: number; category: string; key: string; value: string; note: string; updated_at: string }[] = [];
  let events: { id: number; happened_at: string; title: string; detail: string }[] = [];

  if (identity) {
    const itemsRes = await db.execute({
      sql: "SELECT id, category, key, value, note, updated_at FROM items WHERE identity = ? ORDER BY category, key",
      args: [identity],
    });
    items = itemsRes.rows as unknown as typeof items;

    const eventsRes = await db.execute({
      sql: "SELECT id, happened_at, title, detail FROM events WHERE identity = ? ORDER BY happened_at DESC",
      args: [identity],
    });
    events = eventsRes.rows as unknown as typeof events;
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
        {identities.length === 0 && (
          <p style={{ color: "#64748b", fontSize: "0.875rem" }}>暂无身份</p>
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
        {identity ? (
          <IdentityView identity={identity} items={items} events={events} />
        ) : (
          <p style={{ color: "#64748b" }}>请先添加身份和数据</p>
        )}
      </main>
    </div>
  );
}
