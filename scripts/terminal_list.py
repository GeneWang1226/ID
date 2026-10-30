"""带类型筛选和分页的终端列表，不接触资料存储。"""

from contextlib import contextmanager
import os
import select
import shutil
import sys
import unicodedata


def width(text: str) -> int:
    return sum(0 if unicodedata.combining(c) else 2 if unicodedata.east_asian_width(c) in "WF" else 1 for c in text)


def clip(text: str, limit: int) -> str:
    text = "".join(c if c.isprintable() else " " for c in str(text))
    if width(text) <= limit:
        return text
    result = ""
    for char in text:
        if width(result + char) > max(0, limit - 1):
            break
        result += char
    return result + "…" if limit > 0 else ""


def color(text: str, style: str) -> str:
    if sys.stdout.isatty() and "NO_COLOR" not in os.environ and os.environ.get("TERM") != "dumb":
        return f"\033[{style}m{text}\033[0m"
    return text


def read_key(fd: int) -> str:
    key = os.read(fd, 1)
    if key == b"\x1b":
        while len(key) < 8 and select.select([fd], [], [], 0.04)[0]:
            chunk = os.read(fd, 1)
            if not chunk:
                break
            key += chunk
            if key[-1:] in (b"A", b"B", b"C", b"D", b"~"):
                break
    return {b"\x1b[A": "up", b"\x1b[B": "down", b"\x1b[C": "next", b"\x1b[D": "prev",
            b"\x1b[5~": "prev", b"\x1b[6~": "next", b"\r": "enter", b"\n": "enter",
            b"\x03": "q", b"\x1b": "q"}.get(key, key.decode("ascii", errors="ignore"))


@contextmanager
def screen():
    import termios
    import tty
    fd = sys.stdin.fileno()
    previous = termios.tcgetattr(fd)
    try:
        tty.setcbreak(fd)
        sys.stdout.write("\033[?1049h\033[?25l")
        sys.stdout.flush()
        yield fd
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, previous)
        sys.stdout.write("\033[?25h\033[?1049l")
        sys.stdout.flush()


class RecordBrowser:
    def __init__(self, identity, load, view="all", *, picker=False):
        self.identity, self.load, self.view = identity, load, view
        self.picker = picker
        self.page = self.selected = 0

    def render(self, records, columns, height):
        if columns < 30 or height < 10:
            return [clip("请扩大终端窗口，q 退出", columns)], [], 1
        page_size = max(1, height - 15)
        pages = max(1, (len(records) + page_size - 1) // page_size)
        self.page = min(self.page, pages - 1)
        start = self.page * page_size
        current = records[start:start + page_size]
        self.selected = min(self.selected, max(0, len(current) - 1))
        lines = [color(clip(f"ID  /  {self.identity}", columns), "1;36"), ""]
        tabs = [("all", "全部"), ("password", "密码"), ("info", "个人信息")]
        lines.append(color("选择密码条目", "1;36") if self.picker else
                     "   ".join(color(label, "1;36" if key == self.view else "2") for key, label in tabs))
        lines.append("")
        for kind, label in (("password", "密码"), ("info", "个人信息")):
            group = [(i, row) for i, row in enumerate(current) if row["kind"] == kind]
            if not group:
                continue
            lines.append(color(label, "1"))
            available = max(20, columns - 2)
            if kind == "password":
                widths = [max(6, available - 26), 10, 12] if columns >= 50 and not self.picker else [available - 12, 10]
                headers = ["条目", "密码", "关联资料"][:len(widths)]
            elif columns >= 90:
                widths = [10, 22, max(10, available - 58), 20]
                headers = ["分类", "名称", "内容", "关联密码"]
            elif columns >= 60:
                widths = [max(7, available // 3), max(8, available - 26 - available // 3), 22]
                headers = ["名称", "内容", "关联密码"]
            else:
                widths = [available // 2 - 1, available - available // 2 - 1]
                headers = ["名称", "内容"]

            def row_line(values, selected=False, status=""):
                cells = [clip(value, size) for value, size in zip(values, widths)]
                plain = "  ".join(value + " " * (size - width(value)) for value, size in zip(cells, widths)).rstrip()
                prefix = "› " if selected else "  "
                if selected:
                    return color(prefix + plain, "1;36")
                if status:
                    return prefix + plain.replace(status, color(status, "32" if status == "已设置" else "33"), 1)
                return prefix + plain

            lines.append(color(row_line(headers), "2"))
            for index, row in group:
                if kind == "password":
                    cells = [row["title"], row["status"], str(row.get("linked", 0))]
                else:
                    cells = [row["category"], row["key"], row["value"] or "—", row.get("link") or "—"]
                    if columns < 90:
                        cells = cells[1:]
                    if columns < 60:
                        cells = cells[:2]
                lines.append(row_line(cells, index == self.selected, row.get("status", "")))
            lines.append("")
        if not records:
            lines.append(color("  暂无记录", "2"))
        lines += [color(f"第 {self.page + 1}/{pages} 页  ·  {len(records)} 条", "2"),
                  color(clip("↑↓ 选择  ←→/PgUp/PgDn 翻页  Enter " + ("确认" if self.picker else "详情/操作"), columns), "2")]
        if not self.picker:
            lines.append(color(clip("1 全部  2 密码  3 个人信息  r 刷新  q 退出", columns), "2"))
        else:
            lines.append(color("q 返回", "2"))
        return lines, current, pages

    def choose(self):
        records = self.load(self.view)
        with screen() as fd:
            while True:
                size = shutil.get_terminal_size((100, 24))
                lines, current, pages = self.render(records, size.columns, size.lines)
                sys.stdout.write("\033[H\033[2J" + "\r\n".join(lines))
                sys.stdout.flush()
                key = read_key(fd)
                if key in ("q", "Q", ""):
                    return None
                if key == "enter" and current:
                    return current[self.selected]
                if key in ("next", "n"):
                    self.page, self.selected = min(self.page + 1, pages - 1), 0
                elif key in ("prev", "p"):
                    self.page, self.selected = max(self.page - 1, 0), 0
                elif key in ("down", "j"):
                    if self.selected + 1 < len(current):
                        self.selected += 1
                    elif self.page + 1 < pages:
                        self.page, self.selected = self.page + 1, 0
                elif key in ("up", "k"):
                    if self.selected:
                        self.selected -= 1
                    elif self.page:
                        self.page, self.selected = self.page - 1, max(0, size.lines - 16)
                elif not self.picker and key in ("1", "2", "3"):
                    self.view = {"1": "all", "2": "password", "3": "info"}[key]
                    self.page = self.selected = 0
                    return {"kind": "filter"}
                elif key == "r" and not self.picker:
                    return {"kind": "refresh"}
