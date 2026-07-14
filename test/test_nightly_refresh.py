from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import scripts.nightly_refresh as nightly_refresh


def write_json(path: Path, payload: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, ensure_ascii=False), encoding="utf-8")


def write_cn_render_inputs(root: Path) -> None:
    write_json(
        root / "data" / "lxns_song_list.json",
        {
            "songs": [
                {
                    "id": 1,
                    "title": "Song",
                    "artist": "Artist",
                    "bpm": 120,
                    "difficulties": {
                        "standard": [
                            {
                                "type": "standard",
                                "difficulty": 0,
                                "level": "5",
                                "level_value": 5.0,
                                "note_designer": "Designer",
                                "notes": {"total": 100},
                            }
                        ]
                    },
                }
            ]
        },
    )
    write_json(
        root / "data" / "divingfish_song_list.json",
        [{"id": 1, "title": "Song", "type": "SD", "level": ["5"], "ds": [5.0]}],
    )
    write_json(root / "data" / "maimaidxplate.json", {"content": {"真": [2, 1]}})


class NightlyRefreshTests(unittest.TestCase):
    def test_canonical_json_ignores_object_key_order(self) -> None:
        self.assertEqual(
            nightly_refresh.canonical_json_bytes({"b": 2, "a": [{"d": 4, "c": 3}]}),
            nightly_refresh.canonical_json_bytes({"a": [{"c": 3, "d": 4}], "b": 2}),
        )

    def test_normalize_scalar_treats_integer_float_as_integer(self) -> None:
        self.assertEqual(nightly_refresh.normalize_scalar(5), nightly_refresh.normalize_scalar(5.0))

    def test_cn_render_fingerprint_ignores_non_render_song_fields(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            write_cn_render_inputs(root)
            before = nightly_refresh.cn_render_input_fingerprint(root)

            write_json(
                root / "data" / "divingfish_song_list.json",
                [
                    {
                        "id": 1,
                        "title": "Renamed",
                        "type": "SD",
                        "level": ["5"],
                        "ds": [5.0],
                        "basic_info": {"artist": "Changed", "bpm": 999},
                        "charts": [{"notes": [999, 0, 0, 0], "charter": "Changed"}],
                    }
                ],
            )
            after = nightly_refresh.cn_render_input_fingerprint(root)

            self.assertEqual(before["sha256"], after["sha256"])

    def test_cn_render_fingerprint_changes_when_song_data_changes(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            write_cn_render_inputs(root)
            before = nightly_refresh.cn_render_input_fingerprint(root)

            write_json(
                root / "data" / "divingfish_song_list.json",
                [{"id": 1, "title": "Song", "type": "SD", "level": ["5"], "ds": [5.1]}],
            )
            after = nightly_refresh.cn_render_input_fingerprint(root)

            self.assertNotEqual(before["sha256"], after["sha256"])

    def test_cn_render_fingerprint_ignores_plate_order(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            write_cn_render_inputs(root)
            before = nightly_refresh.cn_render_input_fingerprint(root)

            write_json(root / "data" / "maimaidxplate.json", {"content": {"真": [1, 2]}})
            after = nightly_refresh.cn_render_input_fingerprint(root)

            self.assertEqual(before["sha256"], after["sha256"])

    def test_cn_render_fingerprint_changes_when_plate_data_changes(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            write_cn_render_inputs(root)
            before = nightly_refresh.cn_render_input_fingerprint(root)

            write_json(root / "data" / "maimaidxplate.json", {"content": {"真": [1, 2, 3]}})
            after = nightly_refresh.cn_render_input_fingerprint(root)

            self.assertNotEqual(before["sha256"], after["sha256"])

    def test_main_skips_resource_download_when_cn_inputs_unchanged(self) -> None:
        with mock.patch.object(nightly_refresh, "SOURCES", []), mock.patch.object(
            nightly_refresh,
            "cn_render_input_fingerprint",
            side_effect=[{"sha256": "same"}, {"sha256": "same"}],
        ), mock.patch.object(nightly_refresh, "run_yuzu_resource_refresh") as resource_refresh, mock.patch.object(
            nightly_refresh,
            "run_render_background_regeneration",
        ) as regenerate:
            with self.assertRaises(SystemExit) as exit_context:
                nightly_refresh.main()

        self.assertEqual(exit_context.exception.code, 0)
        resource_refresh.assert_not_called()
        regenerate.assert_not_called()

    def test_main_downloads_resource_before_regenerating_when_cn_inputs_change(self) -> None:
        events = []

        def fake_resource_refresh() -> dict:
            events.append("resource")
            return {"ok": True, "changed": True}

        def fake_regeneration() -> dict:
            events.append("regenerate")
            return {"ok": True, "returncode": 0, "elapsed": 0, "stdout": "", "stderr": ""}

        with mock.patch.object(nightly_refresh, "SOURCES", []), mock.patch.object(
            nightly_refresh,
            "cn_render_input_fingerprint",
            side_effect=[{"sha256": "before"}, {"sha256": "after"}],
        ), mock.patch.object(
            nightly_refresh,
            "run_yuzu_resource_refresh",
            side_effect=fake_resource_refresh,
        ), mock.patch.object(
            nightly_refresh,
            "run_render_background_regeneration",
            side_effect=fake_regeneration,
        ):
            with self.assertRaises(SystemExit) as exit_context:
                nightly_refresh.main()

        self.assertEqual(exit_context.exception.code, 0)
        self.assertEqual(events, ["resource", "regenerate"])


if __name__ == "__main__":
    unittest.main()
