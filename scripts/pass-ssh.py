#!/usr/bin/env python3
"""本地或 SSH 剪贴板复制；用户明确选择时在终端明文显示密码。"""

import argparse
import base64
import select
import subprocess
import sys

from password_entries import matching_entries
from terminal_clipboard import check_clipboard, plaintext_allowed, write_clipboard
from terminal_menu import choose_action
import vault_session


def select_entry(database: str, query: str) -> str | None:
    action = choose_action(
        [("search", "搜索已有条目标题"), ("exit", "取消")],
        "读取未成功，可搜索标题（↑/↓选择，Enter 确认，q 取消）：",
    )
    if action != "search":
        return None
    print("搜索本地条目标题。", file=sys.stderr)
    result = vault_session.run_cli(
        ["keepassxc-cli", "search", database, "title:*"],
        database,
        stdin=sys.stdin, stdout=subprocess.PIPE, text=True, check=False,
    )
    if result.returncode:
        return None
    paths = result.stdout.splitlines()
    if not paths:
        print("密码库中没有可选择的条目。", file=sys.stderr)
        return None
    matches = matching_entries(paths, query)
    if not matches:
        print("没有匹配的标题，请从已有条目中选择。", file=sys.stderr)
    candidates = matches or paths
    options = [
        (str(i), "".join(char if char.isprintable() else "?" for char in path))
        for i, path in enumerate(candidates)
    ]
    selected = choose_action(
        [*options, ("exit", "取消")],
        "选择实际条目（↑/↓选择，Enter 确认，q 取消）：",
    )
    return None if selected == "exit" else candidates[int(selected)]


def read_entry(database: str, entry: str, native: bool) -> subprocess.CompletedProcess:
    command = ["keepassxc-cli", "clip"] if native else ["keepassxc-cli", "show", "-s", "-a", "Password"]
    return vault_session.run_cli(
        [*command, database, entry],
        database,
        stdin=sys.stdin, stdout=None if native else subprocess.PIPE, check=False,
    )


def show_password(password: bytes) -> int:
    if not plaintext_allowed():
        print("明文显示已在全局设置中禁用；可运行 make settings 修改。", file=sys.stderr)
        return 1
    sys.stdout.write(password.decode("utf-8") + "\n")
    sys.stdout.flush()
    return 0


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--show", action="store_true", help="明文显示密码")
    mode.add_argument("--native", action="store_true", help="使用 KeePassXC 本机剪贴板")
    parser.add_argument("--check", action="store_true", help="重新测试当前终端剪贴板")
    parser.add_argument("database", nargs="?")
    parser.add_argument("entry", nargs="?")
    args = parser.parse_args(argv)

    if not sys.stdin.isatty() or not sys.stdout.isatty():
        print("密码操作仅限交互式终端，拒绝写入文件或管道。", file=sys.stderr)
        return 1

    if args.check:
        check_clipboard(force=True)
        return 0
    if not args.database or not args.entry:
        parser.error("需要指定数据库和条目")

    if not args.show and not args.native and check_clipboard() == "unavailable":
        if not plaintext_allowed():
            print("剪贴板不可用，且全局明文显示已禁用；未解锁密码库。", file=sys.stderr)
            return 1
        try:
            answer = input("输入 v 并回车明文显示（会留在终端记录）；直接回车取消：")
        except (EOFError, KeyboardInterrupt):
            return 130
        if answer.strip().lower() != "v":
            return 0
        args.show = True
    if args.show:
        if not plaintext_allowed():
            print("明文显示已在全局设置中禁用；可运行 make settings 修改。", file=sys.stderr)
            return 1
        print("密码将明文显示在当前终端，可能保留在滚屏记录中。", file=sys.stderr)

    try:
        if args.show and vault_session.current() is not None:
            vault_session.current().unlock(args.database, fresh=True)
        result = read_entry(args.database, args.entry, args.native)
        if result.returncode:
            entry = select_entry(args.database, args.entry)
            if entry is not None:
                result = read_entry(args.database, entry, args.native)
    except (KeyboardInterrupt, EOFError):
        return 130
    except (OSError, vault_session.VaultError) as exc:
        print(f"无法读取密码库: {exc}", file=sys.stderr)
        return 1
    if result.returncode:
        return result.returncode
    if args.native:
        return 0

    password = result.stdout.removesuffix(b"\n")
    if args.show:
        return show_password(password)

    payload = base64.b64encode(password).decode("ascii")
    show = False
    try:
        write_clipboard(payload)
        print("已发送复制请求，请在 10 秒内粘贴；无法确认终端是否接收。", file=sys.stderr)
        if plaintext_allowed():
            print("未复制成功？输入 v 再按 Enter 明文显示（会留在终端记录）；直接 Enter 结束。", file=sys.stderr)
        else:
            print("全局设置已禁用明文显示；粘贴后按 Enter 结束。", file=sys.stderr)
        readable, _, _ = select.select([sys.stdin], [], [], 10)
        show = bool(readable) and sys.stdin.readline().strip().lower() == "v"
    except KeyboardInterrupt:
        print("\n已取消。", file=sys.stderr)
        return 130
    finally:
        write_clipboard("")
    if show:
        if vault_session.current() is not None:
            try:
                vault_session.current().unlock(args.database, fresh=True)
            except (KeyboardInterrupt, EOFError):
                return 130
            except (OSError, vault_session.VaultError) as exc:
                print(str(exc), file=sys.stderr)
                return 1
        return show_password(password)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
