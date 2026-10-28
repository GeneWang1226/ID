"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";

type Item = {
  id: number;
  category: string;
  key: string;
  value: string;
  note: string;
  updated_at: string;
};

type Event = {
  id: number;
  happened_at: string;
  title: string;
  detail: string;
};

export default function IdentityView({
  identity,
  items,
  events,
}: {
  identity: string;
  items: Item[];
  events: Event[];
}) {
  const router = useRouter();
  const [newItem, setNewItem] = useState({ category: "account", key: "", value: "", note: "" });
  const [newEvent, setNewEvent] = useState({ title: "", detail: "" });
  const [saving, setSaving] = useState(false);

  async function addItem(e: React.FormEvent) {
    e.preventDefault();
    if (!newItem.key.trim() || !newItem.value.trim()) return;
    setSaving(true);
    await fetch("/api/items", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ identity, ...newItem }),
    });
    setNewItem({ category: "account", key: "", value: "", note: "" });
    setSaving(false);
    router.refresh();
  }

  async function updateItem(id: number, value: string) {
    await fetch(`/api/items/${id}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ value }),
    });
    router.refresh();
  }

  async function deleteItem(id: number) {
    if (!confirm("确定删除这条记录？")) return;
    await fetch(`/api/items/${id}`, { method: "DELETE" });
    router.refresh();
  }

  async function addEvent(e: React.FormEvent) {
    e.preventDefault();
    if (!newEvent.title.trim()) return;
    setSaving(true);
    await fetch("/api/events", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ identity, ...newEvent }),
    });
    setNewEvent({ title: "", detail: "" });
    setSaving(false);
    router.refresh();
  }

  async function deleteEvent(id: number) {
    if (!confirm("确定删除这条事件？")) return;
    await fetch(`/api/events/${id}`, { method: "DELETE" });
    router.refresh();
  }

  const categories = Array.from(new Set(items.map((i) => i.category)));

  return (
    <div>
      <h2 style={{ marginTop: 0 }}>{identity}</h2>

      <section style={{ marginBottom: "2rem" }}>
        <h3 style={{ fontSize: "1rem" }}>条目</h3>
        <table style={{ width: "100%", borderCollapse: "collapse", fontSize: "0.875rem" }}>
          <thead>
            <tr style={{ textAlign: "left", borderBottom: "1px solid #334155" }}>
              <th style={{ padding: "0.5rem" }}>分类</th>
              <th style={{ padding: "0.5rem" }}>名称</th>
              <th style={{ padding: "0.5rem" }}>内容</th>
              <th style={{ padding: "0.5rem" }}>备注</th>
              <th style={{ padding: "0.5rem" }}></th>
            </tr>
          </thead>
          <tbody>
            {items.map((item) => (
              <tr key={item.id} style={{ borderBottom: "1px solid #1e293b" }}>
                <td style={{ padding: "0.5rem" }}>{item.category}</td>
                <td style={{ padding: "0.5rem" }}>{item.key}</td>
                <td style={{ padding: "0.5rem" }}>
                  <input
                    defaultValue={item.value}
                    onBlur={(e) => {
                      if (e.target.value !== item.value) updateItem(item.id, e.target.value);
                    }}
                    style={{
                      background: "transparent",
                      border: "none",
                      color: "#e2e8f0",
                      width: "100%",
                      font: "inherit",
                    }}
                  />
                </td>
                <td style={{ padding: "0.5rem", color: "#94a3b8" }}>{item.note}</td>
                <td style={{ padding: "0.5rem" }}>
                  <button
                    onClick={() => deleteItem(item.id)}
                    style={{ background: "none", border: "none", color: "#f87171", cursor: "pointer" }}
                  >
                    删
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>

        <form onSubmit={addItem} style={{ marginTop: "1rem", display: "flex", gap: "0.5rem", flexWrap: "wrap" }}>
          <select
            value={newItem.category}
            onChange={(e) => setNewItem({ ...newItem, category: e.target.value })}
            style={{ padding: "0.375rem", background: "#1e293b", color: "#e2e8f0", border: "1px solid #475569", borderRadius: "0.25rem" }}
          >
            <option value="account">account</option>
            <option value="address">address</option>
            <option value="preference">preference</option>
            <option value="id-info">id-info</option>
            <option value="contact">contact</option>
          </select>
          <input
            placeholder="名称"
            value={newItem.key}
            onChange={(e) => setNewItem({ ...newItem, key: e.target.value })}
            style={{ padding: "0.375rem", background: "#1e293b", color: "#e2e8f0", border: "1px solid #475569", borderRadius: "0.25rem" }}
          />
          <input
            placeholder="内容"
            value={newItem.value}
            onChange={(e) => setNewItem({ ...newItem, value: e.target.value })}
            style={{ padding: "0.375rem", background: "#1e293b", color: "#e2e8f0", border: "1px solid #475569", borderRadius: "0.25rem", flex: 1, minWidth: "150px" }}
          />
          <input
            placeholder="备注"
            value={newItem.note}
            onChange={(e) => setNewItem({ ...newItem, note: e.target.value })}
            style={{ padding: "0.375rem", background: "#1e293b", color: "#e2e8f0", border: "1px solid #475569", borderRadius: "0.25rem" }}
          />
          <button
            type="submit"
            disabled={saving}
            style={{ padding: "0.375rem 0.75rem", background: "#3b82f6", color: "#fff", border: "none", borderRadius: "0.25rem", cursor: "pointer" }}
          >
            添加
          </button>
        </form>
      </section>

      <section>
        <h3 style={{ fontSize: "1rem" }}>事件</h3>
        <ul style={{ listStyle: "none", padding: 0, margin: 0, fontSize: "0.875rem" }}>
          {events.map((ev) => (
            <li key={ev.id} style={{ padding: "0.5rem", borderBottom: "1px solid #1e293b", display: "flex", gap: "0.5rem", alignItems: "baseline" }}>
              <span style={{ color: "#64748b", flexShrink: 0 }}>{ev.happened_at.slice(0, 16)}</span>
              <span style={{ fontWeight: 500 }}>{ev.title}</span>
              {ev.detail && <span style={{ color: "#94a3b8" }}>{ev.detail}</span>}
              <button
                onClick={() => deleteEvent(ev.id)}
                style={{ marginLeft: "auto", background: "none", border: "none", color: "#f87171", cursor: "pointer", flexShrink: 0 }}
              >
                删
              </button>
            </li>
          ))}
        </ul>

        <form onSubmit={addEvent} style={{ marginTop: "1rem", display: "flex", gap: "0.5rem" }}>
          <input
            placeholder="事件标题"
            value={newEvent.title}
            onChange={(e) => setNewEvent({ ...newEvent, title: e.target.value })}
            style={{ padding: "0.375rem", background: "#1e293b", color: "#e2e8f0", border: "1px solid #475569", borderRadius: "0.25rem" }}
          />
          <input
            placeholder="详情（可选）"
            value={newEvent.detail}
            onChange={(e) => setNewEvent({ ...newEvent, detail: e.target.value })}
            style={{ padding: "0.375rem", background: "#1e293b", color: "#e2e8f0", border: "1px solid #475569", borderRadius: "0.25rem", flex: 1 }}
          />
          <button
            type="submit"
            disabled={saving}
            style={{ padding: "0.375rem 0.75rem", background: "#3b82f6", color: "#fff", border: "none", borderRadius: "0.25rem", cursor: "pointer" }}
          >
            添加
          </button>
        </form>
      </section>
    </div>
  );
}
