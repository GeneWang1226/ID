// AI 配置与调用：与 scripts/ai-organize.py 同一套 OpenAI 兼容接口。
// 配置优先取环境变量（docker compose 注入），其次读 ../.env（本地 next dev）。
import { existsSync, readFileSync } from "fs";
import { join } from "path";

export type DraftItem = { category: string; key: string; value: string; note: string };
export type DraftEvent = { title: string; detail: string };
export type DraftSecret = { title: string; kind: string };
export type AiDraft = {
  reply: string;
  items: DraftItem[];
  events: DraftEvent[];
  secrets: DraftSecret[];
};

function readEnvFile(): Record<string, string> {
  const p = process.env.AI_ENV_PATH || join(process.cwd(), "..", ".env");
  const cfg: Record<string, string> = {};
  if (!existsSync(p)) return cfg;
  for (const line of readFileSync(p, "utf-8").split("\n")) {
    const t = line.trim();
    if (!t || t.startsWith("#") || !t.includes("=")) continue;
    const i = t.indexOf("=");
    cfg[t.slice(0, i).trim()] = t.slice(i + 1).trim().replace(/^["']|["']$/g, "");
  }
  return cfg;
}

export function aiConfig(): { apiKey: string; baseUrl: string; model: string } {
  const file = readEnvFile();
  return {
    apiKey: process.env.AI_API_KEY || file.ai_api_key || "",
    baseUrl: (process.env.AI_BASE_URL || file.ai_base_url || "https://dashscope.aliyuncs.com/compatible-mode/v1").replace(/\/+$/, ""),
    model: process.env.AI_MODEL || file.ai_model || "qwen-plus",
  };
}

const SYSTEM_PROMPT = `你是身份资料整理助手，运行在用户的本地网页对话里。当前身份：__IDENTITY__。
用户会粘贴杂乱资料或提问。你的输出**只包含一个 JSON 对象**，不要任何其他文字：
{
  "reply": "给用户看的自然语言回复（中文，简洁）",
  "items": [{"category": "...", "key": "...", "value": "...", "note": "..."}],
  "events": [{"title": "...", "detail": "..."}],
  "secrets": [{"title": "...", "kind": "密码/授权码/助记词/密钥"}]
}

规则：
- items 的 category 只能是: account / address / preference / id-info / contact
- key 用简洁标识（如邮箱地址、账号名），value 是内容，note 放备注；所有条目都归属当前身份
- events 记录值得留痕的事（注册、导入、换绑），没有就给空数组
- **密码、授权码、恢复助记词、密钥绝不写进 items/events**，只在 secrets 里列标题，提醒用户手动存 kdbx，不要输出机密内容本身
- 用户只是提问或闲聊时，items/events/secrets 都给空数组，在 reply 里回答
- 不确定的字段留空字符串，不要编造`;

type ChatMessage = { role: "user" | "assistant"; content: string };

function asStr(v: unknown): string {
  return typeof v === "string" ? v : v == null ? "" : String(v);
}

function normalize(raw: Record<string, unknown>): AiDraft {
  const items = (Array.isArray(raw.items) ? raw.items : [])
    .map((it): DraftItem => ({
      category: asStr(it?.category),
      key: asStr(it?.key),
      value: asStr(it?.value),
      note: asStr(it?.note),
    }))
    .filter((it) => it.category && it.key && it.value);
  const events = (Array.isArray(raw.events) ? raw.events : [])
    .map((ev): DraftEvent => ({ title: asStr(ev?.title), detail: asStr(ev?.detail) }))
    .filter((ev) => ev.title);
  const secrets = (Array.isArray(raw.secrets) ? raw.secrets : [])
    .map((s): DraftSecret => ({ title: asStr(s?.title), kind: asStr(s?.kind) }))
    .filter((s) => s.title);
  return { reply: asStr(raw.reply), items, events, secrets };
}

export async function aiChat(identity: string, history: ChatMessage[], text: string): Promise<AiDraft> {
  const { apiKey, baseUrl, model } = aiConfig();
  if (!apiKey) throw new Error("未配置 ai_api_key（在 ~/ID/.env 里填，或给容器传 AI_API_KEY）");

  const messages = [
    { role: "system", content: SYSTEM_PROMPT.replace("__IDENTITY__", identity) },
    ...history.slice(-20),
    { role: "user", content: text.slice(0, 20000) },
  ];

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 180_000);
  try {
    const resp = await fetch(`${baseUrl}/chat/completions`, {
      method: "POST",
      signal: controller.signal,
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${apiKey}` },
      body: JSON.stringify({ model, messages, response_format: { type: "json_object" }, temperature: 0.1 }),
    });
    if (!resp.ok) {
      throw new Error(`AI 接口返回 ${resp.status}: ${(await resp.text()).slice(0, 200)}`);
    }
    const data = await resp.json();
    const content: string = data?.choices?.[0]?.message?.content ?? "";
    const m = content.match(/\{[\s\S]*\}/);
    if (!m) {
      // 模型没按格式返回时，至少把原文当回复展示
      return { reply: content || "（AI 没有返回内容）", items: [], events: [], secrets: [] };
    }
    return normalize(JSON.parse(m[0]) as Record<string, unknown>);
  } finally {
    clearTimeout(timer);
  }
}
