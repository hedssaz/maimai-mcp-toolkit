import unittest

from maimai_mcp import server


class MaimaiSearchFormatTests(unittest.TestCase):
    def _dual_chart_song(self):
        return {
            "title": "Believe the Rainbow",
            "id": "835",
            "artist": "Shoichiro Hirata feat.Sana",
            "bpm": 170,
            "version": "maimai",
            "available_chart_types": ["standard", "dx"],
            "matched_charts": [
                {"chart_type": "standard", "difficulty": "Basic", "difficulty_index": 0, "level": "4", "ds": 4.0},
                {"chart_type": "standard", "difficulty": "Master", "difficulty_index": 3, "level": "13", "ds": 13.4},
                {"chart_type": "dx", "difficulty": "Basic", "difficulty_index": 0, "level": "2", "ds": 2.0},
                {"chart_type": "dx", "difficulty": "Master", "difficulty_index": 3, "level": "13", "ds": 13.0},
            ],
            "aliases": ["相信彩虹"],
            "match": {"field": "alias", "mode": "exact", "value": "相信彩虹"},
        }

    def test_compact_output_marks_available_st_dx_charts(self):
        text = server.format_song_compact(self._dual_chart_song(), 1)

        self.assertIn("ID ST#835 / DX#10835", text)
        self.assertIn("谱面 ST/DX", text)
        self.assertIn("命中 别名(精确): 相信彩虹", text)
        self.assertIn("ST #835: Bas 4/4.0 / Mst 13/13.4", text)
        self.assertIn("DX #10835: Bas 2/2.0 / Mst 13/13.0", text)
        self.assertNotIn("STANDARD", text)

    def test_verbose_output_uses_st_dx_chart_labels(self):
        text = server.format_song(self._dual_chart_song(), 1)

        self.assertIn("编号 ST 835 / DX 10835", text)
        self.assertIn("谱面 ST/DX", text)
        self.assertIn("命中 别名(精确): 相信彩虹", text)
        self.assertIn("ST#835 Master", text)
        self.assertIn("DX#10835 Master", text)
        self.assertIn("DX#10835 Master 等级 13 定数 13.0", text)
        self.assertNotIn("STANDARD", text)

    def test_dx_only_compact_output_uses_five_digit_chart_id(self):
        song = {
            "title": "Help me, ERINNNNNN!!",
            "id": "1853",
            "artist": "ビートまりお",
            "bpm": 183,
            "version": "PRiSM PLUS",
            "available_chart_types": ["dx"],
            "matched_charts": [
                {"chart_type": "dx", "difficulty": "Master", "difficulty_index": 3, "level": "13", "ds": 13.1},
            ],
        }

        text = server.format_song_compact(song, 1)

        self.assertIn("#11853", text)
        self.assertIn("DX #11853: Mst 13/13.1", text)
        self.assertNotIn("#1853", text)


if __name__ == "__main__":
    unittest.main()
