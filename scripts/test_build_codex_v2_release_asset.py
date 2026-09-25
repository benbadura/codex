from pathlib import Path
import tarfile
import tempfile
import unittest

from build_codex_v2_release_asset import build_release_asset


class BuildCodexV2ReleaseAssetTest(unittest.TestCase):
    def test_builds_release_assets_without_platform_tar(self) -> None:
        for target in ("aarch64-apple-darwin", "x86_64-pc-windows-msvc"):
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                package = root / "package"
                (package / "bin").mkdir(parents=True)
                suffix = ".exe" if "windows" in target else ""
                for name in (f"codex{suffix}", f"codex-code-mode-host{suffix}"):
                    (package / "bin" / name).write_bytes(b"binary")
                (package / "codex-package.json").write_text("{}", encoding="utf-8")
                source_archive = root / "source.tar.gz"
                with tarfile.open(source_archive, "w:gz") as bundle:
                    for child in package.iterdir():
                        bundle.add(child, arcname=child.name)

                installer = root / "installer.py"
                installer.write_text("# installer\n", encoding="utf-8")
                readme = root / "README.md"
                readme.write_text("instructions\n", encoding="utf-8")
                output = root / "release.tar.gz"

                build_release_asset(
                    source_archive,
                    output,
                    target,
                    installer=installer,
                    readme=readme,
                )

                with tarfile.open(output, "r:gz") as bundle:
                    self.assertEqual(
                        bundle.extractfile("codex-v2/install-codex-v2.py").read(),
                        b"# installer\n",
                    )
                    self.assertEqual(
                        bundle.extractfile("codex-v2/README.md").read(),
                        b"instructions\n",
                    )


if __name__ == "__main__":
    unittest.main()
