import io
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parents[1]))
import vault_session as vault


class Terminal(io.StringIO):
    def isatty(self):
        return True


class VaultSessionTests(unittest.TestCase):
    def setUp(self):
        for name in ("stdin", "stdout"):
            mock = patch.object(vault.sys, name, Terminal())
            mock.start()
            self.addCleanup(mock.stop)

    def test_repeated_commands_share_one_unlock_without_arguments_or_environment_secrets(self):
        with (vault.session() as session, patch.object(vault.getpass, "getpass", return_value="synthetic-master") as prompt,
              patch.object(vault.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as run):
            vault.run_cli(["keepassxc-cli", "ls", "one.kdbx"], "one.kdbx", text=True, check=False)
            vault.run_cli(["keepassxc-cli", "search", "one.kdbx", "title:*"], "one.kdbx", check=False)
            prompt.assert_called_once()
            self.assertEqual(run.call_count, 3)
            self.assertEqual(run.call_args_list[1].kwargs["input"], "synthetic-master\n")
            self.assertEqual(run.call_args_list[2].kwargs["input"], b"synthetic-master\n")
            for call in run.call_args_list:
                self.assertNotIn("synthetic-master", str(call.args))
                self.assertNotIn("synthetic-master", str(call.kwargs.get("env", {})))
            password = session._password
        self.assertIsNone(vault.current())
        self.assertEqual(password, b"\0" * len(password))

    def test_high_privilege_operations_and_switching_vaults_require_another_unlock(self):
        with (vault.session() as session, patch.object(vault.getpass, "getpass", return_value="synthetic-master") as prompt,
              patch.object(vault.subprocess, "run", return_value=SimpleNamespace(returncode=0))):
            session.unlock("one.kdbx")
            first = session._password
            session.unlock("one.kdbx", fresh=True)
            self.assertEqual(first, b"\0" * len(first))
            session.unlock("two.kdbx")
            session.unlock("one.kdbx")
            self.assertEqual(prompt.call_count, 4)

    def test_idle_session_reauthenticates_on_next_operation(self):
        with (vault.session() as session, patch.object(vault.getpass, "getpass", return_value="synthetic-master") as prompt,
              patch.object(vault.subprocess, "run", return_value=SimpleNamespace(returncode=0)),
              patch.object(vault.time, "monotonic", return_value=10) as now):
            session.unlock("one.kdbx")
            now.return_value = 309
            session.unlock("one.kdbx")
            prompt.assert_called_once()
            now.return_value = 610
            session.unlock("one.kdbx")
            self.assertEqual(prompt.call_count, 2)

    def test_failed_unlock_is_not_cached_or_echoed(self):
        with (vault.session() as session, patch.object(vault.getpass, "getpass", return_value="synthetic-master"),
              patch.object(vault.subprocess, "run", return_value=SimpleNamespace(returncode=1)) as run):
            with self.assertRaises(vault.VaultError) as error:
                vault.run_cli(["keepassxc-cli", "show", "one.kdbx", "demo"], "one.kdbx")
            self.assertIsNone(session._password)
            self.assertNotIn("synthetic-master", str(error.exception))
            run.assert_called_once()

    def test_nested_sessions_share_credentials_and_clear_after_exceptions(self):
        password = None
        with self.assertRaises(RuntimeError):
            with (vault.session() as outer, patch.object(vault.getpass, "getpass", return_value="synthetic-master"),
                  patch.object(vault.subprocess, "run", return_value=SimpleNamespace(returncode=0))):
                outer.unlock("one.kdbx")
                password = outer._password
                with vault.session() as inner:
                    self.assertIs(inner, outer)
                self.assertIs(vault.current(), outer)
                raise RuntimeError("cancel session")
        self.assertIsNone(vault.current())
        self.assertEqual(password, b"\0" * len(password))

    def test_redirects_are_rejected_before_reading_or_using_credentials(self):
        with vault.session() as session, patch.object(vault.sys, "stdout", io.StringIO()), patch.object(vault.getpass, "getpass") as prompt:
            with self.assertRaises(vault.VaultError):
                session.unlock("one.kdbx")
            prompt.assert_not_called()

    @unittest.skipUnless(shutil.which("keepassxc-cli"), "需要本机 KeePassXC")
    def test_native_cli_reuses_master_for_add_rename_and_metadata_queries(self):
        with tempfile.TemporaryDirectory() as directory:
            database = str(Path(directory) / "test.kdbx")
            master = "synthetic-master-for-test-only"
            result = subprocess.run(["keepassxc-cli", "db-create", "-p", "-t", "100", database],
                                    input=f"{master}\n{master}\n", text=True, capture_output=True)
            self.assertEqual(result.returncode, 0)
            with vault.session(), patch.object(vault.getpass, "getpass", return_value=master) as prompt:
                result = vault.run_cli(["keepassxc-cli", "add", "-p", database, "Original"], database,
                                       answers=("synthetic-entry-password",), capture_output=True)
                self.assertEqual(result.returncode, 0)
                result = vault.run_cli(["keepassxc-cli", "edit", "-t", "Simple", database, "Original"], database, capture_output=True)
                self.assertEqual(result.returncode, 0)
                result = vault.run_cli(["keepassxc-cli", "ls", "-q", database], database, text=True, capture_output=True)
                self.assertEqual(result.returncode, 0)
                self.assertEqual(result.stdout.strip(), "Simple")
                prompt.assert_called_once()


if __name__ == "__main__":
    unittest.main()
