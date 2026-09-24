#!/usr/bin/env python3
"""Install a locally assembled codex-v2 package on macOS/Linux."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import sqlite3
import stat
import sys
import tempfile


SAFE_BUILD_PART = re.compile(r"^[A-Za-z0-9._-]+$")


def validate_package(package: Path) -> tuple[Path, str]:
    package = package.resolve()
    metadata_path = package / "codex-package.json"
    try:
        metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(
            f"Nieprawidłowe metadane pakietu {metadata_path}: {error}"
        ) from error
    if metadata.get("variant") != "codex":
        raise ValueError(f"Pakiet {package} nie jest wariantem codex.")
    identity = []
    for key in ("version", "target"):
        value = metadata.get(key)
        if not isinstance(value, str) or not SAFE_BUILD_PART.fullmatch(value):
            raise ValueError(f"Nieprawidłowe pole {key!r} w {metadata_path}.")
        identity.append(value)
    for binary in ("codex", "codex-code-mode-host"):
        path = package / "bin" / binary
        if not path.is_file() or not os.access(path, os.X_OK):
            raise ValueError(f"Brak wykonywalnego pliku pakietu: {path}")
    digest = hashlib.sha256()
    with (package / "bin/codex").open("rb") as binary:
        while chunk := binary.read(1024 * 1024):
            digest.update(chunk)
    build_id = f"{identity[0]}-{identity[1]}-{digest.hexdigest()[:12]}"
    return package, build_id


def install_package(package: Path, home: Path, build_id: str) -> Path:
    releases = home / ".local/lib/codex-v2/releases"
    releases.mkdir(parents=True, exist_ok=True)
    try:
        relative = package.relative_to(releases.resolve())
    except ValueError:
        relative = None
    if relative is not None and len(relative.parts) == 1:
        return package

    installed = releases / build_id
    if installed.exists():
        existing, existing_id = validate_package(installed)
        if existing_id != build_id:
            raise ValueError(f"Istniejący pakiet ma inną zawartość: {installed}")
        return existing

    with tempfile.TemporaryDirectory(prefix=".codex-v2-package-", dir=releases) as temp:
        staged = Path(temp) / build_id
        shutil.copytree(package, staged)
        staged.rename(installed)
    return installed


def clone_state(source: Path, destination: Path) -> None:
    """Stage an independent copy before publishing it; never merge existing state."""
    if not source.is_dir():
        raise ValueError(f"Brak katalogu źródłowego: {source}")
    if destination.is_symlink() or destination.exists():
        raise ValueError(
            f"Stan docelowy już istnieje: {destination}. Nie nadpisuję go."
        )
    source_prefixes = {str(source.absolute()) + os.sep, str(source.resolve()) + os.sep}
    source = source.resolve()
    resolved_destination = destination.resolve()
    if source == resolved_destination or source in resolved_destination.parents:
        raise ValueError("Katalog docelowy nie może znajdować się wewnątrz źródła.")
    skipped = []
    databases = []

    def ignore(directory: str, names: list[str]) -> list[str]:
        excluded = []
        for name in names:
            path = Path(directory) / name
            mode = path.lstat().st_mode
            if (
                (Path(directory) == source and name in {"tmp", "locks", "daemon"})
                or name.endswith((".lock", ".sock", ".pid"))
                or not (stat.S_ISDIR(mode) or stat.S_ISREG(mode))
            ):
                excluded.append(name)
                skipped.append(str(path.relative_to(source)))
            elif path.is_file():
                if name.endswith(("-wal", "-shm", "-journal")):
                    # SQLite's backup API includes committed WAL data.
                    if path.with_name(name.rsplit("-", 1)[0]).is_file():
                        excluded.append(name)
                else:
                    with path.open("rb") as handle:
                        is_sqlite = handle.read(16) == b"SQLite format 3\x00"
                    if is_sqlite:
                        databases.append(path)
                        excluded.append(name)
        return excluded

    with tempfile.TemporaryDirectory(
        prefix=".codex-v2-clone-", dir=destination.parent
    ) as temp:
        staged = Path(temp) / "state"
        shutil.copytree(source, staged, ignore=ignore)
        staged.chmod(0o700)
        for database in databases:
            copied = staged / database.relative_to(source)
            reader = sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)
            writer = sqlite3.connect(copied)
            try:
                reader.backup(writer)
                # The session index stores absolute paths. Resume must write to
                # copied rollouts, not the original installation's history.
                columns = writer.execute("PRAGMA table_info(threads)").fetchall()
                if any(column[1] == "rollout_path" for column in columns):
                    for prefix in source_prefixes:
                        writer.execute(
                            "UPDATE threads SET rollout_path = ? || substr(rollout_path, ?) "
                            "WHERE substr(rollout_path, 1, ?) = ?",
                            (
                                str(destination) + os.sep,
                                len(prefix) + 1,
                                len(prefix),
                                prefix,
                            ),
                        )
                writer.commit()
            finally:
                reader.close()
                writer.close()
            shutil.copymode(database, copied)
        staged.rename(destination)
    print(f"Skopiowano stan: {source} → {destination}")
    if skipped:
        print(f"Pominięto dowiązania i pliki runtime ({len(skipped)}):")
        for name in skipped:
            print(f"  {name}")


def install(package: Path, home: Path, source: Path, clone: bool | None) -> None:
    package, build_id = validate_package(package)
    destination = home / ".codex-v2"
    current = home / ".local/lib/codex-v2/current"
    wrapper = home / ".local/bin/codex-v2"
    if (current.exists() or current.is_symlink()) and not current.is_symlink():
        raise ValueError(f"Ścieżka buildu nie jest dowiązaniem: {current}")
    if wrapper.is_symlink():
        raise ValueError(f"Wrapper jest dowiązaniem: {wrapper}. Nie nadpisuję go.")
    if destination.is_symlink():
        raise ValueError(f"Katalog stanu jest dowiązaniem: {destination}.")
    if clone is None:
        if destination.exists():
            print(f"Zachowuję istniejący stan {destination}; pomijam klonowanie.")
            clone = False
        elif not source.is_dir():
            print(f"Brak stanu w {source}; tworzę pusty katalog codex-v2.")
            clone = False
        elif not sys.stdin.isatty():
            raise ValueError(
                "Bez terminala wybierz --clone-state lub --no-clone-state."
            )
        else:
            print("Przed kopiowaniem zamknij sesje i aplikacje korzystające ze źródła.")
            print(
                "Kopia obejmuje konfigurację, historię, sesje i auth.json (dane logowania)."
            )
            try:
                answer = input(
                    f"Sklonować stan oryginalnego Codexa z {source}? [t/N]: "
                )
            except EOFError:
                answer = ""
            clone = answer.strip().lower() in {"t", "tak", "y", "yes"}
    if clone:
        clone_state(source, destination)
    else:
        destination.mkdir(mode=0o700, exist_ok=True)
    package = install_package(package, home, build_id)
    current.parent.mkdir(parents=True, exist_ok=True)
    wrapper.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=current.parent) as temp:
        link = Path(temp) / "current"
        link.symlink_to(package, target_is_directory=True)
        link.replace(current)
    # Quote literal paths: spaces and shell metacharacters in HOME are valid.
    with tempfile.TemporaryDirectory(dir=wrapper.parent) as temp:
        launcher = Path(temp) / "codex-v2"
        launcher.write_text(
            "#!/bin/sh\nexec env "
            + shlex.quote(f"CODEX_HOME={destination}")
            + " "
            + shlex.quote(str(current / "bin/codex"))
            + ' --no-daemon "$@"\n',
            encoding="utf-8",
        )
        launcher.chmod(0o755)
        launcher.replace(wrapper)
    print(
        f"Zainstalowano pakiet {package}\n"
        f"Utworzono {wrapper}\n"
        f"Dodaj {wrapper.parent} do PATH."
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--package-dir",
        type=Path,
        default=Path(__file__).resolve().parent,
        help="Katalog pakietu; domyślnie katalog zawierający instalator.",
    )
    parser.add_argument("--source-home", type=Path, default=Path.home() / ".codex")
    choice = parser.add_mutually_exclusive_group()
    choice.add_argument("--clone-state", dest="clone", action="store_true")
    choice.add_argument("--no-clone-state", dest="clone", action="store_false")
    parser.set_defaults(clone=None)
    args = parser.parse_args()
    if os.name != "posix":
        parser.error("Ten wrapper wymaga macOS lub Linux.")
    try:
        install(
            args.package_dir, Path.home(), args.source_home.expanduser(), args.clone
        )
    except (OSError, ValueError, sqlite3.Error) as error:
        print(f"Błąd instalacji: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
