"""同一交互会话内复用解锁，主密码只通过 stdin 交给 KeePassXC。"""

from contextlib import contextmanager
import getpass
from pathlib import Path
import subprocess
import sys
import time


class VaultError(ValueError):
    """不包含凭据的本地解锁错误。"""


class VaultSession:
    def __init__(self):
        self._password = None
        self.database = None
        self.last_used = 0

    def lock(self):
        if self._password is not None:
            self._password[:] = b"\0" * len(self._password)
        self._password = self.database = None

    def unlock(self, database, *, fresh=False):
        if not sys.stdin.isatty() or not sys.stdout.isatty():
            raise VaultError("会话解锁仅限交互式终端。")
        database = Path(database).resolve()
        if fresh or database != self.database or time.monotonic() - self.last_used >= 300:
            self.lock()
        if self._password is None:
            label = "再次验证主密码" if fresh else "主密码"
            password = getpass.getpass(f"{database.stem} · {label}: ")
            if "\n" in password or "\r" in password:
                raise VaultError("主密码不能包含换行。")
            self._password = bytearray(password.encode())
            del password
            try:
                result = subprocess.run(["keepassxc-cli", "ls", "-q", str(database)],
                                        input=bytes(self._password) + b"\n", stdout=subprocess.DEVNULL,
                                        stderr=subprocess.DEVNULL, check=False)
                if result.returncode:
                    raise VaultError("密码库解锁失败，请重试。")
            except BaseException:
                self.lock()
                raise
            self.database = database
        self.last_used = time.monotonic()

    def run(self, command, database, *, answers=(), **kwargs):
        self.unlock(database)
        payload = bytes(self._password) + b"\n" + b"".join(value.encode() + b"\n" for value in answers)
        kwargs.pop("stdin", None)
        if kwargs.get("text"):
            payload = payload.decode()
        # quiet 隐去 KeePassXC 的重复主密码提示，仍保留命令结果。
        command = [*command[:2], "-q", *command[2:]]
        return subprocess.run(command, input=payload, **kwargs)


_current = None


def current():
    return _current


@contextmanager
def session():
    global _current
    if _current is not None:
        yield _current
        return
    _current = VaultSession()
    try:
        yield _current
    finally:
        _current.lock()
        _current = None


def run_cli(command, database, **kwargs):
    if _current is None:
        return subprocess.run(command, **kwargs)
    return _current.run(command, database, **kwargs)
