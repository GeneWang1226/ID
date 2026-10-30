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
import unicodedata
import xml.etree.ElementTree as ET
from zipfile import BadZipFile, ZipFile

from terminal_menu import choose_action

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
    print("KeePassXC 需要再次解锁，用于保存补充资料。")
    # 临时文件只在项目内、权限 600；内容不进入 argv、日志或模型。
    with tempfile.TemporaryDirectory(prefix=".mail-notes-", dir=archive_dir) as directory:
        attachment = Path(directory) / "原表补充资料.txt"
        private_file(attachment, content.encode("utf-8"))
        result = subprocess.run(["keepassxc-cli", "attachment-import", str(database), title,
                                 "原表补充资料.txt", str(attachment)], check=False)
    if result.returncode:
        print("补充资料未保存；原表仍在本机备份中，可选“仅补录原表资料”重试。")
    return result.returncode == 0


def require_terminal() -> None:
    if not sys.stdin.isatty() or not sys.stdout.isatty():
        raise AccountError("请在自己的交互终端操作；密码输入不经过 AI，输出不写入管道或文件。")
    if shutil.which("keepassxc-cli") is None:
        raise AccountError("请先安装 KeePassXC（Debian/Ubuntu：sudo apt install keepassxc）。")


def select_identity(identity: str | None) -> str | None:
    if not identity:
        names = sorted(path.name for path in (ROOT / "identities").iterdir()
                       if path.is_dir() and re.fullmatch(r"[A-Za-z0-9_-]+", path.name))
        selected = choose_action([(str(i), name) for i, name in enumerate(names)] + [("exit", "退出")],
                                 "选择身份（↑/↓选择，Enter 确认）：")
        if selected == "exit":
            return None
        identity = names[int(selected)]
    vault_for(identity)
    return identity


def vault_for(identity: str) -> Path:
    if not re.fullmatch(r"[A-Za-z0-9_-]+", identity):
        raise AccountError("身份名无效。")
    vault = ROOT / "identities" / identity / "passwords" / (identity + ".kdbx")
    if not vault.is_file():
        raise AccountError("此身份的密码库不存在，未创建或修改密码库。")
    return vault


def account_rows(identity: str) -> list[dict]:
    database = ROOT / "data" / "id.db"
    if not database.exists():
        return []
    with closing(sqlite3.connect(f"file:{database}?mode=ro", uri=True)) as db:
        rows = db.execute("SELECT key,value,note FROM items WHERE identity=? AND category='account' ORDER BY key", (identity,)).fetchall()
    accounts = []
    for key, username, raw in rows:
        try:
            note = json.loads(raw)
        except (TypeError, ValueError):
            note = {}
        if not isinstance(note, dict):
            note = {}
        title = note.get("password_title") or key
        username = note.get("username", username)
        if not isinstance(title, str) or not isinstance(username, str) or not title.strip():
            raise AccountError("账号的标题或用户名格式无效。")
        accounts.append({"title": title, "username": username, "note": note})
    return accounts


def write_entry(identity: str, title: str, username: str | None, *, edit: bool = False) -> int:
    if not title.strip():
        raise AccountError("条目标题不能为空。")
    command = ["keepassxc-cli", "edit" if edit else "add", "-p"]
    if username is not None:
        command += ["-u", username]
    return subprocess.run([*command, str(vault_for(identity)), title], check=False).returncode


def fill_passwords(identity: str | None, title: str | None = None, username: str | None = None) -> int:
    require_terminal()
    identity = select_identity(identity)
    if identity is None:
        return 130
    accounts = account_rows(identity)
    if title:
        if username is None:
            match = next((row for row in accounts if row["title"] == title), None)
            username = match["username"] if match else input("用户名（可空）: ").strip()
        return write_entry(identity, title, username)
    completed = set()
    print("选择已登记账号可预填标题和用户名，也可自定义条目；密码由 KeePassXC 隐藏输入。")
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
        try:
            if note.get("type") != "mail-account" or attach_source(vault_for(identity), title, name, note):
                completed.add(title)
        except AccountError as exc:
            print(str(exc), file=sys.stderr)


def password_states(identity: str) -> dict[str, str]:
    vault = vault_for(identity)
    master = getpass.getpass(f"解锁 {identity} 的密码库（不回显）: ") + "\n"
    try:
        # KeePassXC 在内部检查密码；stdout 始终只有条目路径，不导出密码。
        def query(command):
            result = subprocess.run(["keepassxc-cli", *command], input=master, text=True,
                                    capture_output=True, check=False, env={**os.environ, "LANG": "C", "LC_ALL": "C"})
            no_matches = (command[0] == "search" and result.returncode == 1 and not result.stdout
                          and result.stderr.strip() == "No results for that search term.")
            if result.returncode and not no_matches:
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


def cell_width(value: str) -> int:
    return sum(0 if unicodedata.combining(char) else 2 if unicodedata.east_asian_width(char) in "WF" else 1 for char in value)


def print_status_table(rows: list[tuple[str, str, str]]) -> None:
    headers = ("条目 / 账号", "密码状态", "操作")
    name_width = min(48, max(12, shutil.get_terminal_size((100, 24)).columns - 34))
    table = [headers]
    for name, status, action in rows:
        name = safe_text(name)
        if cell_width(name) > name_width:
            while cell_width(name) > name_width - 1:
                name = name[:-1]
            name += "…"
        table.append((name, status, action))
    widths = [max(cell_width(row[i]) for row in table) for i in range(3)]
    for row in table:
        print("  ".join(value + " " * (widths[i] - cell_width(value)) for i, value in enumerate(row)).rstrip())


def entry_command(target: str, identity: str, title: str, username: str | None = None) -> str:
    values = {"id": identity, "title": title}
    if username is not None:
        values["user"] = username
    if any(any(not char.isprintable() for char in value) for value in values.values()):
        raise AccountError("条目名称或用户名包含控制字符，请先在 KeePassXC 中修正。")
    return "make " + target + " " + " ".join(f"{key}={shlex.quote(value.replace('$', '$$'))}" for key, value in values.items())


def list_passwords(identity: str) -> int:
    states = password_states(identity)
    rows, commands = [], []
    for title, status in states.items():
        commands.append(entry_command("pass-edit", identity, title))
        rows.append((title, status, ("补填" if status == "未填密码" else "修改") + f" [{len(commands)}]"))
    known = set(states)
    for row in account_rows(identity):
        title = row["title"]
        if title.lstrip("/") in known:
            continue
        known.add(title.lstrip("/"))
        commands.append(entry_command("pass-add-manual", identity, title, row["username"]))
        rows.append((title, "未关联条目", f"新增 [{len(commands)}]"))
    print(f"\n身份：{identity}　密码库状态（只显示标题和是否已填密码）\n")
    print_status_table(rows)
    if commands:
        print("\n操作命令（只修改本地密码库）：")
        for index, command in enumerate(commands, 1):
            print(f"[{index}] {command}")
    else:
        print("（暂无条目）")
    if any(status == "未关联条目" for _, status, _ in rows):
        print("\n未关联条目来自资料库；若已用其他标题保存，请使用上方已有条目的修改命令。")
    return 0


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
    args = parser.parse_args()
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
            return list_passwords(identity)
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
    except AccountError as exc:
        print(str(exc), file=sys.stderr)
        return 1
    except (ValueError, OSError, KeyError, IndexError, BadZipFile, ET.ParseError, sqlite3.Error):
        # 解析和数据库异常可能携带原单元格内容，不回显异常详情。
        print("操作未完成，请检查文件格式、归属或本地依赖；未输出原表机密内容。", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
