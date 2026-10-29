#!/usr/bin/env python3
"""用方向键修改非敏感设置，API key 通过隐藏输入单独保存。"""

import getpass
import os
import sys
import tempfile
import termios
import tomllib
import tty
from pathlib import Path

BASE_DIR = Path(__file__).resolve().parent.parent
CONFIG = BASE_DIR / "config" / "config.toml"
SECRETS = BASE_DIR / "config" / "secrets" / ".secrets.env"

DEFAULTS = {
    "base_url": "https://dashscope.aliyuncs.com/compatible-mode/v1",
    "model": "qwen-plus",
    "thinking": False,
    "thinking_budget": 8192,
}


def say(text: str) -> None:
    """raw 模式下显式回车，避免远端终端只换行不回列。"""
    sys.stdout.write(f"\r\n{text}\r\n")
    sys.stdout.flush()


def load() -> dict:
    if not CONFIG.exists():
        return DEFAULTS.copy()
    with CONFIG.open("rb") as f:
        data = tomllib.load(f)
    values = data.get("ai", {})
    return {key: values.get(key, value) for key, value in DEFAULTS.items()}


def update_secret(key: str, value: str) -> None:
    """只更新指定键，保留 secrets 文件中的其他内容；不回显 value。"""
    lines = SECRETS.read_text().splitlines() if SECRETS.exists() else []
    replacement = f"{key}={value}"
    found = False
    output = []
    for line in lines:
        if line.strip().startswith(f"{key}="):
            output.append(replacement)
            found = True
        else:
            output.append(line)
    if not found:
        if output and output[-1].strip():
            output.append("")
        output.append(replacement)
    fd, tmp_name = tempfile.mkstemp(prefix=".secrets.env.", dir=SECRETS.parent)
    try:
        with os.fdopen(fd, "w") as f:
            f.write("\n".join(output).rstrip() + "\n")
            f.flush()
            os.fsync(f.fileno())
        os.chmod(tmp_name, 0o600)
        os.replace(tmp_name, SECRETS)
    finally:
        if os.path.exists(tmp_name):
            os.unlink(tmp_name)


def save_config(values: dict) -> None:
    content = (
        "# 可同步的非敏感配置。make ai / Web 每次调用都会重新读取。\n\n"
        "[ai]\n"
        f"base_url = {values['base_url']!r}\n"
        f"model = {values['model']!r}\n"
        f"thinking = {str(bool(values['thinking'])).lower()}\n"
        f"thinking_budget = {int(values['thinking_budget'])}\n"
    )
    fd, tmp_name = tempfile.mkstemp(prefix=".config.toml.", dir=CONFIG.parent)
    try:
        with os.fdopen(fd, "w") as f:
            f.write(content)
            f.flush()
            os.fsync(f.fileno())
        os.chmod(tmp_name, 0o644)
        os.replace(tmp_name, CONFIG)
    finally:
        if os.path.exists(tmp_name):
            os.unlink(tmp_name)


def edit_value(fd: int, old_stty: list, label: str, current: str) -> str:
    termios.tcsetattr(fd, termios.TCSADRAIN, old_stty)
    try:
        try:
            value = input(f"{label} [{current}]: ").strip()
        except EOFError:
            value = current
    finally:
        tty.setraw(fd)
    return value or current


def edit_key(fd: int, old_stty: list) -> str | None:
    termios.tcsetattr(fd, termios.TCSADRAIN, old_stty)
    try:
        try:
            value = getpass.getpass("新的 AI API key（直接回车保留原值）: ")
        except EOFError:
            value = ""
    finally:
        tty.setraw(fd)
    if value:
        say("✓ AI API key 已修改，保存时写入（内容未显示）。")
        return value
    else:
        say("AI API key 未修改。")
        return None


def render(values: dict, selected: int, key_configured: bool) -> None:
    thinking = bool(values["thinking"])
    rows = [
        f"AI API key       {'已设置' if key_configured else '未设置'}",
        f"AI 接口地址      {values['base_url']}",
        f"AI 模型          {values['model']}",
        f"思考模式         {'● 开   ○ 关' if thinking else '○ 开   ● 关'}",
        f"思考预算         {values['thinking_budget']} token",
        "保存并退出",
        "取消",
    ]
    lines = [
        "╭─ ID 设置 ─────────────────────────────────────╮",
        "│ ↑/↓ 移动，Enter 编辑/切换，q 取消              │",
        "╰───────────────────────────────────────────────╯",
        "",
    ]
    for i, label in enumerate(rows):
        marker = "❯" if i == selected else " "
        lines.append(f"{marker} {label}")
    sys.stdout.write("\033[2J\033[H" + "\r\n".join(lines) + "\r\n")
    sys.stdout.flush()


def main() -> int:
    if not sys.stdin.isatty() or not sys.stdout.isatty():
        print("错误: make settings 需要在交互式终端中运行。", file=sys.stderr)
        return 1

    values = load()
    fd = sys.stdin.fileno()
    old_stty = termios.tcgetattr(fd)
    selected = 0
    pending_key = None
    key_configured = False
    if SECRETS.exists():
        key_configured = any(
            line.strip().startswith("AI_API_KEY=") and line.split("=", 1)[1].strip()
            for line in SECRETS.read_text().splitlines()
        )

    try:
        tty.setraw(fd)
        render(values, selected, key_configured)
        while True:
            key = os.read(fd, 1)
            if key in (b"q", b"Q", b"\x03"):
                say("已取消，未保存 config/config.toml。")
                return 0
            if key in (b"\r", b"\n"):
                if selected == 0:
                    new_key = edit_key(fd, old_stty)
                    if new_key is not None:
                        pending_key = new_key
                        key_configured = True
                elif selected == 1:
                    values["base_url"] = edit_value(fd, old_stty, "AI 接口地址", str(values["base_url"]))
                elif selected == 2:
                    values["model"] = edit_value(fd, old_stty, "AI 模型", str(values["model"]))
                elif selected == 3:
                    values["thinking"] = not bool(values["thinking"])
                elif selected == 4:
                    budget = edit_value(fd, old_stty, "思考预算（正整数）", str(values["thinking_budget"]))
                    try:
                        budget = int(budget)
                        if budget <= 0:
                            raise ValueError
                        values["thinking_budget"] = budget
                    except ValueError:
                        say("思考预算必须是正整数。")
                elif selected == 5:
                    if pending_key is not None:
                        update_secret("AI_API_KEY", pending_key)
                    save_config(values)
                    say(f"✓ 已保存：{CONFIG}")
                    return 0
                else:
                    say("已取消，未保存 config/config.toml。")
                    return 0
                render(values, selected, key_configured)
                continue
            if key == b"\x1b":
                sequence = os.read(fd, 2)
                if sequence == b"[A":
                    selected = (selected - 1) % 7
                elif sequence == b"[B":
                    selected = (selected + 1) % 7
                render(values, selected, key_configured)
            elif key in (b"k", b"j"):
                selected = (selected - 1 if key == b"k" else selected + 1) % 7
                render(values, selected, key_configured)
    except (KeyboardInterrupt, OSError):
        say("已取消，未保存 config/config.toml。")
        return 0
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, old_stty)


if __name__ == "__main__":
    raise SystemExit(main())
