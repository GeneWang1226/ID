import importlib.util
import io
import os
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parents[1]))

def load_script(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).parents[1] / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


clipboard = load_script("pass-ssh")
organizer = load_script("ai-organize")
PASSWORD = "demo-password-仅测试\r"


class Terminal(io.StringIO):
    def isatty(self):
        return True


class PasswordDisplayTests(unittest.TestCase):
    def invoke(self, *, show=False, answer="", readable=False, cancelled=False, code=0, allowed=True, revoke=False):
        output, errors = Terminal(), io.StringIO()
        args = ["pass-ssh.py", *(["--show"] if show else []), "demo.kdbx", "demo"]
        result = SimpleNamespace(returncode=code, stdout=(PASSWORD + "\n").encode())
        with (
            patch.object(clipboard.sys, "argv", args),
            patch.object(clipboard.sys, "stdin", Terminal(answer)),
            patch.object(clipboard.sys, "stdout", output),
            patch.object(clipboard.sys, "stderr", errors),
            patch.object(clipboard.subprocess, "run", return_value=result) as run,
            patch.object(clipboard.select, "select", return_value=([1] if readable else [], [], [])) as select,
            patch.object(clipboard, "check_clipboard", return_value="supported"),
            patch.object(clipboard, "choose_action", return_value="exit"),
            patch.object(clipboard, "plaintext_allowed", return_value=allowed, side_effect=[True, False] if revoke else None),
            patch.dict(os.environ, {"TMUX": ""}),
        ):
            if cancelled:
                select.side_effect = KeyboardInterrupt
            code = clipboard.main()
        return code, output.getvalue(), errors.getvalue(), run.call_args

    def test_timeout_and_enter_do_not_display_password(self):
        for readable in (False, True):
            code, output, errors, call = self.invoke(readable=readable, answer="\n")
            self.assertEqual(code, 0)
            self.assertNotIn(PASSWORD, output + errors)
            self.assertTrue(output.endswith("\033]52;c;\a"))
            self.assertIn("-s", call.args[0])
            self.assertIs(call.kwargs["stdout"], clipboard.subprocess.PIPE)

    def test_only_explicit_v_displays_password(self):
        for answer, visible in (("v\n", True), ("V\n", True), ("y\n", False)):
            _, output, errors, _ = self.invoke(readable=True, answer=answer)
            self.assertEqual(PASSWORD in output, visible)
            self.assertNotIn(PASSWORD, errors)
            if visible:
                self.assertLess(output.index("\033]52;c;\a"), output.index(PASSWORD))

    def test_show_displays_only_password_without_clipboard(self):
        code, output, errors, _ = self.invoke(show=True)
        self.assertEqual(code, 0)
        self.assertEqual(output, PASSWORD + "\n")
        self.assertNotIn(PASSWORD, errors)

    def test_cancel_clears_clipboard_without_plaintext(self):
        code, output, errors, _ = self.invoke(cancelled=True)
        self.assertEqual(code, 130)
        self.assertTrue(output.endswith("\033]52;c;\a"))
        self.assertNotIn(PASSWORD, output + errors)

    def test_unlock_failure_never_sends_or_displays_password(self):
        code, output, errors, _ = self.invoke(code=1)
        self.assertEqual(code, 1)
        self.assertEqual(output, "")
        self.assertNotIn(PASSWORD, errors)

    def test_global_denial_blocks_direct_show_before_unlock(self):
        code, output, errors, call = self.invoke(show=True, allowed=False)
        self.assertEqual(code, 1)
        self.assertIsNone(call)
        self.assertEqual(output, "")
        self.assertIn("禁用", errors)

    def test_global_denial_blocks_v_fallback(self):
        code, output, errors, _ = self.invoke(answer="v\n", readable=True, allowed=False)
        self.assertEqual(code, 1)
        self.assertNotIn(PASSWORD, output + errors)

    def test_unavailable_clipboard_and_disabled_plaintext_do_not_unlock(self):
        with patch.object(clipboard, "check_clipboard", return_value="unavailable"):
            with (
                patch.object(clipboard.sys, "argv", ["pass-ssh.py", "demo.kdbx", "demo"]),
                patch.object(clipboard.sys, "stdin", Terminal()),
                patch.object(clipboard.sys, "stdout", Terminal()),
                patch.object(clipboard.sys, "stderr", io.StringIO()),
                patch.object(clipboard, "plaintext_allowed", return_value=False),
                patch.object(clipboard.subprocess, "run") as run,
            ):
                self.assertEqual(clipboard.main(), 1)
                run.assert_not_called()

    def test_revoking_while_waiting_blocks_display(self):
        code, output, errors, _ = self.invoke(answer="v\n", readable=True, revoke=True)
        self.assertEqual(code, 1)
        self.assertNotIn(PASSWORD, output + errors)

    def test_revoking_after_unlock_blocks_direct_show(self):
        code, output, errors, _ = self.invoke(show=True, revoke=True)
        self.assertEqual(code, 1)
        self.assertNotIn(PASSWORD, output + errors)

    def test_rejects_redirects_and_noninteractive_input_before_unlock(self):
        for stdin, stdout in ((Terminal(), io.StringIO()), (io.StringIO(), Terminal())):
            with (
                patch.object(clipboard.sys, "argv", ["pass-ssh.py", "--show", "demo.kdbx", "demo"]),
                patch.object(clipboard.sys, "stdin", stdin),
                patch.object(clipboard.sys, "stdout", stdout),
                patch.object(clipboard.sys, "stderr", io.StringIO()),
                patch.object(clipboard.subprocess, "run") as run,
            ):
                self.assertEqual(clipboard.main(), 1)
                run.assert_not_called()
            self.assertEqual(stdout.getvalue(), "")

    def test_ai_menu_launches_display_without_capturing_output(self):
        draft = {"items": [], "events": [], "secrets": [{"title": "demo", "action": "copy"}]}
        with (
            patch.object(organizer.sys, "argv", ["ai-organize.py", "--identity", "demo"]),
            patch.object(organizer, "read_config", return_value={"AI_API_KEY": "demo"}),
            patch.object(organizer, "read_input", side_effect=["看看我的密码", None]),
            patch.object(organizer, "call_ai", return_value=draft) as ai,
            patch.object(organizer, "plaintext_allowed", return_value=True),
            patch.object(organizer, "choose_action", return_value="secrets-show") as choose,
            patch.object(organizer.password_cli, "main", return_value=0) as run,
            patch.object(organizer.account_store, "vault_for", return_value=Path("demo.kdbx")),
            patch.object(organizer.sys, "stdout", io.StringIO()),
            patch.object(organizer.sys, "stderr", io.StringIO()),
        ):
            self.assertEqual(organizer.main(), 0)
        self.assertIn("secrets-show", dict(choose.call_args.args[0]))
        self.assertEqual(run.call_args.args[0], ["--show", "--", "demo.kdbx", "demo"])
        self.assertEqual(run.call_args.kwargs, {})
        self.assertEqual(ai.call_count, 1)

    def test_cached_session_requires_fresh_verification_before_plaintext(self):
        vault = clipboard.vault_session
        result = SimpleNamespace(returncode=0, stdout=(PASSWORD + "\n").encode())
        with (patch.object(clipboard.sys, "stdin", Terminal()), patch.object(clipboard.sys, "stdout", Terminal()) as output,
              patch.object(clipboard.sys, "stderr", io.StringIO()), patch.object(clipboard, "plaintext_allowed", return_value=True),
              patch.object(clipboard, "read_entry", return_value=result),
              patch.object(vault.getpass, "getpass", return_value="synthetic-master") as prompt,
              patch.object(vault.subprocess, "run", return_value=SimpleNamespace(returncode=0)), vault.session() as session):
            session.unlock("demo.kdbx")
            self.assertEqual(clipboard.main(["--show", "demo.kdbx", "demo"]), 0)
            self.assertEqual(prompt.call_count, 2)
            self.assertEqual(output.getvalue(), PASSWORD + "\n")

    def test_cached_copy_fallback_reauthenticates_before_showing_plaintext(self):
        vault = clipboard.vault_session
        result = SimpleNamespace(returncode=0, stdout=(PASSWORD + "\n").encode())
        with (patch.object(clipboard.sys, "stdin", Terminal("v\n")), patch.object(clipboard.sys, "stdout", Terminal()) as output,
              patch.object(clipboard.sys, "stderr", io.StringIO()), patch.object(clipboard, "plaintext_allowed", return_value=True),
              patch.object(clipboard, "read_entry", return_value=result), patch.object(clipboard, "check_clipboard", return_value="supported"),
              patch.object(clipboard, "write_clipboard"), patch.object(clipboard.select, "select", return_value=([1], [], [])),
              patch.object(vault.getpass, "getpass", return_value="synthetic-master") as prompt,
              patch.object(vault.subprocess, "run", return_value=SimpleNamespace(returncode=0)), vault.session() as session):
            session.unlock("demo.kdbx")
            self.assertEqual(clipboard.main(["demo.kdbx", "demo"]), 0)
            self.assertEqual(prompt.call_count, 2)
            self.assertEqual(output.getvalue(), PASSWORD + "\n")

    def test_ai_lock_command_is_local_and_not_sent_to_model(self):
        with (patch.object(organizer.sys, "argv", ["ai-organize.py", "--identity", "demo"]),
              patch.object(organizer, "read_config", return_value={"AI_API_KEY": "demo"}),
              patch.object(organizer, "_TTY", False), patch.object(organizer, "read_input", side_effect=["/lock", None]),
              patch.object(organizer, "call_ai") as ai, patch.object(organizer.sys, "stdout", io.StringIO()) as output):
            self.assertEqual(organizer.main(), 0)
            ai.assert_not_called()
            self.assertIn("已锁定", output.getvalue())

    def test_ai_menu_hides_disabled_plaintext(self):
        draft = {"items": [], "events": [], "secrets": [{"title": "demo", "action": "copy"}]}
        with (
            patch.object(organizer.sys, "argv", ["ai-organize.py", "--identity", "demo"]),
            patch.object(organizer, "read_config", return_value={"AI_API_KEY": "demo"}),
            patch.object(organizer, "read_input", return_value="看看我的密码"),
            patch.object(organizer, "call_ai", return_value=draft),
            patch.object(organizer, "plaintext_allowed", return_value=False),
            patch.object(organizer, "choose_action", return_value="exit") as choose,
            patch.object(organizer.sys, "stdout", io.StringIO()),
            patch.object(organizer.sys, "stderr", io.StringIO()),
        ):
            self.assertEqual(organizer.main(), 0)
        self.assertNotIn("secrets-show", dict(choose.call_args.args[0]))

    def test_ai_warns_at_startup_and_hides_unavailable_copy(self):
        draft = {"items": [], "events": [], "secrets": [{"title": "demo", "action": "copy"}]}
        with (
            patch.object(organizer.sys, "argv", ["ai-organize.py", "--identity", "demo"]),
            patch.object(organizer, "_TTY", True),
            patch.object(organizer, "read_config", return_value={"AI_API_KEY": "demo"}),
            patch.object(organizer, "check_clipboard", return_value="unavailable") as check,
            patch.object(organizer, "read_input", return_value="看看我的密码") as read,
            patch.object(organizer, "call_ai", return_value=draft),
            patch.object(organizer, "plaintext_allowed", return_value=True),
            patch.object(organizer, "choose_action", return_value="exit") as choose,
            patch.object(organizer.sys, "stdout", io.StringIO()),
            patch.object(organizer.sys, "stderr", io.StringIO()),
        ):
            def input_after_probe(_):
                check.assert_called_once()
                return "看看我的密码"
            read.side_effect = input_after_probe
            self.assertEqual(organizer.main(), 0)
        options = dict(choose.call_args.args[0])
        self.assertNotIn("secrets-copy", options)
        self.assertIn("secrets-show", options)


class EntryLookupTests(unittest.TestCase):
    def invoke(self, results, choices, *, native=False, allowed=True):
        output, errors = Terminal(), io.StringIO()
        args = ["pass-ssh.py", "--native" if native else "--show", "demo.kdbx", "查一下我的github密码"]
        with (
            patch.object(clipboard.sys, "argv", args),
            patch.object(clipboard.sys, "stdin", Terminal()),
            patch.object(clipboard.sys, "stdout", output),
            patch.object(clipboard.sys, "stderr", errors),
            patch.object(clipboard.subprocess, "run", side_effect=results) as run,
            patch.object(clipboard, "choose_action", side_effect=choices) as choose,
            patch.object(clipboard, "plaintext_allowed", return_value=allowed),
            patch.object(clipboard, "check_clipboard") as probe,
        ):
            code = clipboard.main()
        return code, output.getvalue(), errors.getvalue(), run, choose, probe

    def test_exact_entry_is_attempted_first_without_normalization_or_search(self):
        result = SimpleNamespace(returncode=0, stdout=(PASSWORD + "\n").encode())
        code, output, _, run, choose, _ = self.invoke([result], [])
        self.assertEqual(code, 0)
        self.assertEqual(output, PASSWORD + "\n")
        self.assertEqual(run.call_args.args[0][-1], "查一下我的github密码")
        self.assertEqual(run.call_count, 1)
        choose.assert_not_called()

    def test_natural_language_uses_actual_selected_path_without_changing_entries(self):
        results = [
            SimpleNamespace(returncode=1, stdout=b""),
            SimpleNamespace(returncode=0, stdout="/个人/GitHub 账户密码\n/工作/GITHUB\n/邮件\n"),
            SimpleNamespace(returncode=0, stdout=(PASSWORD + "\n").encode()),
        ]
        code, output, errors, run, choose, _ = self.invoke(results, ["search", "1"])
        self.assertEqual(code, 0)
        self.assertEqual(output, PASSWORD + "\n")
        self.assertNotIn(PASSWORD, errors)
        candidates = dict(choose.call_args_list[1].args[0])
        self.assertEqual(candidates, {"0": "/个人/GitHub 账户密码", "1": "/工作/GITHUB", "exit": "取消"})
        self.assertEqual(run.call_args_list[1].args[0], ["keepassxc-cli", "search", "demo.kdbx", "title:*"])
        self.assertEqual(run.call_args_list[2].args[0][-1], "/工作/GITHUB")
        self.assertTrue(all(call.args[0][1] in {"show", "search"} for call in run.call_args_list))

    def test_cancel_or_failed_search_never_retries_password_read(self):
        for results, choices, calls in (
            ([SimpleNamespace(returncode=1, stdout=b"")], ["exit"], 1),
            ([SimpleNamespace(returncode=1, stdout=b""), SimpleNamespace(returncode=1, stdout="")], ["search"], 2),
            ([SimpleNamespace(returncode=1, stdout=b""), SimpleNamespace(returncode=0, stdout="/GitHub\n")], ["search", "exit"], 2),
        ):
            code, output, errors, run, _, _ = self.invoke(results, choices)
            self.assertEqual(code, 1)
            self.assertNotIn(PASSWORD, output + errors)
            self.assertEqual(run.call_count, calls)

    def test_empty_database_has_no_selectable_entry(self):
        results = [SimpleNamespace(returncode=1, stdout=b""), SimpleNamespace(returncode=0, stdout="")]
        code, output, errors, run, choose, _ = self.invoke(results, ["search"])
        self.assertEqual(code, 1)
        self.assertEqual(choose.call_count, 1)
        self.assertEqual(run.call_count, 2)
        self.assertIn("没有可选择", errors)
        self.assertNotIn(PASSWORD, output + errors)

    def test_unmatched_request_offers_existing_titles(self):
        results = [SimpleNamespace(returncode=1, stdout=b""), SimpleNamespace(returncode=0, stdout="/GitLab\n")]
        _, _, errors, _, choose, _ = self.invoke(results, ["search", "exit"])
        self.assertEqual(dict(choose.call_args.args[0]), {"0": "/GitLab", "exit": "取消"})
        self.assertIn("没有匹配", errors)

    def test_native_copy_retries_clip_without_reading_password_into_python(self):
        results = [
            SimpleNamespace(returncode=1),
            SimpleNamespace(returncode=0, stdout="/GitHub\n"),
            SimpleNamespace(returncode=0),
        ]
        code, output, errors, run, _, probe = self.invoke(results, ["search", "0"], native=True, allowed=False)
        self.assertEqual(code, 0)
        for index in (0, 2):
            self.assertEqual(run.call_args_list[index].args[0][1], "clip")
            self.assertIsNone(run.call_args_list[index].kwargs["stdout"])
        self.assertNotIn(PASSWORD, output + errors)
        probe.assert_not_called()

    def test_title_extraction_handles_lookup_language_and_grouped_entries(self):
        for query in ("查一下我的github密码", "查看我的 GitHub 账户密码", "复制 GitHub 密码", "show my GitHub password"):
            with self.subTest(query=query):
                self.assertEqual(organizer.infer_secret_title(query).casefold(), "github密码")
                self.assertEqual(clipboard.matching_entries(["/GitHub 账户密码", "/工作/GITHUB", "/GitLab"], query),
                                 ["/GitHub 账户密码", "/工作/GITHUB"])


if __name__ == "__main__":
    unittest.main()
