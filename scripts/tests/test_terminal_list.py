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
    return passwords + [dict(kind="info", id=1, category="account", key="github-username", value="demo-user", link=None)]


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
        for page in range(4):
            browser.page = page
            _, current, pages = browser.render(rows, 80, 24)
            self.assertEqual(pages, 4)
            found += current
        self.assertEqual(found, rows)

    def test_next_page_and_row_selection_return_the_full_record(self):
        rows = records(30)
        browser = ui.RecordBrowser("demo", lambda _: rows)
        with (patch.object(ui, "screen", fake_screen), patch.object(ui, "read_key", side_effect=["next", "down", "enter"]),
              patch.object(ui.shutil, "get_terminal_size", return_value=SimpleNamespace(columns=80, lines=24)),
              patch.object(ui.sys, "stdout", Terminal())):
            selected = browser.choose()
        self.assertEqual(selected, rows[10])

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
        with (patch.object(ui, "screen", checked_screen), patch.object(ui, "read_key", side_effect=["2", "3", "q"]),
              patch.object(ui.sys, "stdout", Terminal())):
            self.assertEqual(browser.choose(), {"kind": "filter"})
            self.assertEqual(browser.choose(), {"kind": "filter"})
            self.assertIsNone(browser.choose())
        self.assertEqual(loaded, ["all", "password", "info"])

    def test_color_has_hierarchy_and_can_be_disabled(self):
        with patch.dict(os.environ, {"TERM": "xterm-256color"}, clear=True), patch.object(ui.sys, "stdout", Terminal()):
            self.assertIn("\033[1;36m", ui.color("title", "1;36"))
            with patch.dict(os.environ, {"NO_COLOR": "1"}):
                self.assertEqual(ui.color("title", "1;36"), "title")
        with patch.object(ui.sys, "stdout", io.StringIO()):
            self.assertEqual(ui.color("title", "1;36"), "title")

    def test_escape_and_page_keys_do_not_hang_on_incomplete_sequences(self):
        for sequence, expected in ((b"\x1b", "q"), (b"\x1b[6~", "next"), (b"\x1b[5~", "prev")):
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
