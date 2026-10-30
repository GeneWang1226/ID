from contextlib import contextmanager
import io
import os
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parents[1]))
import terminal_list as ui


class Terminal(io.StringIO):
    def isatty(self):
        return True


@contextmanager
def fake_screen():
    yield 0


def records(count=2):
    passwords = [dict(kind="password", title=f"服务器/{i}", status="未填密码", linked=0) for i in range(count)]
    return passwords + [dict(kind="info", id=1, category="work", key="职位", value="工程师", remark="远程办公", link=None)]


class TerminalListTests(unittest.TestCase):
    def test_table_fits_wide_and_narrow_terminals_with_chinese_alignment(self):
        rows = records()
        rows[0]["title"] = "中文服务器名称" * 20
        for columns in (30, 40, 59, 60, 80, 90, 120):
            browser = ui.RecordBrowser("demo", lambda _: rows)
            with patch.dict(os.environ, {"NO_COLOR": "1"}), patch.object(ui.sys, "stdout", Terminal()):
                lines, current, _ = browser.render(rows, columns, 24)
            self.assertTrue(all(ui.width(line) <= columns for line in lines), (columns, lines))
            self.assertLessEqual(len(lines), 24)
            self.assertEqual(len(current), 3)
            self.assertEqual(rows[0]["title"], "中文服务器名称" * 20)
        browser = ui.RecordBrowser("demo", lambda _: rows)
        with patch.dict(os.environ, {"NO_COLOR": "1"}):
            lines, _, _ = browser.render(rows, 100, 24)
        password_rows = [line for line in lines if "未填密码" in line]
        starts = [ui.width(line[:line.index("未填密码")]) for line in password_rows]
        self.assertEqual(len(set(starts)), 1)

    def test_pagination_preserves_every_record_once(self):
        rows = records(30)
        browser = ui.RecordBrowser("demo", lambda _: rows)
        found = []
        _, _, pages = browser.render(rows, 80, 24)
        for page in range(pages):
            browser.page = page
            _, current, _ = browser.render(rows, 80, 24)
            found += current
        self.assertEqual(found, rows)

    def test_next_page_and_row_selection_return_the_full_record(self):
        rows = records(30)
        browser = ui.RecordBrowser("demo", lambda _: rows)
        with (patch.object(ui, "screen", fake_screen), patch.object(ui, "read_key", side_effect=["next", "down", "enter", "o"]),
              patch.object(ui.shutil, "get_terminal_size", return_value=SimpleNamespace(columns=80, lines=24)),
              patch.object(ui.sys, "stdout", Terminal())):
            selected = browser.choose()
        self.assertEqual(selected, rows[14])

    def test_enter_expands_in_place_and_enter_again_collapses(self):
        rows = [dict(kind="info", id=1, category="work", key="职位", value="工程师", remark="摘要",
                     details=[("备注", "完整备注：第一行\n第二行说明", "37")])]
        browser = ui.RecordBrowser("demo", lambda _: rows)
        frames = []

        def key(_):
            frames.append(output.getvalue().split("\033[H\033[2J")[-1])
            return ["enter", "enter", "q"][len(frames) - 1]

        with (patch.object(ui, "screen", fake_screen), patch.object(ui, "read_key", side_effect=key),
              patch.object(ui.sys, "stdout", Terminal()) as output, patch.dict(os.environ, {"NO_COLOR": "1"})):
            self.assertIsNone(browser.choose())
        self.assertNotIn("完整备注", frames[0])
        self.assertIn("完整备注：第一行", frames[1])
        self.assertIn("第二行说明", frames[1])
        self.assertNotIn("完整备注", frames[2])

    def test_long_expanded_notes_wrap_and_scroll_without_clipping_content(self):
        note = "长备注中文" * 25 + "\n备注末尾，必须完整显示。"
        row = dict(kind="info", id=1, category="work", key="职位", value="工程师", details=[("备注", note, "37")])
        for columns, height in ((30, 12), (80, 24)):
            browser = ui.RecordBrowser("demo", lambda _: [row])
            browser.expanded = ui.record_key(row)
            found = []
            for offset in range(0, 80, height - 8):
                browser.offset = offset
                browser.follow = False
                lines, current, _ = browser.render([row], columns, height)
                self.assertTrue(all(ui.width(line) <= columns for line in lines))
                self.assertLessEqual(len(lines), height)
                found += lines
                self.assertEqual(current, [row])
            for part in ui.wrap(note, columns - 14):
                self.assertTrue(any(line.endswith(part) for line in found), part)
                self.assertNotIn("…", part)

    def test_picker_enter_confirms_and_delete_only_acts_on_expanded_records(self):
        rows = records(1)
        for picker, keys, action in ((True, ["enter"], None), (False, ["x", "enter", "delete"], "delete")):
            browser = ui.RecordBrowser("demo", lambda _: rows, picker=picker)
            with (patch.object(ui, "screen", fake_screen), patch.object(ui, "read_key", side_effect=keys),
                  patch.object(ui.sys, "stdout", Terminal())):
                chosen = browser.choose()
            self.assertEqual(chosen, {**rows[0], "_action": action} if action else rows[0])
        self.assertNotIn("_action", rows[0])

    def test_detail_labels_and_values_cannot_inject_terminal_controls(self):
        row = dict(details=[("\x1b[2J备注", "第一行\n第二行\x1b[31m内容", "37")])
        with patch.dict(os.environ, {"NO_COLOR": "1"}):
            lines = ui.detail_lines(row, 50)
        self.assertNotIn("\x1b", "".join(lines))
        self.assertTrue(any("第一行" in line for line in lines))
        self.assertTrue(any("第二行" in line for line in lines))

    def test_many_dimensions_and_all_three_sections_fit_every_page(self):
        rows = [dict(kind="account", name="GitHub", username="demo-user", status="已设置", remark="开发账号")]
        rows += records(2)[:2]
        rows += [dict(kind="info", category=f"自定义维度{i}", key="属性" * 20, value="内容", remark="备注") for i in range(20)]
        for columns, height in ((30, 12), (60, 16), (100, 24), (120, 40)):
            browser = ui.RecordBrowser("demo", lambda _: rows)
            _, _, pages = browser.render(rows, columns, height)
            found = []
            for page in range(pages):
                browser.page = page
                lines, current, _ = browser.render(rows, columns, height)
                self.assertLessEqual(len(lines), height)
                self.assertTrue(all(ui.width(line) <= columns for line in lines), (columns, lines))
                found += current
            self.assertEqual(found, rows)

    def test_account_login_status_and_notes_are_visible_on_wide_screens(self):
        rows = [dict(kind="account", name="GitHub", username="demo-user", status="已设置", remark="开发账号")]
        browser = ui.RecordBrowser("demo", lambda _: rows, "account")
        lines, _, _ = browser.render(rows, 100, 24)
        output = "\n".join(lines)
        for value in ("GitHub", "demo-user", "已设置", "开发账号"):
            self.assertIn(value, output)

    def test_type_switches_reload_only_the_requested_view(self):
        loaded = []
        active = [False]

        @contextmanager
        def checked_screen():
            active[0] = True
            try:
                yield 0
            finally:
                active[0] = False

        def load(view):
            self.assertFalse(active[0], "解锁和加载应在退出原始终端模式后执行")
            loaded.append(view)
            return []

        browser = ui.RecordBrowser("demo", load)
        with (patch.object(ui, "screen", checked_screen), patch.object(ui, "read_key", side_effect=["2", "3", "4", "d", "q"]),
              patch.object(ui.sys, "stdout", Terminal())):
            self.assertEqual(browser.choose(), {"kind": "filter"})
            self.assertEqual(browser.choose(), {"kind": "filter"})
            self.assertEqual(browser.choose(), {"kind": "filter"})
            self.assertEqual(browser.choose(), {"kind": "dimension"})
            self.assertIsNone(browser.choose())
        self.assertEqual(loaded, ["all", "account", "password", "info", "info"])

    def test_color_has_hierarchy_and_can_be_disabled(self):
        with patch.dict(os.environ, {"TERM": "xterm-256color"}, clear=True), patch.object(ui.sys, "stdout", Terminal()):
            self.assertIn("\033[1;36m", ui.color("title", "1;36"))
            with patch.dict(os.environ, {"NO_COLOR": "1"}):
                self.assertEqual(ui.color("title", "1;36"), "title")
        with patch.object(ui.sys, "stdout", io.StringIO()):
            self.assertEqual(ui.color("title", "1;36"), "title")

    def test_escape_and_page_keys_do_not_hang_on_incomplete_sequences(self):
        for sequence, expected in ((b"\x1b", "q"), (b"\x1b[6~", "page_down"), (b"\x1b[5~", "page_up"), (b"\x1b[3~", "delete")):
            reader, writer = os.pipe()
            try:
                os.write(writer, sequence)
                os.close(writer)
                writer = None
                self.assertEqual(ui.read_key(reader), expected)
            finally:
                os.close(reader)
                if writer is not None:
                    os.close(writer)


if __name__ == "__main__":
    unittest.main()
