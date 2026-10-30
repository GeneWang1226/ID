"""带类型筛选和分页的终端列表，不接触资料存储。"""

from contextlib import contextmanager
from itertools import groupby
import os
import select
import shutil
import sys
import unicodedata


VIEWS = (("all", "全部"), ("account", "账号"), ("password", "密码"), ("info", "个人信息"))
DIMENSIONS = {"address": "地址", "contact": "联系方式", "preference": "偏好", "id-info": "身份信息",
              "work": "工作", "education": "教育", "health": "健康", "finance": "财务", "asset": "资产"}


def category_label(category):
    return DIMENSIONS.get(category, category)


def section(row):
    return row["kind"], row.get("category", "") if row["kind"] == "info" else ""


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


def wrap(text, limit):
    text = "".join(c if c == "\n" or c.isprintable() else " " for c in str(text))
    lines = []
    for paragraph in text.split("\n"):
        line, used = "", 0
        for char in paragraph:
            size = width(char)
            if line and used + size > limit:
                lines.append(line)
                line, used = "", 0
            line += char
            used += size
        lines.append(line)
    return lines


def record_key(row):
    return row["kind"], row.get("id", row.get("title", (row.get("category"), row.get("key"))))


def detail_lines(row, columns):
    fields = row.get("details", [("名称", row.get("title") or row.get("name") or row.get("key", ""), "37"),
                                  ("备注", row.get("remark") or "—", "37")])
    lines = [color("    ┌ 详情", "2")]
    for label, value, style in fields:
        label = "".join(c if c.isprintable() else " " for c in str(label))
        if width(label) > 8:
            lines += [color("    " + part, "2") for part in wrap(label, columns - 4)]
            label = "│"
        prefix = "    " + label
        prefix += " " * (14 - width(prefix))
        for index, part in enumerate(wrap(value or "—", columns - 14)):
            lines.append(color(prefix if index == 0 else "    │" + " " * 9, "2") + color(part, style))
    lines.append(color("    └", "2"))
    return lines


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
            b"\x1b[5~": "page_up", b"\x1b[6~": "page_down", b"\x1b[3~": "delete", b"\r": "enter", b"\n": "enter",
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
    def __init__(self, identity, load, view="all", *, picker=False, category=""):
        self.identity, self.load, self.view = identity, load, view
        self.picker = picker
        self.category = category
        self.page = self.selected = 0
        self.expanded = None
        self.offset = 0
        self.follow = True

    def collapse(self):
        self.expanded = None
        self.offset = 0
        self.follow = True

    def render(self, records, columns, height):
        if columns < 30 or height < 12:
            return [clip("请扩大终端窗口，q 退出", columns)], [], 1
        # 每组都需要标题、表头和间隔；分类多时也不能挤出屏幕。
        batches, used, previous = [[]], 0, None
        for row in records:
            group = section(row)
            cost = 1 + (3 if group != previous else 0)
            if batches[-1] and used + cost > height - 8:
                batches.append([])
                used, previous, cost = 0, None, 4
            batches[-1].append(row)
            used += cost
            previous = group
        pages = len(batches)
        self.page = min(self.page, pages - 1)
        current = batches[self.page]
        self.selected = min(self.selected, max(0, len(current) - 1))
        if self.expanded is not None and not any(record_key(row) == self.expanded for row in current):
            self.collapse()
        lines = [color(clip(f"ID  /  {self.identity}", columns), "1;36"), ""]
        lines.append(color("选择维度" if self.view == "dimension" else "选择密码条目", "1;36") if self.picker else
                     "   ".join(color(label, "1;36" if key == self.view else "2") for key, label in VIEWS))
        lines.append("")
        index, selected_line = 0, 0
        body = []
        for (kind, category), group in groupby(current, key=section):
            label = "个人信息 / " + category_label(category) if kind == "info" else dict(VIEWS).get(kind, "维度")
            body.append(color(clip(label, columns), "1"))
            available = max(20, columns - 2)
            if kind == "password":
                widths = [max(6, available - 26), 10, 12] if columns >= 50 and not self.picker else [available - 12, 10]
                headers = ["条目", "密码", "关联资料"][:len(widths)]
            elif kind == "account":
                if columns >= 100:
                    widths = [22, available - 58, 10, 20]
                    headers = ["账号", "登录名 / 邮箱", "密码", "备注"]
                elif columns >= 60:
                    name_width = min(24, available // 3)
                    widths = [name_width, available - name_width - 14, 10]
                    headers = ["账号", "登录名 / 邮箱", "密码"]
                else:
                    widths, headers = [available - 12, 10], ["登录名 / 邮箱", "密码"]
            elif kind == "dimension":
                widths, headers = [available - 10, 8], ["维度", "条数"]
            elif columns >= 90:
                widths = [22, available - 46, 20]
                headers = ["名称", "内容", "备注"]
            elif columns >= 60:
                widths = [22, available - 24]
                headers = ["名称", "内容"]
            else:
                widths = [available // 2 - 1, available - available // 2 - 1]
                headers = ["名称", "内容"]

            def row_line(values, selected=False, status=""):
                cells = [clip(value, size) for value, size in zip(values, widths)]
                cells = [value + " " * (size - width(value)) for value, size in zip(cells, widths)]
                plain = "  ".join(cells).rstrip()
                prefix = "› " if selected else "  "
                if selected:
                    return color(prefix + plain, "1;36")
                if status:
                    index = 2 if kind == "account" and columns >= 60 else 1
                    cells[index] = color(cells[index], "32" if status == "已设置" else "33")
                    plain = "  ".join(cells).rstrip()
                return prefix + plain

            body.append(color(row_line(headers), "2"))
            for row in group:
                if kind == "password":
                    cells = [row["title"], row["status"], str(row.get("linked", 0))]
                elif kind == "account":
                    cells = [row["name"], row["username"] or "—", row["status"], row.get("remark") or "—"]
                    if columns < 60:
                        cells = cells[1:3]
                elif kind == "dimension":
                    cells = [row["label"], str(row["count"])]
                else:
                    cells = [row["key"], row["value"] or "—", row.get("remark") or "—"]
                if index == self.selected:
                    selected_line = len(body)
                body.append(row_line(cells, index == self.selected, row.get("status", "")))
                if not self.picker and record_key(row) == self.expanded:
                    body += detail_lines(row, columns)
                index += 1
            body.append("")
        if not records:
            body.append(color("  暂无记录", "2"))
        body_height = height - 8
        if self.follow:
            if self.expanded is not None:
                self.offset = max(0, selected_line - 2)
            elif selected_line < self.offset or selected_line >= self.offset + body_height:
                self.offset = max(0, selected_line - body_height + 1)
            self.follow = False
        self.offset = max(0, min(self.offset, len(body) - body_height))
        lines += body[self.offset:self.offset + body_height]
        position = f"第 {self.page + 1}/{pages} 页  ·  {len(records)} 条"
        if self.expanded is not None and len(body) > body_height:
            position += f"  ·  展开 {self.offset + 1}–{min(len(body), self.offset + body_height)}/{len(body)} 行"
        keys = "PgUp/PgDn 滚动  Enter 收起" if self.expanded else "↑↓ 选择  ←→/PgUp/PgDn 翻页  Enter " + ("确认" if self.picker else "展开")
        lines += [color(clip(position, columns), "2"), color(clip(keys, columns), "2")]
        if not self.picker:
            shortcuts = "1 全部  2 账号  3 密码  4 信息  d 维度  r 刷新  q 退出" if columns >= 90 else "1全 2号 3密 4信 d类 r刷 q退"
            if self.expanded is not None:
                password = current[self.selected]["kind"] == "password"
                actions = "o 操作  e 修改密码  " if password else "o 操作  e 编辑备注  f 删除字段  "
                if columns < 60:
                    actions = "o操作 e修改 " if password else "o操作 e备注 f删项 "
                lines.append(color(actions, "2") + color("x 删整条", "31") + color("  q 退出", "2") if columns >= 60 else
                             color(actions, "2") + color("x删条", "31") + color(" q退", "2"))
            else:
                lines.append(color(clip(shortcuts, columns), "2"))
            if self.category and self.view in ("all", "info"):
                lines[3] = color(clip("维度：" + category_label(self.category) + "（d 更换）", columns), "2")
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
                    if self.picker:
                        return current[self.selected]
                    selected = record_key(current[self.selected])
                    self.expanded = None if self.expanded == selected else selected
                    self.offset, self.follow = 0, True
                    continue
                if not self.picker and self.expanded is not None and current:
                    row = current[self.selected]
                    if key == "o":
                        return row
                    action = {"x": "delete", "delete": "delete", "f": "field", "e": "edit" if row["kind"] == "password" else "note"}.get(key)
                    if action and not (action == "field" and row["kind"] == "password"):
                        return {**row, "_action": action}
                if key in ("page_down", "page_up") and self.expanded is not None:
                    self.offset += (1 if key == "page_down" else -1) * max(1, size.lines - 10)
                    continue
                key = {"page_down": "next", "page_up": "prev"}.get(key, key)
                if key in ("next", "n", "prev", "p", "down", "j", "up", "k"):
                    self.collapse()
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
                        self.page, self.selected = self.page - 1, len(records)
                elif not self.picker and key in ("1", "2", "3", "4"):
                    self.view = VIEWS[int(key) - 1][0]
                    self.page = self.selected = 0
                    self.collapse()
                    return {"kind": "filter"}
                elif key == "d" and not self.picker:
                    return {"kind": "dimension"}
                elif key == "r" and not self.picker:
                    return {"kind": "refresh"}
