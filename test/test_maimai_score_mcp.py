from __future__ import annotations

import json
import unittest

import maimai_score_mcp.server as server


class MaimaiScoreMcpTests(unittest.TestCase):
    def test_query_by_song_searches_music_ids_then_calls_b50_mcp(self) -> None:
        # Tier 2 重构后：maimai_score_mcp 直接 import diving_fish_b50_mcp，不再走 stdio。
        # 测试通过替换 diving_fish_b50_mcp.server 上的两个符号来 mock。
        import diving_fish_b50_mcp.server as df_server

        calls: list[tuple[str, dict]] = []

        class FakeMaimaiClient:
            def __init__(self, *args, **kwargs):
                pass

            def __enter__(self):
                return self

            def __exit__(self, *_args):
                return None

            def call_tool(self, tool_name: str, arguments: dict) -> dict:
                calls.append((tool_name, arguments))
                return {
                    "content": [
                        {
                            "type": "text",
                            "text": json.dumps(
                                {
                                    "songs": [
                                        {
                                            "id": "288",
                                            "source_id": "288",
                                            "title": "六兆年と一夜物語",
                                            "artist": "kemu",
                                            "source": "lxns",
                                            "available_chart_types": ["standard", "dx"],
                                            "aliases": ["六兆年"],
                                            "matched_charts": [
                                                {"fit_source_id": "288", "chart_type": "standard"},
                                                {"fit_source_id": "10288", "chart_type": "dx"},
                                                {"fit_source_id": "10288", "chart_type": "dx"},
                                            ],
                                        }
                                    ]
                                }
                            ),
                        }
                    ],
                    "isError": False,
                }

        def fake_query_song_score(arguments: dict) -> dict:
            calls.append(("query_maimai_song_score", arguments))
            return {
                "musicId": arguments["musicId"],
                "player": {"nickname": "Tester", "rating": 15000},
                "records": [
                    {
                        "title": "六兆年と一夜物語",
                        "type": "DX",
                        "levelLabel": "Master",
                        "level": "13+",
                        "ds": 13.8,
                        "achievements": 100.0,
                        "ra": 300,
                        "rate": "sss",
                        "dxScore": 1234,
                    }
                ],
            }

        original_client = df_server.MaimaiLocalSearchClient
        original_query = df_server.query_maimai_song_score
        try:
            df_server.MaimaiLocalSearchClient = FakeMaimaiClient
            df_server.query_maimai_song_score = fake_query_song_score

            result = server.query_maimai_score_by_song({"qq": "10001", "songQuery": "六兆年"})
        finally:
            df_server.MaimaiLocalSearchClient = original_client
            df_server.query_maimai_song_score = original_query

        self.assertEqual(result["musicIds"], [288, 10288])
        self.assertEqual(result["counts"], {"requested": 2, "success": 2, "failure": 0})
        self.assertEqual(calls[0], ("search_maimai_songs", {"query": "六兆年", "limit": 5, "format": "json"}))
        self.assertEqual(calls[1][1]["musicId"], 288)
        self.assertEqual(calls[2][1]["musicId"], 10288)
        self.assertIn("Tester", result["text"])
        self.assertIn("六兆年と一夜物語", result["text"])

    def test_multiple_song_matches_select_first_ranked_candidate(self) -> None:
        song, music_ids = server.select_song_and_music_ids(
            {
                "songs": [
                    {"id": "1", "title": "Best Match"},
                    {"id": "2", "title": "Other Match"},
                ],
                "total_matches": 2,
            }
        )

        self.assertEqual(song["title"], "Best Match")
        self.assertEqual(music_ids, [1])

    def test_multiple_song_matches_skip_candidates_without_music_id(self) -> None:
        song, music_ids = server.select_song_and_music_ids(
            {
                "songs": [
                    {"id": "", "title": "No ID"},
                    {"id": "2", "title": "Usable Match"},
                ],
                "total_matches": 2,
            }
        )

        self.assertEqual(song["title"], "Usable Match")
        self.assertEqual(music_ids, [2])

    def test_score_lookup_reports_auto_selected_candidate(self) -> None:
        text = server.format_score_lookup(
            {
                "songQuery": "ambiguous",
                "selectedSong": {"title": "Best Match"},
                "selection": {"autoSelected": True, "totalMatches": 3, "selectedRank": 1},
                "lookup": {"qq": "10001"},
                "musicIds": [1],
                "counts": {"success": 1, "failure": 0},
                "scores": [],
            }
        )

        self.assertIn("匹配到 3 首，已自动选择最可能结果", text)

    def test_extract_music_ids_uses_matched_chart_ids_when_present(self) -> None:
        self.assertEqual(
            server.extract_music_ids(
                {
                    "id": "288",
                    "matched_charts": [{"fit_source_id": "10288"}],
                }
            ),
            [10288],
        )
        self.assertEqual(server.extract_music_ids({"id": "288", "matched_charts": []}), [288])


if __name__ == "__main__":
    unittest.main()
