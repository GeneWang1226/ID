#!/usr/bin/env python3
"""把一段杂乱资料交给 AI 整理成 items/events 草稿，人工确认后写入 SQLite。

用法:
  scripts/ai-organize.py [--identity 身份名] [资料文件]
  无文件参数时从 stdin 读（粘贴后 Ctrl+D）
  make ai id=jacob          # 推荐：先指定身份再粘贴

配置写在 ~/ID/.env（权限 600，不进 git；模板见 .env.example）:
  ai_api_key=sk-...
  ai_base_url=https://dashscope.aliyuncs.com/compatible-mode/v1
  ai_model=qwen-plus

安全红线:
- 只发非机密资料给云端 AI。密码、授权码、助记词绝不粘贴进来。
- AI 只输出草稿，确认后才写库；机密条目只列「待手动存 kdbx」提醒，不生成内容。
"""

import argparse
import json
import re
import sqlite3
import sys
import urllib.request
from pathlib import Path

BASE_DIR = Path(__file__).resolve().parent.parent
CONFIG = BASE_DIR / ".env"
DB = BASE_DIR / "data" / "id.db"
SCHEMA = BASE_DIR / "template" / "data" / "schema.sql"
IDENTITIES_DIR = BASE_DIR / "identities"

_TTY = sys.stdout.isatty() and sys.stderr.isatty()


def c(code: int, s: str) -> str:
    return f"\033[{code}m{s}\033[0m" if _TTY else s


def head(s: str) -> str:
    return c(96, s)


def ok(s: str) -> str:
    return c(32, s)


def warn(s: str) -> str:
    return c(33, s)


def dim(s: str) -> str:
    return c(2, s)


def err(s: str) -> str:
    return c(31, s)


def list_identities() -> list[str]:
    if not IDENTITIES_DIR.is_dir():
        return []
    return sorted(
        p.name
        for p in IDENTITIES_DIR.iterdir()
        if p.is_dir() and not p.name.startswith(".")
    )


def prompt_identity() -> str:
    names = list_identities()
    hint = f"（{' / '.join(names)}）" if names else ""
    while True:
        print(c(36, f"身份{hint}: "), end="", file=sys.stderr, flush=True)
        line = sys.stdin.readline()
        if not line:
            print(err("错误: 未指定身份"), file=sys.stderr)
            sys.exit(1)
        name = line.strip().lower()
        if not name:
            print(err("  身份不能为空"), file=sys.stderr)
            continue
        if names and name not in names:
            print(warn(f"  警告: identities/ 下没有 '{name}'，仍继续"), file=sys.stderr)
        return name


def system_prompt(identity: str) -> str:
    return f"""你是个人资料整理助手。用户会给你一段杂乱的资料文本，你要提取成结构化数据。

**当前归属身份：{identity}**
- items 和 events 里每条记录的 identity 字段必须是 "{identity}"（除非资料里明确写了另一个已存在身份名且与当前不同，那种情况 identity 用资料里的名字）
- 资料没提身份、或只是账号/号码片段时，一律用 "{identity}"

输出**只包含一个 JSON 对象**，不要任何其他文字，格式：
{{
  "reply": "给用户看的简短说明（中文）：整理出了什么、或为什么没东西可写",
  "items": [{{"identity": "...", "category": "...", "key": "...", "value": "...", "note": "..."}}],
  "events": [{{"identity": "...", "title": "...", "detail": "..."}}],
  "secrets": [{{"identity": "...", "title": "...", "kind": "密码/授权码/助记词/密钥"}}]
}}

规则：
- category 只能是: account / address / preference / id-info / contact
- key 用简洁标识（如 QQ号、github-username），value 是内容，note 放备注
- events 记录值得留痕的事（注册、导入、换绑），没有就给空数组
- 资料过短或无法结构化（如单独一个数字）时，items/events/secrets 都为空，在 reply 里说明原因
- **密码、授权码、恢复助记词、密钥绝不写进 items/events 的 value**，只在 secrets 里列出标题提醒用户手动存 kdbx
- 不确定的字段留空字符串，不要编造"""


def read_config() -> dict:
    cfg = {}
    if not CONFIG.exists():
        return cfg
    for line in CONFIG.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        k, v = line.split("=", 1)
        cfg[k.strip()] = v.strip().strip('"').strip("'")
    return cfg


def call_ai(base_url: str, api_key: str, model: str, identity: str, text: str) -> dict:
    body = json.dumps(
        {
            "model": model,
            "messages": [
                {"role": "system", "content": system_prompt(identity)},
                {"role": "user", "content": text},
            ],
            "response_format": {"type": "json_object"},
            "temperature": 0.1,
        }
    ).encode()
    req = urllib.request.Request(
        base_url.rstrip("/") + "/chat/completions",
        data=body,
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {api_key}"},
    )
    with urllib.request.urlopen(req, timeout=180) as resp:
        data = json.loads(resp.read())
    content = data["choices"][0]["message"]["content"]
    m = re.search(r"\{.*\}", content, re.S)
    if not m:
        raise ValueError(f"AI 没有返回 JSON: {content[:200]}")
    return json.loads(m.group(0))


def normalize_identity(row: dict, default: str) -> None:
    if not (row.get("identity") or "").strip():
        row["identity"] = default


def main() -> int:
    parser = argparse.ArgumentParser(description="AI 整理资料写入 SQLite 草稿")
    parser.add_argument("--identity", "-i", help="归属身份（建议必填；缺则在终端询问）")
    parser.add_argument("file", nargs="?", help="资料文件；省略则从 stdin 读")
    args = parser.parse_args()

    identity = (args.identity or "").strip().lower()
    if not identity:
        if _TTY:
            identity = prompt_identity()
        else:
            print(err("错误: 请指定身份，例如 make ai id=jacob"), file=sys.stderr)
            return 1

    if args.file:
        text = Path(args.file).read_text()
    else:
        print(c(36, f"[{identity}] 请粘贴资料，输入完成后按 Ctrl+D:"), file=sys.stderr)
        text = sys.stdin.read()
    text = text.strip()
    if not text:
        print(err("错误: 没有输入内容"), file=sys.stderr)
        return 1

    if len(text) < 8 and _TTY:
        print(warn("提示: 内容很短，模型多半整理不出条目；需要更多上下文（账号类型、用途等）。"), file=sys.stderr)

    cfg = read_config()
    api_key = cfg.get("ai_api_key", "")
    base_url = cfg.get("ai_base_url", "https://dashscope.aliyuncs.com/compatible-mode/v1")
    model = cfg.get("ai_model", "qwen-plus")
    if not api_key:
        print(err("错误: .env 里还没有 ai_api_key。请填入后重试（模板见 .env.example）"), file=sys.stderr)
        return 1

    print(dim(f"==> 调用 {model} 整理中（身份 {identity}）..."), file=sys.stderr)
    try:
        draft = call_ai(base_url, api_key, model, identity, text)
    except Exception as e:
        print(err(f"错误: AI 调用失败: {e}"), file=sys.stderr)
        return 1

    reply = (draft.get("reply") or "").strip()
    items = draft.get("items") or []
    events = draft.get("events") or []
    secrets = draft.get("secrets") or []

    for it in items:
        normalize_identity(it, identity)
    for ev in events:
        normalize_identity(ev, identity)
    for s in secrets:
        normalize_identity(s, identity)

    if reply:
        print("\n" + head("===== AI 说明 ====="))
        print(reply)

    print("\n" + head("===== 模型原始输出（JSON）====="))
    print(json.dumps(draft, ensure_ascii=False, indent=2))

    print("\n" + head("===== 草稿：条目 ====="))
    if items:
        for i, it in enumerate(items, 1):
            idn = it.get("identity", "")
            flag = warn(" ⚠ 缺身份") if not idn.strip() else ""
            print(
                f"{i}. [{idn}] {it.get('category', '')}/{it.get('key', '')} = {it.get('value', '')}  "
                f"{dim('(' + it.get('note', '') + ')')}{flag}"
            )
    else:
        print(dim("（无）"))

    print("\n" + head("===== 草稿：事件 ====="))
    if events:
        for i, ev in enumerate(events, 1):
            print(f"{i}. [{ev.get('identity', '')}] {ev.get('title', '')} — {dim(ev.get('detail', ''))}")
    else:
        print(dim("（无）"))

    if secrets:
        print("\n" + warn("===== 机密提醒（不写入，需手动存 kdbx）====="))
        for i, s in enumerate(secrets, 1):
            sid = s.get("identity") or identity
            print(
                warn(
                    f"{i}. [{sid}] {s.get('title', '')}（{s.get('kind', '')}）"
                    f"→ make pass-add-manual id={sid}"
                )
            )

    if not items and not events:
        print("\n" + dim("没有可写入 SQLite 的条目/事件。"))
        if reply:
            print(dim("原因见上方「AI 说明」。"))
        else:
            print(dim("资料可能过短或模型未识别出结构化信息；可换 make ai id=<身份> 并贴更完整的资料。"))
        return 0

    bad = [it for it in items if not (it.get("identity") or "").strip()]
    bad += [ev for ev in events if not (ev.get("identity") or "").strip()]
    if bad:
        print(err("\n错误: 仍有条目/事件缺少 identity，已取消写入。请重试或检查模型输出。"), file=sys.stderr)
        return 1

    ans = input("\n" + c(36, f"确认写入 SQLite（身份 {identity}）? [y/N] ")).strip().lower()
    if ans != "y":
        print(dim("已取消，未写入。"))
        return 0

    DB.parent.mkdir(exist_ok=True)
    conn = sqlite3.connect(DB)
    conn.executescript(SCHEMA.read_text())
    for it in items:
        conn.execute(
            "INSERT INTO items(identity,category,key,value,note) VALUES(?,?,?,?,?) "
            "ON CONFLICT(identity,category,key) DO UPDATE SET "
            "value=excluded.value, note=excluded.note, updated_at=datetime('now')",
            (
                it.get("identity", ""),
                it.get("category", ""),
                it.get("key", ""),
                it.get("value", ""),
                it.get("note", ""),
            ),
        )
    for ev in events:
        conn.execute(
            "INSERT INTO events(identity,title,detail) VALUES(?,?,?)",
            (ev.get("identity", ""), ev.get("title", ""), ev.get("detail", "")),
        )
    conn.commit()
    conn.close()
    print(ok(f"✓ 已写入: {len(items)} 条 items, {len(events)} 条 events。"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
