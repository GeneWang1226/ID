"""终端方向键菜单，供 AI 交互和本地密码查询共用。"""

import os
import sys


def choose_action(
    options: list[tuple[str, str]],
    prompt: str = "下一步（↑/↓选择，Enter 确认，q 退出）：",
) -> str:
    """用方向键选择下一步；选项文字只显示，不进入 shell。"""
    if not options:
        return "exit"
    if not sys.stdin.isatty() or not sys.stdout.isatty():
        return "exit"
    try:
        import termios
        import tty

        fd = sys.stdin.fileno()
        old_stty = termios.tcgetattr(fd)
    except (ImportError, AttributeError, OSError):
        print("\n" + prompt)
        for i, (_, label) in enumerate(options, 1):
            print(f"{i}. {label}")
        try:
            answer = input("选择 [1]: ").strip()
        except (EOFError, KeyboardInterrupt):
            return "exit"
        try:
            index = int(answer or "1") - 1
            return options[index][0] if 0 <= index < len(options) else "exit"
        except (ValueError, IndexError):
            return "exit"

    selected = 0

    def render(final: bool = False) -> None:
        if final:
            sys.stdout.write(f"\033[{len(options)}A")
        for i, (_, label) in enumerate(options):
            marker = "✓" if final and i == selected else "❯" if not final and i == selected else " "
            sys.stdout.write(f"\r\033[K{marker} {label}\n")
        sys.stdout.flush()

    print("\n" + prompt)
    try:
        tty.setcbreak(fd)
        render()
        while True:
            key = os.read(fd, 1)
            if key in (b"\r", b"\n"):
                break
            if key in (b"q", b"Q", b"\x03"):
                return "exit"
            if key == b"\x1b":
                sequence = os.read(fd, 2)
                if sequence == b"[A":
                    selected = (selected - 1) % len(options)
                elif sequence == b"[B":
                    selected = (selected + 1) % len(options)
                else:
                    continue
                sys.stdout.write(f"\033[{len(options)}A")
                render()
            elif key in (b"k", b"j"):
                selected = (selected - 1 if key == b"k" else selected + 1) % len(options)
                sys.stdout.write(f"\033[{len(options)}A")
                render()
    except (KeyboardInterrupt, OSError):
        return "exit"
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, old_stty)
    render(final=True)
    return options[selected][0]
