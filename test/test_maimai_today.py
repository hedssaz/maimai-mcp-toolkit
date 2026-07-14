from __future__ import annotations

import unittest
from datetime import datetime
from unittest.mock import patch
from zoneinfo import ZoneInfo

from maimai_mcp import server as mcp_server
from maimai_mcp.today import Song, format_today_maimai, qqhash, song_from_search_result, today_maimai


class TodayMaimaiTest(unittest.TestCase):
    def test_uses_shifted_hash_for_recommended_song(self) -> None:
        songs = [
            Song(id="1", title="A", ds=("1.0",)),
            Song(id="2", title="B", ds=("2.0",)),
            Song(id="3", title="C", ds=("3.0",)),
        ]
        now = datetime(2026, 5, 31, 12, 0, tzinfo=ZoneInfo("Asia/Shanghai"))

        result = today_maimai(123456789, songs, now)

        self.assertEqual(qqhash(123456789, now), 126832560)
        self.assertEqual(result["rp"], 60)
        self.assertEqual(result["good"], ["越级", "夜勤", "练底力", "干饭", "抓绝赞"])
        self.assertEqual(result["bad"], ["拼机", "推分", "练手法", "收歌"])
        self.assertEqual(result["song"].title, "A")

    def test_offset_changes_original_day_hash_input(self) -> None:
        songs = [
            Song(id="1", title="A", ds=("1.0",)),
            Song(id="2", title="B", ds=("2.0",)),
            Song(id="3", title="C", ds=("3.0",)),
        ]
        now = datetime(2026, 5, 31, 12, 0, tzinfo=ZoneInfo("Asia/Shanghai"))

        result = today_maimai(123456789, songs, now, offset=7)

        self.assertEqual(qqhash(123456789, now, offset=7), 130208332)
        self.assertEqual(result["rp"], 32)
        self.assertEqual(result["song"].title, "B")

    def test_format_matches_original_text_shape(self) -> None:
        songs = [Song(id="834", title="PANDORA PARADOXXX", ds=("6.0", "8.0", "13.4", "14.8"))]
        now = datetime(2026, 5, 31, 12, 0, tzinfo=ZoneInfo("Asia/Shanghai"))

        text = format_today_maimai("MaiBot", 123456789, songs, now)

        self.assertIn("今日人品值：60", text)
        self.assertIn("MaiBot提醒您：以上内容均由程序自动生成，仅供娱乐参考", text)
        self.assertIn("ID.834 - PANDORA PARADOXXX", text)
        self.assertTrue(text.endswith("6.0/8.0/13.4/14.8"))

    def test_query_today_maimai_default_bot_name(self) -> None:
        songs = [
            {
                "id": "834",
                "title": "PANDORA PARADOXXX",
                "matched_charts": [{"difficulty_index": 3, "ds": 14.8}],
            }
        ]

        with patch("maimai_mcp.server.collect_song_results", return_value=(songs, {})):
            result = mcp_server.query_today_maimai({"qq": "123456789"})

        self.assertIn("MaiBot提醒您：以上内容均由程序自动生成，仅供娱乐参考", result["text"])

    def test_search_result_without_numeric_id_is_not_a_today_candidate(self) -> None:
        self.assertIsNone(
            song_from_search_result(
                {
                    "id": "Xaleid◆scopiX",
                    "source_id": "Xaleid◆scopiX",
                    "source_ids": {"unsupported": "Xaleid◆scopiX"},
                    "title": "Xaleid◆scopiX",
                    "matched_charts": [{"chart_type": "dx", "difficulty_index": 3, "ds": 14.9}],
                }
            )
        )

    def test_search_result_uses_numeric_source_id_for_today_candidate(self) -> None:
        song = song_from_search_result(
            {
                "id": "Technicians High",
                "source_ids": {"unsupported": "Technicians High", "cn": "11790"},
                "title": "Technicians High",
                "matched_charts": [{"chart_type": "dx", "difficulty_index": 3, "ds": 14.2}],
            }
        )

        self.assertIsNotNone(song)
        self.assertEqual(song.id, "11790")


if __name__ == "__main__":
    unittest.main()
