from __future__ import annotations

import hashlib
import os
import stat
import tempfile
import unittest
import zipfile
from pathlib import Path, PurePosixPath

from scripts.package_astrbot_plugin import (
    ARCHIVE_FILENAMES,
    ARCHIVE_ROOT,
    FIXED_ZIP_TIMESTAMP,
    INCLUDED_FILES,
    build_plugin_archives,
    main,
    verify_plugin_archive,
)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class PackageAstrBotPluginTests(unittest.TestCase):
    def make_source(self, root: Path) -> Path:
        source = root / "source-with-private-looking-parent" / ARCHIVE_ROOT
        source.mkdir(parents=True)
        for index, name in enumerate(INCLUDED_FILES, 1):
            (source / name).write_bytes(f"fixture-{index}:{name}\n".encode("utf-8"))

        # These files deliberately exist but must never enter either archive.
        (source / "secret-local-file.txt").write_text("not packaged", encoding="utf-8")
        pycache = source / "__pycache__"
        pycache.mkdir()
        (pycache / "main.cpython-312.pyc").write_bytes(b"not packaged")
        return source

    def test_builds_both_archives_with_exact_root_and_source_parity(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = self.make_source(root)
            output_dir = root / "dist"

            archives = build_plugin_archives(source, output_dir)

            self.assertEqual(tuple(path.name for path in archives), ARCHIVE_FILENAMES)
            self.assertEqual(archives[0].read_bytes(), archives[1].read_bytes())
            expected_names = [
                f"{ARCHIVE_ROOT}/",
                *(f"{ARCHIVE_ROOT}/{name}" for name in INCLUDED_FILES),
            ]
            for archive_path in archives:
                with self.subTest(archive=archive_path.name):
                    with zipfile.ZipFile(archive_path) as archive:
                        self.assertEqual(archive.namelist(), expected_names)
                        self.assertIsNone(archive.testzip())
                        for name in INCLUDED_FILES:
                            self.assertEqual(
                                archive.read(f"{ARCHIVE_ROOT}/{name}"),
                                (source / name).read_bytes(),
                            )

    def test_archive_metadata_is_fixed_and_contains_no_local_paths(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = self.make_source(root)
            archive_path = build_plugin_archives(source, root / "dist")[0]

            with zipfile.ZipFile(archive_path) as archive:
                self.assertEqual(archive.comment, b"")
                for info in archive.infolist():
                    member = PurePosixPath(info.filename)
                    self.assertEqual(info.date_time, FIXED_ZIP_TIMESTAMP)
                    self.assertFalse(member.is_absolute())
                    self.assertNotIn("..", member.parts)
                    self.assertNotIn("__pycache__", member.parts)
                    self.assertNotIn(str(source), info.filename)
                    self.assertEqual(info.extra, b"")
                    self.assertEqual(info.comment, b"")
                    mode = info.external_attr >> 16
                    if info.is_dir():
                        self.assertTrue(stat.S_ISDIR(mode))
                        self.assertEqual(stat.S_IMODE(mode), 0o755)
                    else:
                        self.assertTrue(stat.S_ISREG(mode))
                        self.assertEqual(stat.S_IMODE(mode), 0o644)

    def test_rebuild_is_byte_deterministic_across_mtime_and_mode_changes(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = self.make_source(root)
            first = build_plugin_archives(source, root / "first")

            for index, name in enumerate(INCLUDED_FILES, 1):
                path = source / name
                os.utime(path, (1_700_000_000 + index, 1_700_000_000 + index))
                path.chmod(0o600 if index % 2 else 0o755)
            second = build_plugin_archives(source, root / "second")

            self.assertEqual(
                [sha256(path) for path in first], [sha256(path) for path in second]
            )
            self.assertEqual(first[0].read_bytes(), second[0].read_bytes())

    def test_existing_archives_require_explicit_overwrite(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = self.make_source(root)
            output_dir = root / "dist"
            archives = build_plugin_archives(source, output_dir)
            before = [sha256(path) for path in archives]

            with self.assertRaises(FileExistsError):
                build_plugin_archives(source, output_dir)

            self.assertEqual([sha256(path) for path in archives], before)
            rebuilt = build_plugin_archives(source, output_dir, overwrite=True)
            self.assertEqual([sha256(path) for path in rebuilt], before)

    def test_verifier_rejects_source_drift_and_unexpected_members(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = self.make_source(root)
            archive_path = build_plugin_archives(source, root / "dist")[0]

            (source / "main.py").write_text(
                "changed after packaging\n", encoding="utf-8"
            )
            with self.assertRaises(ValueError):
                verify_plugin_archive(archive_path, source)

            (source / "main.py").write_bytes(
                f"fixture-{INCLUDED_FILES.index('main.py') + 1}:main.py\n".encode(
                    "utf-8"
                )
            )
            with zipfile.ZipFile(archive_path, "a") as archive:
                archive.writestr(f"{ARCHIVE_ROOT}/__pycache__/main.pyc", b"bad")
            with self.assertRaises(ValueError):
                verify_plugin_archive(archive_path, source)

    def test_symlinked_source_file_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = self.make_source(root)
            outside = root / "outside.py"
            outside.write_text("outside\n", encoding="utf-8")
            target = source / "main.py"
            target.unlink()
            try:
                target.symlink_to(outside)
            except (NotImplementedError, OSError):
                self.skipTest("symlinks are unavailable on this platform")

            with self.assertRaises(ValueError):
                build_plugin_archives(source, root / "dist")

    def test_cli_builds_only_inside_explicit_temporary_output(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = self.make_source(root)
            output_dir = root / "cli-dist"

            exit_code = main(
                [
                    "--source-dir",
                    str(source),
                    "--output-dir",
                    str(output_dir),
                ]
            )

            self.assertEqual(exit_code, 0)
            self.assertEqual(
                sorted(path.name for path in output_dir.iterdir()),
                sorted(ARCHIVE_FILENAMES),
            )


if __name__ == "__main__":
    unittest.main()
