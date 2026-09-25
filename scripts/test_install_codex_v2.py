import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import install_codex_v2 as installer


INSTALLER = Path(__file__).with_name("install_codex_v2.py")


class InstallCodexV2Test(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name) / "home ' with spaces"
        self.home.mkdir()
        self.source = self.home / ".codex"
        self.source.mkdir()
        self.destination = self.home / ".codex-v2"
        self.package = self.home / "package"
        (self.package / "bin").mkdir(parents=True)
        (self.package / "codex-package.json").write_text(
            json.dumps(
                {
                    "version": "1.2.3",
                    "target": "aarch64-apple-darwin",
                    "variant": "codex",
                }
            )
        )
        for name in ("codex", "codex-code-mode-host"):
            binary = self.package / "bin" / name
            binary.write_text('#!/bin/sh\nprintf "%s\\n" "$CODEX_HOME" "$@"\n')
            binary.chmod(0o755)
        (self.source / "config.toml").write_text('model = "test"\n')
        (self.source / "auth.json").write_text('{"fake": "credential"}')
        self.output = io.StringIO()
        redirect = contextlib.redirect_stdout(self.output)
        redirect.__enter__()
        self.addCleanup(redirect.__exit__, None, None, None)

    def install(self, clone=None, *, host=None):
        installer.install(self.package, self.home, self.source, clone, host=host)

    def test_rejects_package_for_a_different_host(self):
        with self.assertRaisesRegex(ValueError, "nie pasuje do tego komputera"):
            self.install(clone=False, host=("Windows", "AMD64"))
        self.assertFalse(self.destination.exists())

    def test_prompt_yes_clones_and_wrapper_preserves_arguments(self):
        with (
            patch("sys.stdin.isatty", return_value=True),
            patch("builtins.input", return_value="tak") as prompt,
        ):
            self.install()
        prompt.assert_called_once()
        self.assertEqual(
            (self.destination / "auth.json").read_bytes(),
            (self.source / "auth.json").read_bytes(),
        )
        self.assertEqual(self.destination.stat().st_mode & 0o777, 0o700)
        (self.destination / "config.toml").write_text("changed")
        self.assertEqual((self.source / "config.toml").read_text(), 'model = "test"\n')
        wrapper = self.home / ".local/bin/codex-v2"
        installed = (self.home / ".local/lib/codex-v2/current").resolve()
        self.assertEqual(installed.parent.name, "releases")
        self.assertNotEqual(installed, self.package)
        self.package.rename(self.home / "removed-download")
        result = subprocess.check_output(
            [str(wrapper), "hello world", "$literal"], text=True
        )
        self.assertEqual(
            result.splitlines(),
            [str(self.destination), "--no-daemon", "hello world", "$literal"],
        )

    def test_reinstall_reuses_identical_managed_package(self):
        self.install(clone=False)
        first = (self.home / ".local/lib/codex-v2/current").resolve()
        self.install()
        second = (self.home / ".local/lib/codex-v2/current").resolve()
        self.assertEqual(first, second)
        self.assertEqual(len(list(first.parent.iterdir())), 1)

    def test_release_bundle_installs_without_package_argument(self):
        bundled_installer = self.package / "install-codex-v2.py"
        shutil.copy2(INSTALLER, bundled_installer)
        release_home = Path(self.temp.name) / "release-home"
        release_home.mkdir()
        result = subprocess.run(
            [sys.executable, str(bundled_installer), "--no-clone-state"],
            env={**os.environ, "HOME": str(release_home)},
            check=True,
            capture_output=True,
            text=True,
        )
        wrapper = release_home / ".local/bin/codex-v2"
        self.assertTrue(wrapper.is_file(), result.stdout)
        self.assertTrue((release_home / ".local/lib/codex-v2/current").is_symlink())
        self.assertIn("Zainstalowano pakiet", result.stdout)

    def test_windows_package_creates_cmd_wrapper_without_symlink(self):
        windows_package = self.home / "windows-package"
        (windows_package / "bin").mkdir(parents=True)
        (windows_package / "codex-package.json").write_text(
            json.dumps(
                {
                    "version": "1.2.3",
                    "target": "x86_64-pc-windows-msvc",
                    "variant": "codex",
                }
            )
        )
        for name in ("codex.exe", "codex-code-mode-host.exe"):
            (windows_package / "bin" / name).write_bytes(b"windows binary")
        windows_home = Path(self.temp.name) / "home %USERNAME% with spaces"
        windows_home.mkdir()

        installer.install(
            windows_package,
            windows_home,
            windows_home / ".codex",
            clone=False,
        )

        wrapper = windows_home / ".local/bin/codex-v2.cmd"
        installed = next((windows_home / ".local/lib/codex-v2/releases").iterdir())
        self.assertFalse((windows_home / ".local/lib/codex-v2/current").exists())
        self.assertEqual(
            wrapper.read_text(),
            "@echo off\n"
            "setlocal\n"
            f'set "CODEX_HOME={str(windows_home / ".codex-v2").replace("%", "%%")}"\n'
            f'"{str(installed / "bin/codex.exe").replace("%", "%%")}" '
            "--no-daemon %*\n",
        )

    def test_decline_and_empty_answer_create_clean_state(self):
        for answer in ("nie", ""):
            with (
                self.subTest(answer=answer),
                patch("sys.stdin.isatty", return_value=True),
                patch("builtins.input", return_value=answer),
            ):
                self.install()
                self.assertEqual(list(self.destination.iterdir()), [])
                self.destination.rmdir()

    def test_noninteractive_requires_explicit_choice(self):
        with patch("sys.stdin.isatty", return_value=False):
            with self.assertRaisesRegex(ValueError, "--clone-state"):
                self.install()
            self.assertFalse(self.destination.exists())
            self.install(clone=False)
        self.assertEqual(list(self.destination.iterdir()), [])

    def test_existing_state_is_preserved_and_explicit_clone_refused(self):
        self.destination.mkdir()
        marker = self.destination / "config.toml"
        marker.write_text("existing")
        with patch("builtins.input") as prompt:
            self.install()
        prompt.assert_not_called()
        with self.assertRaisesRegex(ValueError, "już istnieje"):
            self.install(clone=True)
        self.assertEqual(marker.read_text(), "existing")

    def test_missing_source_explicit_clone_fails_before_install(self):
        self.source = self.home / "missing"
        with self.assertRaisesRegex(ValueError, "źródłowego"):
            self.install(clone=True)
        self.assertFalse(self.destination.exists())
        self.install()
        self.assertEqual(list(self.destination.iterdir()), [])

    def test_clone_includes_wal_and_relocates_rollouts(self):
        sessions = self.source / "sessions"
        sessions.mkdir()
        rollout = sessions / "test.jsonl"
        rollout.write_text("history")
        database = sqlite3.connect(self.source / "state_5.sqlite")
        self.addCleanup(database.close)
        database.execute("PRAGMA journal_mode=WAL")
        database.execute("CREATE TABLE threads (rollout_path TEXT)")
        database.executemany(
            "INSERT INTO threads VALUES (?)",
            [(str(rollout),), (str(self.home / "elsewhere/session.jsonl"),)],
        )
        database.commit()
        self.install(clone=True)
        copied = sqlite3.connect(self.destination / "state_5.sqlite")
        self.addCleanup(copied.close)
        self.assertEqual(
            copied.execute("SELECT * FROM threads").fetchall(),
            [
                (str(self.destination / "sessions/test.jsonl"),),
                (str(self.home / "elsewhere/session.jsonl"),),
            ],
        )
        self.assertEqual(
            database.execute("SELECT * FROM threads LIMIT 1").fetchall(),
            [(str(rollout),)],
        )
        self.assertEqual(
            (self.destination / "sessions/test.jsonl").read_text(), "history"
        )

    def test_skips_symlinks_and_runtime_and_refuses_linked_destination(self):
        (self.source / "skills").symlink_to(self.source, target_is_directory=True)
        (self.source / "tmp").mkdir()
        (self.source / "tmp/live").write_text("runtime")
        (self.source / "server.lock").write_text("lock")
        self.destination.symlink_to(self.source, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "dowiązaniem"):
            self.install(clone=True)
        self.destination.unlink()
        self.install(clone=True)
        self.assertEqual(
            sorted(path.name for path in self.destination.iterdir()),
            ["auth.json", "config.toml"],
        )
        self.assertIn("skills", self.output.getvalue())

    def test_copy_failure_does_not_publish_partial_state(self):
        with patch("shutil.copytree", side_effect=OSError("copy failed")):
            with self.assertRaisesRegex(OSError, "copy failed"):
                self.install(clone=True)
        self.assertFalse(self.destination.exists())
        self.assertEqual(list(self.home.glob(".codex-v2-clone-*")), [])
        self.assertFalse((self.home / ".local/bin/codex-v2").exists())


if __name__ == "__main__":
    unittest.main()
