from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest import mock

import scripts.refresh_yuzu_resource_pack as resource_pack


def write_static_file(root: Path, rel_path: str, content: bytes) -> None:
    path = root / rel_path
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)


def write_static_markers(root: Path) -> None:
    (root / "mai" / "pic").mkdir(parents=True, exist_ok=True)
    (root / "mai" / "cover").mkdir(parents=True, exist_ok=True)


class YuzuResourcePackTests(unittest.TestCase):
    def test_refresh_downloads_extracts_and_overwrites_static(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            static_dir = root / "static"
            write_static_markers(static_dir)
            write_static_file(static_dir, "mai/pic/foo.png", b"old image")

            def fake_download(_url: str, target: Path, _timeout_seconds: int) -> dict:
                target.write_bytes(b"archive")
                return {"contentLength": "123"}

            def fake_extract(_archive: Path, extract_root: Path, _timeout_seconds: int) -> Path:
                source = extract_root / "Resource" / "static"
                write_static_markers(source)
                write_static_file(source, "mai/pic/foo.png", b"new image")
                return source

            with mock.patch.object(
                resource_pack,
                "download_archive",
                side_effect=fake_download,
            ) as download_archive, mock.patch.object(
                resource_pack,
                "extract_archive_source",
                side_effect=fake_extract,
            ) as extract_archive_source:
                result = resource_pack.refresh_resource_pack(
                    url="https://example.test/Resource.7z",
                    static_dir=static_dir,
                    timeout_seconds=1,
                    extract_timeout_seconds=1,
                )

            self.assertTrue(result["ok"])
            self.assertTrue(result["changed"])
            self.assertTrue(result["downloaded"])
            self.assertTrue(result["extracted"])
            self.assertEqual((static_dir / "mai" / "pic" / "foo.png").read_bytes(), b"new image")
            download_archive.assert_called_once()
            extract_archive_source.assert_called_once()


if __name__ == "__main__":
    unittest.main()
