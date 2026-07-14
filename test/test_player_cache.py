from __future__ import annotations

import json
import os
import tempfile
import unittest
from datetime import datetime, timedelta, timezone

from player_cache import store


class PlayerCacheTests(unittest.TestCase):
    def setUp(self) -> None:
        self.previous = os.environ.get("PLAYER_CACHE_DIR")
        self.tmp = tempfile.TemporaryDirectory()
        os.environ["PLAYER_CACHE_DIR"] = self.tmp.name

    def tearDown(self) -> None:
        if self.previous is None:
            os.environ.pop("PLAYER_CACHE_DIR", None)
        else:
            os.environ["PLAYER_CACHE_DIR"] = self.previous
        self.tmp.cleanup()

    def test_write_then_read_b50_roundtrip(self) -> None:
        store.write_player_b50("12345", {"player": {"rating": 15000}, "charts": {"sd": [], "dx": []}})
        entry = store.read_player_b50("12345")
        self.assertIsNotNone(entry)
        self.assertEqual(entry["qq"], "12345")
        self.assertEqual(entry["b50"]["player"]["rating"], 15000)
        self.assertTrue(store.is_player_b50_fresh(entry))

    def test_write_then_read_records_roundtrip(self) -> None:
        records_doc = {"nickname": "tester", "rating": 14000, "records": [{"song_id": 100, "ra": 300}]}
        store.write_player_records("999", records_doc)
        entry = store.read_player_records("999")
        self.assertIsNotNone(entry)
        self.assertEqual(entry["records"]["records"][0]["song_id"], 100)
        self.assertTrue(store.is_player_records_fresh(entry))

    def test_missing_file_returns_none(self) -> None:
        self.assertIsNone(store.read_player_b50("999999"))
        self.assertIsNone(store.read_player_records("999999"))
        self.assertFalse(store.is_player_b50_fresh(None))

    def test_numeric_qq_accepted(self) -> None:
        store.write_player_b50(67890, {"x": 1})
        self.assertIsNotNone(store.read_player_b50("67890"))
        self.assertIsNotNone(store.read_player_b50(67890))

    def test_stale_entry_marked_not_fresh(self) -> None:
        # 写入后手动篡改时间戳到 2 天前
        store.write_player_b50("42", {"x": 1})
        path = store._b50_path("42")
        data = json.loads(path.read_text("utf-8"))
        stale_ts = (datetime.now(timezone.utc) - timedelta(days=2)).isoformat()
        data["fetchedAt"] = stale_ts
        path.write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")

        entry = store.read_player_b50("42")
        self.assertIsNotNone(entry)
        self.assertFalse(store.is_player_b50_fresh(entry))

    def test_invalid_input_silently_skipped(self) -> None:
        store.write_player_b50("", {"x": 1})  # empty qq
        store.write_player_b50("xx", "not a dict")  # wrong shape
        self.assertIsNone(store.read_player_b50(""))
        self.assertIsNone(store.read_player_b50("xx"))

    def test_custom_ttl(self) -> None:
        # ttl_seconds 覆盖仍有效，独立于每日重置逻辑
        store.write_player_b50("42", {"x": 1})
        path = store._b50_path("42")
        data = json.loads(path.read_text("utf-8"))
        recent_ts = (datetime.now(timezone.utc) - timedelta(hours=12)).isoformat()
        data["fetchedAt"] = recent_ts
        path.write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")

        entry = store.read_player_b50("42")
        self.assertIsNotNone(entry)
        # 显式传 ttl_seconds 时走固定时长判断，与每日重置无关
        self.assertTrue(store.is_player_b50_fresh(entry, ttl_seconds=24 * 3600))
        self.assertFalse(store.is_player_b50_fresh(entry, ttl_seconds=6 * 3600))

    def test_records_fresh_and_stale(self) -> None:
        store.write_player_records("abc", {"records": [1, 2, 3]})
        entry = store.read_player_records("abc")
        self.assertIsNotNone(entry)
        self.assertTrue(store.is_player_records_fresh(entry))

        # 篡改到 2 天前
        path = store._records_path("abc")
        data = json.loads(path.read_text("utf-8"))
        data["fetchedAt"] = (datetime.now(timezone.utc) - timedelta(days=2)).isoformat()
        path.write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")

        stale = store.read_player_records("abc")
        self.assertIsNotNone(stale)
        self.assertFalse(store.is_player_records_fresh(stale))

    def test_corrupted_json_returns_none(self) -> None:
        store.write_player_b50("corrupt", {"x": 1})
        path = store._b50_path("corrupt")
        path.write_text("not valid json{{{", encoding="utf-8")
        self.assertIsNone(store.read_player_b50("corrupt"))

    def test_qq_whitespace_normalized(self) -> None:
        store.write_player_b50("  12345  ", {"y": 1})
        self.assertIsNotNone(store.read_player_b50("12345"))
        self.assertIsNotNone(store.read_player_b50("  12345  "))


if __name__ == "__main__":
    unittest.main()
