"use client";

import { useCallback, useEffect, useRef, useState } from "react";

type DraftItem = { category: string; key: string; value: string; note: string };
type DraftEvent = { title: string; detail: string };
type DraftSecret = { title: string; kind: string };
type Draft = { items: DraftItem[]; events: DraftEvent[]; secrets: DraftSecret[] };

type Message = {
  id: number;
  role: "user" | "assistant";
  content: string;
  draft: Draft | null;
  status: string;
};

type Session = { id: number; title: string; created_at: string; last_at: string | null };

const CATEGORIES: Record<string, string> = {
  account: "账号",
  address: "地址",
  preference: "偏好",
  "id-info": "证件",
  contact: "联系方式",
};

function DraftCard({
  message,
  onApplied,
  onDismissed,
}: {
  message: Message;
  onApplied: (id: number, items: number, events: number) => void;
  onDismissed: (id: number) => void;
}) {
  const draft = message.draft!;
  const [checkedItems, setCheckedItems] = useState<boolean[]>(draft.items.map(() => true));
  const [checkedEvents, setCheckedEvents] = useState<boolean[]>(draft.events.map(() => true));
  const [busy, setBusy] = useState(false);
  const hasDraft = draft.items.length + draft.events.length > 0;

  async function apply() {
    setBusy(true);
    const items = draft.items.filter((_, i) => checkedItems[i]);
    const events = draft.events.filter((_, i) => checkedEvents[i]);
    const resp = await fetch("/api/ai/apply", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ message_id: message.id, items, events }),
    });
    setBusy(false);
    if (resp.ok) {
      const r = await resp.json();
      onApplied(message.id, r.items, r.events);
    } else {
      alert(`写入失败: ${(await resp.json()).error}`);
    }
  }

  async function dismiss() {
    setBusy(true);
    await fetch("/api/ai/dismiss", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ message_id: message.id }),
    });
    setBusy(false);
    onDismissed(message.id);
  }

  return (
    <div style={{ marginTop: "0.5rem", border: "1px solid #334155", borderRadius: "0.5rem", overflow: "hidden" }}>
      {draft.items.length > 0 && (
        <div style={{ padding: "0.5rem 0.75rem" }}>
          <div style={{ fontSize: "0.75rem", color: "#38bdf8", marginBottom: "0.25rem" }}>条目草稿</div>
          {draft.items.map((it, i) => (
            <label key={i} style={{ display: "flex", gap: "0.5rem", padding: "0.25rem 0", fontSize: "0.875rem", cursor: message.status ? "default" : "pointer" }}>
              <input
                type="checkbox"
                checked={checkedItems[i]}
                disabled={!!message.status}
                onChange={(e) => setCheckedItems((prev) => prev.map((v, j) => (j === i ? e.target.checked : v)))}
              />
              <span>
                <span style={{ color: "#94a3b8" }}>[{CATEGORIES[it.category] ?? it.category}]</span> {it.key} = {it.value}
                {it.note && <span style={{ color: "#64748b" }}>（{it.note}）</span>}
              </span>
            </label>
          ))}
        </div>
      )}
      {draft.events.length > 0 && (
        <div style={{ padding: "0.5rem 0.75rem", borderTop: draft.items.length ? "1px solid #1e293b" : undefined }}>
          <div style={{ fontSize: "0.75rem", color: "#38bdf8", marginBottom: "0.25rem" }}>事件草稿</div>
          {draft.events.map((ev, i) => (
            <label key={i} style={{ display: "flex", gap: "0.5rem", padding: "0.25rem 0", fontSize: "0.875rem", cursor: message.status ? "default" : "pointer" }}>
              <input
                type="checkbox"
                checked={checkedEvents[i]}
                disabled={!!message.status}
                onChange={(e) => setCheckedEvents((prev) => prev.map((v, j) => (j === i ? e.target.checked : v)))}
              />
              <span>
                {ev.title}
                {ev.detail && <span style={{ color: "#64748b" }}> — {ev.detail}</span>}
              </span>
            </label>
          ))}
        </div>
      )}
      {draft.secrets.length > 0 && (
        <div style={{ padding: "0.5rem 0.75rem", borderTop: "1px solid #1e293b", background: "#451a0322" }}>
          <div style={{ fontSize: "0.75rem", color: "#f59e0b", marginBottom: "0.25rem" }}>机密提醒（不写入，需手动存 kdbx）</div>
          {draft.secrets.map((s, i) => (
            <div key={i} style={{ fontSize: "0.875rem", color: "#fbbf24", padding: "0.125rem 0" }}>
              {s.title}（{s.kind}）→ make pass-add-manual
            </div>
          ))}
        </div>
      )}
      <details style={{ borderTop: "1px solid #1e293b", padding: "0.375rem 0.75rem" }}>
        <summary style={{ fontSize: "0.75rem", color: "#64748b", cursor: "pointer" }}>模型原始输出</summary>
        <pre
          style={{
            margin: "0.375rem 0 0",
            padding: "0.5rem",
            background: "#0f172a",
            borderRadius: "0.375rem",
            fontSize: "0.75rem",
            color: "#94a3b8",
            overflow: "auto",
            maxHeight: "16rem",
            whiteSpace: "pre-wrap",
            wordBreak: "break-all",
          }}
        >
          {JSON.stringify(draft, null, 2)}
        </pre>
      </details>
      {hasDraft && (
        <div style={{ padding: "0.5rem 0.75rem", borderTop: "1px solid #1e293b", display: "flex", gap: "0.5rem", alignItems: "center" }}>
          {message.status === "applied" ? (
            <span style={{ color: "#4ade80", fontSize: "0.875rem" }}>✓ 已写入</span>
          ) : message.status === "dismissed" ? (
            <span style={{ color: "#64748b", fontSize: "0.875rem" }}>已忽略</span>
          ) : (
            <>
              <button
                onClick={apply}
                disabled={busy}
                style={{ background: "#16a34a", color: "#fff", border: "none", borderRadius: "0.375rem", padding: "0.375rem 0.875rem", cursor: "pointer", fontSize: "0.875rem" }}
              >
                确认写入
              </button>
              <button
                onClick={dismiss}
                disabled={busy}
                style={{ background: "transparent", color: "#94a3b8", border: "1px solid #334155", borderRadius: "0.375rem", padding: "0.375rem 0.875rem", cursor: "pointer", fontSize: "0.875rem" }}
              >
                忽略
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}

export default function AiChat({ identities }: { identities: string[] }) {
  const [identity, setIdentity] = useState(identities[0] ?? "");
  const [sessions, setSessions] = useState<Session[]>([]);
  const [current, setCurrent] = useState<number | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const bottomRef = useRef<HTMLDivElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  const loadSessions = useCallback(async (idn: string) => {
    if (!idn) return;
    const resp = await fetch(`/api/ai/sessions?identity=${encodeURIComponent(idn)}`);
    const data = await resp.json();
    setSessions(data.sessions ?? []);
    return data.sessions as Session[];
  }, []);

  const openSession = useCallback(async (id: number) => {
    setCurrent(id);
    const resp = await fetch(`/api/ai/sessions/${id}`);
    const data = await resp.json();
    setMessages(data.messages ?? []);
  }, []);

  useEffect(() => {
    (async () => {
      const list = await loadSessions(identity);
      if (list && list.length > 0) await openSession(list[0].id);
      else setCurrent(null);
      setMessages([]);
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [identity]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, loading]);

  async function newChat() {
    const resp = await fetch("/api/ai/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ identity }),
    });
    const { id } = await resp.json();
    await loadSessions(identity);
    setCurrent(id);
    setMessages([]);
  }

  async function send() {
    const text = input.trim();
    if (!text || loading) return;
    setError("");
    let sid = current;
    if (!sid) {
      const resp = await fetch("/api/ai/sessions", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ identity }),
      });
      sid = (await resp.json()).id;
      setCurrent(sid);
    }
    setInput("");
    setLoading(true);
    const tempId = -Date.now();
    setMessages((prev) => [...prev, { id: tempId, role: "user", content: text, draft: null, status: "" }]);
    try {
      const resp = await fetch("/api/ai/chat", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ session_id: sid, text }),
      });
      const data = await resp.json();
      if (!resp.ok) throw new Error(data.error || `HTTP ${resp.status}`);
      setMessages((prev) => [
        ...prev,
        { id: data.id, role: "assistant", content: data.content, draft: data.draft, status: data.status },
      ]);
      loadSessions(identity);
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
      setMessages((prev) => prev.filter((m) => m.id !== tempId));
    } finally {
      setLoading(false);
    }
  }

  function onFile(e: React.ChangeEvent<HTMLInputElement>) {
    const f = e.target.files?.[0];
    if (!f) return;
    if (f.size > 512 * 1024) {
      setError("文件超过 512KB，请拆分后再传");
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      setInput((prev) => `${prev}${prev ? "\n\n" : ""}--- ${f.name} ---\n${String(reader.result)}`);
    };
    reader.readAsText(f);
    e.target.value = "";
  }

  function onApplied(id: number, items: number, events: number) {
    setMessages((prev) =>
      prev.map((m) =>
        m.id === id ? { ...m, status: "applied", content: `${m.content}\n\n✓ 已写入 ${items} 条条目、${events} 条事件` } : m
      )
    );
  }

  function onDismissed(id: number) {
    setMessages((prev) => prev.map((m) => (m.id === id ? { ...m, status: "dismissed" } : m)));
  }

  return (
    <div style={{ display: "flex", height: "100vh" }}>
      <aside style={{ width: "220px", borderRight: "1px solid #334155", padding: "1rem", flexShrink: 0, display: "flex", flexDirection: "column", gap: "0.75rem", overflow: "auto" }}>
        <a href="/" style={{ color: "#94a3b8", textDecoration: "none", fontSize: "0.875rem" }}>← 数据</a>
        <div>
          <div style={{ fontSize: "0.75rem", color: "#64748b", marginBottom: "0.25rem" }}>身份</div>
          <div style={{ display: "flex", flexDirection: "column", gap: "0.25rem" }}>
            {identities.map((name) => (
              <button
                key={name}
                onClick={() => setIdentity(name)}
                style={{
                  textAlign: "left",
                  padding: "0.375rem 0.5rem",
                  borderRadius: "0.375rem",
                  border: "none",
                  cursor: "pointer",
                  color: name === identity ? "#fff" : "#94a3b8",
                  background: name === identity ? "#3b82f6" : "transparent",
                }}
              >
                {name}
              </button>
            ))}
          </div>
        </div>
        <button
          onClick={newChat}
          disabled={!identity}
          style={{ background: "#3b82f6", color: "#fff", border: "none", borderRadius: "0.375rem", padding: "0.5rem", cursor: "pointer" }}
        >
          + 新建对话
        </button>
        <div style={{ display: "flex", flexDirection: "column", gap: "0.25rem" }}>
          {sessions.map((s) => (
            <button
              key={s.id}
              onClick={() => openSession(s.id)}
              style={{
                textAlign: "left",
                padding: "0.375rem 0.5rem",
                borderRadius: "0.375rem",
                border: "none",
                cursor: "pointer",
                fontSize: "0.8125rem",
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                color: s.id === current ? "#fff" : "#94a3b8",
                background: s.id === current ? "#1e293b" : "transparent",
              }}
            >
              {s.title || "（空对话）"}
            </button>
          ))}
        </div>
      </aside>

      <main style={{ flex: 1, display: "flex", flexDirection: "column", minWidth: 0 }}>
        <div style={{ flex: 1, overflow: "auto", padding: "1.5rem" }}>
          {messages.length === 0 && !loading && (
            <p style={{ color: "#64748b" }}>
              粘贴资料或提问，AI 会整理成条目/事件草稿，确认后才写库。密码、授权码、助记词别贴进来——AI 识别到也只列提醒。
            </p>
          )}
          {messages.map((m) => (
            <div key={m.id} style={{ display: "flex", justifyContent: m.role === "user" ? "flex-end" : "flex-start", marginBottom: "1rem" }}>
              <div
                style={{
                  maxWidth: "75%",
                  padding: "0.625rem 0.875rem",
                  borderRadius: "0.75rem",
                  background: m.role === "user" ? "#3b82f6" : "#1e293b",
                  color: "#e2e8f0",
                  whiteSpace: "pre-wrap",
                  wordBreak: "break-word",
                  fontSize: "0.9375rem",
                }}
              >
                {m.content}
                {m.role === "assistant" && m.draft && (m.draft.items.length + m.draft.events.length + m.draft.secrets.length > 0) && (
                  <DraftCard message={m} onApplied={onApplied} onDismissed={onDismissed} />
                )}
              </div>
            </div>
          ))}
          {loading && <p style={{ color: "#64748b" }}>整理中…</p>}
          {error && <p style={{ color: "#f87171" }}>错误: {error}</p>}
          <div ref={bottomRef} />
        </div>

        <div style={{ borderTop: "1px solid #334155", padding: "0.75rem 1.5rem", display: "flex", gap: "0.5rem", alignItems: "flex-end" }}>
          <input ref={fileRef} type="file" accept=".txt,.md,.csv,.json,.log,.yaml,.yml" style={{ display: "none" }} onChange={onFile} />
          <button
            onClick={() => fileRef.current?.click()}
            title="附加文本文件（txt/md/csv/json）"
            style={{ background: "transparent", color: "#94a3b8", border: "1px solid #334155", borderRadius: "0.375rem", padding: "0.5rem 0.75rem", cursor: "pointer" }}
          >
            📎
          </button>
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                send();
              }
            }}
            placeholder="粘贴资料，Enter 发送，Shift+Enter 换行"
            rows={3}
            style={{
              flex: 1,
              background: "#1e293b",
              color: "#e2e8f0",
              border: "1px solid #334155",
              borderRadius: "0.5rem",
              padding: "0.625rem 0.875rem",
              resize: "vertical",
              fontFamily: "inherit",
              fontSize: "0.9375rem",
            }}
          />
          <button
            onClick={send}
            disabled={loading || !input.trim()}
            style={{
              background: loading || !input.trim() ? "#334155" : "#3b82f6",
              color: "#fff",
              border: "none",
              borderRadius: "0.375rem",
              padding: "0.5rem 1.25rem",
              cursor: loading || !input.trim() ? "default" : "pointer",
            }}
          >
            发送
          </button>
        </div>
      </main>
    </div>
  );
}
