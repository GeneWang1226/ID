import contextlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import shlex
import sqlite3
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET
from zipfile import ZipFile

sys.path.insert(0, str(Path(__file__).parents[1]))
spec = importlib.util.spec_from_file_location("account_store", Path(__file__).parents[1] / "account_store.py")
store = importlib.util.module_from_spec(spec)
spec.loader.exec_module(store)
PROJECT = store.ROOT
SECRET = "SYNTHETIC-SECRET-ONLY-FOR-TEST"


class Terminal(io.StringIO):
    def isatty(self):
        return True


def workbook(path, rows, *, formula=False, shared=False):
    sheet = ET.Element("worksheet", xmlns=store.NS["s"])
    data = ET.SubElement(sheet, "sheetData")
    strings = ET.Element("sst", xmlns=store.NS["s"])
    for number, values in enumerate([store.HEADERS, *rows], 1):
        row = ET.SubElement(data, "row", r=str(number))
        for column, text in zip("ABCDEFG", values):
            cell = ET.SubElement(row, "c", r=f"{column}{number}", t="s" if shared else "inlineStr")
            if shared:
                index = len(strings)
                ET.SubElement(ET.SubElement(strings, "si"), "t").text = text
                ET.SubElement(cell, "v").text = str(index)
            else:
                ET.SubElement(ET.SubElement(cell, "is"), "t").text = text
            if formula and number == 2 and column == "D":
                ET.SubElement(cell, "f").text = '"alice@example.com"'
    with ZipFile(path, "w") as archive:
        archive.writestr("xl/worksheets/sheet1.xml", ET.tostring(sheet))
        if shared:
            archive.writestr("xl/sharedStrings.xml", ET.tostring(strings))


class AccountStoreTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.patch = patch.object(store, "ROOT", self.root)
        self.patch.start()
        self.addCleanup(self.patch.stop)
        for owner in ("alice", "bob"):
            vault = self.root / "identities" / owner / "passwords" / f"{owner}.kdbx"
            vault.parent.mkdir(parents=True)
            vault.touch()
        schema = self.root / "template" / "data" / "schema.sql"
        schema.parent.mkdir(parents=True)
        schema.write_text((PROJECT / "template" / "data" / "schema.sql").read_text())
        self.source = self.root / "source.xlsx"
        self.rows = [["国内", "服务商", "Alice", "alice@example.com", "主账号", SECRET, "恢复信息：" + SECRET],
                     ["国外", "服务商", "待确认", "other@example.net", "", "", ""]]
        workbook(self.source, self.rows)

    def import_accounts(self, owners=None):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            store.import_accounts(self.source, owners or {})
        self.assertNotIn(SECRET, output.getvalue())
        return output.getvalue()

    def account(self):
        with contextlib.closing(sqlite3.connect(self.root / "data" / "id.db")) as db:
            return db.execute("SELECT identity,value,note FROM items ORDER BY identity").fetchall()

    def add_item(self, category, key, value, note="", identity="alice"):
        with contextlib.closing(sqlite3.connect(self.root / "data" / "id.db")) as db, db:
            return db.execute("INSERT INTO items(identity,category,key,value,note) VALUES(?,?,?,?,?)",
                              (identity, category, key, value, note)).lastrowid

    def test_import_is_idempotent_and_keeps_secrets_out_of_sqlite(self):
        for shared in (False, True):
            workbook(self.source, self.rows, shared=shared)
            output = self.import_accounts()
            self.import_accounts()
            rows = self.account()
            self.assertEqual(len(rows), 1)
            self.assertEqual(rows[0][:2], ("alice", "alice@example.com"))
            self.assertNotIn(SECRET, str(rows))
            self.assertIn("待确认", output)
            note = json.loads(rows[0][2])
            self.assertEqual(note["purpose"], "主账号")
            archive = self.root / note["source"]
            self.assertEqual(archive.stat().st_mode & 0o777, 0o600)
            self.assertEqual(archive.read_bytes(), self.source.read_bytes())

    def test_explicit_alias_and_backup_preserve_existing_records(self):
        self.import_accounts()
        with contextlib.closing(sqlite3.connect(self.root / "data" / "id.db")) as db, db:
            db.execute("INSERT INTO items(identity,category,key,value) VALUES('alice','preference','language','中文')")
        self.import_accounts({"待确认": "bob"})
        with contextlib.closing(sqlite3.connect(self.root / "data" / "id.db")) as db:
            self.assertEqual(db.execute("SELECT count(*) FROM items").fetchone()[0], 3)
        backup = sorted((self.root / "backups" / "mail-imports").glob("id-before-*.db"))[-1]
        with contextlib.closing(sqlite3.connect(backup)) as db:
            self.assertEqual(db.execute("SELECT value FROM items WHERE key='language'").fetchone()[0], "中文")

    def test_formula_and_invalid_identity_are_rejected_before_database_write(self):
        workbook(self.source, self.rows, formula=True)
        with self.assertRaises(store.AccountError):
            self.import_accounts()
        self.assertFalse((self.root / "data" / "id.db").exists())
        workbook(self.source, self.rows)
        with self.assertRaises(store.AccountError):
            self.import_accounts({"待确认": "../bob"})
        self.assertFalse((self.root / "data" / "id.db").exists())

    def test_duplicate_aliases_never_silently_overwrite_a_mailbox(self):
        duplicate = [*self.rows[0]]
        duplicate[2] = "别名"
        workbook(self.source, [self.rows[0], duplicate])
        with self.assertRaises(store.AccountError):
            self.import_accounts({"别名": "alice"})
        self.assertFalse((self.root / "data" / "id.db").exists())

    def test_only_selected_rows_secret_fields_are_extracted(self):
        self.rows[1][5] = "OTHER-OWNER-PRIVATE-NOTES"
        workbook(self.source, self.rows, shared=True)
        rows = store.read_sheet(self.source, secret_row=2)
        self.assertEqual(rows[0]["secret_fields"]["密码备注"], SECRET)
        self.assertNotIn("secret_fields", rows[1])
        self.assertNotIn("OTHER-OWNER-PRIVATE-NOTES", str(rows))

    def test_secret_attachment_uses_private_temporary_file_without_secret_argv(self):
        self.import_accounts()
        note = json.loads(self.account()[0][2])
        seen = []

        def run(command, **kwargs):
            self.assertNotIn(SECRET, repr(command) + repr(kwargs))
            self.assertEqual(command[1], "attachment-import")
            attachment = Path(command[-1])
            seen.append(attachment)
            self.assertEqual(attachment.stat().st_mode & 0o777, 0o600)
            self.assertTrue(attachment.is_relative_to(self.root))
            self.assertIn(SECRET, attachment.read_text())
            return SimpleNamespace(returncode=0)

        output = io.StringIO()
        with patch.object(store, "choose_action", return_value="attach"), patch.object(store.subprocess, "run", side_effect=run), contextlib.redirect_stdout(output):
            self.assertTrue(store.attach_source(Path("test.kdbx"), note["password_title"], "alice@example.com", note))
        self.assertNotIn(SECRET, output.getvalue())
        self.assertFalse(seen[0].exists())

    def test_source_mismatch_does_not_invoke_keepass(self):
        self.import_accounts()
        note = json.loads(self.account()[0][2])
        with patch.object(store.subprocess, "run") as run:
            with self.assertRaises(store.AccountError):
                store.attach_source(Path("test.kdbx"), "邮箱", "different@example.com", note)
            run.assert_not_called()

    def test_password_input_is_owned_by_keepass_and_never_captured(self):
        self.import_accounts()
        with (patch.object(store.sys, "stdin", Terminal()),
              patch.object(store.sys, "stdout", Terminal()),
              patch.object(store.shutil, "which", return_value="keepassxc-cli"),
              patch.object(store, "choose_action", side_effect=["0", "add", "exit"]),
              patch.object(store, "attach_source", return_value=True),
              patch.object(store.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as run):
            self.assertEqual(store.fill_passwords("alice"), 0)
        command = run.call_args.args[0]
        self.assertEqual(command[:5], ["keepassxc-cli", "add", "-p", "-u", "alice@example.com"])
        self.assertEqual(command[-1], "邮箱 - alice@example.com")
        self.assertEqual(run.call_args.kwargs, {"check": False})

    def test_failed_add_never_automatically_edits_existing_password(self):
        self.import_accounts()
        with (patch.object(store.sys, "stdin", Terminal()),
              patch.object(store.sys, "stdout", Terminal()),
              patch.object(store.shutil, "which", return_value="keepassxc-cli"),
              patch.object(store, "choose_action", side_effect=["0", "add", "exit"]),
              patch.object(store, "attach_source") as attach,
              patch.object(store.subprocess, "run", return_value=SimpleNamespace(returncode=1)) as run):
            self.assertEqual(store.fill_passwords("alice"), 0)
        run.assert_called_once()
        self.assertEqual(run.call_args.args[0][1], "add")
        attach.assert_not_called()

    def test_noninteractive_password_flow_refuses_before_unlock(self):
        with patch.object(store.sys.stdin, "isatty", return_value=False), patch.object(store.subprocess, "run") as run:
            with self.assertRaises(store.AccountError):
                store.fill_passwords(None)
            run.assert_not_called()

    def test_non_email_accounts_are_available_and_other_identities_are_excluded(self):
        self.import_accounts()
        with contextlib.closing(sqlite3.connect(self.root / "data" / "id.db")) as db, db:
            db.executemany("INSERT INTO items(identity,category,key,value,note) VALUES(?,?,?,?,?)", [
                ("alice", "account", "github-username", "alice-dev", "普通备注"),
                ("bob", "account", "steam", "bob-gamer", ""),
                ("alice", "preference", "language", "中文", ""),
            ])
        rows = store.account_rows("alice")
        self.assertEqual({row["username"] for row in rows}, {"alice@example.com", "alice-dev"})
        self.assertIn("github-username", {row["title"] for row in rows})

    def test_manual_entry_works_without_imported_accounts_or_database(self):
        with (patch.object(store.sys, "stdin", Terminal()), patch.object(store.sys, "stdout", Terminal()),
              patch.object(store.shutil, "which", return_value="keepassxc-cli"),
              patch.object(store, "choose_action", side_effect=["new", "exit"]),
              patch("builtins.input", side_effect=["SSH server", "alice"]),
              patch.object(store.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as run):
            self.assertEqual(store.fill_passwords("alice"), 0)
        self.assertEqual(run.call_args.args[0][-1], "SSH server")
        self.assertIn("alice", run.call_args.args[0])

    def test_explicit_title_keeps_existing_ai_command_working(self):
        with (patch.object(store.sys, "stdin", Terminal()), patch.object(store.sys, "stdout", Terminal()),
              patch.object(store.shutil, "which", return_value="keepassxc-cli"),
              patch.object(store, "choose_action") as choose,
              patch.object(store.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as run):
            self.assertEqual(store.fill_passwords("alice", "GitHub", "alice-dev"), 0)
        choose.assert_not_called()
        self.assertEqual(run.call_args.args[0][-1], "GitHub")
        self.assertEqual(run.call_args.kwargs, {"check": False})

    def test_edit_password_does_not_clear_existing_username(self):
        with patch.object(store.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as run:
            self.assertEqual(store.write_entry("alice", "Grouped/GitHub", None, edit=True), 0)
        self.assertEqual(run.call_args.args[0][1:3], ["edit", "-p"])
        self.assertNotIn("-u", run.call_args.args[0])

    def test_session_edit_reauthenticates_and_keeps_new_password_out_of_arguments(self):
        with (patch.object(store.sys, "stdin", Terminal()), patch.object(store.sys, "stdout", Terminal()),
              patch.object(store.getpass, "getpass", side_effect=["synthetic-master", SECRET]) as prompt,
              patch.object(store.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as run,
              store.vault_session.session()):
            self.assertEqual(store.write_entry("alice", "Grouped/GitHub", None, edit=True), 0)
        self.assertIn("再次验证", prompt.call_args_list[0].args[0])
        self.assertEqual(run.call_args.kwargs["input"], ("synthetic-master\n" + SECRET + "\n").encode())
        self.assertEqual(run.call_args.args[0][1:4], ["edit", "-q", "-p"])
        for call in run.call_args_list:
            self.assertNotIn(SECRET, str(call.args))
            self.assertNotIn("synthetic-master", str(call.args))

    def test_status_detection_only_requests_titles_and_keeps_unknown_separate(self):
        results = [SimpleNamespace(returncode=0, stdout=text) for text in (
            "Group/\nGroup/Empty\nFilled\nHidden\n", "/Group/Empty\n", "/Filled\n")]
        output = Terminal()
        with (patch.object(store.getpass, "getpass", return_value=SECRET),
              patch.object(store.subprocess, "run", side_effect=results) as run,
              contextlib.redirect_stdout(output)):
            states = store.password_states("alice")
        self.assertEqual(states, {"Filled": "已设置", "Group/Empty": "未填密码", "Hidden": "状态未知"})
        self.assertNotIn(SECRET, output.getvalue())
        for call in run.call_args_list:
            self.assertIn(call.args[0][1], ("ls", "search"))
            self.assertNotIn(SECRET, str(call.args))
            self.assertEqual(call.kwargs["input"], SECRET + "\n")

    def test_failed_unlock_never_reports_passwords_as_missing(self):
        with (patch.object(store.getpass, "getpass", return_value=SECRET),
              patch.object(store.subprocess, "run", return_value=SimpleNamespace(returncode=1)),
              contextlib.redirect_stdout(Terminal()) as output):
            with self.assertRaises(store.AccountError):
                store.list_passwords("alice")
        self.assertNotIn("未填密码", output.getvalue())

    def test_failed_session_query_invalidates_cached_unlock(self):
        with (store.vault_session.session() as session,
              patch.object(session, "lock") as lock,
              patch.object(store.vault_session, "run_cli", return_value=SimpleNamespace(returncode=1, stdout="", stderr=""))):
            with self.assertRaises(store.AccountError):
                store.password_states("alice")
            lock.assert_called_once()

    def test_list_keeps_accounts_personal_information_and_passwords_separate(self):
        self.import_accounts()
        self.add_item("work", "职位", "工程师", "远程办公")
        captured = []

        def choose(browser):
            captured.extend(browser.load(browser.view))
            return None

        with (patch.object(store, "password_states", return_value={"GitHub": "已设置", "服务器/空条目": "未填密码"}),
              patch.object(store.RecordBrowser, "choose", new=choose), contextlib.redirect_stdout(Terminal()) as output):
            self.assertEqual(store.list_records("alice"), 0)
        self.assertEqual([row["kind"] for row in captured], ["account", "password", "password", "info"])
        self.assertEqual(captured[0]["username"], "alice@example.com")
        self.assertEqual(captured[-1]["category"], "work")
        self.assertEqual(captured[-1]["remark"], "远程办公")
        self.assertNotIn("make pass-add-manual", output.getvalue())

    def test_info_filter_never_unlocks_or_requires_a_password_vault(self):
        self.import_accounts()
        self.add_item("preference", "语言", "中文")
        (self.root / "identities" / "alice" / "passwords" / "alice.kdbx").unlink()
        captured = []

        def choose(browser):
            captured.extend(browser.load(browser.view))
            return None

        with (patch.object(store, "password_states") as states, patch.object(store.RecordBrowser, "choose", new=choose),
              patch.object(store.shutil, "which", return_value=None), patch.object(store.sys, "stdin", Terminal()),
              patch.object(store.sys, "stdout", Terminal()),
              patch.object(store.sys, "argv", ["account_store.py", "list", "--identity", "alice", "--type", "info"])):
            self.assertEqual(store.main(), 0)
        states.assert_not_called()
        self.assertEqual({row["kind"] for row in captured}, {"info"})

    def test_accounts_use_only_explicit_links_for_password_status(self):
        self.import_accounts({"待确认": "bob"})
        rows = store.account_rows("alice")
        store.set_password_link("alice", rows[0]["id"], "Group/Filled")
        for key, path in (("empty", "Empty"), ("unknown", "Hidden"), ("missing", "Removed")):
            item_id = self.add_item("account", key, key + "-login")
            store.set_password_link("alice", item_id, path)
        self.add_item("account", "Same name", "unlinked-login")
        captured = []

        def choose(browser):
            captured.extend(browser.load(browser.view))
            return None

        with (patch.object(store, "password_states", return_value={"Group/Filled": "已设置", "Empty": "未填密码",
                                                                  "Hidden": "状态未知", "Same name": "已设置"}) as states,
              patch.object(store.RecordBrowser, "choose", new=choose)):
            store.list_records("alice", "account")
        self.assertEqual({row["username"]: row["status"] for row in captured}, {
            "alice@example.com": "已设置", "empty-login": "未填密码", "unknown-login": "状态未知",
            "missing-login": "条目不存在", "unlinked-login": "未关联"})
        self.assertEqual({row["kind"] for row in captured}, {"account"})
        states.assert_called_once_with("alice")

    def test_unlinked_accounts_do_not_require_a_vault(self):
        self.import_accounts()
        captured = []

        def choose(browser):
            captured.extend(browser.load(browser.view))
            return None

        with patch.object(store, "password_states") as states, patch.object(store.RecordBrowser, "choose", new=choose):
            store.list_records("alice", "account")
        states.assert_not_called()
        self.assertEqual(captured[0]["status"], "未关联")

    def test_personal_dimensions_filter_only_personal_information(self):
        self.import_accounts()
        self.add_item("work", "职位", "工程师")
        self.add_item("hobby", "乐器", "吉他")
        self.add_item("work", "职位", "其他身份的职位", identity="bob")
        captured = []

        def choose(browser):
            captured.extend(browser.load(browser.view))
            return None

        with (patch.object(store, "password_states", return_value={}),
              patch.object(store.RecordBrowser, "choose", new=choose)):
            store.list_records("alice", "all", "work")
        self.assertEqual([row["kind"] for row in captured], ["account", "info"])
        self.assertEqual(captured[-1]["value"], "工程师")

    def test_dimension_picker_counts_only_current_identity_personal_information(self):
        self.import_accounts()
        self.add_item("work", "职位", "工程师")
        self.add_item("hobby", "乐器", "吉他")
        self.add_item("work", "职位", "其他职位", identity="bob")
        seen = []

        def choose(browser):
            rows = browser.load(browser.view)
            seen.append((browser.view, rows))
            if len(seen) == 1:
                return {"kind": "dimension"}
            if browser.view == "dimension":
                return next(row for row in rows if row["category"] == "hobby")
            return None

        with patch.object(store, "password_states") as states, patch.object(store.RecordBrowser, "choose", new=choose):
            store.list_records("alice", "info")
        states.assert_not_called()
        self.assertEqual([(r["category"], r["count"]) for r in seen[1][1]], [("", 2), ("hobby", 1), ("work", 1)])
        self.assertEqual([r["category"] for r in seen[-1][1]], ["hobby"])

    def test_note_updates_preserve_account_metadata_and_links_and_reject_other_identity(self):
        self.import_accounts({"待确认": "bob"})
        own = store.info_rows("alice")[0]
        other = store.info_rows("bob")[0]
        metadata = json.loads(own["note"])
        store.set_password_link("alice", own["id"], "Logins/Mail")
        store.update_note("alice", own["id"], "日常使用 · Team's $account")
        updated = store.info_rows("alice")[0]
        self.assertEqual(json.loads(updated["note"]), {**metadata, "purpose": "日常使用 · Team's $account"})
        self.assertEqual(updated["link"], "Logins/Mail")
        self.assertEqual(updated["value"], own["value"])
        self.assertEqual(store.account_rows("alice")[0]["remark"], "日常使用 · Team's $account")
        with self.assertRaises(store.AccountError):
            store.update_note("alice", other["id"], "不可修改")
        self.assertEqual(store.info_rows("bob")[0], other)
        store.update_note("alice", own["id"], "")
        self.assertEqual(json.loads(store.info_rows("alice")[0]["note"]), {**metadata, "purpose": ""})

    def test_personal_notes_support_plain_text_and_preserve_structured_attributes(self):
        self.import_accounts()
        for raw in ("原备注", '{"source":"manual","text":"原备注"}'):
            item_id = self.add_item("work", raw, "工程师", raw)
            store.update_note("alice", item_id, "中文备注\n第二行 ' 引号")
            row = next(r for r in store.info_rows("alice") if r["id"] == item_id)
            self.assertEqual(store.note_text(row["note"]), "中文备注\n第二行 ' 引号")
            if raw.startswith("{"):
                self.assertEqual(json.loads(row["note"])["source"], "manual")
            store.update_note("alice", item_id, "")
            self.assertEqual(store.note_text(next(r for r in store.info_rows("alice") if r["id"] == item_id)["note"]), "")

    def test_notes_menu_saves_clears_and_keeps_on_enter(self):
        self.import_accounts()
        item_id = self.add_item("work", "职位", "工程师", "初始备注")
        for text, expected in (("新备注", "新备注"), ("", "新备注"), ("-", "")):
            row = next(r for r in store.info_rows("alice") if r["id"] == item_id)
            with (patch.object(store.RecordBrowser, "choose", side_effect=[row, None]),
                  patch.object(store, "choose_action", return_value="note"), patch("builtins.input", return_value=text),
                  patch.object(store, "password_states") as states, contextlib.redirect_stdout(Terminal())):
                store.list_records("alice", "info")
            states.assert_not_called()
            self.assertEqual(next(r for r in store.info_rows("alice") if r["id"] == item_id)["note"], expected)

    def test_add_from_account_links_only_after_successful_password_creation(self):
        self.import_accounts()
        row = store.account_rows("alice")[0]
        row["note"] = row["raw_note"]
        for result in (1, 0):
            with (patch.object(store.RecordBrowser, "choose", side_effect=[row, None]),
                  patch.object(store, "choose_action", return_value="add"),
                  patch.object(store, "write_entry", return_value=result) as write, contextlib.redirect_stdout(Terminal())):
                store.list_records("alice", "account")
            write.assert_called_once_with("alice", row["title"], "alice@example.com")
            self.assertEqual(store.info_rows("alice")[0]["link"], None if result else row["title"])

    def test_legacy_pass_list_contains_only_actual_vault_entries(self):
        self.import_accounts()
        captured = []

        def choose(browser):
            captured.extend(browser.load(browser.view))
            return None

        with patch.object(store, "password_states", return_value={"GitHub": "已设置"}), patch.object(store.RecordBrowser, "choose", new=choose):
            store.list_passwords("alice")
        self.assertEqual([(row["kind"], row["title"]) for row in captured], [("password", "GitHub")])

    def test_links_are_scoped_to_identity_and_preserve_personal_data(self):
        self.import_accounts({"待确认": "bob"})
        before = self.account()
        own = store.info_rows("alice")[0]
        other = store.info_rows("bob")[0]
        store.set_password_link("alice", own["id"], "/Logins/GitHub")
        self.assertEqual(store.info_rows("alice")[0]["link"], "Logins/GitHub")
        self.assertIsNone(store.info_rows("bob")[0]["link"])
        with self.assertRaises(store.AccountError):
            store.set_password_link("alice", other["id"], "Logins/GitHub")
        self.assertEqual(self.account(), before)
        self.assertEqual(store.account_rows("alice")[0]["title"], "Logins/GitHub")
        store.set_password_link("alice", own["id"], None)
        self.assertIsNone(store.info_rows("alice")[0]["link"])
        self.assertEqual(self.account(), before)

    def test_delete_record_removes_its_link_and_preserves_other_identity(self):
        self.import_accounts({"待确认": "bob"})
        own, other = store.info_rows("alice")[0], store.info_rows("bob")[0]
        store.set_password_link("alice", own["id"], "Mail")
        store.set_password_link("bob", other["id"], "Mail")
        with self.assertRaises(store.AccountError):
            store.delete_item("alice", other["id"])
        store.delete_item("alice", own["id"])
        self.assertEqual(store.info_rows("alice"), [])
        self.assertEqual(store.info_rows("bob")[0]["link"], "Mail")
        with contextlib.closing(sqlite3.connect(self.root / "data" / "id.db")) as db:
            self.assertFalse(db.execute("SELECT 1 FROM password_links WHERE item_id=?", (own["id"],)).fetchone())
            self.assertTrue(db.execute("SELECT 1 FROM events WHERE identity='alice' AND title='删除资料记录'").fetchone())

    def test_delete_field_keeps_source_metadata_and_required_fields(self):
        self.import_accounts({"待确认": "bob"})
        own, other = store.info_rows("alice")[0], store.info_rows("bob")[0]
        metadata = json.loads(own["note"])
        store.set_password_link("alice", own["id"], "Mail")
        for identity, item_id, field in (("alice", other["id"], "meta:purpose"),
                                         ("alice", own["id"], "meta:source"), ("alice", own["id"], "value")):
            with self.assertRaises(store.AccountError):
                store.delete_field(identity, item_id, field)
        store.delete_field("alice", own["id"], "meta:purpose")
        row = store.info_rows("alice")[0]
        del metadata["purpose"]
        self.assertEqual(json.loads(row["note"]), metadata)
        self.assertEqual(row["value"], own["value"])
        self.assertEqual(row["link"], "Mail")
        store.delete_field("alice", own["id"], "link")
        self.assertIsNone(store.info_rows("alice")[0]["link"])
        item_id = self.add_item("work", "职位", "工程师", "完整备注")
        store.delete_field("alice", item_id, "note")
        self.assertEqual(next(r for r in store.info_rows("alice") if r["id"] == item_id)["note"], "")

    def test_delete_menu_defaults_to_cancel_and_only_deletes_after_confirmation(self):
        self.import_accounts()
        row = {**store.info_rows("alice")[0], "_action": "delete"}
        with (patch.object(store.RecordBrowser, "choose", side_effect=[row, None]),
              patch.object(store, "choose_action", return_value="exit") as choose,
              patch.object(store, "delete_item") as delete, contextlib.redirect_stdout(Terminal())):
            store.list_records("alice", "info")
        delete.assert_not_called()
        self.assertEqual(choose.call_args.args[0][0][0], "exit")
        with (patch.object(store.RecordBrowser, "choose", side_effect=[row, None]),
              patch.object(store, "choose_action", return_value="delete"), contextlib.redirect_stdout(Terminal())):
            store.list_records("alice", "info")
        self.assertEqual(store.info_rows("alice"), [])

    def test_cancel_password_deletion_does_not_unlock_or_change_vault(self):
        row = dict(kind="password", title="GitHub", status="已设置", _action="delete")
        with (patch.object(store.RecordBrowser, "choose", side_effect=[row, None]),
              patch.object(store, "choose_action", return_value="exit"),
              patch.object(store, "delete_password") as delete, contextlib.redirect_stdout(Terminal())):
            store.list_passwords("alice")
        delete.assert_not_called()

    def test_delete_password_reauthenticates_backs_up_and_unlinks_only_current_identity(self):
        self.import_accounts({"待确认": "bob"})
        own, other = store.info_rows("alice")[0], store.info_rows("bob")[0]
        store.set_password_link("alice", own["id"], "GitHub")
        store.set_password_link("bob", other["id"], "GitHub")
        results = [SimpleNamespace(returncode=0, stdout="GitHub\n"), SimpleNamespace(returncode=0),
                   SimpleNamespace(returncode=0, stdout="Recycle Bin/GitHub\n"), SimpleNamespace(returncode=0)]
        with (store.vault_session.session() as session, patch.object(session, "unlock") as unlock,
              patch.object(store.vault_session, "run_cli", side_effect=results) as run, contextlib.redirect_stdout(Terminal())):
            store.delete_password("alice", "GitHub")
        unlock.assert_called_once_with(store.vault_for("alice"), fresh=True)
        self.assertEqual([call.args[0][1] for call in run.call_args_list], ["ls", "rm", "ls", "rm"])
        self.assertEqual(run.call_args.args[0][-1], "Recycle Bin/GitHub")
        self.assertIsNone(store.info_rows("alice")[0]["link"])
        self.assertEqual(store.info_rows("bob")[0]["link"], "GitHub")
        backups = list((self.root / "backups/password-deletions/alice").glob("*.kdbx"))
        self.assertEqual(len(backups), 1)
        self.assertEqual(backups[0].stat().st_mode & 0o777, 0o600)

    def test_delete_password_stops_when_recycled_path_is_ambiguous(self):
        results = [SimpleNamespace(returncode=0, stdout="GitHub\nRecycle Bin/GitHub\n"), SimpleNamespace(returncode=0),
                   SimpleNamespace(returncode=0, stdout="Recycle Bin/GitHub\nRecycle Bin/GitHub\n")]
        with (store.vault_session.session() as session, patch.object(session, "unlock"),
              patch.object(store.vault_session, "run_cli", side_effect=results) as run):
            with self.assertRaises(store.AccountError):
                store.delete_password("alice", "GitHub")
        self.assertEqual(run.call_count, 3)
        self.assertTrue(list((self.root / "backups/password-deletions/alice").glob("*.kdbx")))

    def test_password_backup_cannot_follow_a_link_outside_backup_directory(self):
        outside = self.root / "outside"
        outside.mkdir()
        (self.root / "backups").symlink_to(outside, target_is_directory=True)
        with (store.vault_session.session() as session, patch.object(session, "unlock"),
              patch.object(store.vault_session, "run_cli", return_value=SimpleNamespace(returncode=0, stdout="GitHub\n")) as run):
            with self.assertRaises(store.AccountError):
                store.delete_password("alice", "GitHub")
        run.assert_called_once()
        self.assertEqual(list(outside.iterdir()), [])

    def test_inline_details_include_full_notes_and_hide_import_internals(self):
        self.import_accounts()
        row = store.account_rows("alice")[0]
        row["note"] = row["raw_note"]
        row["status"] = "未关联"
        row["note"] = json.dumps({**json.loads(row["raw_note"]),
                                  "purpose": "完整备注\n第二行"}, ensure_ascii=False)
        details = store.record_details(row, [])
        self.assertIn(("备注", "完整备注\n第二行", "37"), details)
        self.assertNotIn("source_row", str(details))
        self.assertNotIn("backups/", str(details))

    def test_link_action_persists_without_reading_or_modifying_passwords(self):
        self.import_accounts()
        row = store.info_rows("alice")[0]
        with (patch.object(store.RecordBrowser, "choose", side_effect=[row, {"kind": "password", "title": "Logins/GitHub"}, None]),
              patch.object(store, "choose_action", return_value="link"),
              patch.object(store, "password_states", return_value={"Logins/GitHub": "已设置"}),
              patch.object(store, "write_entry") as write,
              contextlib.redirect_stdout(Terminal())):
            self.assertEqual(store.list_records("alice", "info"), 0)
        write.assert_not_called()
        self.assertEqual(store.info_rows("alice")[0]["link"], "Logins/GitHub")

    def test_legacy_email_metadata_is_not_used_as_username(self):
        self.import_accounts()
        with contextlib.closing(sqlite3.connect(self.root / "data" / "id.db")) as db, db:
            db.execute("INSERT INTO items(identity,category,key,value) VALUES('alice','account','other@example.com','proton 国外')")
        row = next(row for row in store.account_rows("alice") if row["title"] == "other@example.com")
        self.assertEqual(row["username"], "other@example.com")

    def test_account_label_is_separate_from_username_key_and_password_path(self):
        self.import_accounts()
        row = store.info_rows("alice")[0]
        note = json.loads(row["note"])
        note["label"] = "邮箱"
        with contextlib.closing(sqlite3.connect(self.root / "data" / "id.db")) as db, db:
            db.execute("UPDATE items SET note=? WHERE id=?", (json.dumps(note), row["id"]))
        store.set_password_link("alice", row["id"], "Accounts/Original title")
        account = store.account_rows("alice")[0]
        self.assertEqual(account["name"], "邮箱")
        self.assertEqual(account["key"], row["key"])
        self.assertEqual(account["username"], "alice@example.com")
        self.assertEqual(account["title"], "Accounts/Original title")

    def test_generated_commands_preserve_make_dollars_and_shell_quotes(self):
        title = "Team's $(literal) entry"
        command = store.entry_command("pass-edit", "alice", title)
        self.assertIn("$$(literal)", command)
        self.assertIn("'\"'\"'", command)
        with self.assertRaises(store.AccountError):
            store.entry_command("pass-edit", "alice", "bad\x1b[2J")

    @unittest.skipUnless(shutil.which("make"), "需要本机 make")
    def test_printed_command_reaches_make_recipe_with_complete_metadata(self):
        scripts = self.root / "scripts"
        scripts.mkdir()
        (scripts / "account_store.py").write_text(
            "import json, os\nprint(json.dumps([os.environ.get(key) for key in "
            "('ID_ENTRY_ID','ID_ENTRY_TITLE','ID_ENTRY_USER')]))\n")
        title, username = "中文 Team's $(literal) entry", "name's $account"
        command = shlex.split(store.entry_command("pass-add-manual", "alice", title, username))
        result = subprocess.run([command[0], "--no-print-directory", "-f", str(PROJECT / "Makefile"), *command[1:]],
                                cwd=self.root, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(json.loads(result.stdout), ["alice", title, username])

    @unittest.skipUnless(shutil.which("keepassxc-cli"), "需要本机 KeePassXC")
    def test_real_cli_detects_empty_and_filled_without_exporting_passwords(self):
        vault = self.root / "identities" / "alice" / "passwords" / "alice.kdbx"
        vault.unlink()
        master = "synthetic-master-for-test-only"

        def run(arguments, values):
            result = subprocess.run(["keepassxc-cli", *arguments], input="\n".join(values) + "\n", text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, "Synthetic KeePassXC setup failed: " + arguments[0])

        run(["db-create", "-p", "-t", "100", str(vault)], [master, master])
        with patch.object(store.getpass, "getpass", return_value=master):
            self.assertEqual(store.password_states("alice"), {})
        run(["add", str(vault), "Empty"], [master])
        with patch.object(store.getpass, "getpass", return_value=master):
            self.assertEqual(store.password_states("alice"), {"Empty": "未填密码"})
        run(["mkdir", str(vault), "Group"], [master])
        run(["add", "-p", str(vault), "Filled"], [master, SECRET])
        with patch.object(store.getpass, "getpass", return_value=master):
            self.assertEqual(store.password_states("alice"), {"Empty": "未填密码", "Filled": "已设置"})
        with (patch.object(store.sys, "stdin", Terminal()), patch.object(store.sys, "stdout", Terminal()),
              patch.object(store.getpass, "getpass", return_value=master) as prompt):
            store.delete_password("alice", "Filled")
            prompt.assert_called_once()
        with patch.object(store.getpass, "getpass", return_value=master):
            self.assertEqual(store.password_states("alice"), {"Empty": "未填密码"})
            backup = next((self.root / "backups/password-deletions/alice").glob("*.kdbx"))
            with patch.object(store, "vault_for", return_value=backup):
                self.assertEqual(store.password_states("alice"), {"Empty": "未填密码", "Filled": "已设置"})


if __name__ == "__main__":
    unittest.main()
