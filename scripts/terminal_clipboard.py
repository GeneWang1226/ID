"""终端剪贴板自测和全局明文权限；不查询本地剪贴板内容。"""

import base64
import getpass
import hashlib
import os
from pathlib import Path
import secrets
import subprocess
import sys
import tomllib

BASE_DIR = Path(__file__).resolve().parent.parent
CONFIG = BASE_DIR / "config" / "config.toml"
CACHE_DIR = BASE_DIR / "config" / ".cache" / "clipboard"


def plaintext_allowed() -> bool:
    try:
        with CONFIG.open("rb") as f:
            return tomllib.load(f).get("terminal", {}).get("allow_plaintext") is True
    except (OSError, ValueError, AttributeError):
        return False


def write_clipboard(payload: str) -> None:
    sequence = f"\033]52;c;{payload}\a"
    if os.environ.get("TMUX"):
        sequence = f"\033Ptmux;\033{sequence}\033\\"
    sys.stdout.write(sequence)
    sys.stdout.flush()


def session_file() -> Path | None:
    try:
        tty = os.ttyname(sys.stdin.fileno())
        connection = os.environ.get("SSH_CONNECTION", "")
        if os.environ.get("TMUX"):
            client = subprocess.run(
                ["tmux", "display-message", "-p", "#{client_tty}:#{client_pid}"],
                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, check=True, timeout=2,
            )
            connection += client.stdout.decode()
        fingerprint = f"{connection}|{tty}|{os.stat(tty).st_ctime_ns}|{os.environ.get('TERM', '')}"
        return CACHE_DIR / hashlib.sha256(fingerprint.encode()).hexdigest()
    except (OSError, ValueError, subprocess.SubprocessError):
        return None


def check_clipboard(force: bool = False) -> str:
    if not (os.environ.get("SSH_CONNECTION") or os.environ.get("SSH_TTY")):
        return "native"
    if not sys.stdin.isatty() or not sys.stdout.isatty():
        return "unknown"

    cache = session_file()
    status = ""
    if cache and not force:
        try:
            status = cache.read_text()
        except (OSError, UnicodeError):
            pass
    if status not in {"supported", "unavailable", "unknown"}:
        print("\n终端剪贴板自测：测试会覆盖当前剪贴板，不涉及密码。", file=sys.stderr)
        marker = "ID-CLIP-" + secrets.token_hex(4)
        try:
            write_clipboard(base64.b64encode(marker.encode()).decode("ascii"))
            answer = getpass.getpass("请在此粘贴测试文字并回车（不回显）；无法粘贴直接回车，q 跳过：")
            status = "supported" if answer == marker else "unknown" if answer.lower() == "q" else "unavailable"
        except (EOFError, KeyboardInterrupt):
            status = "unknown"
        finally:
            write_clipboard("")
        if cache:
            try:
                cache.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
                cache.write_text(status)
            except OSError:
                print("终端自测结果无法缓存，下次可能再次询问。", file=sys.stderr)

    if status == "supported":
        print("终端剪贴板：本次连接已通过粘贴自测。", file=sys.stderr)
    else:
        print("⚠ 全局提示：当前连接的本地剪贴板未验证可用，不保证复制成功。", file=sys.stderr)
        print("可选择明文查看（须在 make settings 允许）；make clipboard-check 可重新检测。", file=sys.stderr)
    return status
