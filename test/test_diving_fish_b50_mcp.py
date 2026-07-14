from __future__ import annotations

import json
import os
import socket
import tempfile
import unittest
import urllib.error
from datetime import datetime, timezone
from pathlib import Path

import diving_fish_b50_mcp.server as server


class DummyResponse:
    status = 200
    headers: dict[str, str] = {}

    def __enter__(self) -> "DummyResponse":
        return self

    def __exit__(self, *_args: object) -> None:
        return None

    def read(self) -> bytes:
        return json.dumps({"nickname": "ok", "charts": {"sd": [], "dx": []}}).encode()


class DummyApiResponse(DummyResponse):
    def read(self) -> bytes:
        return json.dumps(
            {
                "11466": [
                    {
                        "title": "Song",
                        "type": "DX",
                        "level": "13+",
                        "level_label": "Master",
                        "level_index": 3,
                        "ds": 13.8,
                        "achievements": 100.1234,
                        "dxScore": 1234,
                        "fc": "fc",
                        "fs": "fs",
                        "ra": 300,
                        "rate": "sss",
                        "song_id": 11466,
                    }
                ],
            }
        ).encode()


class DivingFishB50McpTests(unittest.TestCase):
    def _clear_current_version_env(self) -> dict[str, str | None]:
        old_env = {name: os.environ.get(name) for name in server.CURRENT_VERSION_ENV_NAMES}
        for name in server.CURRENT_VERSION_ENV_NAMES:
            os.environ.pop(name, None)
        server._divingfish_current_versions.cache_clear()
        return old_env

    def _restore_current_version_env(self, old_env: dict[str, str | None]) -> None:
        for name, value in old_env.items():
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value
        server._divingfish_current_versions.cache_clear()

    def test_player_b50_cache_write_prefers_metadata_rich_results(self) -> None:
        light = {
            "player": {"rating": 15000},
            "chartMetadata": {"skipped": True, "available": False},
        }
        rich = {
            "player": {"rating": 15000},
            "chartMetadata": {"available": True, "matched": 50, "missing": 0},
            "fitIndex": {
                "available": True,
                "label": "明显虚高（水）",
                "b50": {"virtualRating": 300.0, "virtualRatio": 2.0, "counted": 50, "missing": 0},
            },
        }
        existing_light = {"qq": "10001", "fetchedAt": datetime.now(timezone.utc).isoformat(), "b50": light}
        existing_rich = {"qq": "10001", "fetchedAt": datetime.now(timezone.utc).isoformat(), "b50": rich}

        self.assertTrue(server.player_b50_cache_should_write(existing_light, rich))
        self.assertFalse(server.player_b50_cache_should_write(existing_rich, light))

    def test_secret_store_path_falls_back_to_standard_config_dir(self) -> None:
        previous_cwd = Path.cwd()
        previous_env = os.environ.pop("DIVING_FISH_MCP_TOKEN_FILE", None)
        try:
            with tempfile.TemporaryDirectory() as temp_dir:
                data_dir = Path(temp_dir)
                mcp_dir = data_dir / "maimai-mcp"
                config_dir = data_dir / "maimai-config"
                mcp_dir.mkdir()
                config_dir.mkdir()
                token_path = config_dir / ".diving-fish-mcp-secrets.json"
                token_path.write_text("{}", encoding="utf-8")

                os.chdir(mcp_dir)

                self.assertEqual(server.get_secret_store_path(), token_path.resolve())
        finally:
            os.chdir(previous_cwd)
            if previous_env is not None:
                os.environ["DIVING_FISH_MCP_TOKEN_FILE"] = previous_env

    def test_secret_store_path_env_overrides_standard_config_dir(self) -> None:
        previous_env = os.environ.get("DIVING_FISH_MCP_TOKEN_FILE")
        try:
            with tempfile.TemporaryDirectory() as temp_dir:
                token_path = Path(temp_dir) / "custom-secrets.json"
                os.environ["DIVING_FISH_MCP_TOKEN_FILE"] = str(token_path)

                self.assertEqual(server.get_secret_store_path(), token_path.resolve())
        finally:
            if previous_env is None:
                os.environ.pop("DIVING_FISH_MCP_TOKEN_FILE", None)
            else:
                os.environ["DIVING_FISH_MCP_TOKEN_FILE"] = previous_env

    def test_query_player_retries_temporary_network_errors(self) -> None:
        calls = {"count": 0, "sleep": 0}
        original_urlopen = server.urllib.request.urlopen
        original_sleep = server.time.sleep

        def flaky_urlopen(*_args: object, **_kwargs: object) -> DummyResponse:
            calls["count"] += 1
            if calls["count"] == 1:
                raise urllib.error.URLError(
                    socket.gaierror(-3, "Temporary failure in name resolution")
                )
            return DummyResponse()

        try:
            server.urllib.request.urlopen = flaky_urlopen
            server.time.sleep = lambda _seconds: calls.__setitem__("sleep", calls["sleep"] + 1)

            result = server.post_diving_fish({"qq": "10001", "b50": "1"}, timeout_ms=1000)
        finally:
            server.urllib.request.urlopen = original_urlopen
            server.time.sleep = original_sleep

        self.assertEqual(calls, {"count": 2, "sleep": 1})
        self.assertEqual(result["nickname"], "ok")

    def test_query_maimai_song_score_uses_bound_developer_token(self) -> None:
        seen = {}
        original_urlopen = server.urllib.request.urlopen
        original_load_secret_store = server.load_secret_store

        def fake_urlopen(request: object, **_kwargs: object) -> DummyApiResponse:
            seen["url"] = request.full_url
            seen["headers"] = dict(request.header_items())
            seen["body"] = json.loads(request.data.decode())
            return DummyApiResponse()

        try:
            server.urllib.request.urlopen = fake_urlopen
            server.load_secret_store = lambda: {"developerToken": "dev-token"}

            result = server.query_maimai_song_score({"qq": "10001", "musicId": "id11466"})
        finally:
            server.urllib.request.urlopen = original_urlopen
            server.load_secret_store = original_load_secret_store

        self.assertTrue(seen["url"].endswith("/api/maimaidxprober/dev/player/record"))
        self.assertEqual(seen["headers"]["Developer-token"], "dev-token")
        self.assertEqual(seen["body"], {"qq": "10001", "music_id": [11466]})
        self.assertEqual(result["record"]["title"], "Song")
        self.assertEqual(result["records"][0]["title"], "Song")
        self.assertNotIn("raw", server.public_song_score_result(result, include_raw=False)["record"])

    def test_generic_single_song_api_wraps_numeric_music_id(self) -> None:
        seen = {}
        original_urlopen = server.urllib.request.urlopen

        def fake_urlopen(request: object, **_kwargs: object) -> DummyApiResponse:
            seen["body"] = json.loads(request.data.decode())
            return DummyApiResponse()

        try:
            server.urllib.request.urlopen = fake_urlopen

            result = server.call_diving_fish_api(
                {
                    "operation": "maimai_dev_player_record_post",
                    "developerToken": "dev-token",
                    "body": {"qq": "10001", "music_id": 11466},
                }
            )
        finally:
            server.urllib.request.urlopen = original_urlopen

        self.assertEqual(seen["body"], {"qq": "10001", "music_id": [11466]})
        self.assertEqual(result["status"], 200)

    def test_normalize_maimai_song_score_accepts_records_array_shape(self) -> None:
        result = server.normalize_maimai_song_score_response(
            {
                "operation": "maimai_dev_player_record_post",
                "status": 200,
                "data": {
                    "additional_rating": 10,
                    "nickname": "Tester",
                    "plate": "舞舞",
                    "rating": 15000,
                    "records": [
                        {
                            "title": "Song",
                            "type": "DX",
                            "level": "13+",
                            "level_label": "Master",
                            "song_id": 11466,
                        }
                    ],
                },
            },
            {"qq": "10001"},
            music_id=11466,
        )

        self.assertEqual(result["player"]["nickname"], "Tester")
        self.assertEqual(result["records"][0]["title"], "Song")

    def test_player_records_normalization_accepts_id_as_song_id(self) -> None:
        original_call_api = server.call_diving_fish_api

        def fake_call_api(_arguments: dict[str, object]) -> dict[str, object]:
            return {
                "status": 200,
                "endpoint": {"path": "/dev/player/records"},
                "data": {
                    "nickname": "Tester",
                    "rating": 15000,
                    "records": [
                        {
                            "id": 11739,
                            "title": "184億回のマルチトニック",
                            "type": "DX",
                            "level": "14",
                            "level_index": 3,
                            "ds": 14.1,
                            "achievements": 100.0,
                            "dxScore": 1234,
                            "fc": "fc",
                            "fs": "fs",
                            "ra": 350,
                            "rate": "sss",
                        }
                    ],
                },
            }

        try:
            server.call_diving_fish_api = fake_call_api
            result = server.query_maimai_player_records({"qq": "10001"})
        finally:
            server.call_diving_fish_api = original_call_api

        self.assertEqual(result["records"][0]["songId"], 11739)

    def test_player_records_version_filter_uses_local_song_version_when_raw_missing(self) -> None:
        original_call_api = server.call_diving_fish_api
        if hasattr(server._divingfish_song_version_index, "cache_clear"):
            server._divingfish_song_version_index.cache_clear()

        def fake_call_api(_arguments: dict[str, object]) -> dict[str, object]:
            return {
                "status": 200,
                "endpoint": {"path": "/dev/player/records"},
                "data": {
                    "nickname": "Tester",
                    "rating": 15000,
                    "records": [
                        {
                            "id": 8,
                            "title": "True Love Song",
                            "type": "SD",
                            "level": "12",
                            "level_index": 3,
                            "ds": 12.4,
                            "achievements": 100.0,
                            "dxScore": 1234,
                            "fc": "fc",
                            "fs": "",
                            "ra": 250,
                            "rate": "sss",
                        }
                    ],
                },
            }

        try:
            server.call_diving_fish_api = fake_call_api
            matched = server.query_maimai_player_records({"qq": "10001", "version": ["maimai"]})
            mismatched = server.query_maimai_player_records({"qq": "10001", "version": ["maimai PLUS"]})
        finally:
            server.call_diving_fish_api = original_call_api

        self.assertEqual(matched["counts"]["total"], 1)
        self.assertEqual(matched["counts"]["filtered"], 1)
        self.assertEqual(matched["records"][0]["songId"], 8)
        self.assertEqual(mismatched["counts"]["filtered"], 0)

    def test_player_records_version_filter_prefers_raw_version_when_present(self) -> None:
        original_call_api = server.call_diving_fish_api

        def fake_call_api(_arguments: dict[str, object]) -> dict[str, object]:
            return {
                "status": 200,
                "endpoint": {"path": "/dev/player/records"},
                "data": {
                    "nickname": "Tester",
                    "records": [
                        {
                            "id": 8,
                            "title": "True Love Song",
                            "type": "SD",
                            "version": "maimai PLUS",
                            "level": "12",
                            "level_index": 3,
                            "achievements": 100.0,
                        }
                    ],
                },
            }

        try:
            server.call_diving_fish_api = fake_call_api
            result = server.query_maimai_player_records({"qq": "10001", "version": ["maimai"]})
        finally:
            server.call_diving_fish_api = original_call_api

        self.assertEqual(result["counts"]["filtered"], 0)

    def test_b50_enrichment_uses_batch_maimai_search_result(self) -> None:
        result = server.normalize_b50_response(
            {
                "nickname": "Tester",
                "charts": {
                    "sd": [
                        {
                            "title": "Song",
                            "type": "SD",
                            "level": "13+",
                            "level_label": "Master",
                            "level_index": 3,
                            "ds": 13.8,
                            "achievements": 100.1234,
                            "ra": 300,
                            "song_id": 11466,
                        }
                    ],
                    "dx": [],
                },
            },
            {"qq": "10001"},
        )

        class FakeClient:
            def __init__(self, *, timeout_ms: int) -> None:
                self.timeout_ms = timeout_ms

            def __enter__(self) -> "FakeClient":
                return self

            def __exit__(self, *_args: object) -> None:
                return None

        original_client = server.MaimaiLocalSearchClient
        original_batch = server.call_maimai_batch_search_json
        seen = {}

        def fake_batch(_client: object, items: list[dict[str, object]]) -> dict[str, object]:
            seen["items"] = items
            return {
                "items": [
                    {
                        "key": "sd:0:id",
                        "ok": True,
                        "result": {
                            "songs": [
                                {
                                    "matched_charts": [
                                        {
                                            "chart_type": "standard",
                                            "difficulty_index": 3,
                                            "fit_diff": 13.65,
                                            "fit_delta": 0.15,
                                            "fit_label": "虚高",
                                            "fit_source_id": "11466",
                                        }
                                    ]
                                }
                            ]
                        },
                    }
                ]
            }

        try:
            server.MaimaiLocalSearchClient = FakeClient
            server.call_maimai_batch_search_json = fake_batch
            server.enrich_b50_with_maimai_local_search(result, timeout_ms=1000)
        finally:
            server.MaimaiLocalSearchClient = original_client
            server.call_maimai_batch_search_json = original_batch

        self.assertEqual(seen["items"][0]["query"], "id11466")
        self.assertEqual(len(seen["items"]), 1)
        song = result["charts"]["sd"][0]
        self.assertEqual(song["fitDiff"], 13.65)
        self.assertEqual(song["fitDelta"], 0.15)
        self.assertEqual(song["fitLabel"], "虚高")
        self.assertEqual(result["chartMetadata"]["matched"], 1)
        self.assertEqual(result["chartMetadata"]["idSearchItems"], 1)
        self.assertEqual(result["chartMetadata"]["fallbackSearchItems"], 0)

    def test_b50_enrichment_uses_title_fallback_only_when_id_misses(self) -> None:
        result = server.normalize_b50_response(
            {
                "nickname": "Tester",
                "charts": {
                    "sd": [
                        {
                            "title": "Song",
                            "type": "SD",
                            "level": "13+",
                            "level_label": "Master",
                            "level_index": 3,
                            "ds": 13.8,
                            "achievements": 100.1234,
                            "ra": 300,
                            "song_id": 11466,
                        }
                    ],
                    "dx": [],
                },
            },
            {"qq": "10001"},
        )

        class FakeClient:
            def __init__(self, *, timeout_ms: int) -> None:
                self.timeout_ms = timeout_ms

            def __enter__(self) -> "FakeClient":
                return self

            def __exit__(self, *_args: object) -> None:
                return None

        original_client = server.MaimaiLocalSearchClient
        original_batch = server.call_maimai_batch_search_json
        calls = []

        def fake_batch(_client: object, items: list[dict[str, object]]) -> dict[str, object]:
            calls.append(items)
            if items[0]["key"] == "sd:0:id":
                return {"items": [{"key": "sd:0:id", "ok": True, "result": {"songs": []}}]}
            return {
                "items": [
                    {
                        "key": "sd:0:title",
                        "ok": True,
                        "result": {
                            "songs": [
                                {
                                    "matched_charts": [
                                        {
                                            "chart_type": "standard",
                                            "difficulty_index": 3,
                                            "fit_diff": 13.9,
                                            "fit_delta": -0.1,
                                            "fit_label": "虚低",
                                        }
                                    ]
                                }
                            ]
                        },
                    }
                ]
            }

        try:
            server.MaimaiLocalSearchClient = FakeClient
            server.call_maimai_batch_search_json = fake_batch
            server.enrich_b50_with_maimai_local_search(result, timeout_ms=1000)
        finally:
            server.MaimaiLocalSearchClient = original_client
            server.call_maimai_batch_search_json = original_batch

        self.assertEqual(len(calls), 2)
        self.assertEqual(calls[0][0]["query"], "id11466")
        self.assertEqual(calls[1][0]["query"], "Song")
        song = result["charts"]["sd"][0]
        self.assertEqual(song["fitLabel"], "虚低")
        self.assertEqual(result["chartMetadata"]["searchItems"], 2)
        self.assertEqual(result["chartMetadata"]["fallbackSearchItems"], 1)

    def test_b50_summary_can_filter_and_sort_by_fit_metadata(self) -> None:
        result = {
            "player": {"nickname": "Tester", "rating": 15000, "plate": "舞舞"},
            "counts": {"sd": 2, "dx": 0, "total": 2},
            "ratingBreakdown": {"sd": 610, "dx": 0, "total": 610},
            "charts": {
                "sd": [
                    {
                        "title": "Low Fit",
                        "type": "SD",
                        "level": "13+",
                        "levelLabel": "Master",
                        "ds": 13.7,
                        "achievements": 100.0,
                        "ra": 300,
                        "fitDiff": 13.9,
                        "fitDelta": -0.2,
                        "fitLabel": "虚低",
                    },
                    {
                        "title": "High Fit",
                        "type": "SD",
                        "level": "14",
                        "levelLabel": "Master",
                        "ds": 14.1,
                        "achievements": 100.5,
                        "ra": 310,
                        "fitDiff": 13.6,
                        "fitDelta": 0.5,
                        "fitLabel": "虚高",
                    },
                ],
                "dx": [],
            },
        }

        options = server.normalize_b50_display_options(
            {"fitDeltaMin": 0, "sortBy": "fitDelta", "sortOrder": "desc"}
        )
        text = server.format_b50_summary(result, 50, "b50", options)

        self.assertIn("High Fit", text)
        self.assertNotIn("Low Fit", text)
        self.assertIn("拟合 13.6", text)
        self.assertIn("差值 +0.5", text)

    def test_compute_b50_fit_index_uses_exact_maimai_rating_formula(self) -> None:
        result = {
            "charts": {
                "sd": [
                    # ds=14.0, fitDiff=13.7（虚高 0.3）, ach=100.6%（SSS+ 系数 22.4）
                    # actual_ra = floor(14.0 * 1.005 * 22.4) = floor(315.168) = 315 (=ra)
                    # fitted_ra = floor(13.7 * 1.005 * 22.4) = floor(308.4144) = 308
                    # virtual = 7
                    {"ra": 315, "ds": 14.0, "fitDiff": 13.7, "achievements": 100.6, "fitDelta": 0.3},
                    # ds=13.5, fitDiff=13.7（虚低 -0.2）, ach=100.5%（系数 22.4）
                    # actual_ra = floor(13.5 * 1.005 * 22.4) = floor(303.912) = 303
                    # fitted_ra = floor(13.7 * 1.005 * 22.4) = floor(308.4144) = 308
                    # virtual = -5
                    {"ra": 303, "ds": 13.5, "fitDiff": 13.7, "achievements": 100.5, "fitDelta": -0.2},
                ],
                "dx": [
                    # ds=14.1, fitDiff=13.6（虚高 0.5）, ach=100.2%（SSS 系数 21.6）
                    # actual_ra = floor(14.1 * 1.002 * 21.6) = floor(305.1475) = 305
                    # fitted_ra = floor(13.6 * 1.002 * 21.6) = floor(294.3275) = 294
                    # virtual = 11
                    {"ra": 305, "ds": 14.1, "fitDiff": 13.6, "achievements": 100.2, "fitDelta": 0.5},
                    # 缺 fitDiff → 不计入分母
                    {"ra": 200, "ds": 13.0, "achievements": 99.0},
                ],
            }
        }

        server.compute_b50_fit_index(result)

        fit = result["fitIndex"]
        self.assertTrue(fit["available"])
        self.assertEqual(fit["b50"]["counted"], 3)
        self.assertEqual(fit["b50"]["missing"], 1)
        self.assertEqual(fit["b35"]["counted"], 2)
        self.assertEqual(fit["b15"]["counted"], 1)

        expected_virtual = float(7 - 5 + 11)  # 13
        expected_ratio = expected_virtual / (315 + 303 + 305) * 100.0
        self.assertAlmostEqual(fit["b50"]["virtualRating"], expected_virtual, places=6)
        self.assertAlmostEqual(fit["b50"]["virtualRatio"], expected_ratio, places=6)
        self.assertEqual(fit["label"], server.fit_index_label(fit["b50"]["virtualRatio"]))

    def test_current_versions_use_latest_ranked_waterfish_version(self) -> None:
        old_env = self._clear_current_version_env()
        songs = [
            {
                "basic_info": {
                    "from": "maimai でらっくす PRiSM",
                    "is_new": True,
                }
            },
            {
                "basic_info": {
                    "from": "maimai でらっくす PRiSM PLUS",
                    "is_new": False,
                }
            },
        ]

        try:
            self.assertEqual(
                server._divingfish_current_versions_from_songs(songs),
                {"maimai でらっくす PRiSM PLUS"},
            )
        finally:
            self._restore_current_version_env(old_env)

    def test_current_versions_can_be_overridden_by_env(self) -> None:
        old_env = self._clear_current_version_env()
        try:
            os.environ["MAIMAI_LOCAL_CURRENT_VERSIONS"] = "Future A; Future B"
            os.environ.pop("MAIMAI_CURRENT_VERSIONS", None)
            self.assertEqual(
                server._divingfish_current_versions_from_songs(
                    [
                        {
                            "basic_info": {
                                "from": "maimai でらっくす PRiSM PLUS",
                                "is_new": False,
                            }
                        }
                    ]
                ),
                {"Future A", "Future B"},
            )
        finally:
            self._restore_current_version_env(old_env)

    def test_query_computed_b50_splits_by_waterfish_current_version(self) -> None:
        old_song = {
            "id": 1001,
            "title": "Old Song",
            "type": "DX",
            "version": "25500",
            "basic_info": {"title": "Old Song", "from": "maimai でらっくす PRiSM", "is_new": True},
        }
        raw_high_old_song = {
            "id": 1003,
            "title": "Raw High Old Song",
            "type": "DX",
            "version": "25500",
            "basic_info": {
                "title": "Raw High Old Song",
                "from": "maimai でらっくす PRiSM",
                "is_new": True,
            },
        }
        new_song = {
            "id": 1002,
            "title": "New Song",
            "type": "DX",
            "version": "25599",
            "basic_info": {"title": "New Song", "from": "maimai でらっくす PRiSM PLUS", "is_new": False},
        }
        old_env = os.environ.get("DIVING_FISH_SONG_LIST_PATH")
        old_identity_dir = os.environ.get("QQ_IDENTITY_CACHE_DIR")
        old_current_version_env = self._clear_current_version_env()
        original_call = server.call_diving_fish_api
        original_batch_search = server.call_maimai_batch_search_json

        def fake_call(arguments: dict[str, object]) -> dict[str, object]:
            self.assertEqual(arguments["operation"], "maimai_dev_player_records_get")
            return {
                "status": 200,
                "endpoint": {"path": "/dev/player/records"},
                "data": {
                    "nickname": "Tester",
                    "rating": 9999,
                    "additional_rating": 10,
                    "plate": "舞舞",
                    "records": [
                        {
                            "song_id": 1001,
                            "title": "Old Song",
                            "type": "DX",
                            "level": "13+",
                            "level_label": "Master",
                            "level_index": 3,
                            "ds": 13.7,
                            "achievements": 100.0,
                            "dxScore": 1000,
                            "ra": 300,
                            "rate": "sss",
                        },
                        {
                            "song_id": 1001,
                            "title": "Old Song",
                            "type": "DX",
                            "level": "13+",
                            "level_label": "Master",
                            "level_index": 3,
                            "ds": 13.7,
                            "achievements": 99.0,
                            "dxScore": 900,
                            "ra": 250,
                            "rate": "ss",
                        },
                        {
                            "song_id": 1002,
                            "title": "New Song",
                            "type": "DX",
                            "level": "14",
                            "level_label": "Master",
                            "level_index": 3,
                            "ds": 14.0,
                            "achievements": 100.0,
                            "dxScore": 1100,
                            "ra": 310,
                            "rate": "sss",
                        },
                        {
                            "song_id": 1003,
                            "title": "Raw High Old Song",
                            "type": "DX",
                            "level": "14",
                            "level_label": "Master",
                            "level_index": 3,
                            "ds": 14.0,
                            "achievements": 100.0,
                            "dxScore": 1150,
                            "ra": 350,
                            "rate": "sss",
                        },
                        {
                            "song_id": 9999,
                            "title": "Unknown Song",
                            "type": "DX",
                            "version": "maimai でらっくす PRiSM PLUS",
                            "level": "15",
                            "level_label": "Master",
                            "level_index": 3,
                            "ds": 15.0,
                            "achievements": 100.0,
                            "dxScore": 1200,
                            "ra": 400,
                            "rate": "sss",
                        },
                    ],
                },
            }

        def fake_batch_search(_client: object, items: list[dict[str, object]]) -> dict[str, object]:
            fit_by_query = {
                "id1001": 14.0,
                "id1002": 13.0,
                "id1003": 10.0,
            }
            results = []
            for item in items:
                query = str(item.get("query") or "")
                fit_diff = fit_by_query.get(query)
                if fit_diff is None:
                    results.append({"key": item.get("key"), "ok": False, "error": {"message": "not found"}})
                    continue
                ds = 13.7 if query == "id1001" else 14.0
                results.append(
                    {
                        "key": item.get("key"),
                        "ok": True,
                        "result": {
                            "songs": [
                                {
                                    "matched_charts": [
                                        {
                                            "chart_type": "dx",
                                            "difficulty_index": 3,
                                            "ds": ds,
                                            "fit_diff": fit_diff,
                                            "fit_delta": ds - fit_diff,
                                            "fit_label": "虚高" if ds > fit_diff else "虚低",
                                        }
                                    ]
                                }
                            ]
                        },
                    }
                )
            return {"summary": {"requested": len(items)}, "items": results}

        with tempfile.TemporaryDirectory() as tmp:
            song_list = Path(tmp) / "divingfish_song_list.json"
            song_list.write_text(
                json.dumps([old_song, raw_high_old_song, new_song], ensure_ascii=False),
                encoding="utf-8",
            )
            os.environ["DIVING_FISH_SONG_LIST_PATH"] = str(song_list)
            os.environ["QQ_IDENTITY_CACHE_DIR"] = str(Path(tmp) / "identity")
            server.call_diving_fish_api = fake_call
            server.call_maimai_batch_search_json = fake_batch_search
            try:
                server._divingfish_song_version_index.cache_clear()
                server._divingfish_current_versions.cache_clear()
                server._fast_fitted_b50_metadata_index.cache_clear()
                result = server.query_computed_b50({"qq": "10001", "includeChartMetadata": False})
            finally:
                server.call_diving_fish_api = original_call
                server.call_maimai_batch_search_json = original_batch_search
                server._divingfish_song_version_index.cache_clear()
                server._divingfish_current_versions.cache_clear()
                server._fast_fitted_b50_metadata_index.cache_clear()
                if old_env is None:
                    os.environ.pop("DIVING_FISH_SONG_LIST_PATH", None)
                else:
                    os.environ["DIVING_FISH_SONG_LIST_PATH"] = old_env
                if old_identity_dir is None:
                    os.environ.pop("QQ_IDENTITY_CACHE_DIR", None)
                else:
                    os.environ["QQ_IDENTITY_CACHE_DIR"] = old_identity_dir
                self._restore_current_version_env(old_current_version_env)

        self.assertEqual([song["title"] for song in result["charts"]["sd"]], ["Old Song", "Raw High Old Song"])
        self.assertEqual([song["title"] for song in result["charts"]["dx"]], ["New Song"])
        self.assertEqual(result["ratingBreakdown"], {"sd": 518, "dx": 280, "total": 798})
        self.assertEqual(result["player"]["rating"], 798)
        self.assertEqual(result["player"]["actualRating"], 9999)
        self.assertEqual(result["charts"]["sd"][0]["fittedRa"], 302)
        self.assertEqual(result["charts"]["sd"][0]["originalRa"], 300)
        self.assertEqual(result["charts"]["sd"][1]["fittedRa"], 216)
        self.assertEqual(result["charts"]["sd"][1]["originalRa"], 350)
        self.assertEqual(result["computedB50"]["currentVersions"], ["maimai でらっくす PRiSM PLUS"])
        self.assertEqual(result["computedB50"]["skipped"]["missingVersion"], 1)
        self.assertEqual(result["computedB50"]["skipped"]["duplicateLowerRa"], 1)
        self.assertEqual(result["chartMetadata"]["matched"], 4)

    def test_query_computed_b50_uses_fast_fit_metadata_index(self) -> None:
        song_list_payload = [
            {
                "id": 1001,
                "title": "Fast Old",
                "type": "DX",
                "ds": [4.0, 7.0, 10.0, 13.7],
                "level": ["4", "7", "10", "13+"],
                "charts": [{}, {}, {}, {}],
                "basic_info": {"title": "Fast Old", "from": "maimai でらっくす PRiSM", "is_new": True},
            },
            {
                "id": 1002,
                "title": "Fast New",
                "type": "DX",
                "ds": [5.0, 8.0, 11.0, 14.0],
                "level": ["5", "8", "11", "14"],
                "charts": [{}, {}, {}, {}],
                "basic_info": {"title": "Fast New", "from": "maimai でらっくす PRiSM PLUS", "is_new": False},
            },
        ]
        chart_stats_payload = {
            "charts": {
                "1001": [{}, {}, {}, {"fit_diff": 13.5, "cnt": 10, "diff": "13+"}],
                "1002": [{}, {}, {}, {"fit_diff": 13.0, "cnt": 12, "diff": "14"}],
            }
        }
        old_song_env = os.environ.get("DIVING_FISH_SONG_LIST_PATH")
        old_stats_env = os.environ.get("DIVING_FISH_CHART_STATS_PATH")
        old_identity_dir = os.environ.get("QQ_IDENTITY_CACHE_DIR")
        old_current_version_env = self._clear_current_version_env()
        original_call = server.call_diving_fish_api
        original_batch_search = server.call_maimai_batch_search_json

        def fake_call(_arguments: dict[str, object]) -> dict[str, object]:
            return {
                "status": 200,
                "endpoint": {"path": "/dev/player/records"},
                "data": {
                    "nickname": "Fast Tester",
                    "rating": 12345,
                    "records": [
                        {
                            "song_id": 1001,
                            "title": "Fast Old",
                            "type": "DX",
                            "level": "13+",
                            "level_label": "Master",
                            "level_index": 3,
                            "ds": 13.7,
                            "achievements": 100.0,
                            "ra": 300,
                        },
                        {
                            "song_id": 1002,
                            "title": "Fast New",
                            "type": "DX",
                            "level": "14",
                            "level_label": "Master",
                            "level_index": 3,
                            "ds": 14.0,
                            "achievements": 100.0,
                            "ra": 310,
                        },
                    ],
                },
            }

        def fail_batch_search(_client: object, _items: list[dict[str, object]]) -> dict[str, object]:
            raise AssertionError("fast computed B50 metadata should not call batch search")

        with tempfile.TemporaryDirectory() as tmp:
            song_list = Path(tmp) / "divingfish_song_list.json"
            chart_stats = Path(tmp) / "divingfish_chart_stats.json"
            song_list.write_text(json.dumps(song_list_payload, ensure_ascii=False), encoding="utf-8")
            chart_stats.write_text(json.dumps(chart_stats_payload, ensure_ascii=False), encoding="utf-8")
            os.environ["DIVING_FISH_SONG_LIST_PATH"] = str(song_list)
            os.environ["DIVING_FISH_CHART_STATS_PATH"] = str(chart_stats)
            os.environ["QQ_IDENTITY_CACHE_DIR"] = str(Path(tmp) / "identity")
            server.call_diving_fish_api = fake_call
            server.call_maimai_batch_search_json = fail_batch_search
            try:
                server._divingfish_song_version_index.cache_clear()
                server._divingfish_current_versions.cache_clear()
                server._fast_fitted_b50_metadata_index.cache_clear()
                result = server.query_computed_b50({"qq": "10001"})
            finally:
                server.call_diving_fish_api = original_call
                server.call_maimai_batch_search_json = original_batch_search
                server._divingfish_song_version_index.cache_clear()
                server._divingfish_current_versions.cache_clear()
                server._fast_fitted_b50_metadata_index.cache_clear()
                if old_song_env is None:
                    os.environ.pop("DIVING_FISH_SONG_LIST_PATH", None)
                else:
                    os.environ["DIVING_FISH_SONG_LIST_PATH"] = old_song_env
                if old_stats_env is None:
                    os.environ.pop("DIVING_FISH_CHART_STATS_PATH", None)
                else:
                    os.environ["DIVING_FISH_CHART_STATS_PATH"] = old_stats_env
                if old_identity_dir is None:
                    os.environ.pop("QQ_IDENTITY_CACHE_DIR", None)
                else:
                    os.environ["QQ_IDENTITY_CACHE_DIR"] = old_identity_dir
                self._restore_current_version_env(old_current_version_env)

        self.assertEqual(result["chartMetadata"]["source"], "maimai-local-index")
        self.assertEqual(result["chartMetadata"]["fastMatched"], 2)
        self.assertEqual(result["chartMetadata"]["matched"], 2)
        self.assertEqual(result["charts"]["sd"][0]["fitDiff"], 13.5)
        self.assertEqual(result["charts"]["dx"][0]["fitDiff"], 13.0)
        self.assertEqual(
            result["ratingBreakdown"],
            {
                "sd": server.maimai_dx_ra(13.5, 100.0),
                "dx": server.maimai_dx_ra(13.0, 100.0),
                "total": server.maimai_dx_ra(13.5, 100.0) + server.maimai_dx_ra(13.0, 100.0),
            },
        )

    def test_fast_fit_metadata_index_reuses_disk_cache_when_hash_matches(self) -> None:
        song_list_payload = [
            {
                "id": 1001,
                "title": "Fast Cache",
                "type": "DX",
                "ds": [4.0, 7.0, 10.0, 13.7],
                "level": ["4", "7", "10", "13+"],
                "basic_info": {"title": "Fast Cache", "from": "maimai でらっくす PRiSM"},
            }
        ]
        chart_stats_payload = {"charts": {"1001": [{}, {}, {}, {"fit_diff": 13.5}]}}
        old_song_env = os.environ.get("DIVING_FISH_SONG_LIST_PATH")
        old_stats_env = os.environ.get("DIVING_FISH_CHART_STATS_PATH")
        old_cache_env = os.environ.get("DIVING_FISH_FIT_INDEX_CACHE_DIR")
        original_builder = server._build_fast_fitted_b50_metadata_index

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            song_list = root / "divingfish_song_list.json"
            chart_stats = root / "divingfish_chart_stats.json"
            cache_dir = root / "fit-index-cache"
            song_list.write_text(json.dumps(song_list_payload, ensure_ascii=False), encoding="utf-8")
            chart_stats.write_text(json.dumps(chart_stats_payload, ensure_ascii=False), encoding="utf-8")
            os.environ["DIVING_FISH_SONG_LIST_PATH"] = str(song_list)
            os.environ["DIVING_FISH_CHART_STATS_PATH"] = str(chart_stats)
            os.environ["DIVING_FISH_FIT_INDEX_CACHE_DIR"] = str(cache_dir)
            try:
                server._fast_fitted_b50_metadata_index.cache_clear()
                first = server._fast_fitted_b50_metadata_index()
                self.assertEqual(first["id:1001|dx|3"]["fit_diff"], 13.5)
                self.assertTrue((cache_dir / "fit-metadata-index.json").exists())

                server._fast_fitted_b50_metadata_index.cache_clear()

                def fail_builder() -> dict[str, dict[str, object]]:
                    raise AssertionError("unchanged fit index should be loaded from disk cache")

                server._build_fast_fitted_b50_metadata_index = fail_builder
                second = server._fast_fitted_b50_metadata_index()
            finally:
                server._build_fast_fitted_b50_metadata_index = original_builder
                server._fast_fitted_b50_metadata_index.cache_clear()
                if old_song_env is None:
                    os.environ.pop("DIVING_FISH_SONG_LIST_PATH", None)
                else:
                    os.environ["DIVING_FISH_SONG_LIST_PATH"] = old_song_env
                if old_stats_env is None:
                    os.environ.pop("DIVING_FISH_CHART_STATS_PATH", None)
                else:
                    os.environ["DIVING_FISH_CHART_STATS_PATH"] = old_stats_env
                if old_cache_env is None:
                    os.environ.pop("DIVING_FISH_FIT_INDEX_CACHE_DIR", None)
                else:
                    os.environ["DIVING_FISH_FIT_INDEX_CACHE_DIR"] = old_cache_env

        self.assertEqual(second, first)

    def test_fast_fit_metadata_index_rebuilds_when_fit_hash_changes(self) -> None:
        song_list_payload = [
            {
                "id": 1001,
                "title": "Fast Cache",
                "type": "DX",
                "ds": [4.0, 7.0, 10.0, 13.7],
                "level": ["4", "7", "10", "13+"],
                "basic_info": {"title": "Fast Cache", "from": "maimai でらっくす PRiSM"},
            }
        ]
        old_song_env = os.environ.get("DIVING_FISH_SONG_LIST_PATH")
        old_stats_env = os.environ.get("DIVING_FISH_CHART_STATS_PATH")
        old_cache_env = os.environ.get("DIVING_FISH_FIT_INDEX_CACHE_DIR")

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            song_list = root / "divingfish_song_list.json"
            chart_stats = root / "divingfish_chart_stats.json"
            song_list.write_text(json.dumps(song_list_payload, ensure_ascii=False), encoding="utf-8")
            chart_stats.write_text(
                json.dumps({"charts": {"1001": [{}, {}, {}, {"fit_diff": 13.5}]}}, ensure_ascii=False),
                encoding="utf-8",
            )
            os.environ["DIVING_FISH_SONG_LIST_PATH"] = str(song_list)
            os.environ["DIVING_FISH_CHART_STATS_PATH"] = str(chart_stats)
            os.environ["DIVING_FISH_FIT_INDEX_CACHE_DIR"] = str(root / "fit-index-cache")
            try:
                server._fast_fitted_b50_metadata_index.cache_clear()
                first = server._fast_fitted_b50_metadata_index()
                chart_stats.write_text(
                    json.dumps({"charts": {"1001": [{}, {}, {}, {"fit_diff": 13.9}]}}, ensure_ascii=False),
                    encoding="utf-8",
                )
                server._fast_fitted_b50_metadata_index.cache_clear()
                second = server._fast_fitted_b50_metadata_index()
            finally:
                server._fast_fitted_b50_metadata_index.cache_clear()
                if old_song_env is None:
                    os.environ.pop("DIVING_FISH_SONG_LIST_PATH", None)
                else:
                    os.environ["DIVING_FISH_SONG_LIST_PATH"] = old_song_env
                if old_stats_env is None:
                    os.environ.pop("DIVING_FISH_CHART_STATS_PATH", None)
                else:
                    os.environ["DIVING_FISH_CHART_STATS_PATH"] = old_stats_env
                if old_cache_env is None:
                    os.environ.pop("DIVING_FISH_FIT_INDEX_CACHE_DIR", None)
                else:
                    os.environ["DIVING_FISH_FIT_INDEX_CACHE_DIR"] = old_cache_env

        self.assertEqual(first["id:1001|dx|3"]["fit_diff"], 13.5)
        self.assertEqual(second["id:1001|dx|3"]["fit_diff"], 13.9)

    def test_maimai_dx_ra_matches_known_examples(self) -> None:
        # 100.5%+ SSS+ at ds=14.5: floor(14.5 * 1.005 * 22.4) = floor(326.424) = 326
        self.assertEqual(server.maimai_dx_ra(14.5, 100.5351), 326)
        # 100.0% SSS at ds=13.0: floor(13.0 * 1.0 * 21.6) = floor(280.8) = 280
        self.assertEqual(server.maimai_dx_ra(13.0, 100.0), 280)
        # Achievement cap: 101% still uses 100.5 + 22.4 multiplier
        self.assertEqual(server.maimai_dx_ra(14.5, 101.0), 326)
        # 99.5% SS+ at ds=13.0: floor(13.0 * 0.995 * 21.1) = floor(272.9285) = 272
        self.assertEqual(server.maimai_dx_ra(13.0, 99.5), 272)

    def test_compute_b50_fit_index_marks_unavailable_when_all_missing(self) -> None:
        result = {"charts": {"sd": [{"ra": 300, "ds": 13.7}], "dx": []}}

        server.compute_b50_fit_index(result)

        self.assertFalse(result["fitIndex"]["available"])
        self.assertEqual(result["fitIndex"]["b50"]["counted"], 0)
        self.assertEqual(result["fitIndex"]["b50"]["missing"], 1)
        self.assertIsNone(result["fitIndex"]["label"])

    def test_fit_index_appears_in_summary_when_available(self) -> None:
        result = {
            "player": {"nickname": "Tester", "rating": 15000, "plate": "舞舞"},
            "counts": {"sd": 1, "dx": 0, "total": 1},
            "ratingBreakdown": {"sd": 300, "dx": 0, "total": 300},
            "charts": {
                "sd": [
                    {
                        "title": "Song",
                        "type": "SD",
                        "level": "13+",
                        "levelLabel": "Master",
                        "ds": 13.7,
                        "achievements": 100.0,
                        "ra": 300,
                        "fitDiff": 13.4,
                        "fitDelta": 0.3,
                        "fitLabel": "虚高",
                    }
                ],
                "dx": [],
            },
        }
        server.compute_b50_fit_index(result)

        text = server.format_b50_summary(result, 50, "b50")

        self.assertIn("虚高指数", text)
        self.assertIn("ra", text)
        self.assertIn("%", text)

    def test_diving_fish_api_records_get_writes_player_cache(self) -> None:
        import os, tempfile
        from player_cache import store as player_cache_store

        previous_cache_dir = os.environ.get("PLAYER_CACHE_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["PLAYER_CACHE_DIR"] = tmpdir
            try:
                original_urlopen = server.urllib.request.urlopen
                original_load_secret_store = server.load_secret_store
                server.load_secret_store = lambda: {"developerToken": "dev-token-abc"}

                records_payload = {
                    "nickname": "Tester",
                    "rating": 15000,
                    "records": [
                        {"song_id": 11466, "level_index": 3, "type": "DX", "achievements": 100.5, "ra": 326},
                        {"song_id": 11466, "level_index": 2, "type": "DX", "achievements": 99.5, "ra": 280},
                    ],
                }

                class _Resp:
                    status = 200
                    headers: dict[str, str] = {}
                    def __enter__(self): return self
                    def __exit__(self, *_): return None
                    def read(self): return json.dumps(records_payload).encode()

                server.urllib.request.urlopen = lambda *a, **kw: _Resp()

                api_arguments = {
                    "operation": "maimai_dev_player_records_get",
                    "query": {"qq": "555"},
                    "timeoutMs": 5000,
                }
                try:
                    server.call_diving_fish_api(api_arguments)
                finally:
                    server.urllib.request.urlopen = original_urlopen
                    server.load_secret_store = original_load_secret_store

                entry = player_cache_store.read_player_records("555")
                self.assertIsNotNone(entry)
                self.assertEqual(entry["records"]["nickname"], "Tester")
                self.assertEqual(len(entry["records"]["records"]), 2)
            finally:
                if previous_cache_dir is None:
                    os.environ.pop("PLAYER_CACHE_DIR", None)
                else:
                    os.environ["PLAYER_CACHE_DIR"] = previous_cache_dir

    def test_query_song_score_merges_into_existing_records_cache(self) -> None:
        import os, tempfile
        from player_cache import store as player_cache_store

        previous_cache_dir = os.environ.get("PLAYER_CACHE_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["PLAYER_CACHE_DIR"] = tmpdir
            try:
                # 预先存一份 records 缓存
                player_cache_store.write_player_records(
                    "888",
                    {
                        "nickname": "Tester",
                        "rating": 15000,
                        "records": [
                            {"song_id": 11466, "level_index": 3, "achievements": 99.0, "ra": 280},
                            {"song_id": 11467, "level_index": 3, "achievements": 100.0, "ra": 290},
                        ],
                    },
                )

                original_urlopen = server.urllib.request.urlopen
                original_load_secret_store = server.load_secret_store
                server.load_secret_store = lambda: {"developerToken": "dev-token-abc"}

                # /dev/player/record_post 返回新的 11466 record（更高分）
                new_record = {"song_id": 11466, "level_index": 3, "type": "DX", "achievements": 100.5, "ra": 326}

                class _Resp:
                    status = 200
                    headers: dict[str, str] = {}
                    def __enter__(self): return self
                    def __exit__(self, *_): return None
                    def read(self): return json.dumps({"11466": [new_record]}).encode()

                server.urllib.request.urlopen = lambda *a, **kw: _Resp()
                try:
                    server.query_maimai_song_score({"qq": "888", "musicId": 11466})
                finally:
                    server.urllib.request.urlopen = original_urlopen
                    server.load_secret_store = original_load_secret_store

                entry = player_cache_store.read_player_records("888")
                self.assertIsNotNone(entry)
                records_list = entry["records"]["records"]
                self.assertEqual(len(records_list), 2)  # 仍是 2 条，没有重复
                # 11466 那条被替换为高分
                updated = next(r for r in records_list if r["song_id"] == 11466)
                self.assertEqual(updated["achievements"], 100.5)
                self.assertEqual(updated["ra"], 326)
                # 11467 那条不变
                kept = next(r for r in records_list if r["song_id"] == 11467)
                self.assertEqual(kept["achievements"], 100.0)
            finally:
                if previous_cache_dir is None:
                    os.environ.pop("PLAYER_CACHE_DIR", None)
                else:
                    os.environ["PLAYER_CACHE_DIR"] = previous_cache_dir

    def test_query_song_score_does_not_create_records_cache_when_absent(self) -> None:
        import os, tempfile
        from player_cache import store as player_cache_store

        previous_cache_dir = os.environ.get("PLAYER_CACHE_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["PLAYER_CACHE_DIR"] = tmpdir
            try:
                original_urlopen = server.urllib.request.urlopen
                original_load_secret_store = server.load_secret_store
                server.load_secret_store = lambda: {"developerToken": "dev-token-abc"}

                class _Resp:
                    status = 200
                    headers: dict[str, str] = {}
                    def __enter__(self): return self
                    def __exit__(self, *_): return None
                    def read(self): return json.dumps({"11466": [{"song_id": 11466, "level_index": 3, "achievements": 100.5}]}).encode()

                server.urllib.request.urlopen = lambda *a, **kw: _Resp()
                try:
                    server.query_maimai_song_score({"qq": "777", "musicId": 11466})
                finally:
                    server.urllib.request.urlopen = original_urlopen
                    server.load_secret_store = original_load_secret_store

                # 关键：缓存不应该被新建（避免群单曲榜误判 records 已最新）
                self.assertIsNone(player_cache_store.read_player_records("777"))
            finally:
                if previous_cache_dir is None:
                    os.environ.pop("PLAYER_CACHE_DIR", None)
                else:
                    os.environ["PLAYER_CACHE_DIR"] = previous_cache_dir


if __name__ == "__main__":
    unittest.main()
