#!/usr/bin/env python3
"""通用账号密码录入、状态清单和本地资料导入。"""

import argparse
from collections import Counter
from contextlib import closing
from datetime import datetime
import hashlib
import getpass
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
from zipfile import BadZipFile, ZipFile

from terminal_menu import choose_action
from terminal_list import RecordBrowser, category_label, clip, color
import vault_session

ROOT = Path(__file__).resolve().parents[1]
NS = {"s": "http://schemas.openxmlformats.org/spreadsheetml/2006/main"}
HEADERS = ["分类", "服务商", "使用者", "账号", "备注", "密码备注", "详细数据"]


class AccountError(ValueError):
    """可安全显示给用户的错误，不携带机密单元格内容。"""


def read_sheet(path: Path, *, secret_row: int | None = None) -> list[dict]:
    with ZipFile(path) as archive:
        if any(info.file_size > 16 * 1024 * 1024 for info in archive.infolist()):
            raise AccountError("表格过大，请先拆分后再导入。")
        strings = []
        if "xl/sharedStrings.xml" in archive.namelist():
            strings = ["".join(node.itertext()) for node in ET.fromstring(
                archive.read("xl/sharedStrings.xml")
            ).findall("s:si", NS)]
        # shortcut: 仅支持当前邮箱表的首张工作表；表结构变化时再扩展映射。
        rows = ET.fromstring(archive.read("xl/worksheets/sheet1.xml")).findall("s:sheetData/s:row", NS)

    def value(cell):
        if cell is None:
            return ""
        if cell.find("s:f", NS) is not None:
            raise AccountError("不导入公式，请先将公式转为值。")
        if cell.get("t") == "inlineStr":
            return "".join(cell.find("s:is", NS).itertext())
        node = cell.find("s:v", NS)
        text = "" if node is None else node.text or ""
        return strings[int(text)] if cell.get("t") == "s" and text else text

    result = []
    for index, row in enumerate(rows):
        cells = {re.sub(r"\d+$", "", cell.get("r", "")): cell for cell in row}
        if index == 0:
            if [value(cells.get(column)).strip() for column in "ABCDEFG"] != HEADERS:
                raise AccountError("表头须为：分类、服务商、使用者、账号、备注、密码备注、详细数据。")
            continue
        public = [value(cells.get(column)).strip() for column in "ABCDE"]
        if not any(public):
            continue
        region, provider, owner, email, purpose = public
        if any(not text.isprintable() for text in (region, provider, owner, email) if text):
            raise AccountError("邮箱资料包含终端控制字符，未写入。")
        if len(email) > 254 or not re.fullmatch(r"[^@\s/\\]+@[^@\s/\\]+\.[^@\s/\\]+", email):
            raise AccountError("邮箱地址无效，未写入。")
        record = dict(row=int(row.get("r")), region=region, provider=provider,
                      owner=owner, email=email, purpose=purpose)
        if record["row"] == secret_row:
            record["secret_fields"] = {HEADERS[i]: value(cells.get(column))
                                       for i, column in ((5, "F"), (6, "G"))}
        result.append(record)
    if not rows:
        raise AccountError("表格为空。")
    return result


def private_file(path: Path, content: bytes = b"") -> None:
    with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "wb") as stream:
        stream.write(content)


def import_accounts(source: Path, owners: dict[str, str]) -> None:
    content = source.read_bytes()
    archive_dir = ROOT / "backups" / "mail-imports"
    archive_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    archive = archive_dir / (hashlib.sha256(content).hexdigest() + ".xlsx")
    if not archive.exists():
        private_file(archive, content)
    records = read_sheet(archive)
    identities = {path.name.casefold(): path.name for path in (ROOT / "identities").iterdir()
                  if path.is_dir() and re.fullmatch(r"[A-Za-z0-9_-]+", path.name)}
    for alias, identity in owners.items():
        if identity not in identities.values():
            raise AccountError("归属映射必须指向已有身份。")
        identities[alias.casefold()] = identity
    ready, pending, seen = [], Counter(), set()
    for record in records:
        identity = identities.get(record["owner"].casefold())
        if identity is None:
            pending[record["owner"]] += 1
        else:
            key = (identity, record["email"].casefold())
            if key in seen:
                raise AccountError("表格内有同一身份的重复邮箱，请合并重复行后导入。")
            seen.add(key)
            ready.append((identity, record))
    if not ready:
        raise AccountError("没有归属已明确的邮箱，未写入。")

    database = ROOT / "data" / "id.db"
    database.parent.mkdir(exist_ok=True)
    if not database.exists():
        private_file(database)
    with closing(sqlite3.connect(database)) as db:
        backup = archive_dir / ("id-before-" + datetime.now().strftime("%Y%m%d-%H%M%S-%f") + ".db")
        private_file(backup)
        with closing(sqlite3.connect(backup)) as target:
            db.backup(target)
        db.executescript((ROOT / "template" / "data" / "schema.sql").read_text())
        with db:
            for identity, record in ready:
                note = {"type": "mail-account", "provider": record["provider"],
                        "region": record["region"], "purpose": record["purpose"],
                        "password_title": "邮箱 - " + record["email"],
                        "source": str(archive.relative_to(ROOT)),
                        "source_row": record["row"], "source_owner": record["owner"]}
                db.execute("""INSERT INTO items(identity,category,key,value,note) VALUES(?,?,?,?,?)
                    ON CONFLICT(identity,category,key) DO UPDATE SET
                    value=excluded.value, note=excluded.note, updated_at=datetime('now')
                    WHERE items.value!=excluded.value OR items.note!=excluded.note""",
                           (identity, "account", "email:" + record["email"].casefold(),
                            record["email"], json.dumps(note, ensure_ascii=False)))
    counts = Counter(identity for identity, _ in ready)
    print("已整理邮箱：" + "，".join(f"{identity} {count} 个" for identity, count in sorted(counts.items())))
    print(f"原表和导入前备份：{archive_dir.relative_to(ROOT)}/（仅本机，权限受限，不进 git）")
    print("密码备注和详细数据未写入 SQLite；运行 make pass-add-manual 选择账号并录入密码。")
    if pending:
        print("待确认归属：" + "，".join(f"{owner} {count} 个" for owner, count in pending.items()))


def attach_source(database: Path, title: str, account: str, note: dict) -> bool:
    source = (ROOT / note["source"]).resolve()
    archive_dir = (ROOT / "backups" / "mail-imports").resolve()
    if not source.is_relative_to(archive_dir) or not source.is_file():
        raise AccountError("本机原表备份不存在；补充资料请在保留原表的设备上录入。")
    record = next((row for row in read_sheet(source, secret_row=note["source_row"])
                   if row["row"] == note["source_row"]), None)
    if record is None or record["email"] != account or record["owner"] != note["source_owner"]:
        raise AccountError("原表与当前邮箱不一致，未保存补充资料。")
    fields = record["secret_fields"]
    content = "\n\n".join(f"{key}：\n{value}" for key, value in fields.items() if value.strip())
    if not content:
        print("此邮箱没有原表补充资料。")
        return True
    selected = choose_action([("attach", "将此邮箱原表的密码备注、详细数据存为加密附件"), ("exit", "暂不保存补充资料")],
                             "补充资料（不显示内容；↑/↓选择，Enter 确认）：")
    if selected != "attach":
        return False
    print("正在将补充资料保存到密码库。")
    # 临时文件只在项目内、权限 600；内容不进入 argv、日志或模型。
    with tempfile.TemporaryDirectory(prefix=".mail-notes-", dir=archive_dir) as directory:
        attachment = Path(directory) / "原表补充资料.txt"
        private_file(attachment, content.encode("utf-8"))
        result = vault_session.run_cli(["keepassxc-cli", "attachment-import", str(database), title,
                                       "原表补充资料.txt", str(attachment)], database, check=False)
    if result.returncode:
        print("补充资料未保存；原表仍在本机备份中，可选“仅补录原表资料”重试。")
    return result.returncode == 0


def require_terminal() -> None:
    if not sys.stdin.isatty() or not sys.stdout.isatty():
        raise AccountError("请在自己的交互终端操作；密码输入不经过 AI，输出不写入管道或文件。")


def select_identity(identity: str | None) -> str | None:
    if not identity:
        names = sorted(path.name for path in (ROOT / "identities").iterdir()
                       if path.is_dir() and re.fullmatch(r"[A-Za-z0-9_-]+", path.name))
        selected = choose_action([(str(i), name) for i, name in enumerate(names)] + [("exit", "退出")],
                                 "选择身份（↑/↓选择，Enter 确认）：")
        if selected == "exit":
            return None
        identity = names[int(selected)]
    if not re.fullmatch(r"[A-Za-z0-9_-]+", identity) or not (ROOT / "identities" / identity).is_dir():
        raise AccountError("身份不存在。")
    return identity


def vault_for(identity: str) -> Path:
    if shutil.which("keepassxc-cli") is None:
        raise AccountError("请先安装 KeePassXC（Debian/Ubuntu：sudo apt install keepassxc）。")
    if not re.fullmatch(r"[A-Za-z0-9_-]+", identity):
        raise AccountError("身份名无效。")
    vault = ROOT / "identities" / identity / "passwords" / (identity + ".kdbx")
    if not vault.is_file():
        raise AccountError("此身份的密码库不存在，未创建或修改密码库。")
    return vault


def info_rows(identity: str) -> list[dict]:
    database = ROOT / "data" / "id.db"
    if not database.exists():
        return []
    with closing(sqlite3.connect(f"file:{database}?mode=ro", uri=True)) as db:
        db.row_factory = sqlite3.Row
        if not db.execute("SELECT 1 FROM sqlite_master WHERE name='items'").fetchone():
            return []
        rows = [dict(row) for row in db.execute("SELECT id,category,key,value,note FROM items WHERE identity=? ORDER BY category,key", (identity,))]
        links = {}
        if db.execute("SELECT 1 FROM sqlite_master WHERE name='password_links'").fetchone():
            links = dict(db.execute("SELECT l.item_id,l.entry_path FROM password_links l JOIN items i ON i.id=l.item_id WHERE i.identity=?", (identity,)))
        for row in rows:
            row.update(kind="info", link=links.get(row["id"]))
        return rows


def set_password_link(identity: str, item_id: int, path: str | None) -> None:
    if path is not None and (not path.strip("/") or not path.isprintable()):
        raise AccountError("密码条目路径无效。")
    with closing(sqlite3.connect(ROOT / "data" / "id.db")) as db:
        db.execute("PRAGMA foreign_keys=ON")
        if not db.execute("SELECT 1 FROM items WHERE id=? AND identity=?", (item_id, identity)).fetchone():
            raise AccountError("该资料不属于当前身份。")
        db.execute("""CREATE TABLE IF NOT EXISTS password_links (
            item_id INTEGER PRIMARY KEY REFERENCES items(id) ON DELETE CASCADE,
            entry_path TEXT NOT NULL)""")
        with db:
            if path is None:
                db.execute("DELETE FROM password_links WHERE item_id=?", (item_id,))
            else:
                db.execute("INSERT INTO password_links VALUES(?,?) ON CONFLICT(item_id) DO UPDATE SET entry_path=excluded.entry_path",
                           (item_id, path.lstrip("/")))


def parse_note(raw: str) -> dict | str:
    try:
        detail = json.loads(raw)
    except (TypeError, ValueError):
        return raw
    return detail if isinstance(detail, dict) else raw


def note_text(raw: str) -> str:
    detail = parse_note(raw)
    if isinstance(detail, dict):
        return str(detail.get("purpose" if detail.get("type") == "mail-account" else "text") or "")
    return detail


def update_note(identity: str, item_id: int, text: str) -> None:
    with closing(sqlite3.connect(ROOT / "data" / "id.db")) as db, db:
        row = db.execute("SELECT note FROM items WHERE id=? AND identity=?", (item_id, identity)).fetchone()
        if row is None:
            raise AccountError("该资料不属于当前身份。")
        detail = parse_note(row[0])
        if isinstance(detail, dict):
            detail["purpose" if detail.get("type") == "mail-account" else "text"] = text
            text = json.dumps(detail, ensure_ascii=False)
        db.execute("UPDATE items SET note=?,updated_at=datetime('now') WHERE id=? AND identity=?", (text, item_id, identity))


NOTE_INTERNAL = {"type", "label", "password_title", "source", "source_row", "source_owner", "username"}
NOTE_LABELS = {"provider": "服务商", "region": "地区", "purpose": "备注", "text": "备注"}


def note_fields(raw):
    detail = parse_note(raw)
    if isinstance(detail, dict):
        return [("meta:" + key, NOTE_LABELS.get(key, key), str(value))
                for key, value in detail.items() if key not in NOTE_INTERNAL and value]
    return [("note", "备注", detail)] if detail else []


def record_details(row, infos):
    kind = row["kind"]
    if kind == "password":
        fields = [("条目", row["title"], "37")]
    elif kind == "account":
        fields = [("账号", row["name"], "37"), ("登录名", row["username"], "37")]
    else:
        fields = [("维度", category_label(row["category"]), "2"), ("名称", row["key"], "37"), ("内容", row["value"], "37")]
    if "status" in row:
        fields.append(("密码状态", row["status"], "32" if row["status"] == "已设置" else "33"))
    if kind == "password":
        linked = "\n".join(f"{item['key']}: {item['value']}" for item in infos if item["link"] == row["title"])
        fields.append(("关联资料", linked or "—", "2"))
    else:
        notes = note_fields(row["note"])
        fields += [(label, value, "37") for _, label, value in notes] or [("备注", "—", "2")]
        fields.append(("关联密码", row["link"] or "—", "2"))
    return fields


def delete_field(identity: str, item_id: int, field: str) -> None:
    if field == "link":
        set_password_link(identity, item_id, None)
        return
    with closing(sqlite3.connect(ROOT / "data" / "id.db")) as db, db:
        row = db.execute("SELECT note FROM items WHERE id=? AND identity=?", (item_id, identity)).fetchone()
        if row is None:
            raise AccountError("该资料不属于当前身份。")
        if field not in {key for key, _, _ in note_fields(row[0])}:
            raise AccountError("该字段不能删除；名称和内容属于记录本身，可选择删除整条。")
        detail = parse_note(row[0])
        if isinstance(detail, dict):
            del detail[field.removeprefix("meta:")]
            note = json.dumps(detail, ensure_ascii=False)
        else:
            note = ""
        db.execute("UPDATE items SET note=?,updated_at=datetime('now') WHERE id=? AND identity=?", (note, item_id, identity))


def delete_item(identity: str, item_id: int) -> None:
    with closing(sqlite3.connect(ROOT / "data" / "id.db")) as db, db:
        db.execute("PRAGMA foreign_keys=ON")
        if not db.execute("SELECT 1 FROM items WHERE id=? AND identity=?", (item_id, identity)).fetchone():
            raise AccountError("该资料不属于当前身份。")
        if db.execute("SELECT 1 FROM sqlite_master WHERE name='password_links'").fetchone():
            db.execute("DELETE FROM password_links WHERE item_id=?", (item_id,))
        db.execute("DELETE FROM items WHERE id=? AND identity=?", (item_id, identity))
        db.execute("INSERT INTO events(identity,title,detail) VALUES(?,?,?)", (identity, "删除资料记录", f"资料 ID {item_id}"))


def delete_password(identity: str, title: str) -> None:
    if not title.strip("/") or not title.isprintable():
        raise AccountError("密码条目路径无效。")
    title = title.lstrip("/")
    database = vault_for(identity)
    with vault_session.session() as session:
        session.unlock(database, fresh=True)

        def paths():
            result = vault_session.run_cli(["keepassxc-cli", "ls", "-R", "-f", str(database)], database,
                                           text=True, capture_output=True, check=False)
            if result.returncode:
                raise AccountError("未能核对密码条目；已创建的加密备份会保留。")
            return [path.lstrip("/") for path in result.stdout.splitlines() if path and not path.endswith("/")]

        before = paths()
        if title not in before:
            raise AccountError("密码条目已不存在，请刷新列表。")
        folder = (ROOT / "backups" / "password-deletions" / identity).resolve()
        if not folder.is_relative_to(ROOT.resolve() / "backups"):
            raise AccountError("备份目录不能通过符号链接指向项目备份目录之外，未删除密码。")
        folder.mkdir(parents=True, exist_ok=True, mode=0o700)
        backup = folder / (datetime.now().strftime("%Y%m%d-%H%M%S-%f") + ".kdbx")
        private_file(backup)
        with database.open("rb") as source, backup.open("wb") as target:
            shutil.copyfileobj(source, target)
        result = vault_session.run_cli(["keepassxc-cli", "rm", str(database), title], database, check=False)
        if result.returncode:
            raise AccountError("密码条目未删除，加密备份已保留。")
        try:
            after = paths()
            added = [path for path in (Counter(after) - Counter(before)).elements() if path.rsplit("/", 1)[-1] != "[empty]"]
            moved = [path for path in added if path.rsplit("/", 1)[-1] == title.rsplit("/", 1)[-1]]
            if len(added) != len(moved) or len(moved) > 1 or any(path in before for path in moved):
                raise AccountError("条目已移入回收站，路径存在歧义，请在 KeePassXC 中继续整理；加密备份已保留。")
            if moved:
                result = vault_session.run_cli(["keepassxc-cli", "rm", str(database), moved[0]], database, check=False)
                if result.returncode:
                    raise AccountError("条目已移入回收站，尚未彻底删除；加密备份已保留。")
        finally:
            if (ROOT / "data" / "id.db").exists():
                with closing(sqlite3.connect(ROOT / "data" / "id.db")) as db, db:
                    if db.execute("SELECT 1 FROM sqlite_master WHERE name='password_links'").fetchone():
                        db.execute("DELETE FROM password_links WHERE entry_path=? AND item_id IN (SELECT id FROM items WHERE identity=?)", (title, identity))
        print("密码条目已删除；加密备份：" + str(backup.relative_to(ROOT)))


def account_rows(identity: str) -> list[dict]:
    accounts = []
    for row in info_rows(identity):
        if row["category"] != "account":
            continue
        key, username, raw = row["key"], row["value"], row["note"]
        note = parse_note(raw)
        if not isinstance(note, dict):
            note = {}
        title = row["link"] or note.get("password_title") or key
        email_key = ":" not in key and re.fullmatch(r"[^@\s/\\]+@[^@\s/\\]+\.[^@\s/\\]+", key)
        username = note.get("username", key if email_key else username)
        if not isinstance(title, str) or not isinstance(username, str) or not title.strip():
            raise AccountError("账号的标题或用户名格式无效。")
        accounts.append({**row, "kind": "account", "title": title, "username": username, "note": note,
                         "raw_note": raw, "name": note.get("label") or note.get("provider") or key, "remark": note_text(raw)})
    return accounts


def write_entry(identity: str, title: str, username: str | None, *, edit: bool = False) -> int:
    if not title.strip():
        raise AccountError("条目标题不能为空。")
    command = ["keepassxc-cli", "edit" if edit else "add", "-p"]
    if username is not None:
        command += ["-u", username]
    vault = vault_for(identity)
    command += [str(vault), title]
    if vault_session.current() is None:
        return subprocess.run(command, check=False).returncode
    vault_session.current().unlock(vault, fresh=edit)
    password = getpass.getpass("新条目密码: ")
    if "\n" in password or "\r" in password:
        raise AccountError("密码不能包含换行。")
    try:
        return vault_session.run_cli(command, vault, answers=(password,), check=False).returncode
    finally:
        del password


def fill_passwords(identity: str | None, title: str | None = None, username: str | None = None) -> int:
    require_terminal()
    identity = select_identity(identity)
    if identity is None:
        return 130
    accounts = account_rows(identity)
    if title:
        match = next((row for row in accounts if row["title"] == title), None)
        if username is None:
            username = match["username"] if match else input("用户名（可空）: ").strip()
        result = write_entry(identity, title, username)
        if not result and match:
            set_password_link(identity, match["id"], title)
        return result
    completed = set()
    print("选择账号或自定义条目。")
    while True:
        selected = choose_action([(str(i), ("✓ 本次已完成 · " if row["title"] in completed else "") + safe_text(row["title"]))
                                  for i, row in enumerate(accounts)] + [("new", "自定义条目（填写标题和用户名）"), ("exit", "退出")],
                                 f"{identity} 的账号（↑/↓选择，Enter 确认）：")
        if selected == "exit":
            return 0
        if selected == "new":
            title = input("条目标题: ").strip()
            name = username if username is not None else input("用户名（可空）: ").strip()
            write_entry(identity, title, name)
            continue
        row = accounts[int(selected)]
        title, name, note = row["title"], row["username"], row["note"]
        options = [("add", "新增密码条目（已有同名条目时拒绝覆盖）"),
                   ("edit", "更新密码库中这个同名条目的密码")]
        if note.get("type") == "mail-account":
            options.append(("notes", "仅补录原表资料（需已存在同名条目）"))
        operation = choose_action([*options, ("exit", "返回")], safe_text(f"{title} · 用户名：{name}"))
        if operation == "exit":
            continue
        if operation != "notes":
            if write_entry(identity, title, name, edit=operation == "edit"):
                print("未保存密码；如已存在同名条目，请明确选择更新。")
                continue
            print("密码已保存到当前身份的 KeePass 数据库。")
            set_password_link(identity, row["id"], title)
        try:
            if note.get("type") != "mail-account" or attach_source(vault_for(identity), title, name, note):
                completed.add(title)
        except AccountError as exc:
            print(str(exc), file=sys.stderr)


def password_states(identity: str) -> dict[str, str]:
    vault = vault_for(identity)
    master = None if vault_session.current() else getpass.getpass(f"{identity} · 主密码: ") + "\n"
    try:
        # KeePassXC 在内部检查密码；stdout 始终只有条目路径，不导出密码。
        def query(command):
            options = dict(text=True, capture_output=True, check=False, env={**os.environ, "LANG": "C", "LC_ALL": "C"})
            if master is None:
                result = vault_session.run_cli(["keepassxc-cli", *command], vault, **options)
            else:
                result = subprocess.run(["keepassxc-cli", *command], input=master, **options)
            no_matches = (command[0] == "search" and result.returncode == 1 and not result.stdout
                          and result.stderr.strip() == "No results for that search term.")
            if result.returncode and not no_matches:
                if vault_session.current() is not None:
                    vault_session.current().lock()
                raise AccountError("密码库解锁或状态查询失败，未判断密码是否为空。")
            # shortcut: CLI 路径按行输出；标题含换行时需先在 KeePassXC 中重命名。
            return {path.lstrip("/") for path in result.stdout.splitlines() if path and not path.endswith("/")}
        paths = query(["ls", "-q", "-R", "-f", str(vault)])
        empty = query(["search", "-q", str(vault), "*pw:\\A\\z"])
        filled = query(["search", "-q", str(vault), "*pw:[\\s\\S]"])
        paths = {path for path in paths if path.rsplit("/", 1)[-1] != "[empty]" or path in empty | filled}
        return {path: "未填密码" if path in empty and path not in filled else
                "已设置" if path in filled and path not in empty else "状态未知" for path in sorted(paths)}
    finally:
        del master


def safe_text(value: str) -> str:
    return "".join(char if char.isprintable() else "?" for char in value)


def entry_command(target: str, identity: str, title: str, username: str | None = None) -> str:
    values = {"id": identity, "title": title}
    if username is not None:
        values["user"] = username
    if any(any(not char.isprintable() for char in value) for value in values.values()):
        raise AccountError("条目名称或用户名包含控制字符，请先在 KeePassXC 中修正。")
    return "make " + target + " " + " ".join(f"{key}={shlex.quote(value.replace('$', '$$'))}" for key, value in values.items())


def list_records(identity: str, view: str = "all", category: str = "") -> int:
    states = None

    def passwords():
        nonlocal states
        if states is None:
            states = password_states(identity)
        return states

    def load(selected_type):
        infos = info_rows(identity)
        records = []
        if selected_type in ("all", "account"):
            for row in account_rows(identity):
                status = passwords().get(row["link"], "条目不存在") if row["link"] else "未关联"
                records.append({**row, "note": row["raw_note"], "status": status})
        if selected_type in ("all", "password"):
            counts = Counter(row["link"] for row in infos if row["link"])
            records += [dict(kind="password", title=title, status=status, linked=counts[title])
                        for title, status in passwords().items()]
        if selected_type in ("all", "info"):
            records += [{**row, "remark": note_text(row["note"])} for row in infos
                        if row["category"] != "account" and (not browser.category or row["category"] == browser.category)]
        for row in records:
            row["details"] = record_details(row, infos)
        return records

    browser = RecordBrowser(identity, load, view, category=category)
    while True:
        row = browser.choose()
        if row is None:
            return 0
        if row["kind"] == "refresh":
            states = None
            continue
        if row["kind"] == "filter":
            continue
        if row["kind"] == "dimension":
            counts = Counter(item["category"] for item in info_rows(identity) if item["category"] != "account")
            candidates = [dict(kind="dimension", category="", label="全部维度", count=sum(counts.values()))]
            candidates += [dict(kind="dimension", category=key, label=category_label(key), count=count)
                           for key, count in sorted(counts.items())]
            chosen = RecordBrowser(identity, lambda _: candidates, "dimension", picker=True).choose()
            if chosen:
                browser.category, browser.view = chosen["category"], "info"
                browser.page = browser.selected = 0
            continue
        if row["kind"] == "password":
            print(color("\n" + safe_text(row["title"]), "1;36"))
            action = row.get("_action") or choose_action([("edit", "补填/修改密码"), ("command", "查看修改命令"),
                                                           ("delete", "删除整条密码"), ("exit", "返回")], "操作")
            if action == "edit":
                write_entry(identity, row["title"], None, edit=True)
                states = None
            elif action == "command":
                print(entry_command("pass-edit", identity, row["title"]))
                input("回车返回 ")
            elif action == "delete":
                confirmed = choose_action([("exit", "取消"), ("delete", "确认删除密码条目")],
                                          safe_text(f"删除 [{identity}] {row['title']} 及其内容？先备份加密密码库，保留关联资料，需要再次验证主密码。"))
                if confirmed == "delete":
                    try:
                        delete_password(identity, row["title"])
                    except (AccountError, vault_session.VaultError) as exc:
                        print(str(exc), file=sys.stderr)
                        input("回车返回 ")
                    states = None
            continue
        print(color("\n" + safe_text(row.get("name") or row["key"]), "1;36"))
        options = [("note", "编辑备注"), ("link", "关联/更换密码条目")]
        if row["kind"] == "account" and (not row["link"] or row.get("status") == "条目不存在"):
            options.append(("add", "新增密码并关联"))
        if row["link"]:
            options += [("edit", "修改关联密码"), ("command", "查看修改命令"), ("unlink", "解除关联")]
        action = row.get("_action") or choose_action([*options, ("field", "删除单个字段"), ("delete", "删除整条记录"), ("exit", "返回")], "操作")
        if action == "note":
            text = input("新备注（Enter 保留，- 清空）: ")
            if text:
                update_note(identity, row["id"], "" if text == "-" else text)
        elif action == "add":
            if not write_entry(identity, row["title"], row["username"]):
                set_password_link(identity, row["id"], row["title"])
                states = None
        elif action == "link":
            candidates = [dict(kind="password", title=title, status=status) for title, status in passwords().items()]
            chosen = RecordBrowser(identity, lambda _: candidates, "password", picker=True).choose()
            if chosen:
                set_password_link(identity, row["id"], chosen["title"])
        elif action == "unlink":
            set_password_link(identity, row["id"], None)
        elif action == "field":
            field_width = max(1, shutil.get_terminal_size((100, 24)).columns - 3)
            fields = [(key, clip(label, field_width)) for key, label, _ in note_fields(row["note"])]
            if row["link"]:
                fields.append(("link", "密码关联"))
            field = choose_action([("exit", "取消"), *fields], "选择要删除的字段；名称和内容可通过删除整条记录移除。")
            if field != "exit":
                confirmed = choose_action([("exit", "取消"), ("delete", "确认删除这个字段")],
                                          safe_text(f"[{identity}] {row.get('name') or row['key']} · {dict(fields)[field]}"))
                if confirmed == "delete":
                    delete_field(identity, row["id"], field)
        elif action == "delete":
            confirmed = choose_action([("exit", "取消"), ("delete", "确认删除资料记录")],
                                      safe_text(f"删除 [{identity}] {row.get('name') or row['key']} 及其关联？密码条目保留。"))
            if confirmed == "delete":
                delete_item(identity, row["id"])
        elif action in ("edit", "command"):
            if row["link"] not in passwords():
                print("关联条目已不存在，请重新关联。")
                input("回车返回 ")
            elif action == "edit":
                write_entry(identity, row["link"], None, edit=True)
                states = None
            else:
                print(entry_command("pass-edit", identity, row["link"]))
                input("回车返回 ")


def list_passwords(identity: str) -> int:
    return list_records(identity, "password")


@vault_session.session()
def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    importer = commands.add_parser("import", help="只将非机密字段写入 SQLite")
    importer.add_argument("file", type=Path)
    importer.add_argument("--owner", action="append", default=[], metavar="原使用者=身份")
    for command, help_text in (("add", "选择账号录入密码"), ("edit", "修改已有条目密码"), ("list", "列出条目、密码状态及修改命令")):
        command_parser = commands.add_parser(command, help=help_text)
        command_parser.add_argument("--identity", default=os.environ.get("ID_ENTRY_ID") or None)
        command_parser.add_argument("--title", default=os.environ.get("ID_ENTRY_TITLE") or None)
        command_parser.add_argument("--username", default=os.environ.get("ID_ENTRY_USER") or None)
        if command == "list":
            command_parser.add_argument("--type", choices=("all", "account", "password", "info"),
                                        default=os.environ.get("ID_LIST_TYPE") or "all")
            command_parser.add_argument("--category", default=os.environ.get("ID_LIST_CATEGORY") or "")
    args = parser.parse_args()
    if args.command == "list" and args.type not in ("all", "account", "password", "info"):
        parser.error("type 只能是 all、account、password 或 info")
    try:
        if args.command == "import":
            if any("=" not in item for item in args.owner):
                raise AccountError("归属参数格式：--owner 原使用者=身份")
            import_accounts(args.file, dict(item.split("=", 1) for item in args.owner))
            return 0
        require_terminal()
        if args.command == "add":
            return fill_passwords(args.identity, args.title, args.username)
        identity = select_identity(args.identity)
        if identity is None:
            return 130
        if args.command == "list":
            return list_records(identity, args.type, args.category)
        title = args.title
        if not title:
            states = password_states(identity)
            paths = list(states)
            selected = choose_action([(str(i), safe_text(path) + " · " + states[path]) for i, path in enumerate(paths)] + [("exit", "取消")],
                                     "选择要修改的密码条目（↑/↓选择，Enter 确认）：")
            if selected == "exit":
                return 130
            title = paths[int(selected)]
        return write_entry(identity, title, args.username, edit=True)
    except (KeyboardInterrupt, EOFError):
        print("\n已取消，已保存的条目会保留。")
        return 130
    except (AccountError, vault_session.VaultError) as exc:
        print(str(exc), file=sys.stderr)
        return 1
    except (ValueError, OSError, KeyError, IndexError, BadZipFile, ET.ParseError, sqlite3.Error):
        # 解析和数据库异常可能携带原单元格内容，不回显异常详情。
        print("操作未完成，请检查文件格式、归属或本地依赖；未输出原表机密内容。", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
