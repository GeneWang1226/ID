#!/usr/bin/env python3
"""把一段杂乱资料交给 AI 整理成 items/events 草稿，人工确认后写入 SQLite。

用法:
  scripts/ai-organize.py <资料文件>
  或:  some-command | scripts/ai-organize.py

配置写在 ~/ID/config（权限 600，不进 git）:
  ai_api_key=sk-...
  ai_base_url=https://dashscope.aliyuncs.com/compatible-mode/v1   # 可换成 Ollama: http://localhost:11434/v1
  ai_model=qwen-plus

安全红线:
- 只发非机密资料给云端 AI。密码、授权码、助记词绝不粘贴进来。
- AI 只输出草稿，确认后才写库；机密条目只列「待手动存 kdbx」提醒，不生成内容。
"""

import json
import re
import sqlite3
import sys
import urllib.request
from pathlib import Path

BASE_DIR = Path(__file__).resolve().parent.parent
CONFIG = BASE_DIR / "config"
DB = BASE_DIR / "data" / "id.db"
SCHEMA = BASE_DIR / "template" / "data" / "schema.sql"

# 颜色（仅终端输出时生效；管道/重定向自动无色）
_TTY = sys.stdout.isatty()
def c(code: int, s: str) -> str:
    return f"\033[{code}m{s}\033[0m" if _TTY else s
def head(s: str) -> str: return c(96, s)    # 亮青标题
def ok(s: str) -> str:   return c(32, s)    # 绿
def warn(s: str) -> str: return c(33, s)    # 黄
def dim(s: str) -> str:  return c(2, s)     # 暗灰

SYSTEM_PROMPT = """你是个人资料整理助手。用户会给你一段杂乱的资料文本，你要提取成结构化数据。

输出**只包含一个 JSON 对象**，不要任何其他文字，格式：
{
  "items": [{"identity": "...", "category": "...", "key": "...", "value": "...", "note": "..."}],
  "events": [{"identity": "...", "title": "...", "detail": "..."}],
  "secrets": [{"identity": "...", "title": "...", "kind": "密码/授权码/助记词/密钥"}]
}

规则：
- identity 用小写身份名（如 gene、jacob、jigsaw）；资料里出现的人名映射到对应身份
- category 只能是: account / address / preference / id-info / contact
- key 用简洁标识（如邮箱地址、账号名），value 是内容，note 放备注
- events 记录值得留痕的事（注册、导入、换绑），没有就给空数组
- **密码、授权码、恢复助记词、密钥绝不写进 items/events 的 value**，只在 secrets 里列出标题提醒用户手动存 kdbx，不要输出机密内容本身
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


def call_ai(base_url: str, api_key: str, model: str, text: str) -> dict:
    body = json.dumps({
        "model": model,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": text},
        ],
        "response_format": {"type": "json_object"},
        "temperature": 0.1,
    }).encode()
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


def main() -> int:
    # 读输入：文件参数或 stdin
    if len(sys.argv) > 1 and sys.argv[1] not in ("-", ""):
        text = Path(sys.argv[1]).read_text()
    else:
        print(c(36, "请粘贴资料，输入完成后按 Ctrl+D:"), file=sys.stderr)
        text = sys.stdin.read()
    if not text.strip():
        print(c(31, "错误: 没有输入内容"), file=sys.stderr)
        return 1

    cfg = read_config()
    api_key = cfg.get("ai_api_key", "")
    base_url = cfg.get("ai_base_url", "https://dashscope.aliyuncs.com/compatible-mode/v1")
    model = cfg.get("ai_model", "qwen-plus")
    if not api_key:
        print(c(31, "错误: config 里还没有 ai_api_key。请填入后重试（见脚本头部注释）"), file=sys.stderr)
        return 1

    print(dim(f"==> 调用 {model} 整理中..."), file=sys.stderr)
    try:
        draft = call_ai(base_url, api_key, model, text)
    except Exception as e:
        print(c(31, f"错误: AI 调用失败: {e}"), file=sys.stderr)
        return 1

    items = draft.get("items") or []
    events = draft.get("events") or []
    secrets = draft.get("secrets") or []

    print("\n" + head("===== 草稿：条目 ====="))
    for i, it in enumerate(items, 1):
        print(f"{i}. [{it.get('identity','')}] {it.get('category','')}/{it.get('key','')} = {it.get('value','')}  {dim('(' + it.get('note','') + ')')}")
    print("\n" + head("===== 草稿：事件 ====="))
    for i, ev in enumerate(events, 1):
        print(f"{i}. [{ev.get('identity','')}] {ev.get('title','')} — {dim(ev.get('detail',''))}")
    if secrets:
        print("\n" + warn("===== 机密提醒（不写入，需手动存 kdbx）====="))
        for i, s in enumerate(secrets, 1):
            print(warn(f"{i}. [{s.get('identity','')}] {s.get('title','')}（{s.get('kind','')}）→ make pass-add-manual id={s.get('identity','')}"))

    if not items and not events:
        print("\n" + dim("没有可写入的内容。"))
        return 0

    ans = input("\n" + c(36, "确认写入 SQLite? [y/N] ")).strip().lower()
    if ans != "y":
        print(dim("已取消，未写入。"))
        return 0

    DB.parent.mkdir(exist_ok=True)
    conn = sqlite3.connect(DB)
    conn.executescript(SCHEMA.read_text())
    for it in items:
        conn.execute(
            "INSERT INTO items(identity,category,key,value,note) VALUES(?,?,?,?,?) "
            "ON CONFLICT(identity,category,key) DO UPDATE SET value=excluded.value, note=excluded.note, updated_at=datetime('now')",
            (it.get("identity",""), it.get("category",""), it.get("key",""),
             it.get("value",""), it.get("note","")),
        )
    for ev in events:
        conn.execute(
            "INSERT INTO events(identity,title,detail) VALUES(?,?,?)",
            (ev.get("identity",""), ev.get("title",""), ev.get("detail","")),
        )
    conn.commit()
    conn.close()
    print(ok(f"✓ 已写入: {len(items)} 条 items, {len(events)} 条 events。"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
