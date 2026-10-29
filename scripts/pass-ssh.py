#!/usr/bin/env python3
"""在 SSH 终端用 OSC 52 写入本地剪贴板，不把密码打印到终端。"""

import base64
import os
import subprocess
import sys
import time


def main() -> int:
    if len(sys.argv) != 3:
        print("用法: pass-ssh.py <数据库> <条目>", file=sys.stderr)
        return 2

    try:
        result = subprocess.run(
            ["keepassxc-cli", "show", "-a", "Password", sys.argv[1], sys.argv[2]],
            stdin=sys.stdin,
            stdout=subprocess.PIPE,
            check=False,
        )
    except OSError as exc:
        print(f"无法读取密码库: {exc}", file=sys.stderr)
        return 1
    if result.returncode:
        return result.returncode

    password = result.stdout
    if password.endswith(b"\n"):
        password = password[:-1]
    if password.endswith(b"\r"):
        password = password[:-1]
    payload = base64.b64encode(password).decode("ascii")
    write_clipboard(payload)
    sys.stdout.flush()
    print("已发送到本地终端剪贴板（OSC 52），10 秒后清空。", file=sys.stderr)
    time.sleep(10)
    write_clipboard("")
    sys.stdout.flush()
    return 0


def write_clipboard(payload: str) -> None:
    sequence = f"\033]52;c;{payload}\a"
    if os.environ.get("TMUX"):
        # tmux 要求把 OSC 52 再包一层 DCS，并转义内层 ESC。
        sequence = f"\033Ptmux;\033{sequence}\033\\"
    sys.stdout.write(sequence)


if __name__ == "__main__":
    raise SystemExit(main())
