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
      const db = getDb(identity);
      const itemsRes = await db.execute(
        "SELECT id, category, key, value, note, updated_at FROM items ORDER BY category, key"
      );
      items = itemsRes.rows as unknown as typeof items;

      const eventsRes = await db.execute(
        "SELECT id, happened_at, title, detail FROM events ORDER BY happened_at DESC"
      );
      events = eventsRes.rows as unknown as typeof events;
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
        {identities.length === 0 && (
          <p style={{ color: "#64748b", fontSize: "0.875rem" }}>暂无已开通 Turso 的身份</p>
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
              ? "请先运行 ~/ID/scripts/turso-setup.sh <身份名> 开通数据库"
              : "身份不存在或未开通"}
          </p>
        )}
      </main>
    </div>
  );
}
