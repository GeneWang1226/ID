#!/usr/bin/env python3
"""把一段杂乱资料交给 AI 整理成 items/events 草稿，人工确认后写入 SQLite。

用法:
  scripts/ai-organize.py [--identity 身份名] [资料文件]
  无文件参数时进入交互输入（空行提交，Ctrl+C 取消）
  make ai                   # 方向键选择身份后再粘贴
  make ai id=jacob          # 直接指定身份

非敏感配置写在 ~/ID/config/config.toml（可同步）；敏感配置写在 ~/ID/config/secrets/.secrets.env（权限 600，不进 git）:
  AI_API_KEY=sk-...
  [ai] base_url / model / thinking / thinking_budget

安全红线:
- 只发非机密资料给云端 AI。密码、授权码、助记词绝不粘贴进来。
- AI 只输出草稿，确认后才写库；机密条目只列「待手动存 kdbx」提醒，不生成内容。
"""

import argparse
import json
import os
import re
import shlex
import sqlite3
import subprocess
import sys
import tomllib
import urllib.request
from pathlib import Path

from password_entries import infer_secret_title
from terminal_clipboard import check_clipboard, plaintext_allowed
from terminal_menu import choose_action

BASE_DIR = Path(__file__).resolve().parent.parent
CONFIG = BASE_DIR / "config" / "config.toml"
SECRETS = BASE_DIR / "config" / "secrets" / ".secrets.env"
LEGACY_ENV = BASE_DIR / "docker" / ".env"
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
    selector = BASE_DIR / "scripts" / "select-identity.sh"
    if selector.exists():
        result = subprocess.run(
            [str(selector)], cwd=BASE_DIR, stdin=sys.stdin, stdout=subprocess.PIPE, text=True
        )
        if result.returncode == 130:
            raise KeyboardInterrupt
        name = result.stdout.strip().lower()
    else:
        hint = f"（{' / '.join(names)}）" if names else ""
        print(c(36, f"身份{hint}: "), end="", file=sys.stderr, flush=True)
        name = sys.stdin.readline().strip().lower()
    if not name:
        print(err("错误: 未指定身份"), file=sys.stderr)
        sys.exit(1)
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
  "secrets": [{{"identity": "...", "title": "...", "kind": "密码/授权码/助记词/密钥", "action": "copy/save"}}]
}}

规则：
- category 只能是: account / address / preference / id-info / contact
- key 用简洁标识（如 QQ号、github-username），value 是内容，note 放备注
- item 只有 category、key、value 都非空时才输出；只有“有一个账号/密码”但没有可保存内容时，不要生成空 value 占位条目
- events 记录值得留痕的事（注册、导入、换绑），没有就给空数组
- 资料过短或无法结构化（如单独一个数字）时，items/events/secrets 都为空，在 reply 里说明原因
- **密码、授权码、恢复助记词、密钥绝不写进 items/events 的 value**。用户要查看已有机密时，secrets.action 用 "copy"；用户要保存新机密时用 "save"。两者都只列标题，不要输出机密内容
- 不确定的字段不要编造；无法填满必填字段时不要输出该条记录"""


def read_config() -> dict:
    cfg = {}
    if CONFIG.exists():
        with CONFIG.open("rb") as f:
            ai = tomllib.load(f).get("ai", {})
        for key in ("base_url", "model", "thinking", "thinking_budget"):
            if key in ai:
                cfg[f"ai_{key}"] = str(ai[key])

    if SECRETS.exists():
        lines = SECRETS.read_text().splitlines()
    else:
        lines = []
    for line in lines:
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        k, v = line.split("=", 1)
        cfg[k.strip()] = v.strip().strip('"').strip("'")

    if "AI_API_KEY" not in cfg and os.environ.get("AI_API_KEY"):
        cfg["AI_API_KEY"] = os.environ["AI_API_KEY"]
    # 兼容旧安装：迁移到 config/secrets/.secrets.env 后可删除旧 docker/.env 中的 ai_api_key。
    if "AI_API_KEY" not in cfg and LEGACY_ENV.exists():
        for line in LEGACY_ENV.read_text().splitlines():
            if line.strip().startswith("ai_api_key="):
                cfg["AI_API_KEY"] = line.split("=", 1)[1].strip().strip('"').strip("'")
                break
    return cfg


def config_bool(cfg: dict, key: str, default: bool = False) -> bool:
    raw = cfg.get(key)
    if raw is None:
        return default
    value = str(raw).strip().lower()
    if not value:
        return default
    return value in {"1", "true", "yes", "on", "y"}


def call_ai(
    base_url: str,
    api_key: str,
    model: str,
    identity: str,
    text: str,
    history: list[dict] | None = None,
    thinking: bool = False,
    thinking_budget: str = "",
) -> dict:
    request_body = {
        "model": model,
        "messages": [
            {"role": "system", "content": system_prompt(identity)},
            *(history or [])[-10:],
            {"role": "user", "content": text},
        ],
        "response_format": {"type": "json_object"},
        "temperature": 0.1,
        "enable_thinking": thinking,
    }
    if thinking and thinking_budget.strip():
        try:
            budget = int(thinking_budget)
        except ValueError as exc:
            raise ValueError("ai_thinking_budget 必须是正整数") from exc
        if budget <= 0:
            raise ValueError("ai_thinking_budget 必须是正整数")
        request_body["thinking_budget"] = budget
    body = json.dumps(request_body).encode()
    req = urllib.request.Request(
        base_url.rstrip("/") + "/chat/completions",
        data=body,
        headers={"Content-Type": "application/json", "Authorization": f"Bearer {api_key}"},
    )
    with urllib.request.urlopen(req, timeout=180) as resp:
        data = json.loads(resp.read())
    message = data["choices"][0]["message"]
    reasoning = message.get("reasoning_content") or ""
    if reasoning:
        print(dim("\n思考 › " + str(reasoning)), file=sys.stderr)
    content = message.get("content") or ""
    m = re.search(r"\{.*\}", content, re.S)
    if not m:
        raise ValueError(f"AI 没有返回 JSON: {content[:200]}")
    draft = json.loads(m.group(0))
    if not isinstance(draft, dict):
        raise ValueError("AI 返回的 JSON 不是对象")
    return draft


def read_input(identity: str) -> str | None:
    """交互终端按空行提交，避免普通资料也必须用 Ctrl+D。"""
    if not _TTY:
        return sys.stdin.read()

    print("\n" + head("╭─ AI 资料整理 ─────────────────────────────────╮"))
    print(head(f"│ 当前身份：{identity:<36}│"))
    print(head("│ 输入非机密资料；空行提交，Ctrl+C 取消。        │"))
    print(head("╰──────────────────────────────────────────────╯"))
    lines = []
    try:
        while True:
            prompt = c(36, "你 › ") if not lines else c(36, "  │ ")
            line = input(prompt)
            if not line.strip():
                if lines:
                    return "\n".join(lines)
                print(dim("  请输入资料后再提交。"))
                continue
            lines.append(line)
    except EOFError:
        return "\n".join(lines)
    except KeyboardInterrupt:
        print("\n" + dim("已取消。"), file=sys.stderr)
        return None


def normalize_identity(row: dict, default: str) -> None:
    if not str(row.get("identity") or "").strip():
        row["identity"] = default


def valid_rows(value: object, required: tuple[str, ...]) -> tuple[list[dict], int]:
    """只让字段完整的模型记录进入预览和写库流程。"""
    if not isinstance(value, list):
        return [], 0
    rows = [
        row
        for row in value
        if isinstance(row, dict) and all(str(row.get(field) or "").strip() for field in required)
    ]
    return rows, len(value) - len(rows)


COPY_WORDS = ("查看", "看看", "看", "显示", "复制", "取出", "拿到", "使用", "查一下", "view", "show", "copy", "retrieve")
SAVE_WORDS = ("保存", "存入", "存", "录入", "新增", "添加", "设置", "重置", "重设", "更新", "修改", "save", "add", "store", "create", "reset", "update")
SECRET_WORDS = ("密码", "授权码", "助记词", "密钥", "password", "passwd", "passcode", "token", "secret")


def infer_secret_action(secret: dict, text: str) -> str:
    """模型未给 action 时，按用户动词避免把查看误当成新增。"""
    action = str(secret.get("action") or "").strip().lower()
    if action in {"copy", "view", "show", "retrieve", "use"}:
        return "copy"
    if action in {"save", "add", "store", "create", "set"}:
        return "save"
    lower = text.lower()
    wants_copy = any(word in lower for word in COPY_WORDS)
    wants_save = any(word in lower for word in SAVE_WORDS)
    return "copy" if wants_copy and not wants_save else "save"


def infer_lookup_secret(text: str, identity: str) -> dict | None:
    """查看请求没有被模型列出时，生成不含密码内容的本地复制提示。"""
    lower = text.lower()
    if not any(word in lower for word in COPY_WORDS):
        return None
    if any(word in lower for word in SAVE_WORDS) or not any(word in lower for word in SECRET_WORDS):
        return None
    return {"identity": identity, "title": infer_secret_title(text), "kind": "密码", "action": "copy"}


def infer_save_secret(text: str, identity: str) -> dict | None:
    """保存/重置请求没有给出密码内容时，生成预填标题的录入提示。"""
    lower = text.lower()
    if not any(word in lower for word in SAVE_WORDS) or not any(word in lower for word in SECRET_WORDS):
        return None
    return {"identity": identity, "title": infer_secret_title(text), "kind": "密码", "action": "save"}


def launch_secret_entries(secrets: list[dict]) -> list[dict]:
    added = []
    for secret in secrets:
        identity = str(secret.get("identity") or "")
        title = str(secret.get("title") or "")
        print(dim(f"\n正在打开：make pass-add-manual id={shlex.quote(identity)} title={shlex.quote(title)}"))
        try:
            result = subprocess.run(
                ["make", "pass-add-manual", f"id={identity}", f"title={title}"],
                cwd=BASE_DIR,
                check=False,
            )
        except OSError as exc:
            print(warn(f"无法启动密码录入: {exc}"), file=sys.stderr)
            continue
        if result.returncode:
            print(warn(f"密码录入已结束（退出码 {result.returncode}）。"), file=sys.stderr)
        else:
            added.append(secret)
    return added


def copy_secret_entries(secrets: list[dict], show: bool = False) -> list[dict]:
    failed = []
    target = "pass-show" if show else "pass"
    for secret in secrets:
        identity = str(secret.get("identity") or "")
        title = str(secret.get("title") or "")
        print(dim(f"\n正在打开：make {target} id={shlex.quote(identity)} title={shlex.quote(title)}"))
        result = subprocess.run(
            ["make", target, f"id={identity}", f"title={title}"],
            cwd=BASE_DIR,
            check=False,
        )
        if result.returncode:
            print(warn(f"密码操作未完成（退出码 {result.returncode}）。"), file=sys.stderr)
            print(
                warn(
                    f"请检查解锁结果和终端提示；条目标题不匹配时，可运行 "
                    f"make pass-list id={shlex.quote(identity)} 查看实际标题。"
                ),
                file=sys.stderr,
            )
            failed.append(secret)
    return failed


def write_draft(items: list[dict], events: list[dict]) -> None:
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


def main() -> int:
    parser = argparse.ArgumentParser(description="AI 整理资料写入 SQLite 草稿")
    parser.add_argument("--identity", "-i", help="归属身份（建议必填；缺则在终端询问）")
    parser.add_argument("file", nargs="?", help="资料文件；省略则从 stdin 读")
    args = parser.parse_args()

    identity = (args.identity or "").strip().lower()
    if not identity:
        if _TTY:
            try:
                identity = prompt_identity()
            except KeyboardInterrupt:
                print("\n" + dim("已取消。"), file=sys.stderr)
                return 0
        else:
            print(err("错误: 请指定身份，例如 make ai id=jacob"), file=sys.stderr)
            return 1

    clipboard_status = check_clipboard() if _TTY and not args.file else "unknown"
    cfg = read_config()
    api_key = cfg.get("AI_API_KEY", "")
    base_url = cfg.get("ai_base_url", "https://dashscope.aliyuncs.com/compatible-mode/v1")
    model = cfg.get("ai_model", "qwen-plus")
    thinking = config_bool(cfg, "ai_thinking")
    thinking_budget = cfg.get("ai_thinking_budget", "")
    if not api_key:
        print(err("错误: config/secrets/.secrets.env 里还没有 AI_API_KEY。请复制 config/secrets/.secrets.env.example 后填写"), file=sys.stderr)
        return 1

    first_text = Path(args.file).read_text() if args.file else None
    history = []
    while True:
        if first_text is not None:
            text = first_text
            first_text = None
        else:
            text = read_input(identity)
            if text is None:
                return 0
        text = text.strip()
        if not text:
            print(err("错误: 没有输入内容"), file=sys.stderr)
            if args.file:
                return 1
            continue

        if len(text) < 8 and _TTY:
            print(warn("提示: 内容很短，模型多半整理不出条目；需要更多上下文（账号类型、用途等）。"), file=sys.stderr)

        print(dim(f"\nAI › 正在调用 {model} 整理（身份 {identity}）…"), file=sys.stderr)
        try:
            draft = call_ai(
                base_url,
                api_key,
                model,
                identity,
                text,
                history,
                thinking=thinking,
                thinking_budget=thinking_budget,
            )
        except KeyboardInterrupt:
            print("\n" + dim("AI 调用已取消。"), file=sys.stderr)
            return 0
        except Exception as e:
            print(err(f"错误: AI 调用失败: {e}"), file=sys.stderr)
            return 1
        history.extend(
            [
                {"role": "user", "content": text},
                {"role": "assistant", "content": json.dumps(draft, ensure_ascii=False)},
            ]
        )
        history = history[-10:]

        reply_value = draft.get("reply")
        reply = reply_value.strip() if isinstance(reply_value, str) else ""
        items, dropped_items = valid_rows(draft.get("items"), ("category", "key", "value"))
        events, dropped_events = valid_rows(draft.get("events"), ("title",))
        secrets, _ = valid_rows(draft.get("secrets"), ("title",))

        for it in items:
            normalize_identity(it, identity)
        for ev in events:
            normalize_identity(ev, identity)
        for s in secrets:
            normalize_identity(s, identity)
            s["action"] = infer_secret_action(s, text)

        if not secrets:
            lookup = infer_lookup_secret(text, identity)
            save = infer_save_secret(text, identity)
            if lookup:
                secrets = [lookup]
                reply = "AI 不读取密码库内容；已按查看请求生成本地复制命令。若条目存在，执行后会复制到剪贴板；若不存在，再选择保存新密码。"
            elif save:
                secrets = [save]
                reply = "AI 不读取或保存密码明文；已按保存请求生成预填标题的本地录入命令，请在终端输入新密码。"

        if reply:
            print("\n" + dim("AI ›"))
            print(dim(reply))

        print("\n" + dim("===== 模型原始输出（JSON）====="))
        print(dim(json.dumps(draft, ensure_ascii=False, indent=2)))

        dropped = dropped_items + dropped_events
        if dropped:
            print(warn(f"\n已忽略 {dropped} 条不完整草稿，未写入 SQLite。"))

        print("\n" + head("===== 草稿：条目 ====="))
        if items:
            for i, it in enumerate(items, 1):
                idn = str(it.get("identity") or "")
                flag = warn(" ⚠ 缺身份") if not idn.strip() else ""
                print(
                    f"{i}. [{idn}] {it.get('category', '')}/{it.get('key', '')} = {it.get('value', '')}  "
                    f"{dim('(' + str(it.get('note') or '') + ')')}{flag}"
                )
        else:
            print(dim("（无）"))

        print("\n" + head("===== 草稿：事件 ====="))
        if events:
            for i, ev in enumerate(events, 1):
                print(f"{i}. [{ev.get('identity', '')}] {ev.get('title', '')} — {dim(str(ev.get('detail') or ''))}")
        else:
            print(dim("（无）"))

        allow_plaintext = plaintext_allowed()
        if secrets:
            has_copy = any(s.get("action") == "copy" for s in secrets)
            heading = "===== 密码操作 =====" if has_copy else "===== 机密提醒（不写入，需手动存 kdbx）====="
            print("\n" + warn(heading))
            print(warn("密码由本地 KeePassXC 处理，不发送给模型；选择明文显示会留在终端记录。"))
            for i, s in enumerate(secrets, 1):
                sid = str(s.get("identity") or identity)
                title = str(s.get("title") or "")
                kind = str(s.get("kind") or "")
                action = str(s.get("action") or "save")
                if action == "copy":
                    if clipboard_status == "unavailable":
                        command = (
                            f"make pass-show id={shlex.quote(sid)} title={shlex.quote(title)}"
                            if allow_plaintext else "make clipboard-check"
                        )
                        suffix = "（明文显示）" if allow_plaintext else "（明文已禁用，请重新检测剪贴板）"
                    else:
                        command = f"make pass id={shlex.quote(sid)} title={shlex.quote(title)}"
                        suffix = "（复制已有条目到剪贴板）"
                else:
                    command = f"make pass-add-manual id={shlex.quote(sid)} title={shlex.quote(title)}"
                    suffix = "（新增条目并手动录入）"
                print(
                    warn(
                        f"{i}. [{sid}] {title}（{kind}）{suffix}"
                        f"\n   → {command}"
                    )
                )

        if not items and not events and not secrets:
            print("\n" + dim("没有可写入 SQLite 的条目/事件。"))
            if reply:
                print(dim("原因见上方「AI ›」回复。"))
            else:
                print(dim("资料可能过短或模型未识别出结构化信息；可继续向 AI 提问。"))

        bad = [it for it in items if not str(it.get("identity") or "").strip()]
        bad += [ev for ev in events if not str(ev.get("identity") or "").strip()]
        if bad:
            print(err("\n错误: 仍有条目/事件缺少 identity，已取消写入。请重试或检查模型输出。"), file=sys.stderr)
            return 1

        options = []
        save_secrets = [s for s in secrets if s.get("action") == "save"]
        copy_secrets = [s for s in secrets if s.get("action") == "copy"]
        if save_secrets:
            options.append(("secrets-save", "新增并录入密码"))
        if copy_secrets:
            if clipboard_status != "unavailable":
                options.append(("secrets-copy", "复制已有密码到剪贴板"))
            if allow_plaintext:
                options.append(("secrets-show", "明文显示密码（会留在终端记录）"))
        if items or events:
            options.append(("write", f"确认写入 SQLite（身份 {identity}）"))
        options.extend([("continue", "继续向 AI 提问"), ("exit", "退出" )])
        action = choose_action(options)
        if action == "secrets-save":
            added = launch_secret_entries(save_secrets)
            if added:
                options = []
                if clipboard_status != "unavailable":
                    options.append(("copy", "复制刚保存的密码到剪贴板"))
                if plaintext_allowed():
                    options.append(("show", "明文显示密码（会留在终端记录）"))
                options.append(("continue", "继续向 AI 提问"))
                copy_action = choose_action(options)
                if copy_action == "copy":
                    copy_secret_entries(added)
                elif copy_action == "show":
                    copy_secret_entries(added, show=True)
            continue
        if action == "secrets-copy":
            copy_secret_entries(copy_secrets)
            continue
        if action == "secrets-show":
            copy_secret_entries(copy_secrets, show=True)
            continue
        if action == "write":
            write_draft(items, events)
            continue
        if action == "continue":
            continue
        return 0


if __name__ == "__main__":
    sys.exit(main())
