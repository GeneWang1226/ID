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
        t = Terminal(self, ["manage", "--id", "alice", "--type", "password"])
        t.expect("主密码:")
        t.send("wrong-fixture-master\r")
        t.expect("解锁失败")
        output = t.finish(1)
        self.assertNotIn(b"wrong-fixture-master", output)

    def test_personal_info_expands_scrolls_edits_and_deletes(self):
        note = "\n".join([f"第{i:02}行的详细备注" for i in range(35)])
        self.item(note=note)
        t = Terminal(self, ["manage", "--id", "alice", "--type", "info"])
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
        t = Terminal(self, ["manage", "--id", "alice"])
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
        t = Terminal(self, ["manage", "--id", "alice", "--type", "password"])
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
        t = Terminal(self, ["manage", "--id", "alice", "--type", "password"])
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
        t = Terminal(self, ["manage", "--id", "alice", "--type", "account"])
        t.expect("未关联")
        t.send("q")
        self.assertNotIn("主密码:".encode(), t.finish())

    def test_manage_entry_and_legacy_list_alias_open_the_same_interface(self):
        self.item()
        for command in ["manage", "list"]:
            with self.subTest(command=command):
                t = Terminal(self, [command, "--id", "alice", "--type", "info"])
                t.expect("身份管理  /  alice")
                t.expect("工程师")
                t.send("q")
                t.finish()
        help_text = self.run_cli("--help").stdout
        self.assertIn("manage", help_text)

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
        t.expect("空行提交")
        t.send("查看我的GitHub密码\r\r")
        t.expect("本地操作：make pass")
        t.expect("下一步")
        t.send(DOWN + ENTER)
        t.expect("空行提交")
        t.send("/lock\r\r")
        t.expect("密码库已锁定")
        t.expect("空行提交")
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
                t = Terminal(self, ["manage", "--id", "alice", "--type", "password"])
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

    def ai_server(self, responses):
        requests = []
        class Handler(BaseHTTPRequestHandler):
            def do_POST(handler):
                requests.append(json.loads(handler.rfile.read(int(handler.headers["Content-Length"]))))
                status, value = responses[min(len(requests)-1, len(responses)-1)]
                body = json.dumps(value).encode() if isinstance(value, dict) else value.encode()
                handler.send_response(status)
                handler.send_header("Content-Type", "application/json" if isinstance(value, dict) else "text/html")
                handler.send_header("Content-Length", str(len(body)))
                handler.send_header("X-Request-Id", "9e2330b2-566e-91a1-b1bf-cb068c0e908c")
                handler.end_headers()
                handler.wfile.write(body)
            def log_message(self, *args):
                pass
        try:
            server = HTTPServer(("127.0.0.1", 0), Handler)
        except PermissionError:
            self.skipTest("local test server not permitted")
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        secret = self.root / "config/secrets/.secrets.env"
        secret.write_text("AI_API_KEY=synthetic-api-key\n")
        secret.chmod(0o600)
        (self.root / "config/config.toml").write_text(f'[ai]\nbase_url="http://127.0.0.1:{server.server_port}/v1"\nmodel="fixture-model"\n')
        return requests

    def test_ai_http_errors_are_actionable_and_never_echo_provider_body(self):
        cases = [
            ({"error": {"code": "AllocationQuota.FreeTierOnly", "message": "synthetic-api-key " + MASTER}}, "免费额度"),
            ({"code": "AccessDenied.Unpurchased", "message": "synthetic-api-key"}, "服务开通状态"),
            ({"error": {"code": "Policy.synthetic-api-key.Denied"}}, "未收到可识别"),
            ("<html>synthetic-api-key " + MASTER + "\x1b]52;c;bad\x07</html>", "未收到可识别"),
            ("synthetic-api-key" + "x"*17000, "未收到可识别"),
        ]
        for body, hint in cases:
            with self.subTest(hint=hint):
                requests = self.ai_server([(403, body)])
                t = Terminal(self, ["ai", "--id", "alice"])
                t.expect("主密码:")
                t.send(MASTER + "\r")
                t.expect("空行提交")
                t.send("保存一个测试账号\r\r")
                t.expect("HTTP 403 Forbidden")
                t.expect(hint)
                t.expect("输入已保留")
                t.send(b"q")
                output = t.finish()
                self.assertEqual(len(requests), 1)
                self.assertIn(b"9e2330b2-566e-91a1-b1bf-cb068c0e908c", output)
                self.assertNotIn(b"synthetic-api-key", output)
                self.assertNotIn(b"\x1b]52;c;bad", output)
                self.assertEqual(self.db("SELECT count(*) FROM items"), [(0,)])

    def test_ai_account_form_survives_empty_model_actions_and_links_local_password(self):
        draft = {"reply": "请提供账号信息", "items": [], "events": [], "secrets": []}
        requests = self.ai_server([(200, {"choices": [{"message": {"content": json.dumps(draft)}}]})])
        t = Terminal(self, ["ai", "--id", "alice"])
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("空行提交")
        t.send("存一个telegram的账号\r\r")
        t.expect("make item id='alice' cat=account key='telegram'")
        t.expect("填写账号 telegram")
        self.assertEqual(self.db("SELECT count(*) FROM items"), [(0,)])
        t.send(ENTER)
        t.expect("登录名 / 邮箱 / 手机号:")
        t.send("@synthetic_login\r")
        t.expect("备注（可空）:")
        t.send("本地备注\r")
        t.expect("make pass-add-manual id='alice' title='telegram' user='@synthetic_login'")
        t.expect("账号密码")
        t.send(ENTER)
        t.expect("新条目密码:")
        t.send(PASSWORD + "\r")
        t.expect("已关联密码")
        t.expect("空行提交")
        t.send("/quit\r\r")
        output = t.finish()
        self.assertEqual(self.db("SELECT identity,category,key,value,note FROM items"),
                         [("alice", "account", "telegram", "@synthetic_login", "本地备注")])
        self.assertEqual(self.db("SELECT entry_path FROM password_links"), [("telegram",)])
        self.assertEqual(len(requests), 1)
        self.assertNotIn("@synthetic_login", json.dumps(requests))
        self.assertNotIn("本地备注", json.dumps(requests, ensure_ascii=False))
        self.assertNotIn(PASSWORD, json.dumps(requests))
        self.assertNotIn(PASSWORD.encode(), output)
        self.assertEqual(output.count("主密码:".encode()), 1)
        self.assertEqual(output.count(b"\x1b[?1049h"), 1)
        self.assertEqual(output.count(b"\x1b[?1049l"), 1)
        self.assertIn("telegram", self.keepass("alice", "ls"))
        self.assertNotIn("telegram", self.keepass("bob", "ls"))

    def test_ai_account_action_can_skip_password_and_will_not_overwrite(self):
        draft = {"reply": "填写账号", "items": [], "events": [], "secrets": [],
                 "actions": [{"identity": "alice", "action": "account-add", "title": "Custom Service"}]}
        requests = self.ai_server([(200, {"choices": [{"message": {"content": json.dumps(draft)}}]})])
        t = Terminal(self, ["ai", "--id", "alice"])
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("空行提交")
        t.send("新增一条账号资料\r\r")
        t.expect("填写账号 Custom Service")
        t.send(ENTER)
        t.expect("登录名 / 邮箱 / 手机号:")
        t.send("tester@example.com\r")
        t.expect("备注（可空）:")
        t.send(ENTER)
        t.expect("账号密码")
        t.send(DOWN*2 + ENTER)
        t.expect("空行提交")
        t.send("再新增一次\r\r")
        t.expect("填写账号 Custom Service")
        t.send(ENTER)
        t.expect("已有同名账号，未覆盖")
        t.expect("空行提交")
        t.send("/quit\r\r")
        output = t.finish()
        self.assertEqual(self.db("SELECT identity,key,value,note FROM items"),
                         [("alice", "Custom Service", "tester@example.com", "")])
        self.assertEqual(self.db("SELECT count(*) FROM password_links"), [(0,)])
        self.assertNotIn("Custom Service", self.keepass("alice", "ls"))
        self.assertEqual(len(requests), 2)
        self.assertNotIn("tester@example.com", json.dumps(requests))
        self.assertNotIn(PASSWORD.encode(), output)

    def test_ai_account_form_rejects_empty_login_and_concurrent_duplicate(self):
        draft = {"reply": "填写账号", "actions": [{"action": "account-add", "title": "Telegram"}]}
        self.ai_server([(200, {"choices": [{"message": {"content": json.dumps(draft)}}]})])
        t = Terminal(self, ["ai", "--id", "alice"])
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("空行提交")
        t.send("存一个Telegram的账号\r\r")
        t.expect("填写账号 Telegram")
        t.send(ENTER)
        t.expect("登录名 / 邮箱 / 手机号:")
        t.send(ENTER)
        t.expect("账号为空，未保存")
        self.assertEqual(self.db("SELECT count(*) FROM items"), [(0,)])
        t.expect("空行提交")
        t.send("存一个Telegram的账号\r\r")
        t.expect("填写账号 Telegram")
        t.send(ENTER)
        t.expect("登录名 / 邮箱 / 手机号:")
        self.item(key="Telegram", category="account", note="另一个窗口保存的资料")
        t.send("replacement\r")
        t.expect("备注（可空）:")
        t.send("不能覆盖\r")
        t.expect("已有同名账号，未覆盖")
        t.expect("空行提交")
        t.send("/quit\r\r")
        t.finish()
        self.assertEqual(self.db("SELECT value,note FROM items"),
                         [("test@example.com", "另一个窗口保存的资料")])
        self.assertEqual(self.db("SELECT count(*) FROM password_links"), [(0,)])

    def test_ai_error_keeps_input_and_reloads_settings_before_retry(self):
        draft = {"reply": "重试成功", "items": [], "events": [], "secrets": []}
        requests = self.ai_server([
            (403, {"error": {"code": "AccessDenied", "message": "private provider details"}}),
            (200, {"choices": [{"message": {"content": json.dumps(draft)}}]}),
        ])
        t = Terminal(self, ["ai", "--id", "alice"])
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("空行提交")
        t.send("保存一个测试账号\r\r")
        t.expect("输入已保留")
        t.send(DOWN + ENTER)
        t.expect("ID 设置")
        t.send(DOWN*2 + ENTER)
        t.expect("AI 模型（Enter 保留）:")
        t.send("updated-fixture-model\r")
        t.expect("AI 模型          updated-fixture-model")
        t.send(DOWN*4 + ENTER)
        t.expect("设置已保存")
        t.expect("输入已保留")
        t.send(ENTER)
        t.expect("重试成功")
        t.expect("下一步")
        t.send(b"q")
        output = t.finish()
        self.assertEqual(len(requests), 2)
        self.assertEqual(requests[0]["messages"], requests[1]["messages"])
        self.assertEqual(requests[1]["model"], "updated-fixture-model")
        self.assertEqual(output.count("主密码:".encode()), 1)
        self.assertNotIn(b"synthetic-api-key", output)

    def test_ai_error_can_return_to_input_without_retry(self):
        requests = self.ai_server([(403, {"error": {"code": "AccessDenied"}})])
        t = Terminal(self, ["ai", "--id", "alice"])
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("空行提交")
        t.send("保存一个测试账号\r\r")
        t.expect("输入已保留")
        t.send(DOWN*2 + ENTER)
        t.expect("空行提交")
        t.send("/quit\r\r")
        t.finish()
        self.assertEqual(len(requests), 1)

    def test_ai_fullscreen_hides_details_and_keeps_multiline_editor_when_resized(self):
        draft = {"reply": "界面测试", "items": [], "events": [], "secrets": [],
                 "zz_debug": "RAW_JSON_MARKER"}
        response = {"choices": [{"message": {"content": json.dumps(draft),
                                              "reasoning_content": "THINKING_MARKER"}}]}
        requests = self.ai_server([(200, response)])
        t = Terminal(self, ["ai", "--id", "alice"])
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("空行提交")
        t.send("保存测试AC")
        t.send(b"\x1b[D\x7fB\x1b[C\r")
        fcntl.ioctl(t.slave, termios.TIOCSWINSZ, struct.pack("HHHH", 18, 40, 0, 0))
        t.send("第二行备注\r\r")
        t.expect("下一步")
        self.assertEqual(requests[0]["messages"][-1]["content"], "保存测试BC\n第二行备注")
        self.assertNotIn(b"RAW_JSON_MARKER", t.output)
        self.assertNotIn(b"THINKING_MARKER", t.output)
        t.send(b"\x1bOQ")
        t.expect("F2 返回对话")
        t.expect("RAW_JSON_MARKER")
        t.send(b"\x1b[5~"*6)
        t.expect("THINKING_MARKER")
        t.send(b"\x1bOQ")
        t.expect("界面测试")
        t.send("q")
        output = t.finish()
        self.assertEqual(len(requests), 1)
        self.assertEqual(output.count(b"\x1b[?1049h"), 1)
        self.assertEqual(output.count(b"\x1b[?1049l"), 1)

    def test_ai_conversation_scroll_and_ctrl_c_restore_terminal(self):
        draft = {"reply": "SCROLL_TOP\n" + "\n".join(f"第{i}行回复" for i in range(35)) + "\nSCROLL_END"}
        self.ai_server([(200, {"choices": [{"message": {"content": json.dumps(draft)}}]})])
        t = Terminal(self, ["ai", "--id", "alice"])
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("空行提交")
        t.send("测试滚动\r\r")
        t.expect("下一步")
        self.assertNotIn(b"SCROLL_TOP", t.output)
        t.send(b"\x1b[5~"*6)
        t.expect("SCROLL_TOP")
        t.send(b"\x1b[6~"*6)
        t.expect("SCROLL_END")
        t.send(ENTER)
        t.expect("空行提交")
        t.send(b"\x03")
        output = t.finish()
        self.assertEqual(output.count(b"\x1b[?1049h"), 1)
        self.assertEqual(output.count(b"\x1b[?1049l"), 1)

    def test_ai_clipboard_plaintext_fallback_waits_and_is_not_added_to_conversation(self):
        self.add()
        draft = {"reply": "本地查看", "secrets": [{"title": "GitHub", "action": "copy"}]}
        requests = self.ai_server([(200, {"choices": [{"message": {"content": json.dumps(draft)}}]})])
        config = self.root / "config/config.toml"
        config.write_text(config.read_text() + "\n[terminal]\nallow_plaintext=true\n")
        t = Terminal(self, ["ai", "--id", "alice"], env={"SSH_CONNECTION": "synthetic-ssh"})
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("粘贴测试文字并回车")
        t.send(ENTER)
        t.expect("空行提交")
        t.send("查看我的GitHub密码\r\r")
        t.expect("下一步")
        t.send(ENTER)
        t.expect("剪贴板不可用，是否明文显示密码")
        t.send(DOWN + ENTER)
        t.expect("再次验证主密码:")
        t.send(MASTER + "\r")
        t.expect(PASSWORD)
        t.expect("按 Enter 返回 AI:")
        start = t.position
        t.send(ENTER)
        t.expect("空行提交")
        t.send("/quit\r\r")
        output = t.finish()
        self.assertNotIn(PASSWORD.encode(), output[start:])
        self.assertNotIn(PASSWORD, json.dumps(requests))
        self.assertEqual(output.count(b"\x1b[?1049h"), 1)
        self.assertEqual(output.count(b"\x1b[?1049l"), 1)
