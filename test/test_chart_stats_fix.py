"""Test chart_stats_lookup_ids and attach_fit_stats fixes.

Verifies:
  1. DX charts never get SD chart_stats by ID fallback.
  2. DX charts never get SD fit data via attach_fit_stats when levels differ.
  3. SD charts still correctly receive fit data.
"""

from __future__ import annotations

import json
import unittest
from pathlib import Path

from maimai_mcp.search import (
    DEFAULT_CHART_STATS_PATH,
    DEFAULT_DATA_PATH,
    attach_fit_stats,
    chart_stats_lookup_ids,
    load_chart_stats,
    load_music_data,
    source_records,
    search_songs,
)


PACKAGE_ROOT = Path(__file__).resolve().parent


def _dx_charts_for_song(title: str, ds_match: float | None = None, level_match: str | None = None):
    """Search for a song and return only its DX matched_charts."""
    result = search_songs(query=title)
    songs = result.get("songs", [])
    if not songs:
        return []
    song = songs[0]
    charts = song.get("matched_charts", [])
    dx = [c for c in charts if c.get("chart_type") == "dx"]
    if ds_match is not None:
        dx = [c for c in dx if float(c.get("ds", 0)) == ds_match]
    if level_match is not None:
        dx = [c for c in dx if c.get("level") == level_match]
    return dx


def _sd_charts_for_song(title: str, ds_match: float | None = None):
    result = search_songs(query=title)
    songs = result.get("songs", [])
    if not songs:
        return []
    song = songs[0]
    charts = song.get("matched_charts", [])
    sd = [c for c in charts if c.get("chart_type") == "standard"]
    if ds_match is not None:
        sd = [c for c in sd if float(c.get("ds", 0)) == ds_match]
    return sd


class TestChartStatsLookupIds(unittest.TestCase):
    """Direct tests for chart_stats_lookup_ids."""

    @classmethod
    def setUpClass(cls):
        cls.data = load_music_data()

    def _music_by_title(self, title):
        for music in self.data:
            from maimai_mcp.search import music_title_values, normalize_text
            for t in music_title_values(music):
                if normalize_text(t) == normalize_text(title):
                    return music
        return None

    def test_dx_chart_does_not_fallback_to_raw_sd_id(self):
        """バッド・ダンス・ホール DX chart should not return '622' as lookup ID."""
        music = self._music_by_title("バッド・ダンス・ホール")
        self.assertIsNotNone(music, "Song not found")

        fake_dx_chart = {"chart_type": "dx", "difficulty_index": 0}
        ids = chart_stats_lookup_ids(music, fake_dx_chart)

        # Should only contain "10622" (mapped from LXNS cn id=622), NOT raw "622"
        self.assertNotIn(
            "622", ids,
            "DX chart should NOT fallback to raw SD ID '622'",
        )
        self.assertIn(
            "10622", ids,
            "DX chart should attempt +10000 mapped ID '10622'",
        )

    def test_sd_chart_uses_raw_id(self):
        music = self._music_by_title("バッド・ダンス・ホール")
        fake_sd_chart = {"chart_type": "standard", "difficulty_index": 0}
        ids = chart_stats_lookup_ids(music, fake_sd_chart)
        self.assertIn("622", ids, "SD chart should use raw ID '622'")
        self.assertNotIn("10622", ids, "SD chart should NOT use +10000 key")

    def test_dx_chart_with_cn_id_gets_plus_10000(self):
        """DX chart from a song with cn source: cn id < 10000 should be
        mapped to +10000 for chart_stats lookup."""
        # ハッピーシンセサイザ has cn id=44, chart_stats has key 10044 for DX
        music = self._music_by_title("ハッピーシンセサイザ")
        if music is None:
            self.skipTest("ハッピーシンセサイザ not found in data")
        fake_dx_chart = {"chart_type": "dx", "difficulty_index": 0}
        ids = chart_stats_lookup_ids(music, fake_dx_chart)
        self.assertIn("10044", ids, "DX chart should map cn id=44 to 10044")
        self.assertNotIn("44", ids, "DX chart should NOT fallback to raw id=44")


class TestAttachFitStatsLevelGuard(unittest.TestCase):
    """Tests for the level_matches guard added to attach_fit_stats."""

    @classmethod
    def setUpClass(cls):
        cls.chart_stats = load_chart_stats()

    def test_dx_basic_no_sd_fit(self):
        """DX Basic should NOT fall back to raw SD chart_stats ID."""
        music = {"_source_records": {"cn": {"id": 622}, "divingfish": {"id": 622}}}
        chart = {"chart_type": "dx", "difficulty_index": 0, "level": "3", "ds": 3.0}

        attached = attach_fit_stats(music, [chart], {"622": [{"fit_diff": 5.0, "diff": "5"}]})

        self.assertIsNone(attached[0].get("fit_diff"))
        self.assertIsNone(attached[0].get("fit_delta"))

    def test_dx_master_no_sd_fit(self):
        """DX Master should NOT attach a +10000 stat when the level differs."""
        music = {"_source_records": {"cn": {"id": 622}, "divingfish": {"id": 622}}}
        chart = {"chart_type": "dx", "difficulty_index": 3, "level": "13+", "ds": 13.7}
        stats = {"10622": [{}, {}, {}, {"fit_diff": 13.378, "diff": "13"}]}

        attached = attach_fit_stats(music, [chart], stats)

        self.assertIsNone(attached[0].get("fit_diff"))
        self.assertIsNone(attached[0].get("fit_delta"))

    def test_sd_master_still_has_fit(self):
        """SD Master (level=13, ds=13.4) should still get correct fit data."""
        sd_charts = _sd_charts_for_song("バッド・ダンス・ホール", ds_match=13.4)
        self.assertTrue(len(sd_charts) >= 1, "SD Master chart should exist (ds=13.4)")
        for chart in sd_charts:
            self.assertIsNotNone(
                chart.get("fit_diff"),
                f"SD Master chart with ds={chart.get('ds')} should have fit_diff",
            )
            # Should be approximately 13.3868 (the known fit value)
            fit = chart.get("fit_diff")
            self.assertTrue(
                13.3 < fit < 13.5,
                f"SD Master fit_diff {fit} should be near 13.38",
            )

    def test_sd_basic_still_has_fit(self):
        """SD Basic (level=5, ds=5) should still get fit data."""
        sd_charts = _sd_charts_for_song("バッド・ダンス・ホール", ds_match=5.0)
        self.assertTrue(len(sd_charts) >= 1, "SD Basic chart should exist (ds=5.0)")
        for chart in sd_charts:
            self.assertIsNotNone(
                chart.get("fit_diff"),
                f"SD Basic chart with ds={chart.get('ds')} should have fit_diff",
            )

    def test_dx_song_with_own_chart_stats_still_works(self):
        """ハッピーシンセサイザ DX charts (which have their own chart_stats entry
        at 10044) should still get fit data."""
        dx_charts = _dx_charts_for_song("ハッピーシンセサイザ", ds_match=12.8)
        self.assertTrue(len(dx_charts) >= 1, "DX Master chart should exist (ds=12.8)")
        for chart in dx_charts:
            self.assertIsNotNone(
                chart.get("fit_diff"),
                f"DX chart with level {chart.get('level')} ds={chart.get('ds')} should have fit_diff"
            )


class TestBadDanceHallSearchResult(unittest.TestCase):
    """End-to-end verification of search result for バッド・ダンス・ホール."""

    def test_dx_charts_no_fit_in_search_result(self):
        result = search_songs(query="バッド・ダンス・ホール")
        songs = result.get("songs", [])
        self.assertEqual(len(songs), 1)
        song = songs[0]
        for chart in song.get("matched_charts", []):
            if chart.get("chart_type") == "dx":
                self.assertIsNone(
                    chart.get("fit_diff"),
                    f"DX {chart.get('difficulty')} should NOT have fit_diff in search result"
                )
            elif chart.get("chart_type") == "standard":
                self.assertIsNotNone(
                    chart.get("fit_diff"),
                    f"SD {chart.get('difficulty')} should have fit_diff in search result"
                )


if __name__ == "__main__":
    unittest.main()
