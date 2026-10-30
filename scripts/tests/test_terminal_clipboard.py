import io
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parents[1]))
import terminal_clipboard as terminal
import settings


class Terminal(io.StringIO):
    def isatty(self):
        return True


class TerminalClipboardTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def probe(self, cache, answer, *, force=False, cancelled=False):
        output, errors = Terminal(), io.StringIO()
        with (
            patch.dict(os.environ, {"SSH_CONNECTION": "demo-client", "TMUX": ""}),
            patch.object(terminal.sys, "stdin", Terminal()),
            patch.object(terminal.sys, "stdout", output),
            patch.object(terminal.sys, "stderr", errors),
            patch.object(terminal, "session_file", return_value=cache),
            patch.object(terminal.secrets, "token_hex", return_value="demo"),
            patch.object(terminal.getpass, "getpass", return_value=answer) as prompt,
        ):
            if cancelled:
                prompt.side_effect = KeyboardInterrupt
            status = terminal.check_clipboard(force=force)
        return status, output.getvalue(), errors.getvalue(), prompt.call_count

    def test_success_is_cached_for_the_connection(self):
        cache = self.root / "cache"
        status, _, _, prompts = self.probe(cache, "ID-CLIP-demo")
        self.assertEqual((status, prompts), ("supported", 1))
        status, output, _, prompts = self.probe(cache, "")
        self.assertEqual((status, prompts), ("supported", 0))
        self.assertEqual(output, "")

    def test_failed_paste_warns_without_recording_pasted_content(self):
        cache = self.root / "cache"
        status, output, errors, _ = self.probe(cache, "unrelated-clipboard-content")
        self.assertEqual(status, "unavailable")
        self.assertIn("全局提示", errors)
        self.assertTrue(output.endswith("\033]52;c;\a"))
        self.assertEqual(cache.read_text(), "unavailable")
        self.assertNotIn("unrelated-clipboard-content", output + errors)

    def test_retest_can_replace_a_failed_result(self):
        cache = self.root / "cache"
        cache.write_text("unavailable")
        status, _, _, prompts = self.probe(cache, "ID-CLIP-demo", force=True)
        self.assertEqual((status, prompts), ("supported", 1))

    def test_skip_is_not_classified_as_support(self):
        status, _, errors, _ = self.probe(self.root / "cache", "q")
        self.assertEqual(status, "unknown")
        self.assertIn("未验证", errors)

    def test_sessions_do_not_share_results(self):
        with (
            patch.object(terminal.os, "ttyname", return_value="/dev/pts/demo"),
            patch.object(terminal.os, "stat", return_value=SimpleNamespace(st_ctime_ns=1)),
            patch.object(terminal.sys, "stdin", SimpleNamespace(fileno=lambda: 0)),
            patch.dict(os.environ, {"SSH_CONNECTION": "client-one", "TMUX": ""}),
        ):
            first = terminal.session_file()
            os.environ["SSH_CONNECTION"] = "client-two"
            self.assertNotEqual(first, terminal.session_file())

    def test_noninteractive_probe_does_not_send_or_prompt(self):
        with (
            patch.dict(os.environ, {"SSH_CONNECTION": "demo-client"}),
            patch.object(terminal.sys, "stdin", io.StringIO()),
            patch.object(terminal.getpass, "getpass") as prompt,
            patch.object(terminal, "write_clipboard") as write,
        ):
            self.assertEqual(terminal.check_clipboard(), "unknown")
        prompt.assert_not_called()
        write.assert_not_called()

    def test_probe_cancellation_is_unknown_and_clears_test_clipboard(self):
        status, output, _, _ = self.probe(self.root / "cache", "", cancelled=True)
        self.assertEqual(status, "unknown")
        self.assertTrue(output.endswith("\033]52;c;\a"))

    def test_permission_fails_closed_and_reloads_each_time(self):
        config = self.root / "config.toml"
        with patch.object(terminal, "CONFIG", config):
            self.assertFalse(terminal.plaintext_allowed())
            config.write_text("[terminal]\nallow_plaintext = true\n")
            self.assertTrue(terminal.plaintext_allowed())
            config.write_text("[terminal]\nallow_plaintext = false\n")
            self.assertFalse(terminal.plaintext_allowed())
            config.write_text("invalid toml!")
            self.assertFalse(terminal.plaintext_allowed())

    def test_settings_saves_both_ai_and_plaintext_permission(self):
        config = self.root / "config.toml"
        with patch.object(settings, "CONFIG", config):
            values = settings.load()
            self.assertFalse(values["allow_plaintext"])
            values.update(model="demo-model", allow_plaintext=True)
            settings.save_config(values)
            self.assertEqual(settings.load(), values)
            self.assertIn('model = "demo-model"', config.read_text())
            self.assertIn('base_url = "https://', config.read_text())
            values["allow_plaintext"] = False
            settings.save_config(values)
            self.assertFalse(settings.load()["allow_plaintext"])


if __name__ == "__main__":
    unittest.main()
