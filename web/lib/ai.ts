// AI 配置与调用：与 src/ai.rs 同一套 OpenAI 兼容接口。
// API key 取环境变量/本机 secrets；非敏感设置从 config.toml 热加载。
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
  const p = process.env.AI_SECRETS_PATH || join(process.cwd(), "..", "config", "secrets", ".secrets.env");
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

function readTomlFile(): Record<string, string | boolean | number> {
  const p = process.env.AI_CONFIG_PATH || join(process.cwd(), "..", "config", "config.toml");
  if (!existsSync(p)) return {};
  const values: Record<string, string | boolean | number> = {};
  let section = "";
  for (const raw of readFileSync(p, "utf-8").split("\n")) {
    const line = raw.replace(/#.*/, "").trim();
    if (!line) continue;
    const header = line.match(/^\[([^\]]+)\]$/);
    if (header) {
      section = header[1];
      continue;
    }
    if (section !== "ai") continue;
    const match = line.match(/^([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.+)$/);
    if (!match) continue;
    const value = match[2].trim();
    if (/^".*"$/.test(value)) values[match[1]] = value.slice(1, -1);
    else if (value === "true" || value === "false") values[match[1]] = value === "true";
    else if (/^\d+$/.test(value)) values[match[1]] = Number(value);
  }
  return values;
}

export function aiConfig(): { apiKey: string; baseUrl: string; model: string; thinking: boolean; thinkingBudget: number } {
  const file = readEnvFile();
  const legacy: Record<string, string> = process.env.AI_ENV_PATH ? {} : (() => {
    const p = join(process.cwd(), "..", "docker", ".env");
    if (!existsSync(p)) return {};
    return readFileSync(p, "utf-8").split("\n").reduce<Record<string, string>>((out, line) => {
      const i = line.indexOf("=");
      if (i > 0 && line.slice(0, i).trim() === "ai_api_key") out.ai_api_key = line.slice(i + 1).trim();
      return out;
    }, {});
  })();
  const config = readTomlFile();
  return {
    apiKey: process.env.AI_API_KEY || file.AI_API_KEY || legacy.ai_api_key || "",
    baseUrl: (process.env.AI_BASE_URL || String(config.base_url || "https://dashscope.aliyuncs.com/compatible-mode/v1")).replace(/\/+$/, ""),
    model: process.env.AI_MODEL || String(config.model || "qwen-plus"),
    thinking: process.env.AI_THINKING === "1" || config.thinking === true,
    thinkingBudget: Number(process.env.AI_THINKING_BUDGET || config.thinking_budget || 0),
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
- 账号的 category 用 account；其他个人信息按维度使用 address / preference / id-info / contact / work / education 等，也可用简短分类名记录其他事情和属性
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
  const { apiKey, baseUrl, model, thinking, thinkingBudget } = aiConfig();
  if (!apiKey) throw new Error("未配置 AI_API_KEY（请运行 make settings，或填写 config/secrets/.secrets.env）");

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
      body: JSON.stringify({
        model,
        messages,
        response_format: { type: "json_object" },
        temperature: 0.1,
        enable_thinking: thinking,
        ...(thinking && thinkingBudget > 0 ? { thinking_budget: thinkingBudget } : {}),
      }),
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
