"""Exercise the compiled CLI using disposable data and real KeePassXC, never user vaults."""
import base64
import fcntl
import json
from http.server import BaseHTTPRequestHandler, HTTPServer
import threading
import os
from pathlib import Path
import pty
import re
import select
import shutil
import sqlite3
import struct
import subprocess
import tempfile
import termios
import time
import unittest
import zipfile
from xml.sax.saxutils import escape

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug/id"
MASTER = "synthetic-test-master"
PASSWORD = "synthetic-test-entry"
DOWN = b"\x1b[B"
ENTER = b"\r"


class Terminal:
    def __init__(self, case, args, env=None, width=100, height=24):
        self.case = case
        self.fd, self.slave = pty.openpty()
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
        self.before = termios.tcgetattr(self.slave)
        self.child = subprocess.Popen([str(BIN), "--root", str(case.root), *args],
                                      stdin=self.slave, stdout=self.slave, stderr=self.slave,
                                      env={**case.env, **(env or {})}, start_new_session=True)
        self.output = bytearray()
        self.position = 0
        case.addCleanup(self.close)

    def send(self, text):
        os.write(self.fd, text.encode() if isinstance(text, str) else text)

    def read(self, timeout=.1):
        if select.select([self.fd], [], [], timeout)[0]:
            try:
                value = os.read(self.fd, 65536)
            except OSError:
                return
            self.output.extend(value)

    def expect(self, text, timeout=10):
        wanted = text.encode()
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            found = self.output.find(wanted, self.position)
            if found >= 0:
                self.position = found + len(wanted)
                return
            self.read()
        self.case.fail(f"Timed out waiting for {text!r}; tail={bytes(self.output[-2000:])!r}")

    def finish(self, code=0):
        end = time.monotonic() + 10
        while self.child.poll() is None and time.monotonic() < end:
            self.read()
        self.case.assertEqual(self.child.poll(), code, bytes(self.output[-1500:]))
        while select.select([self.fd], [], [], .05)[0]:
            self.read(0)
        self.case.assertEqual(termios.tcgetattr(self.slave), self.before)
        self.case.assertNotIn(MASTER.encode(), self.output)
        return bytes(self.output)

    def close(self):
        if self.child.poll() is None:
            self.child.kill()
            self.child.wait()
        os.close(self.fd)
        os.close(self.slave)


@unittest.skipUnless(BIN.exists() and shutil.which("keepassxc-cli"), "build CLI and install KeePassXC first")
class CliTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="id-rust-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "template/data").mkdir(parents=True)
        shutil.copyfile(ROOT / "template/data/schema.sql", self.root / "template/data/schema.sql")
        (self.root / "config/secrets").mkdir(parents=True)
        (self.root / "config/config.toml").write_text("[terminal]\nallow_plaintext = false\n")
        self.runtime = self.root / "runtime"
        self.runtime.mkdir(mode=0o700)
        self.env = {k: v for k, v in os.environ.items()
                    if not k.startswith(("ID_", "AI_", "SSH_", "TMUX", "NO_COLOR"))}
        self.env.update(TERM="xterm-256color", XDG_RUNTIME_DIR=str(self.runtime))
        for name in ["alice", "bob"]:
            folder = self.root / f"identities/{name}/passwords"
            folder.mkdir(parents=True, mode=0o700)
            (folder.parent / "profile.md").write_text("Synthetic test identity only")
            (folder.parent / "AGENTS.md").write_text("Synthetic test fixture")
            self.keepass(name, "db-create", ["-p"], MASTER + "\n" + MASTER + "\n")
        self.run_cli("db-init")

    def run_cli(self, *args, code=0, env=None):
        result = subprocess.run([str(BIN), "--root", str(self.root), *args], env={**self.env, **(env or {})},
                                text=True, capture_output=True, timeout=20)
        self.assertEqual(result.returncode, code, result.stderr)
        self.assertNotIn(MASTER, result.stdout + result.stderr)
        return result

    def keepass(self, name, operation, args=(), data=None, code=0):
        vault = self.root / f"identities/{name}/passwords/{name}.kdbx"
        result = subprocess.run(["keepassxc-cli", operation, "-q", str(vault), *args],
                                input=data or MASTER + "\n", text=True, capture_output=True,
                                env={**self.env, "LANG": "C", "LC_ALL": "C"}, timeout=15)
        self.assertEqual(result.returncode, code, f"KeePassXC fixture {operation} failed")
        vault.chmod(0o600)
        return result.stdout

    def add(self, title="GitHub", password=PASSWORD, name="alice"):
        self.keepass(name, "add", [title, "-p", "-u", "tester"], MASTER + "\n" + password + "\n")

    def item(self, key="职位", note="备注", category="work", identity="alice"):
        self.run_cli("item", "--id", identity, "--cat", category, "--key", key,
                     "--value", "test@example.com" if category == "account" else "工程师", "--note", note)

    def db(self, query, parameters=()):
        with sqlite3.connect(self.root / "data/id.db") as db:
            result = db.execute(query, parameters).fetchall()
        return result

    def test_make_arguments_are_data_not_shell_code(self):
        payload = "O'Reilly; `touch SHOULD-NOT-EXIST`; \"quoted\""
        result = subprocess.run(["make", "--no-print-directory", "item", "id=alice", "cat=work",
                                 "key=test", "value=" + payload, "note="], cwd=ROOT,
                                env={**self.env, "ID_ROOT": str(self.root)}, text=True, capture_output=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.db("SELECT value,note FROM items"), [(payload, "")])
        self.assertFalse((ROOT / "SHOULD-NOT-EXIST").exists())
        self.run_cli("item", "--id", "../bob", "--cat", "work", "--key", "x", "--value", "x", "--note", "", code=1)

    def test_settings_arrows_toggle_and_cancel_restore_terminal(self):
        t = Terminal(self, ["settings"])
        t.expect("明文显示密码")
        t.send(DOWN * 5 + ENTER)
        t.expect("● 开")
        t.send(DOWN + ENTER)
        t.expect("设置已保存")
        t.finish()
        self.assertIn("allow_plaintext = true", (self.root / "config/config.toml").read_text())
        t = Terminal(self, ["settings"], width=40)
        t.expect("ID 设置")
        t.send(b"\x03")
        t.finish()

    def test_hidden_input_add_password_and_wrong_master(self):
        t = Terminal(self, ["pass-add-manual", "--id", "alice", "--title", "GitHub", "--user", "tester"])
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("新条目密码:")
        t.send(PASSWORD + "\r")
        t.expect("密码已保存")
        output = t.finish()
        self.assertNotIn(PASSWORD.encode(), output)
        self.assertEqual(self.keepass("alice", "show", ["-s", "-a", "Password", "GitHub"]).rstrip("\n"), PASSWORD)
        t = Terminal(self, ["list", "--id", "alice", "--type", "password"])
        t.expect("主密码:")
        t.send("wrong-fixture-master\r")
        t.expect("解锁失败")
        output = t.finish(1)
        self.assertNotIn(b"wrong-fixture-master", output)

    def test_personal_info_expands_scrolls_edits_and_deletes(self):
        note = "\n".join([f"第{i:02}行的详细备注" for i in range(35)])
        self.item(note=note)
        t = Terminal(self, ["list", "--id", "alice", "--type", "info"])
        t.expect("Enter 展开")
        t.send(ENTER)
        t.expect("第00行的详细备注")
        t.send(b"\x1b[6~" * 3)
        t.expect("第34行的详细备注")
        t.send("e")
        t.expect("新备注")
        t.send("新的备注\r")
        t.expect("新的备注")
        t.send("f")
        t.expect("选择要删除的字段")
        t.send(ENTER)
        t.expect("新的备注")
        t.send("x")
        t.expect("关联的密码条目保留")
        t.send(ENTER)
        t.expect("新的备注")
        t.send("x")
        t.expect("关联的密码条目保留")
        t.send(DOWN + ENTER)
        t.expect("暂无记录")
        t.send("q")
        output = t.finish()
        self.assertNotIn("主密码:".encode(), output)
        self.assertEqual(self.db("SELECT count(*) FROM items"), [(0,)])
        self.assertIn(b"\x1b[", output)

    def test_password_status_and_session_reuse(self):
        self.add()
        self.add("Empty", "")
        self.item(key="GitHub", category="account")
        item_id = self.db("SELECT id FROM items")[0][0]
        self.db("INSERT INTO password_links VALUES (?,?)", (item_id, "GitHub"))
        t = Terminal(self, ["list", "--id", "alice"])
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("未填密码")
        t.expect("GitHub")
        t.send("3")
        t.expect("Enter 展开")
        t.send("r")
        t.expect("Enter 展开")
        t.send("q")
        output = t.finish()
        self.assertEqual(output.count("主密码:".encode()), 1)
        self.assertNotIn(PASSWORD.encode(), output)

    def test_sensitive_edit_requires_master_again(self):
        self.add()
        t = Terminal(self, ["list", "--id", "alice", "--type", "password"])
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("Enter 展开")
        t.send(ENTER + b"e")
        t.expect("再次验证主密码:")
        t.send(MASTER + "\r")
        t.expect("新条目密码:")
        t.send("synthetic-updated\r")
        t.expect("Enter 收起")
        t.send("q")
        output = t.finish()
        self.assertNotIn(b"synthetic-updated", output)
        self.assertEqual(self.keepass("alice", "show", ["-s", "-a", "Password", "GitHub"]).strip(), "synthetic-updated")

    def test_plaintext_fails_closed_and_reauthenticates(self):
        self.add()
        t = Terminal(self, ["pass-show", "--id", "alice", "--title", "GitHub"])
        t.expect("明文显示已禁用")
        output = t.finish(1)
        self.assertNotIn("主密码:".encode(), output)
        (self.root / "config/config.toml").write_text("[terminal]\nallow_plaintext = true\n")
        t = Terminal(self, ["pass-show", "--id", "alice", "--title", "查一下我的github密码"])
        t.expect("再次验证主密码:")
        t.send(MASTER + "\r")
        t.expect("选择密码条目")
        t.send(ENTER)
        t.expect(PASSWORD)
        t.finish()

    def test_password_deletion_backs_up_and_only_unlinks_current_identity(self):
        self.add()
        self.item(key="GitHub", category="account")
        self.item(key="GitHub", category="account", identity="bob")
        for item_id, in self.db("SELECT id FROM items"):
            self.db("INSERT INTO password_links VALUES (?,?)", (item_id, "GitHub"))
        t = Terminal(self, ["list", "--id", "alice", "--type", "password"])
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("Enter 展开")
        t.send(ENTER + b"x")
        t.expect("先备份密码库")
        t.send(DOWN + ENTER)
        t.expect("再次验证主密码:")
        t.send(MASTER + "\r")
        t.expect("暂无记录")
        t.send("q")
        t.finish()
        backups = list((self.root / "backups/password-deletions/alice").glob("*.kdbx"))
        self.assertEqual(len(backups), 1)
        self.assertEqual(backups[0].stat().st_mode & 0o777, 0o600)
        self.assertEqual(self.db("SELECT identity FROM items JOIN password_links ON items.id=item_id"), [("bob",)])
        self.assertNotIn("GitHub", self.keepass("alice", "ls", ["-R", "-f"]))

    def test_master_change_and_identity_backfill(self):
        self.add()
        t = Terminal(self, ["passwd", "--id", "alice"])
        t.expect("再次验证主密码:")
        t.send(MASTER + "\r")
        t.expect("新主密码:")
        t.send("synthetic-next-master\r")
        t.expect("再次输入新主密码:")
        t.send("synthetic-next-master\r")
        t.expect("主密码已修改")
        t.finish()
        self.keepass("alice", "ls", data=MASTER + "\n", code=1)
        self.keepass("alice", "ls", data="synthetic-next-master\n")
        secret = self.root / "config/secrets/.secrets.env"
        secret.write_text("KDBX_INITIAL_PASSWORD=synthetic-initial\n")
        secret.chmod(0o600)
        self.run_cli("new", "--id", "charlie")
        vault = self.root / "identities/charlie/passwords/charlie.kdbx"
        before = vault.read_bytes()  # Encrypted synthetic fixture only.
        self.run_cli("new", "--id", "charlie")
        self.assertEqual(vault.read_bytes(), before)
        self.assertEqual(vault.stat().st_mode & 0o777, 0o600)

    def test_tag_rebuild_matches_escaped_titles_preserves_password_and_attachment(self):
        self.add("A & B")
        attachment = self.root / "test-attachment.txt"
        attachment.write_text("synthetic attachment")
        self.keepass("alice", "attachment-import", ["A & B", "test.txt", str(attachment)])
        t = Terminal(self, ["tag", "--id", "alice", "--title", "A & B", "--tags", "ai-deny"])
        t.expect("数据库级自定义设置需复查")
        t.send(DOWN + ENTER)
        t.expect("再次验证主密码:")
        t.send(MASTER + "\r")
        t.expect("标签已更新")
        t.finish()
        self.assertEqual(self.keepass("alice", "show", ["-a", "Tags", "A & B"]).strip(), "ai-deny")
        self.assertEqual(self.keepass("alice", "show", ["-s", "-a", "Password", "A & B"]).strip(), PASSWORD)
        target = self.root / "attachment-result.txt"
        self.keepass("alice", "attachment-export", ["A & B", "test.txt", str(target)])
        self.assertEqual(target.read_text(), "synthetic attachment")

    def test_grant_redacts_output_and_is_consumed_once(self):
        self.add()
        t = Terminal(self, ["grant", "--id", "alice", "--title", "GitHub"])
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("token:")
        output = t.finish()
        self.assertNotIn(PASSWORD.encode(), output)
        token = re.search(rb"token: ([0-9a-f]{32})", output)[1].decode()
        result = self.run_cli("use", "--token", token, "--cmd", 'printf "value=%s" "$ID_SECRET"; printf "%s" "$ID_SECRET" >&2')
        self.assertEqual(result.stdout, "value=***")
        self.assertEqual(result.stderr, "***")
        self.run_cli("use", "--token", token, "--cmd", "true", code=1)
        self.assertFalse(list((self.runtime / "id-grants").iterdir()))

    def test_import_archives_original_but_never_puts_secret_columns_in_sqlite(self):
        source = self.root / "accounts.xlsx"
        headers = ["分类", "服务商", "使用者", "账号", "备注", "密码备注", "详细数据"]
        values = ["国外", "Proton & Mail", "alice", "alice@example.com", "用途", "synthetic-private-note", "synthetic-recovery"]
        rows = []
        for number, values in enumerate([headers, values], 1):
            cells = ''.join(f'<c r="{chr(65+i)}{number}" t="inlineStr"><is><t>{escape(v)}</t></is></c>' for i, v in enumerate(values))
            rows.append(f'<row r="{number}">{cells}</row>')
        with zipfile.ZipFile(source, "w") as z:
            z.writestr("xl/worksheets/sheet1.xml", '<worksheet><sheetData>' + ''.join(rows) + '</sheetData></worksheet>')
        self.run_cli("import", str(source))
        self.run_cli("import", str(source))
        rows = self.db("SELECT identity,value,note FROM items")
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0][:2], ("alice", "alice@example.com"))
        self.assertIn("Proton & Mail", rows[0][2])
        self.assertNotIn("synthetic-private-note", rows[0][2])
        self.assertNotIn("synthetic-recovery", rows[0][2])
        for path in (self.root / "backups/mail-imports").iterdir():
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_unlinked_accounts_do_not_need_vault_unlock(self):
        self.item(key="GitHub", category="account")
        t = Terminal(self, ["list", "--id", "alice", "--type", "account"])
        t.expect("未关联")
        t.send("q")
        self.assertNotIn("主密码:".encode(), t.finish())

    def test_ssh_clipboard_checks_marker_and_clears_after_copy(self):
        self.add()
        t = Terminal(self, ["pass", "--id", "alice", "--title", "GitHub"],
                     env={"SSH_CONNECTION": "127.0.0.1 1234 127.0.0.1 22", "SSH_TTY": "/dev/pts/synthetic"})
        t.expect("q 跳过:")
        payload = re.search(rb"\x1b]52;c;([^\x07]*)\x07", t.output)[1]
        marker = base64.b64decode(payload).decode()
        self.assertTrue(marker.startswith("ID-CLIP-"))
        t.send(marker + "\r")
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("10 秒内粘贴")
        t.send(ENTER)
        output = t.finish()
        values = re.findall(rb"\x1b]52;c;([^\x07]*)\x07", output)
        self.assertIn(base64.b64encode(PASSWORD.encode()), values)
        self.assertEqual(values[-1], b"")
        self.assertNotIn(PASSWORD.encode(), output)

    def test_failed_clipboard_test_never_copies_password(self):
        self.add()
        t = Terminal(self, ["pass", "--id", "alice", "--title", "GitHub"],
                     env={"SSH_CONNECTION": "127.0.0.1 1234 127.0.0.1 22", "SSH_TTY": "/dev/pts/synthetic"})
        t.expect("q 跳过:")
        t.send(ENTER)
        t.expect("全局提示")
        output = t.finish(1)
        self.assertNotIn(base64.b64encode(PASSWORD.encode()), output)
        self.assertNotIn(PASSWORD.encode(), output)
        self.assertNotIn("主密码:".encode(), output)

    def test_plaintext_revocation_applies_during_open_session(self):
        self.add()
        config = self.root / "config/config.toml"
        config.write_text("[terminal]\nallow_plaintext = true\n")
        t = Terminal(self, ["pass-show", "--id", "alice", "--title", "GitHub"])
        t.expect("再次验证主密码:")
        config.write_text("[terminal]\nallow_plaintext = false\n")
        t.send(MASTER + "\r")
        t.expect("明文显示已禁用")
        self.assertNotIn(PASSWORD.encode(), t.finish(1))

    def test_secret_settings_are_private_and_cancellation_does_not_save(self):
        t = Terminal(self, ["settings"])
        t.expect("AI API key")
        t.send(ENTER)
        t.expect("新的 AI API key")
        t.send("synthetic-api-key\r")
        t.expect("已设置")
        t.send(b"q")
        output = t.finish()
        self.assertNotIn(b"synthetic-api-key", output)
        self.assertFalse((self.root / "config/secrets/.secrets.env").exists())
        t = Terminal(self, ["settings"])
        t.expect("AI API key")
        t.send(ENTER)
        t.expect("新的 AI API key")
        t.send("synthetic-api-key\r")
        t.expect("已设置")
        t.send(DOWN * 6 + ENTER)
        t.expect("设置已保存")
        t.finish()
        secret = self.root / "config/secrets/.secrets.env"
        self.assertEqual(secret.stat().st_mode & 0o777, 0o600)
        self.assertIn("synthetic-api-key", secret.read_text())

    def test_ai_requests_never_contain_master_and_local_intent_survives_model_refusal(self):
        requests = []
        draft = {"reply": "未查询密码库", "items": [], "events": [], "secrets": []}
        class Handler(BaseHTTPRequestHandler):
            def do_POST(handler):
                requests.append(json.loads(handler.rfile.read(int(handler.headers["Content-Length"]))))
                body = json.dumps({"choices": [{"message": {"content": json.dumps(draft)}}]}).encode()
                handler.send_response(200)
                handler.send_header("Content-Type", "application/json")
                handler.send_header("Content-Length", str(len(body)))
                handler.end_headers()
                handler.wfile.write(body)
            def log_message(self, *args):
                pass
        try:
            server = HTTPServer(("127.0.0.1", 0), Handler)
        except PermissionError:
            self.skipTest("local test server not permitted")
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        secret = self.root / "config/secrets/.secrets.env"
        secret.write_text("AI_API_KEY=synthetic-api-key\n")
        secret.chmod(0o600)
        (self.root / "config/config.toml").write_text(f'[ai]\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nmodel="fixture-model"\nthinking=true\nthinking_budget=128\n')
        t = Terminal(self, ["ai", "--id", "alice"])
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("你 ›")
        t.send("查看我的GitHub密码\r\r")
        t.expect("本地操作：make pass")
        t.expect("下一步")
        t.send(DOWN + ENTER)
        t.expect("你 ›")
        t.send("/lock\r\r")
        t.expect("密码库已锁定")
        t.expect("你 ›")
        t.send("/quit\r\r")
        output = t.finish()
        self.assertEqual(len(requests), 1)
        self.assertEqual(requests[0]["model"], "fixture-model")
        self.assertEqual(requests[0]["thinking_budget"], 128)
        self.assertNotIn(MASTER, json.dumps(requests))
        self.assertNotIn(PASSWORD.encode(), output)
        self.assertEqual(self.db("SELECT count(*) FROM items"), [(0,)])

    def set_tags(self, title, tags):
        t = Terminal(self, ["tag", "--id", "alice", "--title", title, "--tags", tags])
        t.expect("数据库级自定义设置需复查")
        t.send(DOWN + ENTER)
        t.expect("再次验证主密码:")
        t.send(MASTER + "\r")
        t.expect("标签已更新")
        t.finish()

    def issue_grant(self, title="GitHub"):
        t = Terminal(self, ["grant", "--id", "alice", "--title", title])
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("token:")
        return re.search(rb"token: ([0-9a-f]{32})", t.finish())[1].decode()

    def test_grant_policy_deny_precedes_auto_and_expired_tokens_are_removed(self):
        self.add()
        self.set_tags("GitHub", "ai-auto;AI-DENY")
        t = Terminal(self, ["grant", "--id", "alice", "--title", "GitHub"])
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("拒绝签发")
        self.assertNotIn(b"token:", t.finish(1))
        self.set_tags("GitHub", "ai-auto")
        token = self.issue_grant()
        for _ in range(2):
            self.run_cli("use", "--token", token, "--cmd", "true")
        metadata = self.runtime / f"id-grants/{token}.meta"
        data = json.loads(metadata.read_text())
        data["expires"] = 0
        metadata.write_text(json.dumps(data))
        self.run_cli("use", "--token", token, "--cmd", "true", code=1)
        self.assertFalse(list((self.runtime / "id-grants").iterdir()))
        token = self.issue_grant()
        self.run_cli("revoke", "--all")
        self.run_cli("use", "--token", token, "--cmd", "true", code=1)

    def test_oversized_grant_output_is_not_partially_printed(self):
        self.add()
        token = self.issue_grant()
        result = self.run_cli("use", "--token", token, "--cmd", "head -c 17000000 /dev/zero", code=1)
        self.assertEqual(result.stdout, "")
        self.assertIn("输出超过限制", result.stderr)

    def test_empty_marker_is_not_confused_with_real_password_titles(self):
        for title in ["Real[empty]", "[empty]"]:
            with self.subTest(title=title):
                self.add(title)
                t = Terminal(self, ["list", "--id", "alice", "--type", "password"])
                t.expect("主密码:")
                t.send(MASTER + "\r")
                t.expect(title)
                t.send(ENTER + b"x")
                t.expect("先备份密码库")
                t.send(DOWN + ENTER)
                t.expect("再次验证主密码:")
                t.send(MASTER + "\r")
                t.expect("暂无记录")
                t.send("q")
                t.finish()
