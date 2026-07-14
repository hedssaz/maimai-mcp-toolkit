from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import maimai_mcp.search as search_module
from maimai_mcp.search import collect_song_results, list_versions, search_songs, source_regions


class MaimaiSearchTests(unittest.TestCase):
    def test_search_cache_is_invalidated_when_public_source_changes(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            data_path = root / "songs.json"
            alias_path = root / "aliases.json"
            chart_stats_path = root / "stats.json"
            pinyin_alias_path = root / "pinyin.json"
            artist_alias_path = root / "artists.json"
            charter_alias_path = root / "charters.json"
            data_path.write_text("[]", encoding="utf-8")
            alias_path.write_text("{}", encoding="utf-8")
            chart_stats_path.write_text('{"charts": {}}', encoding="utf-8")
            pinyin_alias_path.write_text(
                '{"version": 2, "aliases": []}',
                encoding="utf-8",
            )
            artist_alias_path.write_text("{}", encoding="utf-8")
            charter_alias_path.write_text("{}", encoding="utf-8")

            search_module.clear_search_caches()
            self.assertFalse(
                search_module.refresh_search_caches_if_sources_changed(
                    data_path=data_path,
                    alias_path=alias_path,
                    chart_stats_path=chart_stats_path,
                    pinyin_alias_path=pinyin_alias_path,
                    artist_alias_path=artist_alias_path,
                    charter_alias_path=charter_alias_path,
                )
            )
            search_module.load_search_context(
                data_path,
                alias_path,
                chart_stats_path,
                pinyin_alias_path,
                search_module.current_pinyin_alias_bucket(),
            )
            self.assertGreater(search_module.load_search_context.cache_info().currsize, 0)

            data_path.write_text("[{}]", encoding="utf-8")

            self.assertTrue(
                search_module.refresh_search_caches_if_sources_changed(
                    data_path=data_path,
                    alias_path=alias_path,
                    chart_stats_path=chart_stats_path,
                    pinyin_alias_path=pinyin_alias_path,
                    artist_alias_path=artist_alias_path,
                    charter_alias_path=charter_alias_path,
                )
            )
            self.assertEqual(search_module.load_search_context.cache_info().currsize, 0)
            search_module.clear_search_caches()

    def test_numeric_query_still_prefers_exact_song_id(self) -> None:
        result = search_songs(query="1663", limit=5)

        self.assertGreaterEqual(result["total_matches"], 1)
        self.assertEqual(result["songs"][0]["id"], "1663")
        self.assertEqual(result["songs"][0]["title"], "系ぎて")

    @unittest.skipIf(importlib.util.find_spec("pypinyin") is None, "pypinyin is not installed")
    def test_pinyin_alias_library_matches_song_title_without_polluting_display_aliases(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            pinyin_alias_path = Path(temp_dir) / "pinyin_aliases.json"

            result = search_songs(query="liuzhaonian", limit=5, pinyin_alias_path=pinyin_alias_path)

            self.assertGreaterEqual(result["total_matches"], 1)
            self.assertEqual(result["songs"][0]["title"], "六兆年と一夜物語")
            self.assertTrue(pinyin_alias_path.exists())
            self.assertNotIn("liuzhaonian", result["songs"][0]["aliases"])
            self.assertEqual(result["songs"][0]["match"]["field"], "pinyin")
            self.assertEqual(result["songs"][0]["match"]["mode"], "exact")

    @unittest.skipIf(importlib.util.find_spec("pypinyin") is None, "pypinyin is not installed")
    def test_rain_snow_song_matches_pinyin_and_homophone_queries(self) -> None:
        for query in ("ylsx", "yulushuangxue"):
            with self.subTest(query=query):
                result = search_songs(query=query, limit=5)

                self.assertGreaterEqual(result["total_matches"], 1)
                self.assertEqual(result["songs"][0]["title"], "雨露霜雪")
                self.assertEqual(result["songs"][0]["match"]["field"], "pinyin")
                self.assertEqual(result["songs"][0]["match"]["mode"], "exact")

        result = search_songs(query="语录爽学", limit=5)

        self.assertGreaterEqual(result["total_matches"], 1)
        self.assertEqual(result["songs"][0]["title"], "雨露霜雪")
        self.assertNotIn("语录爽学", result["songs"][0]["aliases"])
        self.assertEqual(result["songs"][0]["match"]["field"], "pinyin")
        self.assertEqual(result["songs"][0]["match"]["mode"], "exact")
        self.assertEqual(result["songs"][0]["match"]["value"], "yulushuangxue")

    @unittest.skipIf(importlib.util.find_spec("pypinyin") is None, "pypinyin is not installed")
    def test_pinyin_alias_preserves_numeric_suffixes(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            pinyin_alias_path = Path(temp_dir) / "pinyin_aliases.json"

            result = search_songs(query="xxch2", limit=5, pinyin_alias_path=pinyin_alias_path)

        self.assertGreaterEqual(result["total_matches"], 1)
        self.assertEqual(result["songs"][0]["title"], "プリズム△▽リズム")
        self.assertEqual(result["songs"][0]["match"]["field"], "pinyin")
        self.assertEqual(result["songs"][0]["match"]["mode"], "exact")
        self.assertEqual(result["songs"][0]["match"]["value"], "xxch2")

    def test_lxns_year_version_filter_uses_cn_song_version(self) -> None:
        songs, _criteria = collect_song_results(
            version="2025",
            region_has="cn",
            genre="maimai",
            limit=200,
        )

        self.assertGreater(len(songs), 0)
        self.assertNotIn("炎歌 -ほむらうた-", {song["title"] for song in songs})
        for song in songs:
            cn_version = song.get("source_fields", {}).get("cn", {}).get("version")
            self.assertTrue(str(cn_version).startswith("25"), song["title"])

    def test_cn_region_availability_uses_only_cn_and_divingfish_sources(self) -> None:
        self.assertTrue(source_regions([{"source": "cn"}])["cn"])
        self.assertTrue(source_regions([{"source": "divingfish"}])["cn"])
        self.assertFalse(source_regions([{"source": "unsupported", "regions": {"cn": True}}])["cn"])

    def test_search_region_has_cn_returns_only_cn_and_divingfish_sources(self) -> None:
        data = [
            {
                "_source_records": {
                    "cn": {
                        "id": 9101,
                        "title": "CN Only",
                        "artist": "Tester",
                        "genre": "maimai",
                        "version": 25000,
                        "difficulties": {
                            "dx": [
                                {
                                    "difficulty": 3,
                                    "level": "13",
                                    "level_value": 13.0,
                                    "notes": {},
                                }
                            ]
                        },
                    }
                }
            },
            {
                "_source_records": {
                    "divingfish": {
                        "id": 9102,
                        "title": "DivingFish Only",
                        "type": "DX",
                        "level": ["1", "2", "3", "13"],
                        "ds": [1.0, 2.0, 3.0, 13.0],
                        "charts": [{}, {}, {}, {}],
                        "basic_info": {
                            "title": "DivingFish Only",
                            "artist": "Tester",
                            "genre": "maimai",
                            "from": "maimai",
                        },
                    }
                }
            },
            {
                "_source_records": {
                    "unsupported": {
                        "id": 9103,
                        "title": "Unsupported Only",
                        "artist": "Tester",
                        "category": "maimai",
                        "sheets": [
                            {
                                "type": "dx",
                                "difficulty": "master",
                                "level": "13",
                                "internalLevelValue": 13.0,
                                "regions": {"cn": True},
                            }
                        ],
                    }
                }
            },
            {
                "_source_records": {
                    "other": {
                        "songId": "other-only-fixture",
                        "title": "Other Only",
                        "artist": "Tester",
                        "category": "maimai",
                        "sheets": [
                            {
                                "type": "dx",
                                "difficulty": "master",
                                "level": "13",
                                "internalLevelValue": 13.0,
                                "regions": {"cn": True},
                            }
                        ],
                    }
                }
            },
        ]
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            data_path = root / "songs.json"
            alias_path = root / "aliases.json"
            pinyin_alias_path = root / "pinyin_aliases.json"
            chart_stats_path = root / "chart_stats.json"
            with patch.object(search_module, "load_music_data", return_value=data):
                result = search_songs(
                    region_has="cn",
                    limit=10,
                    data_path=data_path,
                    alias_path=alias_path,
                    pinyin_alias_path=pinyin_alias_path,
                    chart_stats_path=chart_stats_path,
                )

        self.assertEqual(result["total_matches"], 2)
        self.assertEqual({song["title"] for song in result["songs"]}, {"CN Only", "DivingFish Only"})

    def test_search_merges_divingfish_st_dx_duplicate_song_results(self) -> None:
        by_title = search_songs(query="Selector", limit=5)
        by_dx_id = search_songs(query="id10574", limit=5)

        self.assertEqual(by_title["total_matches"], 1)
        self.assertEqual(by_dx_id["total_matches"], 1)
        song = by_title["songs"][0]
        self.assertEqual(song["title"], "Selector")
        self.assertIn("standard", song["available_chart_types"])
        self.assertIn("dx", song["available_chart_types"])
        self.assertIn("10574", song.get("source_id_aliases", {}).get("divingfish", []))

    def test_version_list_exposes_latest_cn_year(self) -> None:
        result = list_versions()

        self.assertTrue(result["latest_cn_versions"])
        self.assertTrue(result["latest_cn_years"])
        self.assertEqual(result["latest_cn_years"][0], str(result["latest_cn_versions"][0])[:2])


if __name__ == "__main__":
    unittest.main()
