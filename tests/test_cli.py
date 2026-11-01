"""Exercise the compiled CLI using disposable data and real KeePassXC, never user vaults."""
import base64
from contextlib import closing
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
import struct
import subprocess
import tempfile
import termios
import time
import unittest
import zipfile
import xml.etree.ElementTree as ET
from xml.sax.saxutils import escape

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug/id"
MASTER = "synthetic-test-master"
PASSWORD = "synthetic-test-entry"
DOWN = b"\x1b[B"
ENTER = b"\r"


class Terminal:
    def __init__(self, case, args, env=None, width=100, height=24, command=None):
        self.case = case
        self.fd, self.slave = pty.openpty()
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
        self.before = termios.tcgetattr(self.slave)
        self.child = subprocess.Popen(command or [str(BIN), "--root", str(case.root), *args],
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
        (self.root / "template").mkdir()
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

    def item(self, key="职位", note="备注", category="work", identity="alice", value=None):
        t=Terminal(self,["item","--id",identity,"--cat",category,"--key",key,
                         "--value",value or ("test@example.com" if category=="account" else "工程师"),"--note",note])
        t.expect("主密码:");t.send(MASTER+"\r");t.expect("已保存");t.finish()

    def entries(self, identity="alice"):
        root=ET.fromstring(self.keepass(identity,"export"))
        recycle=root.findtext('./Meta/RecycleBinUUID')
        def collect(group):
            if recycle and group.findtext('UUID')==recycle:return []
            return group.findall('Entry')+[e for child in group.findall('Group') for e in collect(child)]
        return collect(root.find('./Root/Group'))

    def values(self, entry):
        return {s.findtext("Key"):s.findtext("Value","") for s in entry.findall("String")}


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
        self.assertEqual(self.keepass("alice", "show", ["-s", "-a", "Password", "ID/账号/GitHub"]).rstrip("\n"), PASSWORD)
        t = Terminal(self, ["manage", "--id", "alice", "--type", "password"])
        t.expect("主密码:")
        t.send("wrong-fixture-master\r")
        t.expect("解锁失败")
        output = t.finish(1)
        self.assertNotIn(b"wrong-fixture-master", output)



    def test_sensitive_edit_requires_master_again(self):
        self.add()
        t = Terminal(self, ["manage", "--id", "alice", "--type", "password"])
        t.expect("主密码:")
        t.send(MASTER + "\r")
        t.expect("Enter 展开")
        t.send(ENTER + b"o")
        t.expect("设置密码")
        t.send(DOWN*3+ENTER)
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
        legacy=self.root / 'template/data';legacy.mkdir()
        (legacy/'obsolete').write_text('Synthetic obsolete template data')
        t = Terminal(self, ["new", "--id", "charlie"])
        t.expect("设置主密码:");t.send(MASTER + "\r")
        t.expect("再次输入主密码:");t.send(MASTER + "\r")
        t.expect("基础信息已保存到 ID_info/charlie", timeout=30);output=t.finish()
        self.assertNotIn(b"synthetic-initial", output)
        vault = self.root / "identities/charlie/passwords/charlie.kdbx"
        self.assertFalse((vault.parent.parent/'data').exists())
        self.keepass('charlie','ls',data='synthetic-initial\n',code=1)
        root=ET.fromstring(self.keepass('charlie','export'))
        group=next(g for g in root.findall('./Root/Group/Group') if g.findtext('Name')=='ID_info')
        basic=group.find('Entry');original=basic.findtext('UUID')
        self.assertEqual(self.values(basic)['Title'],'charlie')
        self.assertEqual(self.values(basic)['身份名'],'charlie')
        self.assertEqual(self.values(basic)['Password'],'')
        self.assertNotIn(MASTER,ET.tostring(root,encoding='unicode'))
        self.item(identity='charlie',key='姓名',category='id-info',value='Synthetic')
        entries=self.entries('charlie');self.assertEqual(len(entries),1)
        self.assertEqual(entries[0].findtext('UUID'),original)
        self.assertEqual(self.values(entries[0])['姓名'],'Synthetic')
        t=Terminal(self,['manage','--id','charlie'])
        t.expect('主密码:');t.send(MASTER+'\r');t.expect('ID_info');t.send(ENTER)
        t.expect('身份名');t.send('q');t.finish()
        before = vault.read_bytes()  # Encrypted synthetic fixture only.
        self.run_cli("new", "--id", "charlie")
        self.assertEqual(vault.read_bytes(), before)
        self.assertEqual(vault.stat().st_mode & 0o777, 0o600)

    def test_new_identity_rejects_empty_mismatched_or_cancelled_password_without_files(self):
        for first,second in [('', ''),(MASTER,'synthetic-mismatch')]:
            t=Terminal(self,['new','--id','charlie'])
            t.expect('设置主密码:');t.send(first+'\r')
            t.expect('再次输入主密码:');t.send(second+'\r')
            t.expect('未创建身份');output=t.finish(1)
            if second:self.assertNotIn(second.encode(),output)
            self.assertFalse((self.root/'identities/charlie').exists())
        t=Terminal(self,['new'])
        t.expect('新身份名:');t.send('charlie\r');t.expect('设置主密码:');t.send(MASTER+'\r')
        t.expect('再次输入主密码:');t.send(b'\x03');t.expect('已取消');t.finish(130)
        self.assertFalse((self.root/'identities/charlie').exists())

    def test_tag_rebuild_matches_escaped_titles_preserves_password_and_attachment(self):
        self.add("A & B")
        attachment = self.root / "test-attachment.txt"
        attachment.write_text("synthetic attachment")
        self.keepass("alice", "attachment-import", ["A & B", "test.txt", str(attachment)])
        t = Terminal(self, ["tag", "--id", "alice", "--title", "A & B", "--tags", "ai-deny"])
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
        self.assertEqual(self.entries(), [])

    def set_tags(self, title, tags):
        t = Terminal(self, ["tag", "--id", "alice", "--title", title, "--tags", tags])
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
                t.expect("将保留加密备份")
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
                self.assertEqual(self.entries(), [])




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



    def test_ai_tools_cannot_execute_commands_and_repeated_queries_are_bounded(self):
        for name, expected, rounds in [("shell", "不支持的工具", 1), ("manage", "查询次数过多", 5)]:
            with self.subTest(name=name):
                tool = {"role": "assistant", "content": None, "tool_calls": [{
                    "id": "manage-1", "type": "function", "function": {"name": name,
                        "arguments": json.dumps({"type": "info"})}}]}
                requests = self.ai_server([(200, {"choices": [{"message": tool}]})])
                t = Terminal(self, ["ai", "--id", "alice"])
                t.expect("主密码:")
                t.send(MASTER + "\r")
                t.expect("空行提交")
                t.send("查一下现有资料\r\r")
                t.expect(expected, timeout=30)
                t.expect("输入已保留")
                t.send("q")
                output = t.finish()
                self.assertEqual(len(requests), rounds)
                self.assertNotIn(PASSWORD.encode(), output)
                self.assertEqual(self.entries(), [])

    def test_native_info_account_icon_and_attachment_roundtrip_without_sqlite(self):
        self.add()
        attachment=self.root/'synthetic.pdf';attachment.write_bytes(b'%PDF synthetic recovery document')
        self.keepass('alice','attachment-import',['GitHub','recovery.pdf',str(attachment)])
        before=ET.fromstring(self.keepass('alice','export'))
        old_uuid=before.findtext('./Root/Group/Entry/UUID')
        self.item(key='姓名',category='id-info',value='Synthetic Name',note='完整备注\n第二行')
        self.item(key='生日',category='id-info',value='2000-01-01')
        self.item(key='电话号码',category='account',value='5550100')
        self.item(key='GitHub',category='account',value='tester',note='账号备注')
        entries=self.entries();self.assertEqual(len(entries),2)
        info=next(e for e in entries if self.values(e)['Title']=='基础信息')
        self.assertEqual(self.values(info)['姓名'],'Synthetic Name')
        self.assertEqual(self.values(info)['电话号码'],'5550100')
        self.assertEqual(self.values(info)['Password'],'')
        account=next(e for e in entries if self.values(e)['Title']=='GitHub')
        self.assertEqual(account.findtext('UUID'),old_uuid)
        self.assertEqual(self.values(account)['Password'],PASSWORD)
        self.assertEqual(account.findtext('Binary/Key'),'recovery.pdf')
        self.assertFalse((self.root/'data/id.db').exists())
        target=self.root/'roundtrip.pdf';self.keepass('alice','attachment-export',['GitHub','recovery.pdf',str(target)])
        self.assertEqual(target.read_bytes(),attachment.read_bytes())
        self.assertTrue(account.findall('./History/Entry'))
        self.assertTrue(any(self.values(h).get('Password')==PASSWORD for h in account.findall('./History/Entry')))
        backups=list((self.root/'backups/vaults/alice').glob('*.kdbx'))
        self.assertGreaterEqual(len(backups),4)
        self.assertTrue(all(p.stat().st_mode&0o777==0o600 for p in backups))

    def test_manage_icons_full_notes_and_local_protected_fields(self):
        self.item(key='姓名',category='id-info',value='Synthetic',note='字段备注\n完整内容')
        t=Terminal(self,['manage','--id','alice','--type','info'])
        t.expect('主密码:');t.send(MASTER+'\r');t.expect('Enter 展开');t.send(ENTER)
        t.expect('完整内容');t.send('o');t.expect('修改图标');t.send(DOWN*5+ENTER)
        t.expect('条目图标');t.send(DOWN*4+ENTER);t.expect('Enter 收起')
        t.send('o');t.expect('添加字段');t.send(DOWN*2+ENTER)
        t.expect('字段名称:');t.send('恢复码\r');t.expect('再次验证主密码:');t.send(MASTER+'\r')
        t.expect('字段内容:');t.send('SYNTHETIC-RECOVERY-CODE\r')
        t.expect('维度:');t.send('security\r');t.expect('字段备注:');t.send('应急使用\r')
        t.expect('受保护');t.send('q');output=t.finish()
        self.assertNotIn(b'SYNTHETIC-RECOVERY-CODE',output)
        info=self.entries()[0];self.assertEqual(info.findtext('IconID'),'19')
        field=next(s.find('Value') for s in info.findall('String') if s.findtext('Key')=='恢复码')
        self.assertEqual(field.attrib.get('ProtectInMemory'),'True')
        self.assertEqual(field.text,'SYNTHETIC-RECOVERY-CODE')

    def test_delete_entry_keeps_encrypted_backup_and_other_identity(self):
        self.add();self.add(name='bob')
        t=Terminal(self,['manage','--id','alice'])
        t.expect('主密码:');t.send(MASTER+'\r');t.expect('Enter 展开');t.send(ENTER+b'x')
        t.expect('将保留加密备份');t.send(DOWN+ENTER);t.expect('再次验证主密码:');t.send(MASTER+'\r')
        t.expect('暂无记录');t.send('q');t.finish()
        self.assertEqual(self.entries(),[]);self.assertEqual(len(self.entries('bob')),1)
        backup=next((self.root/'backups/vaults/alice').glob('*.kdbx'))
        result=subprocess.run(['keepassxc-cli','ls','-q',str(backup)],input=MASTER+'\n',text=True,capture_output=True)
        self.assertEqual(result.returncode,0);self.assertIn('GitHub',result.stdout)

    def test_empty_password_account_is_visible_and_edit_reuses_same_entry(self):
        self.item(key='Telegram',category='account',value='tester')
        initial=self.entries()[0].findtext('UUID')
        t=Terminal(self,['pass-add-manual','--id','alice','--title','Telegram'])
        t.expect('主密码:');t.send(MASTER+'\r');t.expect('新条目密码:');t.send(PASSWORD+'\r');t.expect('密码已保存');t.finish()
        self.assertEqual(len(self.entries()),1);self.assertEqual(self.entries()[0].findtext('UUID'),initial)
        t=Terminal(self,['list','--id','alice']);t.expect('主密码:');t.send(MASTER+'\r');t.expect('已设置');t.send('3');t.expect('Enter 展开');t.send('q')
        self.assertEqual(t.finish().count('主密码:'.encode()),1)

    def test_kdbx_cli_ignores_legacy_database_and_has_no_migration_command(self):
        folder=self.root/'data';folder.mkdir();source=folder/'id.db'
        before=b'Synthetic obsolete data: not a valid database'
        source.write_bytes(before)
        self.item(key='姓名',category='id-info',value='Synthetic')
        self.assertEqual(source.read_bytes(),before)
        self.assertEqual(self.values(self.entries()[0])['姓名'],'Synthetic')
        help_text=self.run_cli('--help').stdout.lower()
        self.assertNotIn('migrate',help_text)
        self.assertNotIn('sqlite',help_text)
        self.run_cli('migrate',code=2)

    def test_ai_phone_draft_goes_to_basic_information_and_account_stays_native(self):
        draft={'reply':'待确认','items':[{'category':'account','key':'电话号码','value':'5550100'}, {'category':'account','key':'Telegram','value':'5550100'}]}
        self.ai_server([(200,{'choices':[{'message':{'content':json.dumps(draft)}}]})])
        t=Terminal(self,['ai','--id','alice']);t.expect('主密码:');t.send(MASTER+'\r');t.expect('空行提交')
        t.send('记录电话与账号\r\r');t.expect('草稿：contact / 电话号码');t.expect('下一步');t.send(ENTER)
        t.expect('空行提交');t.send('/quit\r\r');t.finish()
        entries=self.entries();self.assertEqual(len(entries),2)
        basic=next(e for e in entries if self.values(e)['Title']=='基础信息')
        self.assertEqual(self.values(basic)['电话号码'],'5550100')
        account=next(e for e in entries if self.values(e)['Title']=='Telegram')
        self.assertEqual(self.values(account)['UserName'],'5550100')

    def test_ai_manage_queries_only_public_data_and_password_presence(self):
        self.add();self.item(key='Telegram',category='account',value='tester');self.item()
        self.add('PRIVATE_BOB',name='bob')
        local=Terminal(self,['manage','--id','alice','--type','info'])
        local.expect('主密码:');local.send(MASTER+'\r');local.expect('Enter 展开');local.send(ENTER+b'o')
        local.expect('添加字段');local.send(DOWN*2+ENTER);local.expect('字段名称:');local.send('恢复码\r')
        local.expect('再次验证主密码:');local.send(MASTER+'\r');local.expect('字段内容:');local.send('SYNTHETIC-RECOVERY\r')
        local.expect('维度:');local.send('security\r');local.expect('字段备注:');local.send(ENTER)
        local.expect('受保护');local.send('q');local.finish()
        tool={'role':'assistant','content':None,'tool_calls':[{'id':'m1','type':'function','function':{'name':'manage','arguments':json.dumps({'type':'all'})}}]}
        requests=self.ai_server([(200,{'choices':[{'message':tool}]}),(200,{'choices':[{'message':{'content':json.dumps({'reply':'查询完成'})}}]})])
        t=Terminal(self,['ai','--id','alice']);t.expect('主密码:');t.send(MASTER+'\r');t.expect('空行提交')
        t.send('哪些账号还没有密码\r\r');t.expect('正在查询身份管理');t.expect('下一步');t.send('q');output=t.finish()
        data=json.loads(requests[1]['messages'][-1]['content'])
        self.assertEqual({r['name']:r['password_status'] for r in data['rows'] if r['category']=='account'},{'GitHub':'已设置','Telegram':'未填密码'})
        self.assertTrue(any(r.get('key')=='职位' for r in data['rows']))
        for secret in [MASTER,PASSWORD,'PRIVATE_BOB','SYNTHETIC-RECOVERY']:self.assertNotIn(secret,json.dumps(requests))
        self.assertNotIn(PASSWORD.encode(),output)

    def test_ai_account_form_creates_one_local_entry_and_keeps_password_local(self):
        requests=self.ai_server([(200,{'choices':[{'message':{'content':json.dumps({'reply':'填写账号'})}}]})])
        t=Terminal(self,['ai','--id','alice']);t.expect('主密码:');t.send(MASTER+'\r');t.expect('空行提交')
        t.send('存一个telegram的账号\r\r');t.expect('填写账号 telegram');t.send(ENTER)
        t.expect('登录名 / 邮箱 / 手机号:');t.send('@synthetic_login\r');t.expect('备注（可空）:');t.send('本地备注\r')
        t.expect('账号密码');t.send(ENTER);t.expect('新条目密码:');t.send(PASSWORD+'\r');t.expect('空行提交');t.send('/quit\r\r')
        output=t.finish();self.assertEqual(len(self.entries()),1)
        values=self.values(self.entries()[0]);self.assertEqual(values['UserName'],'@synthetic_login');self.assertEqual(values['Password'],PASSWORD)
        for secret in [PASSWORD,'@synthetic_login','本地备注']:self.assertNotIn(secret,json.dumps(requests,ensure_ascii=False))
        self.assertNotIn(PASSWORD.encode(),output)

    def test_concurrent_vault_update_is_not_overwritten(self):
        self.item(key='Telegram',category='account',value='tester')
        t=Terminal(self,['pass-add-manual','--id','alice','--title','Telegram'])
        t.expect('主密码:');t.send(MASTER+'\r');t.expect('新条目密码:')
        self.add('Concurrent change')
        t.send('SYNTHETIC-UNSAVED\r')
        t.expect('已有新版本');output=t.finish(1)
        self.assertNotIn(b'SYNTHETIC-UNSAVED',output)
        values=[self.values(e) for e in self.entries()]
        self.assertEqual(next(v for v in values if v['Title']=='Telegram')['Password'],'')
        self.assertTrue(any(v['Title']=='Concurrent change' for v in values))

    def test_make_arguments_are_data_not_shell_code(self):
        payload="O'Reilly; `touch SHOULD-NOT-EXIST`; $(touch SHOULD-NOT-EXIST)"
        t=Terminal(self,[],env={'ID_ROOT':str(self.root)},command=['make','--no-print-directory','item','id=alice','cat=work','key=test','value='+payload,'note='])
        t.expect('主密码:');t.send(MASTER+'\r');t.expect('已保存');t.finish()
        self.assertEqual(self.values(self.entries()[0])['test'],payload)
        self.assertFalse((ROOT/'SHOULD-NOT-EXIST').exists())
        self.run_cli('item','--id','../bob','--cat','work','--key','x','--value','x','--note','',code=1)

    def test_spreadsheet_import_encrypts_private_columns_and_skips_unknown_owner(self):
        self.add()
        self.keepass('alice','edit',['GitHub','-u','test@example.com'])
        headers=['分类','服务商','使用者','账号','备注','密码备注','详细数据']
        rows=[headers,['国外','Proton','alice','test@example.com','SYNTHETIC-PURPOSE','SYNTHETIC-HINT','SYNTHETIC-DETAIL'],['国内','163','unknown','other@example.com','','','']]
        xml='<worksheet><sheetData>'
        for number,row in enumerate(rows,1):
            xml+=f'<row r="{number}">'+''.join(f'<c r="{chr(65+i)}{number}" t="inlineStr"><is><t>{escape(v)}</t></is></c>' for i,v in enumerate(row))+'</row>'
        xml+='</sheetData></worksheet>'
        source=self.root/'synthetic.xlsx'
        with zipfile.ZipFile(source,'w') as z:z.writestr('xl/worksheets/sheet1.xml',xml)
        for _ in range(2):
            t=Terminal(self,['import',str(source)]);t.expect('主密码:');t.send(MASTER+'\r');t.expect('导入完成');output=t.finish()
            self.assertNotIn(b'SYNTHETIC-HINT',output)
        entries=self.entries();self.assertEqual(len(entries),2)
        self.assertEqual(self.values(next(e for e in entries if self.values(e)['Title']=='GitHub'))['Password'],PASSWORD)
        mail=next(e for e in entries if self.values(e)['Title']=='Proton')
        self.assertEqual(self.values(mail)['UserName'],'test@example.com')
        self.assertEqual(mail.findtext('IconID'),'19')
        for field in ['原表备注','密码备注','详细数据']:
            node=next(s.find('Value') for s in mail.findall('String') if s.findtext('Key')==field)
            self.assertEqual(node.attrib.get('ProtectInMemory'),'True')
        self.assertEqual(self.entries('bob'),[])
        self.assertFalse((self.root/'data/id.db').exists())
        self.assertFalse((self.root/'backups/mail-imports').exists())

    def test_delete_field_preserves_other_native_fields(self):
        self.item(key='姓名',category='id-info',value='Synthetic')
        self.item(key='生日',category='id-info',value='2000-01-01')
        t=Terminal(self,['manage','--id','alice','--type','info'])
        t.expect('主密码:');t.send(MASTER+'\r');t.expect('Enter 展开');t.send(ENTER+b'f')
        t.expect('删除字段');t.send(ENTER);t.expect('删除 基础信息 的');t.send(DOWN+ENTER)
        t.expect('再次验证主密码:');t.send(MASTER+'\r');t.expect('Enter 收起');t.send('q');t.finish()
        values=self.values(self.entries()[0]);self.assertNotIn('姓名',values);self.assertEqual(values['生日'],'2000-01-01')

    def test_existing_group_account_is_updated_in_place(self):
        self.keepass('alice','mkdir',['Accounts'])
        self.add('Accounts/GitHub')
        initial=self.entries_in_group('Accounts')[0].findtext('UUID')
        self.item(key='github',category='account',value='updated-user')
        grouped=self.entries_in_group('Accounts');self.assertEqual(len(grouped),1)
        self.assertEqual(grouped[0].findtext('UUID'),initial)
        self.assertEqual(self.values(grouped[0])['UserName'],'updated-user')
        self.assertEqual(self.values(grouped[0])['Password'],PASSWORD)
        self.assertEqual(len(self.entries()),1)

    def entries_in_group(self,name):
        root=ET.fromstring(self.keepass('alice','export'))
        return next(g for g in root.findall('.//Group') if g.findtext('Name')==name).findall('Entry')

    def legacy_root_fixture(self):
        self.add()
        self.add('Keep Root')
        self.keepass('alice','mkdir',['Work']);self.add('Work/Keep Work')
        self.item(key='姓名',category='id-info',value='Synthetic')
        root=ET.fromstring(self.keepass('alice','export'))
        parent=root.find('./Root/Group')
        project=next(g for g in parent.findall('Group') if g.findtext('Name')=='ID')
        info=next(g for g in project.findall('Group') if g.findtext('Name')=='身份信息')
        foundation=info.find('Entry');info.remove(foundation);parent.append(foundation);parent.remove(project)
        github=next(e for e in parent.findall('Entry') if self.values(e)['Title']=='GitHub')
        data=github.find('CustomData')
        if data is None:data=ET.SubElement(github,'CustomData')
        marker=ET.SubElement(data,'Item');ET.SubElement(marker,'Key').text='ID.LocalTransfer:alice:item:1'
        ET.SubElement(marker,'Value').text='done'
        source=self.root/'synthetic-legacy.xml';ET.ElementTree(root).write(source,encoding='utf-8')
        imported=self.root/'legacy.kdbx'
        result=subprocess.run(['keepassxc-cli','import','-q','-p',str(source),str(imported)],
                              input=MASTER+'\n'+MASTER+'\n',text=True,capture_output=True)
        self.assertEqual(result.returncode,0,'Synthetic legacy fixture import failed')
        vault=self.root/'identities/alice/passwords/alice.kdbx';imported.replace(vault);vault.chmod(0o600)
        return {self.values(e)['Title']:e.findtext('UUID') for e in self.entries()}

    def test_organize_legacy_entries_preserves_native_data_and_is_idempotent(self):
        ids=self.legacy_root_fixture()
        source=self.root/'synthetic-recovery.pdf';source.write_bytes(b'%PDF synthetic recovery document')
        self.keepass('alice','attachment-import',['GitHub','recovery.pdf',str(source)])
        self.keepass('alice','edit',['GitHub','-u','updated-user'])
        before=next(e for e in self.entries() if self.values(e)['Title']=='GitHub')
        history={e.findtext('Times/LastModificationTime') for e in before.findall('History/Entry')}
        self.assertTrue(history)
        t=Terminal(self,['organize','--id','alice'])
        t.expect('主密码:');t.send(MASTER+'\r');t.expect('整理 2 项项目资料');t.send(DOWN+ENTER)
        t.expect('已整理 2 项项目资料',timeout=30);output=t.finish()
        self.assertNotIn(PASSWORD.encode(),output)
        account=self.entries_in_group('账号')[0]
        self.assertEqual(account.findtext('UUID'),ids['GitHub'])
        self.assertEqual(self.values(account)['Password'],PASSWORD)
        self.assertEqual(self.values(account)['UserName'],'updated-user')
        self.assertTrue(history.issubset({e.findtext('Times/LastModificationTime') for e in account.findall('History/Entry')}))
        self.assertEqual(self.entries_in_group('ID_info')[0].findtext('UUID'),ids['基础信息'])
        self.assertEqual(self.values(self.entries_in_group('ID_info')[0])['姓名'],'Synthetic')
        parent=ET.fromstring(self.keepass('alice','export')).find('./Root/Group')
        self.assertEqual([self.values(e)['Title'] for e in parent.findall('Entry')],['Keep Root'])
        self.assertEqual(self.entries_in_group('Work')[0].findtext('UUID'),ids['Keep Work'])
        extracted=self.root/'recovered.pdf'
        self.keepass('alice','attachment-export',['ID/账号/GitHub','recovery.pdf',str(extracted)])
        self.assertEqual(extracted.read_bytes(),source.read_bytes())
        self.assertTrue(list((self.root/'backups/vaults/alice').glob('*.kdbx')))
        vault=self.root/'identities/alice/passwords/alice.kdbx';unchanged=vault.read_bytes()
        t=Terminal(self,['organize','--id','alice'])
        t.expect('主密码:');t.send(MASTER+'\r');t.expect('无需整理');t.finish()
        self.assertEqual(vault.read_bytes(),unchanged)
        self.assertEqual(self.entries('bob'),[])

    def test_organize_cancel_and_collision_keep_encrypted_original(self):
        self.legacy_root_fixture()
        vault=self.root/'identities/alice/passwords/alice.kdbx';before=vault.read_bytes()
        t=Terminal(self,['organize','--id','alice'])
        t.expect('主密码:');t.send(MASTER+'\r');t.expect('整理 2 项项目资料');t.send(ENTER);t.finish()
        self.assertEqual(vault.read_bytes(),before)
        self.keepass('alice','mkdir',['ID']);self.keepass('alice','mkdir',['ID/账号'])
        self.add('ID/账号/GitHub',password='synthetic-other-entry')
        before=vault.read_bytes()
        t=Terminal(self,['organize','--id','alice'])
        t.expect('主密码:');t.send(MASTER+'\r')
        t.expect('目标群组已有同名条目');t.finish(1)
        self.assertEqual(vault.read_bytes(),before)

    def test_native_project_groups_and_gui_rename_preserve_uuid(self):
        self.item(key='姓名',category='id-info',value='Synthetic')
        self.item(key='GitHub',category='account',value='tester')
        t=Terminal(self,['event','--id','alice','--title','Synthetic event','--detail','Synthetic detail'])
        t.expect('主密码:');t.send(MASTER+'\r');t.expect('已记录');t.finish()
        root=ET.fromstring(self.keepass('alice','export'))
        group=next(g for g in root.findall('./Root/Group/Group') if g.findtext('Name')=='ID')
        self.assertEqual({g.findtext('Name') for g in group.findall('Group')},{'身份信息','账号','事件'})
        info=next(g for g in group.findall('Group') if g.findtext('Name')=='身份信息')
        foundation=info.find('Entry');old_uuid=foundation.findtext('UUID')
        group.find('Name').text='My ID';info.find('Name').text='Profile'
        for field in foundation.findall('./CustomData/Item'):
            if field.findtext('Key')=='ID.Kind':foundation.find('CustomData').remove(field)
        source=self.root/'synthetic-rename.xml';ET.ElementTree(root).write(source,encoding='utf-8')
        imported=self.root/'renamed.kdbx'
        result=subprocess.run(['keepassxc-cli','import','-q','-p',str(source),str(imported)],input=MASTER+'\n'+MASTER+'\n',text=True,capture_output=True)
        self.assertEqual(result.returncode,0,'Synthetic GUI fixture import failed')
        vault=self.root/'identities/alice/passwords/alice.kdbx';imported.replace(vault);vault.chmod(0o600)
        self.item(key='生日',category='id-info',value='2000-01-01')
        self.item(key='Mail',category='account',value='synthetic@example.com')
        root=ET.fromstring(self.keepass('alice','export'))
        self.assertFalse(any(g.findtext('Name')=='ID' for g in root.findall('./Root/Group/Group')))
        info=self.entries_in_group('Profile');self.assertEqual(len(info),1)
        self.assertEqual(info[0].findtext('UUID'),old_uuid)
        self.assertEqual(self.values(info[0])['生日'],'2000-01-01')
        self.assertEqual(len(self.entries()),4)

    def test_organize_existing_native_info_group_preserves_uuid_fields_attachments_and_history(self):
        self.item(key='姓名',category='id-info',value='Synthetic')
        self.keepass('alice','mkdir',['Work']);self.add('Work/Keep Work')
        root=ET.fromstring(self.keepass('alice','export'))
        group=next(g for g in root.findall('.//Group') if g.findtext('Name')=='身份信息')
        group_uuid=group.findtext('UUID');foundation=group.find('Entry');entry_uuid=foundation.findtext('UUID')
        for item in foundation.findall('./CustomData/Item'):
            if item.findtext('Key')=='ID.Kind':foundation.find('CustomData').remove(item)
        field=ET.SubElement(foundation,'String');ET.SubElement(field,'Key').text='Recovery'
        ET.SubElement(field,'Value',ProtectInMemory='True').text='synthetic-recovery-code'
        source=self.root/'synthetic-info.xml';ET.ElementTree(root).write(source,encoding='utf-8')
        imported=self.root/'info.kdbx'
        result=subprocess.run(['keepassxc-cli','import','-q','-p',str(source),str(imported)],
                              input=MASTER+'\n'+MASTER+'\n',text=True,capture_output=True)
        self.assertEqual(result.returncode,0,'Synthetic info fixture import failed')
        vault=self.root/'identities/alice/passwords/alice.kdbx';imported.replace(vault);vault.chmod(0o600)
        attachment=self.root/'synthetic-info.pdf';attachment.write_bytes(b'%PDF synthetic info document')
        self.keepass('alice','attachment-import',['ID/身份信息/基础信息','recovery.pdf',str(attachment)])
        self.keepass('alice','edit',['ID/身份信息/基础信息','--notes','Synthetic history'])
        before=next(e for e in self.entries() if e.findtext('UUID')==entry_uuid)
        history={e.findtext('Times/LastModificationTime') for e in before.findall('History/Entry')}
        self.assertTrue(history)
        t=Terminal(self,['organize','--id','alice'])
        t.expect('主密码:');t.send(MASTER+'\r');t.expect('整理 1 项项目资料');t.send(DOWN+ENTER)
        t.expect('基础信息位于 ID_info/alice',timeout=30);output=t.finish()
        self.assertNotIn(b'synthetic-recovery-code',output)
        self.assertIn('alice',self.keepass('alice','ls',['ID_info']))
        root=ET.fromstring(self.keepass('alice','export'))
        group=next(g for g in root.findall('./Root/Group/Group') if g.findtext('Name')=='ID_info')
        self.assertEqual(group.findtext('UUID'),group_uuid)
        foundation=group.find('Entry');self.assertEqual(foundation.findtext('UUID'),entry_uuid)
        self.assertEqual(self.values(foundation)['Title'],'alice')
        self.assertEqual(self.values(foundation)['身份名'],'alice')
        self.assertEqual(self.values(foundation)['姓名'],'Synthetic')
        secret=next(s.find('Value') for s in foundation.findall('String') if s.findtext('Key')=='Recovery')
        self.assertEqual(secret.text,'synthetic-recovery-code');self.assertEqual(secret.attrib.get('ProtectInMemory'),'True')
        self.assertTrue(history.issubset({e.findtext('Times/LastModificationTime') for e in foundation.findall('History/Entry')}))
        self.assertEqual(self.keepass('alice','attachment-export',['--stdout','ID_info/alice','recovery.pdf','-']).encode(),attachment.read_bytes())
        self.assertEqual(self.values(self.entries_in_group('Work')[0])['Title'],'Keep Work')
        unchanged=vault.read_bytes()
        t=Terminal(self,['organize','--id','alice'])
        t.expect('主密码:');t.send(MASTER+'\r');t.expect('无需整理');t.finish()
        self.assertEqual(vault.read_bytes(),unchanged)
        self.assertEqual(self.entries('bob'),[])

    def test_organize_rejects_an_existing_different_info_group(self):
        self.item(key='姓名',category='id-info',value='Synthetic')
        self.keepass('alice','mkdir',['ID_info'])
        vault=self.root/'identities/alice/passwords/alice.kdbx';before=vault.read_bytes()
        t=Terminal(self,['organize','--id','alice'])
        t.expect('主密码:');t.send(MASTER+'\r');t.expect('目标群组 ID_info 已存在');t.finish(1)
        self.assertEqual(vault.read_bytes(),before)

    def test_grouped_password_short_title_and_explicit_existing_group(self):
        self.keepass('alice','mkdir',['Work'])
        self.item(key='Work/GitHub',category='account',value='tester')
        self.assertEqual(self.values(self.entries_in_group('Work')[0])['Title'],'GitHub')
        self.item(key='Mail',category='account',value='synthetic@example.com')
        t=Terminal(self,['pass-add-manual','--id','alice','--title','Mail'])
        t.expect('主密码:');t.send(MASTER+'\r');t.expect('新条目密码:');t.send(PASSWORD+'\r');t.expect('密码已保存');t.finish()
        t=Terminal(self,['grant','--id','alice','--title','Mail'])
        t.expect('主密码:');t.send(MASTER+'\r');t.expect('token:');output=t.finish()
        token=re.search(rb'token: ([0-9a-f]{32})',output)[1].decode()
        result=self.run_cli('use','--token',token,'--cmd','printf "%s" "$ID_SECRET"')
        self.assertEqual(result.stdout,'***')
        (self.root/'config/config.toml').write_text('[terminal]\nallow_plaintext = true\n')
        t=Terminal(self,['pass-show','--id','alice','--title','Mail'])
        t.expect('再次验证主密码:');t.send(MASTER+'\r');t.expect(PASSWORD);t.finish()
        self.assertEqual(len(self.entries()),2)

    def test_field_type_cancel_does_not_write_or_open_value_prompt(self):
        self.add();vault=self.root/'identities/alice/passwords/alice.kdbx';before=vault.read_bytes()
        t=Terminal(self,['manage','--id','alice'])
        t.expect('主密码:');t.send(MASTER+'\r');t.expect('Enter 展开');t.send(ENTER+b'o')
        t.expect('添加字段');t.send(DOWN*2+ENTER);t.expect('字段名称:');t.send('Nickname\r')
        t.expect('字段类型');t.send('q');t.expect('Enter 收起');t.send('q');output=t.finish()
        self.assertNotIn('字段内容:'.encode(),output);self.assertEqual(vault.read_bytes(),before)

    def test_edit_preserves_recycle_bin_contents_and_attachments(self):
        self.add('Deleted')
        source=self.root/'synthetic-trash.txt';source.write_text('SYNTHETIC-TRASH-ATTACHMENT')
        self.keepass('alice','attachment-import',['Deleted','old.txt',str(source)])
        self.keepass('alice','rm',['Deleted'])
        xml=ET.fromstring(self.keepass('alice','export'))
        recycle=xml.findtext('./Meta/RecycleBinUUID')
        trash=next(g for g in xml.findall('./Root/Group/Group') if g.findtext('UUID')==recycle)
        title=trash.findtext('Name')+'/Deleted'
        self.item()
        xml=ET.fromstring(self.keepass('alice','export'))
        self.assertEqual(xml.findtext('./Meta/RecycleBinUUID'),recycle)
        self.assertEqual(self.keepass('alice','attachment-export',['--stdout',title,'old.txt','-']),'SYNTHETIC-TRASH-ATTACHMENT')
        self.assertEqual([self.values(e)['Title'] for e in self.entries()],['基础信息'])
