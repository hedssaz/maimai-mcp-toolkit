from __future__ import annotations

import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock
from urllib.error import HTTPError

from maimai_mcp import source_refresh
import scripts.update_divingfish_data as update_divingfish_data


class SourceRefreshTests(unittest.TestCase):
    def test_source_status_expires_when_any_target_is_stale(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            fresh = root / "fresh.json"
            stale = root / "stale.json"
            fresh.write_text("[]", encoding="utf-8")
            stale.write_text("[]", encoding="utf-8")
            now = time.time()
            fresh_time = now - 60
            stale_time = now - 3600
            fresh.touch()
            stale.touch()
            import os

            os.utime(fresh, (fresh_time, fresh_time))
            os.utime(stale, (stale_time, stale_time))

            with mock.patch.object(source_refresh, "ROOT", root), mock.patch.object(
                source_refresh,
                "source_definitions",
                return_value={
                    "combo": {
                        "label": "combo",
                        "targets": [fresh, stale],
                        "command": ["true"],
                    }
                },
            ):
                status = source_refresh.source_status("combo", ttl_days=30 / 1440, now=now)

        self.assertTrue(status["expired"])
        self.assertEqual(["stale.json"], status["expired_targets"])

    def test_divingfish_304_touches_existing_target(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            target = root / "divingfish_song_list.json"
            etag_file = root / ".divingfish_etag"
            target.write_text("[]", encoding="utf-8")
            etag_file.write_text("etag", encoding="utf-8")
            old_time = time.time() - 3600
            import os

            os.utime(target, (old_time, old_time))

            http_error = HTTPError(
                url=update_divingfish_data.URL,
                code=304,
                msg="Not Modified",
                hdrs={},
                fp=None,
            )
            with mock.patch.object(update_divingfish_data, "TARGET", target), mock.patch.object(
                update_divingfish_data, "ETAG_FILE", etag_file
            ), mock.patch("urllib.request.urlopen", side_effect=http_error):
                count, updated = update_divingfish_data.download_json()
            refreshed_mtime = target.stat().st_mtime

        self.assertEqual(0, count)
        self.assertFalse(updated)
        self.assertGreater(refreshed_mtime, old_time)


if __name__ == "__main__":
    unittest.main()
