#!/usr/bin/env python3
"""Wrap a canonical Codex package with the codex-v2 installer files."""

import argparse
from pathlib import Path
import shutil
import tarfile
import tempfile


SCRIPT_DIR = Path(__file__).resolve().parent
DEFAULT_INSTALLER = SCRIPT_DIR / "install_codex_v2.py"
DEFAULT_README = SCRIPT_DIR / "codex_v2_release_README.md"


def build_release_asset(
    source_archive: Path,
    output_archive: Path,
    target: str,
    installer: Path = DEFAULT_INSTALLER,
    readme: Path = DEFAULT_README,
) -> None:
    output_archive.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="codex-v2-release-") as temp_dir:
        package_dir = Path(temp_dir) / "codex-v2"
        package_dir.mkdir()
        with tarfile.open(source_archive, "r:gz") as bundle:
            bundle.extractall(package_dir, filter="data")
        shutil.copy2(installer, package_dir / "install-codex-v2.py")
        shutil.copy2(readme, package_dir / "README.md")
        with tarfile.open(output_archive, "w:gz") as bundle:
            bundle.add(package_dir, arcname="codex-v2")

    validate_release_asset(output_archive, target)


def validate_release_asset(archive: Path, target: str) -> None:
    suffix = ".exe" if "windows" in target else ""
    required = {
        f"codex-v2/bin/codex{suffix}",
        f"codex-v2/bin/codex-code-mode-host{suffix}",
        "codex-v2/codex-package.json",
        "codex-v2/install-codex-v2.py",
        "codex-v2/README.md",
    }
    with tarfile.open(archive, "r:gz") as bundle:
        names = set(bundle.getnames())
    missing = required - names
    if missing:
        raise RuntimeError(f"{target}: release bundle is missing {sorted(missing)}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-archive", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--target", required=True)
    args = parser.parse_args()
    build_release_asset(args.source_archive, args.output, args.target)
    print(f"Built codex-v2 release asset at {args.output.resolve()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
