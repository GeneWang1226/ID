"""Check toolchain discovery using fake Cargo commands, without opening a vault."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
MAKE = shutil.which("make")


@unittest.skipUnless(MAKE, "install Make first")
class MakeTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="id-make-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        shutil.copyfile(ROOT / "Makefile", self.root / "Makefile")
        for name in ("Cargo.toml", "Cargo.lock"):
            (self.root / name).touch()
        self.home = self.root / "home with spaces"
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.env = {k: v for k, v in os.environ.items()
                    if k not in ("CARGO", "CARGO_HOME", "MAKEFLAGS", "MFLAGS")}
        self.env.update(HOME=str(self.home), PATH=str(self.bin))

    def cargo(self, path, label):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f'#!/bin/sh\nprintf "%s\\n" "{label}:$*"\n')
        path.chmod(0o755)
        return path

    def build(self, *args, env=None):
        return subprocess.run([MAKE, "--no-print-directory", "build", *args],
                              cwd=self.root, env={**self.env, **(env or {})},
                              text=True, capture_output=True, timeout=10)

    def test_rustup_directory_works_without_cargo_on_path(self):
        self.cargo(self.home / ".cargo/bin/cargo", "home")
        result = self.build()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("home:build --locked", result.stdout)

    def test_custom_cargo_home_and_path_precedence(self):
        custom = self.root / "custom cargo"
        self.cargo(custom / "bin/cargo", "custom")
        result = self.build(env={"CARGO_HOME": str(custom)})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("custom:build --locked", result.stdout)
        self.cargo(self.bin / "cargo", "path")
        result = self.build(env={"CARGO_HOME": str(custom)})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("path:build --locked", result.stdout)

    def test_explicit_override_is_used_and_invalid_override_does_not_fall_back(self):
        self.cargo(self.bin / "cargo", "path")
        explicit = self.cargo(self.root / "explicit cargo", "explicit")
        result = self.build("CARGO=" + str(explicit))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("explicit:build --locked", result.stdout)
        result = self.build("CARGO=/missing/cargo")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("未找到 Cargo", result.stdout)

    def test_missing_cargo_explains_installation(self):
        for target in ("build", "test"):
            with self.subTest(target=target):
                result = subprocess.run([MAKE, "--no-print-directory", target],
                                        cwd=self.root, env=self.env, text=True,
                                        capture_output=True, timeout=10)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("Rust 1.89+", result.stdout)
                self.assertIn("https://rust-lang.org/tools/install/", result.stdout)
                self.assertNotIn("No such file", result.stderr)
