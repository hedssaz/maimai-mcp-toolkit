from __future__ import annotations

import os
import json
import tempfile
import threading
import time
import unittest
from datetime import datetime, timedelta, timezone

import group_b50_mcp.server as server
import diving_fish_b50_mcp.server as df_server
from group_b50_mcp import song_rank
from group_b50_mcp import job_runner


def wait_for_job(group_id: str) -> dict:
    thread = job_runner.BACKGROUND_JOBS.get((server.B50_FEATURE, group_id))
    if thread:
        thread.join(timeout=2)
    deadline = time.time() + 2
    while time.time() < deadline:
        status = server.read_job_status(group_id)
        if status and status.get("status") in {"completed", "failed"}:
            return status
        time.sleep(0.01)
    return server.read_job_status(group_id) or {}


def wait_for_song_job(group_id: str) -> dict:
    thread = job_runner.BACKGROUND_JOBS.get((song_rank.SONG_FEATURE, group_id))
    if thread:
        thread.join(timeout=3)
    deadline = time.time() + 3
    while time.time() < deadline:
        status = song_rank.song_read_job_status(group_id)
        if status and status.get("status") in {"completed", "failed"}:
            return status
        time.sleep(0.01)
    return song_rank.song_read_job_status(group_id) or {}


def fake_b50(qq: str, rating: int) -> dict:
    return {
        "source": "diving-fish",
        "lookup": {"qq": qq},
        "requestedAt": datetime.now(timezone.utc).isoformat(),
        "player": {
            "nickname": f"player-{qq}",
            "rating": rating,
            "plate": "未知",
        },
        "counts": {"sd": 1, "dx": 1, "total": 2},
        "ratingBreakdown": {"sd": rating // 2, "dx": rating - rating // 2, "total": rating},
        "charts": {
            "sd": [{"title": "old", "type": "SD", "levelLabel": "Master", "ra": rating // 2}],
            "dx": [{"title": "new", "type": "DX", "levelLabel": "Master", "ra": rating - rating // 2}],
        },
        "raw": {"hidden": True},
    }


def fake_records_doc(qq: str, rating: int = 15000) -> dict:
    return {
        "nickname": f"player-{qq}",
        "rating": rating,
        "records": [
            {
                "song_id": 1000,
                "title": "song",
                "type": "DX",
                "level_index": 3,
                "level": "13",
                "level_label": "Master",
                "achievements": 100.0,
                "ra": 300,
            }
        ],
    }


class GroupB50McpTests(unittest.TestCase):
    def setUp(self) -> None:
        self.previous_qq_cache_dir = os.environ.get("QQ_IDENTITY_CACHE_DIR")
        self.qq_cache_tmp = tempfile.TemporaryDirectory()
        os.environ["QQ_IDENTITY_CACHE_DIR"] = self.qq_cache_tmp.name
        # 隔离 player_cache，防止本地 player-cache/ 里的 b50 数据干扰群榜 overlay。
        self.previous_player_cache_dir = os.environ.get("PLAYER_CACHE_DIR")
        self.player_cache_tmp = tempfile.TemporaryDirectory()
        os.environ["PLAYER_CACHE_DIR"] = self.player_cache_tmp.name

    def tearDown(self) -> None:
        if self.previous_qq_cache_dir is None:
            os.environ.pop("QQ_IDENTITY_CACHE_DIR", None)
        else:
            os.environ["QQ_IDENTITY_CACHE_DIR"] = self.previous_qq_cache_dir
        self.qq_cache_tmp.cleanup()
        if self.previous_player_cache_dir is None:
            os.environ.pop("PLAYER_CACHE_DIR", None)
        else:
            os.environ["PLAYER_CACHE_DIR"] = self.previous_player_cache_dir
        self.player_cache_tmp.cleanup()

    def test_query_b50_batch_progress_callback_reports_each_completed_qq(self) -> None:
        original_query = df_server.query_b50
        try:
            df_server.query_b50 = lambda arguments, **kwargs: fake_b50(
                str(arguments["qq"]),
                15000 + int(str(arguments["qq"])[-1]),
            )
            progress: list[dict] = []

            result = df_server.query_b50_batch(
                {
                    "qqs": ["10001", "10002"],
                    "groupId": "123",
                    "queryDelayMs": 0,
                    "maxConcurrency": 1,
                    "includeChartMetadata": False,
                    "_progressCallback": progress.append,
                }
            )

            self.assertEqual(result["counts"], {"requested": 2, "success": 2, "failure": 0})
            self.assertEqual([item["completed"] for item in progress], [1, 2])
            self.assertEqual([item["qq"] for item in progress], ["10001", "10002"])
        finally:
            df_server.query_b50 = original_query

    def test_b50_chart_metadata_cache_reuses_duplicate_song_metadata(self) -> None:
        original_batch_search = df_server.call_maimai_batch_search_json
        calls: list[list[str]] = []
        chart = {
            "chart_type": "dx",
            "difficulty_index": 3,
            "level": "14",
            "ds": 14.0,
            "fit_diff": 13.8,
            "fit_delta": 0.2,
            "fit_label": "虚高",
        }

        def fake_batch_search(_client, items):
            calls.append([str(item["key"]) for item in items])
            return {
                "items": [
                    {
                        "key": item["key"],
                        "ok": True,
                        "result": {"songs": [{"matched_charts": [chart]}]},
                    }
                    for item in items
                ]
            }

        def b50_with_same_song() -> dict:
            return {
                "counts": {"total": 1},
                "charts": {
                    "sd": [],
                    "dx": [
                        {
                            "songId": 11451,
                            "title": "same",
                            "type": "DX",
                            "levelLabel": "Master",
                            "level": "14",
                            "ds": 14.0,
                            "achievements": 100.0,
                            "ra": 300,
                        }
                    ],
                },
            }

        try:
            df_server.call_maimai_batch_search_json = fake_batch_search
            metadata_cache: dict[str, dict | None] = {}
            metadata_lock = threading.Lock()
            client = df_server.MaimaiLocalSearchClient(timeout_ms=1000)
            first = b50_with_same_song()
            second = b50_with_same_song()

            df_server.enrich_b50_with_maimai_local_search(
                first,
                timeout_ms=1000,
                client=client,
                metadata_cache=metadata_cache,
                metadata_cache_lock=metadata_lock,
            )
            df_server.enrich_b50_with_maimai_local_search(
                second,
                timeout_ms=1000,
                client=client,
                metadata_cache=metadata_cache,
                metadata_cache_lock=metadata_lock,
            )

            self.assertEqual(len(calls), 1)
            self.assertEqual(first["chartMetadata"]["metadataCacheHits"], 0)
            self.assertEqual(second["chartMetadata"]["metadataCacheHits"], 1)
            self.assertEqual(second["chartMetadata"]["searchItems"], 0)
            self.assertTrue(second["fitIndex"]["available"])
        finally:
            df_server.call_maimai_batch_search_json = original_batch_search

    def test_report_uses_cache_and_writes_sorted_files(self) -> None:
        previous_cache_dir = os.environ.get("GROUP_B50_CACHE_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["GROUP_B50_CACHE_DIR"] = tmpdir
            calls = {"members": 0, "query": 0}
            original_fetch = server.fetch_group_members
            original_batch = server.query_b50_batch_via_mcp

            def fetch_members(*args, **kwargs):
                calls["members"] += 1
                return [
                    {"userId": "10002", "displayName": "high", "nickname": "high", "card": None},
                    {"userId": "10001", "displayName": "low", "nickname": "low", "card": None},
                ]

            def query_batch(qqs, **kwargs):
                calls["query"] += 1
                return {
                    "counts": {"requested": len(qqs), "success": len(qqs), "failure": 0},
                    "results": [
                        {
                            "qq": qq,
                            "ok": True,
                            "player": fake_b50(qq, 16000 if qq == "10002" else 15000)["player"],
                            "rating": 16000 if qq == "10002" else 15000,
                            "b50Rating": 16000 if qq == "10002" else 15000,
                            "result": fake_b50(qq, 16000 if qq == "10002" else 15000),
                            "error": None,
                        }
                        for qq in qqs
                    ],
                }

            try:
                server.fetch_group_members = fetch_members
                server.query_b50_batch_via_mcp = query_batch

                started = server.group_b50_report({"groupId": "123", "queryDelayMs": 0})
                self.assertIn("已启动后台刷新任务", started["text"])
                self.assertEqual(wait_for_job("123")["status"], "completed")
                result = server.group_b50_job_status({"groupId": "123"})
                self.assertEqual(calls, {"members": 1, "query": 1})
                self.assertIn("low", result["text"])
                self.assertLess(result["text"].index("low"), result["text"].index("high"))
                self.assertTrue(os.path.exists(result["reportFiles"]["asc"]))
                self.assertTrue(os.path.exists(result["reportFiles"]["desc"]))
                self.assertTrue(result["cache"]["containsDetailedB50"])
                self.assertIn("charts", result["data"]["results"][0]["b50"])
                self.assertEqual(result["data"]["skippedCount"], 0)

                rank = server.group_b50_member_rank(
                    {"groupId": "123", "qq": "10002", "contextSize": 1, "queryDelayMs": 0}
                )
                self.assertTrue(rank["found"])
                self.assertEqual(rank["rank"]["rankDesc"], 1)
                self.assertEqual(rank["rank"]["rankAsc"], 2)
                self.assertEqual(rank["rank"]["totalRanked"], 2)
                self.assertIn("倒序排名: 1 / 2", rank["text"])
                self.assertIn("附近排名", rank["text"])

                first = server.group_b50_rank_at({"groupId": "123", "rank": 1, "queryDelayMs": 0})
                self.assertTrue(first["found"])
                self.assertEqual(first["member"]["userId"], "10002")
                self.assertEqual(first["rankInfo"]["requestedRank"], 1)
                self.assertEqual(first["rankInfo"]["sortOrder"], "desc")
                self.assertIn("rating 倒序第 1 名", first["text"])

                lowest = server.group_b50_rank_at(
                    {"groupId": "123", "rank": 1, "sortOrder": "asc", "outputMode": "detail", "queryDelayMs": 0}
                )
                self.assertTrue(lowest["found"])
                self.assertEqual(lowest["member"]["userId"], "10001")
                self.assertIn("完整 B50", lowest["text"])

                missing_rank = server.group_b50_rank_at({"groupId": "123", "rank": 3, "queryDelayMs": 0})
                self.assertFalse(missing_rank["found"])
                self.assertIn("第 3 名不存在", missing_rank["text"])

                cached = server.group_b50_report({"groupId": "123", "queryDelayMs": 0})
                self.assertEqual(calls, {"members": 1, "query": 1})
                self.assertEqual(cached["cacheRefreshReason"], "hit")
                self.assertIn("本次使用一天内缓存", cached["text"])

                ranged = server.group_b50_report({"groupId": "123", "queryDelayMs": 0, "startRank": 2, "endRank": 2})
                self.assertIn("输出: 第 2-2 名", ranged["text"])
                self.assertIn("| 2 | 10002 |", ranged["text"])
                self.assertNotIn("| 1 | 10001 |", ranged["text"])

                filtered = server.group_b50_report(
                    {"groupId": "123", "queryDelayMs": 0, "ratingMin": 14000, "ratingMax": 15000}
                )
                self.assertIn("rating 14000 到 15000", filtered["text"])
                self.assertIn("low", filtered["text"])
                self.assertNotIn("high", filtered["text"])
            finally:
                server.fetch_group_members = original_fetch
                server.query_b50_batch_via_mcp = original_batch
                if previous_cache_dir is None:
                    os.environ.pop("GROUP_B50_CACHE_DIR", None)
                else:
                    os.environ["GROUP_B50_CACHE_DIR"] = previous_cache_dir

    def test_unqueryable_and_zero_rating_are_not_cached(self) -> None:
        previous_cache_dir = os.environ.get("GROUP_B50_CACHE_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["GROUP_B50_CACHE_DIR"] = tmpdir
            original_fetch = server.fetch_group_members
            original_batch = server.query_b50_batch_via_mcp

            try:
                server.fetch_group_members = lambda *args, **kwargs: [
                    {"userId": "10001", "displayName": "ok", "nickname": "ok", "card": None},
                    {"userId": "10002", "displayName": "zero", "nickname": "zero", "card": None},
                    {"userId": "10003", "displayName": "hidden", "nickname": "hidden", "card": None},
                ]
                server.query_b50_batch_via_mcp = lambda qqs, **kwargs: {
                    "counts": {"requested": len(qqs), "success": 2, "failure": 1},
                    "results": [
                        {
                            "qq": "10001",
                            "ok": True,
                            "player": fake_b50("10001", 15000)["player"],
                            "rating": 15000,
                            "b50Rating": 15000,
                            "result": fake_b50("10001", 15000),
                            "error": None,
                        },
                        {
                            "qq": "10002",
                            "ok": True,
                            "player": fake_b50("10002", 0)["player"],
                            "rating": 0,
                            "b50Rating": 0,
                            "result": fake_b50("10002", 0),
                            "error": None,
                        },
                        {
                            "qq": "10003",
                            "ok": False,
                            "player": None,
                            "rating": None,
                            "b50Rating": None,
                            "result": None,
                            "error": {"code": "FORBIDDEN", "message": "hidden"},
                        },
                    ],
                }

                started = server.group_b50_report({"groupId": "789", "queryDelayMs": 0})
                self.assertIn("已启动后台刷新任务", started["text"])
                self.assertEqual(wait_for_job("789")["status"], "completed")
                result = server.group_b50_job_status({"groupId": "789"})
                self.assertEqual(result["data"]["successCount"], 1)
                self.assertEqual(result["data"]["skippedCount"], 2)
                self.assertEqual([item["userId"] for item in result["data"]["results"]], ["10001"])
                self.assertNotIn("zero", result["text"])
                self.assertNotIn("hidden", result["text"])

                missing = server.group_b50_member_rank({"groupId": "789", "qq": "10003", "queryDelayMs": 0})
                self.assertFalse(missing["found"])
                self.assertEqual(missing["member"]["userId"], "10003")
                self.assertIn("没有可输出的排名", missing["text"])
            finally:
                server.fetch_group_members = original_fetch
                server.query_b50_batch_via_mcp = original_batch
                if previous_cache_dir is None:
                    os.environ.pop("GROUP_B50_CACHE_DIR", None)
                else:
                    os.environ["GROUP_B50_CACHE_DIR"] = previous_cache_dir

    def test_output_limit_slices_after_sort_and_filter(self) -> None:
        previous_cache_dir = os.environ.get("GROUP_B50_CACHE_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["GROUP_B50_CACHE_DIR"] = tmpdir
            original_fetch = server.fetch_group_members
            original_batch = server.query_b50_batch_via_mcp

            try:
                server.fetch_group_members = lambda *args, **kwargs: [
                    {"userId": "10001", "displayName": "low", "nickname": "low", "card": None},
                    {"userId": "10002", "displayName": "mid", "nickname": "mid", "card": None},
                    {"userId": "10003", "displayName": "high", "nickname": "high", "card": None},
                ]
                ratings = {"10001": 14000, "10002": 15000, "10003": 16000}
                server.query_b50_batch_via_mcp = lambda qqs, **kwargs: {
                    "counts": {"requested": len(qqs), "success": len(qqs), "failure": 0},
                    "results": [
                        {
                            "qq": qq,
                            "ok": True,
                            "player": fake_b50(qq, ratings[qq])["player"],
                            "rating": ratings[qq],
                            "b50Rating": ratings[qq],
                            "result": fake_b50(qq, ratings[qq]),
                            "error": None,
                        }
                        for qq in qqs
                    ],
                }

                started = server.group_b50_report(
                    {
                        "groupId": "999",
                        "sortOrder": "desc",
                        "outputMode": "detail",
                        "outputLimit": 1,
                        "queryDelayMs": 0,
                    }
                )
                self.assertIn("已启动后台刷新任务", started["text"])
                self.assertEqual(wait_for_job("999")["status"], "completed")
                top = server.group_b50_job_status(
                    {
                        "groupId": "999",
                        "sortOrder": "desc",
                        "outputMode": "detail",
                        "outputLimit": 1,
                    }
                )
                self.assertIn("输出: 前 1 人", top["text"])
                self.assertIn("本次展示: 1 条", top["text"])
                self.assertIn("high", top["text"])
                self.assertNotIn("mid", top["text"])
                self.assertNotIn("low", top["text"])
                self.assertEqual(len(top["data"]["results"]), 3)

                bottom = server.group_b50_report(
                    {
                        "groupId": "999",
                        "sortOrder": "asc",
                        "outputMode": "rating",
                        "outputLimit": 2,
                        "ratingMin": 14000,
                        "ratingMax": 15000,
                        "queryDelayMs": 0,
                    }
                )
                self.assertIn("匹配: 2 条", bottom["text"])
                self.assertIn("low", bottom["text"])
                self.assertIn("mid", bottom["text"])
                self.assertNotIn("high", bottom["text"])
                self.assertNotIn("## 1.", bottom["text"])
            finally:
                server.fetch_group_members = original_fetch
                server.query_b50_batch_via_mcp = original_batch
                if previous_cache_dir is None:
                    os.environ.pop("GROUP_B50_CACHE_DIR", None)
                else:
                    os.environ["GROUP_B50_CACHE_DIR"] = previous_cache_dir

    def test_stale_cache_is_refreshed(self) -> None:
        previous_cache_dir = os.environ.get("GROUP_B50_CACHE_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["GROUP_B50_CACHE_DIR"] = tmpdir
            original_fetch = server.fetch_group_members
            original_batch = server.query_b50_batch_via_mcp

            try:
                server.group_cache_dir("456").mkdir(parents=True)
                server.write_cache(
                    "456",
                    {
                        "groupId": "456",
                        "fetchedAt": (datetime.now(timezone.utc) - timedelta(days=4)).isoformat(),
                        "memberCount": 0,
                        "successCount": 0,
                        "failureCount": 0,
                        "members": [],
                        "results": [],
                    },
                )

                server.fetch_group_members = lambda *args, **kwargs: [
                    {"userId": "10001", "displayName": "low", "nickname": "low", "card": None}
                ]
                server.query_b50_batch_via_mcp = lambda qqs, **kwargs: {
                    "counts": {"requested": len(qqs), "success": len(qqs), "failure": 0},
                    "results": [
                        {
                            "qq": qq,
                            "ok": True,
                            "player": fake_b50(qq, 15000)["player"],
                            "rating": 15000,
                            "b50Rating": 15000,
                            "result": fake_b50(qq, 15000),
                            "error": None,
                        }
                        for qq in qqs
                    ],
                }

                result = server.group_b50_report({"groupId": "456", "queryDelayMs": 0})
                self.assertEqual(result["cacheRefreshReason"], "stale")
                self.assertEqual(wait_for_job("456")["status"], "completed")
                result = server.group_b50_job_status({"groupId": "456"})
                self.assertEqual(result["cacheRefreshReason"], "stale")
                self.assertIn("本次已重新拉取并刷新缓存", result["text"])
                self.assertNotIn("本次使用一天内缓存", result["text"])
                self.assertEqual(result["cache"]["memberCount"], 1)
            finally:
                server.fetch_group_members = original_fetch
                server.query_b50_batch_via_mcp = original_batch
                if previous_cache_dir is None:
                    os.environ.pop("GROUP_B50_CACHE_DIR", None)
                else:
                    os.environ["GROUP_B50_CACHE_DIR"] = previous_cache_dir

    def test_transient_b50_failures_do_not_write_partial_cache(self) -> None:
        previous_cache_dir = os.environ.get("GROUP_B50_CACHE_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["GROUP_B50_CACHE_DIR"] = tmpdir
            original_fetch = server.fetch_group_members
            original_batch = server.query_b50_batch_via_mcp

            try:
                server.fetch_group_members = lambda *args, **kwargs: [
                    {"userId": "10001", "displayName": "ok", "nickname": "ok", "card": None},
                    {"userId": "10002", "displayName": "dns", "nickname": "dns", "card": None},
                ]
                server.query_b50_batch_via_mcp = lambda qqs, **kwargs: {
                    "counts": {"requested": len(qqs), "success": 1, "failure": 1},
                    "results": [
                        {
                            "qq": "10001",
                            "ok": True,
                            "player": fake_b50("10001", 15000)["player"],
                            "rating": 15000,
                            "b50Rating": 15000,
                            "result": fake_b50("10001", 15000),
                            "error": None,
                        },
                        {
                            "qq": "10002",
                            "ok": False,
                            "player": None,
                            "rating": None,
                            "b50Rating": None,
                            "result": None,
                            "error": {"code": "NETWORK_ERROR", "message": "temporary dns failure"},
                        },
                    ],
                }

                started = server.group_b50_report({"groupId": "654", "queryDelayMs": 0})
                self.assertIn("已启动后台刷新任务", started["text"])
                status = wait_for_job("654")
                self.assertEqual(status["status"], "failed")
                self.assertEqual(status["error"]["code"], "B50_TRANSIENT_FAILURE")
                self.assertIsNone(server.read_cache("654"))
            finally:
                server.fetch_group_members = original_fetch
                server.query_b50_batch_via_mcp = original_batch
                if previous_cache_dir is None:
                    os.environ.pop("GROUP_B50_CACHE_DIR", None)
                else:
                    os.environ["GROUP_B50_CACHE_DIR"] = previous_cache_dir

    def test_force_refresh_prevents_stale_job_from_overwriting_new_cache(self) -> None:
        previous_cache_dir = os.environ.get("GROUP_B50_CACHE_DIR")
        group_id = "force-race"
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["GROUP_B50_CACHE_DIR"] = tmpdir
            original_fetch = server.fetch_group_members
            original_batch = server.query_b50_batch_via_mcp
            first_query_entered = threading.Event()
            release_first_query = threading.Event()
            query_lock = threading.Lock()
            query_count = {"value": 0}

            def query_batch(qqs, **kwargs):
                with query_lock:
                    query_count["value"] += 1
                    call_index = query_count["value"]
                if call_index == 1:
                    first_query_entered.set()
                    self.assertTrue(release_first_query.wait(timeout=2))
                    rating = 12000
                else:
                    rating = 16000
                return {
                    "counts": {"requested": len(qqs), "success": len(qqs), "failure": 0},
                    "results": [
                        {
                            "qq": qq,
                            "ok": True,
                            "player": fake_b50(qq, rating)["player"],
                            "rating": rating,
                            "b50Rating": rating,
                            "result": fake_b50(qq, rating),
                            "error": None,
                        }
                        for qq in qqs
                    ],
                }

            try:
                server.fetch_group_members = lambda *args, **kwargs: [
                    {"userId": "10001", "displayName": "player", "nickname": "player", "card": None}
                ]
                server.query_b50_batch_via_mcp = query_batch

                server.group_b50_report({"groupId": group_id, "queryDelayMs": 0})
                self.assertTrue(first_query_entered.wait(timeout=2))
                stale_thread = job_runner.BACKGROUND_JOBS[(server.B50_FEATURE, group_id)]

                server.group_b50_report({"groupId": group_id, "forceRefresh": True, "queryDelayMs": 0})
                fresh_thread = job_runner.BACKGROUND_JOBS[(server.B50_FEATURE, group_id)]
                fresh_thread.join(timeout=2)
                cache = server.read_cache(group_id)
                self.assertIsNotNone(cache)
                self.assertEqual(cache["results"][0]["rating"], 16000)
                status_report = server.group_b50_job_status({"groupId": group_id})
                self.assertEqual(status_report["cacheRefreshReason"], "forceRefresh")
                self.assertIn("本次已重新拉取并刷新缓存", status_report["text"])

                release_first_query.set()
                stale_thread.join(timeout=2)
                cache = server.read_cache(group_id)
                self.assertIsNotNone(cache)
                self.assertEqual(cache["results"][0]["rating"], 16000)
                self.assertEqual(server.read_job_status(group_id)["status"], "completed")
            finally:
                release_first_query.set()
                server.fetch_group_members = original_fetch
                server.query_b50_batch_via_mcp = original_batch
                job_runner.BACKGROUND_JOBS.pop((server.B50_FEATURE, group_id), None)
                if previous_cache_dir is None:
                    os.environ.pop("GROUP_B50_CACHE_DIR", None)
                else:
                    os.environ["GROUP_B50_CACHE_DIR"] = previous_cache_dir

    def test_group_cache_status_displays_utc_time_in_local_timezone(self) -> None:
        previous_cache_dir = os.environ.get("GROUP_B50_CACHE_DIR")
        previous_tz = os.environ.get("GROUP_B50_DISPLAY_TZ")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["GROUP_B50_CACHE_DIR"] = tmpdir
            os.environ["GROUP_B50_DISPLAY_TZ"] = "Asia/Shanghai"
            try:
                server.write_cache(
                    "time",
                    {
                        "groupId": "time",
                        "fetchedAt": "2026-05-27T16:58:31.395064+00:00",
                        "memberCount": 0,
                        "successCount": 0,
                        "failureCount": 0,
                        "skippedCount": 0,
                        "members": [],
                        "results": [],
                    },
                )

                status_text = server.format_cache_status_text(server.cache_status({"groupId": "time"}))
                self.assertIn("生成时间: 2026-05-28 00:58:31 +08:00", status_text)
                job_text = server.format_job_status_text(
                    "time",
                    {
                        "status": "completed",
                        "startedAt": "2026-05-27T16:58:29.420017+00:00",
                        "finishedAt": "2026-05-27T16:58:31.395064+00:00",
                        "message": "done",
                    },
                    server.cache_status({"groupId": "time"}),
                )
                self.assertIn("启动时间: 2026-05-28 00:58:29 +08:00", job_text)
                self.assertIn("完成时间: 2026-05-28 00:58:31 +08:00", job_text)
            finally:
                if previous_cache_dir is None:
                    os.environ.pop("GROUP_B50_CACHE_DIR", None)
                else:
                    os.environ["GROUP_B50_CACHE_DIR"] = previous_cache_dir
                if previous_tz is None:
                    os.environ.pop("GROUP_B50_DISPLAY_TZ", None)
                else:
                    os.environ["GROUP_B50_DISPLAY_TZ"] = previous_tz


    def test_sort_and_filter_by_fit_index(self) -> None:
        cache = {
            "groupId": "fit",
            "fetchedAt": "2026-05-27T16:58:31.395064+00:00",
            "cacheRefreshReason": "hit",
            "memberCount": 3,
            "successCount": 3,
            "failureCount": 0,
            "skippedCount": 0,
            "members": [],
            "results": [
                {
                    "userId": "1",
                    "displayName": "watery",
                    "ok": True,
                    "rating": 14500,
                    "b50Rating": 14500,
                    "fitIndex": {
                        "available": True,
                        "label": "明显虚高（水）",
                        "virtualRating": 180.0,
                        "virtualRatio": 1.5,
                        "counted": 50,
                        "missing": 0,
                    },
                },
                {
                    "userId": "2",
                    "displayName": "solid",
                    "ok": True,
                    "rating": 15500,
                    "b50Rating": 15500,
                    "fitIndex": {
                        "available": True,
                        "label": "明显虚低（硬实力）",
                        "virtualRating": -240.0,
                        "virtualRatio": -1.6,
                        "counted": 50,
                        "missing": 0,
                    },
                },
                {
                    "userId": "3",
                    "displayName": "noisy",
                    "ok": True,
                    "rating": 14000,
                    "b50Rating": 14000,
                    "fitIndex": {
                        "available": True,
                        "label": "略微虚高",
                        "virtualRating": 50.0,
                        "virtualRatio": 0.3,
                        "counted": 50,
                        "missing": 0,
                    },
                },
            ],
        }

        # sortBy=fitIndex desc → 最水的排第一
        watery_first = server.sorted_results(cache, "desc", sort_by="fitIndex")
        self.assertEqual([row["userId"] for row in watery_first], ["1", "3", "2"])

        # sortBy=fitIndex asc → 硬实力排第一
        solid_first = server.sorted_results(cache, "asc", sort_by="fitIndex")
        self.assertEqual([row["userId"] for row in solid_first], ["2", "3", "1"])

        # fitIndex 区间筛选：只看 >= 1.0% 的
        only_watery = server.sorted_results(
            cache, "desc", sort_by="fitIndex", fit_index_min=1.0
        )
        self.assertEqual([row["userId"] for row in only_watery], ["1"])

        # 默认 rating 排序不受影响
        rating_desc = server.sorted_results(cache, "desc")
        self.assertEqual([row["userId"] for row in rating_desc], ["2", "1", "3"])

    def test_sorted_results_overlays_fresh_player_cache_b50(self) -> None:
        from player_cache import store as player_cache_store

        # 群缓存里 QQ "10001" rating=14000，"10002" rating=15000
        cache = {
            "groupId": "999",
            "fetchedAt": "2026-05-27T16:58:31+00:00",
            "results": [
                {
                    "userId": "10001",
                    "displayName": "low",
                    "ok": True,
                    "rating": 14000,
                    "b50Rating": 14000,
                    "b50": {"player": {"rating": 14000}, "ratingBreakdown": {"total": 14000}},
                },
                {
                    "userId": "10002",
                    "displayName": "high",
                    "ok": True,
                    "rating": 15000,
                    "b50Rating": 15000,
                    "b50": {"player": {"rating": 15000}, "ratingBreakdown": {"total": 15000}},
                },
            ],
        }
        # 10001 之后单独查了自己的 B50，rating 涨到 16000
        player_cache_store.write_player_b50(
            "10001",
            {"player": {"rating": 16000, "nickname": "lowToHigh"}, "ratingBreakdown": {"total": 16000}},
        )

        rows = server.sorted_results(cache, "desc")
        # overlay 后 10001 是 16000，排第一
        self.assertEqual([row["userId"] for row in rows], ["10001", "10002"])
        self.assertEqual(rows[0]["rating"], 16000)
        self.assertEqual(rows[0]["b50Rating"], 16000)
        # 10002 没查，rating 保持群缓存的 15000
        self.assertEqual(rows[1]["rating"], 15000)

    def test_sorted_results_preserves_fit_index_when_player_cache_is_light(self) -> None:
        from player_cache import store as player_cache_store

        cached_fit = {
            "available": True,
            "label": "明显虚高（水）",
            "virtualRating": 120.0,
            "virtualRatio": 1.2,
            "counted": 50,
            "missing": 0,
        }
        cache = {
            "groupId": "999",
            "results": [
                {
                    "userId": "10001",
                    "displayName": "player",
                    "ok": True,
                    "rating": 14000,
                    "b50Rating": 14000,
                    "fitIndex": cached_fit,
                    "b50": {"player": {"rating": 14000}, "fitIndex": cached_fit},
                },
            ],
        }
        player_cache_store.write_player_b50(
            "10001",
            {
                "player": {"rating": 16000, "nickname": "new"},
                "ratingBreakdown": {"total": 16000},
                "chartMetadata": {"skipped": True, "available": False},
            },
        )

        rows = server.sorted_results(cache, "desc")

        self.assertEqual(rows[0]["rating"], 16000)
        self.assertEqual(rows[0]["fitIndex"], cached_fit)

    def test_build_group_cache_does_not_reuse_light_player_b50_cache(self) -> None:
        from player_cache import store as player_cache_store

        previous_cache_dir = os.environ.get("GROUP_B50_CACHE_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["GROUP_B50_CACHE_DIR"] = tmpdir
            player_cache_store.write_player_b50(
                "10001",
                {
                    "player": {"rating": 15000, "nickname": "light"},
                    "ratingBreakdown": {"total": 15000},
                    "chartMetadata": {"skipped": True, "available": False},
                },
            )
            original_fetch = server.fetch_group_members
            original_batch = server.query_b50_batch_via_mcp
            queried: list[list[str]] = []
            rich_b50 = fake_b50("10001", 15000)
            rich_b50["fitIndex"] = {
                "available": True,
                "label": "明显虚高（水）",
                "b50": {"virtualRating": 100.0, "virtualRatio": 1.0, "counted": 50, "missing": 0},
            }

            try:
                server.fetch_group_members = lambda *args, **kwargs: [
                    {"userId": "10001", "displayName": "player", "nickname": "player", "card": None}
                ]

                def query_batch(qqs, **kwargs):
                    queried.append(list(qqs))
                    return {
                        "counts": {"requested": len(qqs), "success": len(qqs), "failure": 0},
                        "results": [
                            {
                                "qq": "10001",
                                "ok": True,
                                "player": rich_b50["player"],
                                "rating": 15000,
                                "b50Rating": 15000,
                                "fitIndex": server.summarize_fit_index_for_group(rich_b50["fitIndex"]),
                                "result": rich_b50,
                                "error": None,
                            }
                        ],
                    }

                server.query_b50_batch_via_mcp = query_batch
                server.write_job_status(
                    "light-cache",
                    {
                        "jobId": "job",
                        "groupId": "light-cache",
                        "status": "running",
                        "startedAt": datetime.now(timezone.utc).isoformat(),
                    },
                )
                cache = server.build_group_cache(
                    "light-cache",
                    "job",
                    {},
                    timeout_ms=1000,
                    query_delay_ms=0,
                    max_concurrency=1,
                    batch_size=100,
                    max_members=None,
                )

                self.assertEqual(queried, [["10001"]])
                self.assertEqual(cache["cacheHitCount"], 0)
                self.assertTrue(cache["results"][0]["fitIndex"]["available"])
            finally:
                server.fetch_group_members = original_fetch
                server.query_b50_batch_via_mcp = original_batch
                if previous_cache_dir is None:
                    os.environ.pop("GROUP_B50_CACHE_DIR", None)
                else:
                    os.environ["GROUP_B50_CACHE_DIR"] = previous_cache_dir

    def test_overlay_skips_player_cache_with_invalid_rating(self) -> None:
        from player_cache import store as player_cache_store

        cache = {
            "groupId": "999",
            "results": [
                {
                    "userId": "10001",
                    "displayName": "x",
                    "ok": True,
                    "rating": 14000,
                    "b50Rating": 14000,
                },
            ],
        }
        # player_cache 里被污染：rating=0（隐私设置改了）
        player_cache_store.write_player_b50("10001", {"player": {"rating": 0}})

        rows = server.sorted_results(cache, "desc")
        # rating=0 不应该污染群榜，保留群缓存的 14000
        self.assertEqual(rows[0]["rating"], 14000)

    def test_summarize_fit_index_handles_both_shapes(self) -> None:
        # 已摘要过的形状（来自 query_b50_batch）
        flat = server.summarize_fit_index_for_group(
            {"available": True, "label": "略微虚高", "virtualRating": 47.3, "virtualRatio": 0.31, "counted": 50, "missing": 0}
        )
        self.assertEqual(flat["virtualRatio"], 0.31)
        self.assertTrue(flat["available"])

        # 完整的 fitIndex 结构（直接来自 enrich）
        nested = server.summarize_fit_index_for_group(
            {
                "available": True,
                "label": "明显虚高（水）",
                "b50": {"virtualRating": 200.0, "virtualRatio": 1.5, "counted": 50, "missing": 0},
            }
        )
        self.assertEqual(nested["virtualRatio"], 1.5)
        self.assertTrue(nested["available"])

        self.assertIsNone(server.summarize_fit_index_for_group(None))

    def test_song_force_refresh_can_preheat_cache_without_song(self) -> None:
        previous_cache_dir = os.environ.get("GROUP_SONG_CACHE_DIR")
        previous_window = os.environ.get("GROUP_SONG_RECORDS_BATCH_WINDOW_MS")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["GROUP_SONG_CACHE_DIR"] = tmpdir
            os.environ["GROUP_SONG_RECORDS_BATCH_WINDOW_MS"] = "0"
            original_fetch_members = server.fetch_group_members
            original_fetch_records = song_rank._fetch_player_records_via_diving_fish
            try:
                server.fetch_group_members = lambda *args, **kwargs: [
                    {"userId": "10001", "displayName": "player", "nickname": "player", "card": None}
                ]
                song_rank._fetch_player_records_via_diving_fish = (
                    lambda qq, **kwargs: fake_records_doc(qq)
                )

                started = song_rank.group_song_score_report(
                    {"groupId": "song-preheat", "forceRefresh": True, "queryDelayMs": 0}
                )
                self.assertIsNone(started["musicId"])
                self.assertIn("已启动后台任务", started["text"])
                self.assertEqual(wait_for_song_job("song-preheat")["status"], "completed")

                status = song_rank.group_song_score_report({"groupId": "song-preheat"})
                self.assertIsNone(status["musicId"])
                self.assertIn("单曲成绩缓存已就绪", status["text"])
                self.assertEqual(status["cache"]["successCount"], 1)
            finally:
                server.fetch_group_members = original_fetch_members
                song_rank._fetch_player_records_via_diving_fish = original_fetch_records
                job_runner.BACKGROUND_JOBS.pop((song_rank.SONG_FEATURE, "song-preheat"), None)
                if previous_cache_dir is None:
                    os.environ.pop("GROUP_SONG_CACHE_DIR", None)
                else:
                    os.environ["GROUP_SONG_CACHE_DIR"] = previous_cache_dir
                if previous_window is None:
                    os.environ.pop("GROUP_SONG_RECORDS_BATCH_WINDOW_MS", None)
                else:
                    os.environ["GROUP_SONG_RECORDS_BATCH_WINDOW_MS"] = previous_window

    def test_song_refresh_batches_and_dedupes_records_across_groups(self) -> None:
        previous_cache_dir = os.environ.get("GROUP_SONG_CACHE_DIR")
        previous_window = os.environ.get("GROUP_SONG_RECORDS_BATCH_WINDOW_MS")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["GROUP_SONG_CACHE_DIR"] = tmpdir
            os.environ["GROUP_SONG_RECORDS_BATCH_WINDOW_MS"] = "100"
            original_fetch_members = server.fetch_group_members
            original_fetch_records = song_rank._fetch_player_records_via_diving_fish
            barrier = threading.Barrier(2)
            calls: list[str] = []
            calls_lock = threading.Lock()

            def fetch_members(group_id, *args, **kwargs):
                barrier.wait(timeout=2)
                members = {
                    "song-g1": ["10001", "10002"],
                    "song-g2": ["10002", "10003"],
                }[str(group_id)]
                return [
                    {"userId": qq, "displayName": qq, "nickname": qq, "card": None}
                    for qq in members
                ]

            def fetch_records(qq, **kwargs):
                with calls_lock:
                    calls.append(str(qq))
                return fake_records_doc(str(qq))

            try:
                server.fetch_group_members = fetch_members
                song_rank._fetch_player_records_via_diving_fish = fetch_records

                first = song_rank.group_song_score_report(
                    {"groupId": "song-g1", "forceRefresh": True, "queryDelayMs": 0, "maxConcurrency": 3}
                )
                second = song_rank.group_song_score_report(
                    {"groupId": "song-g2", "forceRefresh": True, "queryDelayMs": 0, "maxConcurrency": 3}
                )
                self.assertIn("已启动后台任务", first["text"])
                self.assertIn("已启动后台任务", second["text"])
                self.assertEqual(wait_for_song_job("song-g1")["status"], "completed")
                self.assertEqual(wait_for_song_job("song-g2")["status"], "completed")

                self.assertEqual(sorted(calls), ["10001", "10002", "10003"])
                g1 = song_rank.song_read_cache("song-g1")
                g2 = song_rank.song_read_cache("song-g2")
                self.assertEqual(g1["successCount"], 2)
                self.assertEqual(g2["successCount"], 2)
                self.assertEqual(g1["sharedFetchCount"] + g2["sharedFetchCount"], 1)
            finally:
                server.fetch_group_members = original_fetch_members
                song_rank._fetch_player_records_via_diving_fish = original_fetch_records
                job_runner.BACKGROUND_JOBS.pop((song_rank.SONG_FEATURE, "song-g1"), None)
                job_runner.BACKGROUND_JOBS.pop((song_rank.SONG_FEATURE, "song-g2"), None)
                if previous_cache_dir is None:
                    os.environ.pop("GROUP_SONG_CACHE_DIR", None)
                else:
                    os.environ["GROUP_SONG_CACHE_DIR"] = previous_cache_dir
                if previous_window is None:
                    os.environ.pop("GROUP_SONG_RECORDS_BATCH_WINDOW_MS", None)
                else:
                    os.environ["GROUP_SONG_RECORDS_BATCH_WINDOW_MS"] = previous_window

    def test_song_member_rank_returns_desc_and_asc_rank_info(self) -> None:
        from player_cache import store as player_cache_store

        previous_song_cache_dir = os.environ.get("GROUP_SONG_CACHE_DIR")
        previous_player_cache_dir = os.environ.get("PLAYER_CACHE_DIR")
        with tempfile.TemporaryDirectory() as song_tmp, tempfile.TemporaryDirectory() as player_tmp:
            try:
                os.environ["GROUP_SONG_CACHE_DIR"] = song_tmp
                os.environ["PLAYER_CACHE_DIR"] = player_tmp
                group_id = "song-rank-info"

                def records_doc(qq: str, achievements: float) -> dict:
                    doc = fake_records_doc(qq)
                    doc["records"][0]["achievements"] = achievements
                    doc["records"][0]["ra"] = int(achievements * 3)
                    return doc

                for qq, achievements in (("10001", 99.0), ("10002", 100.5), ("10003", 100.0)):
                    player_cache_store.write_player_records(qq, records_doc(qq, achievements))

                song_rank.song_write_cache(
                    group_id,
                    {
                        "groupId": group_id,
                        "feature": song_rank.SONG_FEATURE,
                        "fetchedAt": datetime.now(timezone.utc).isoformat(),
                        "results": [
                            {"userId": "10001", "displayName": "low", "ok": True, "rating": 14000},
                            {"userId": "10002", "displayName": "high", "ok": True, "rating": 15000},
                            {"userId": "10003", "displayName": "mid", "ok": True, "rating": 14500},
                        ],
                    },
                )

                result = song_rank.group_song_score_member_rank(
                    {"groupId": group_id, "qq": "10003", "musicId": 1000, "contextSize": 1}
                )

                self.assertTrue(result["found"])
                self.assertEqual(result["rank"], 2)
                self.assertEqual(result["reverseRank"], 2)
                self.assertEqual(result["rankInfo"]["rankDesc"], 2)
                self.assertEqual(result["rankInfo"]["rankAsc"], 2)
                self.assertEqual(result["rankInfo"]["totalRanked"], 3)
                self.assertIn("达成率倒序排名: 2 / 3", result["text"])
                self.assertIn("达成率正序排名: 2 / 3", result["text"])
                self.assertIn("附近排名（达成率排名正序）", result["text"])
                self.assertNotIn("附近排名（达成率升序）", result["text"])

                ranged = song_rank.group_song_score_report(
                    {"groupId": group_id, "musicId": 1000, "startRank": 2, "endRank": 2}
                )
                self.assertIn("输出: 第 2-2 名", ranged["text"])
                self.assertIn("| 2 | 10003 |", ranged["text"])
                self.assertNotIn("| 1 | 10002 |", ranged["text"])
            finally:
                if previous_song_cache_dir is None:
                    os.environ.pop("GROUP_SONG_CACHE_DIR", None)
                else:
                    os.environ["GROUP_SONG_CACHE_DIR"] = previous_song_cache_dir
                if previous_player_cache_dir is None:
                    os.environ.pop("PLAYER_CACHE_DIR", None)
                else:
                    os.environ["PLAYER_CACHE_DIR"] = previous_player_cache_dir

    def test_song_query_single_level_search_fallback_overrides_requested_level(self) -> None:
        original_search = song_rank._call_maimai_local_search_tool
        calls: list[dict] = []

        def fake_search(tool_name: str, arguments: dict, *, timeout_ms: int) -> dict:
            del tool_name, timeout_ms
            calls.append(dict(arguments))
            if arguments.get("difficulty") == "Master":
                songs: list[dict] = []
            else:
                songs = [
                    {
                        "id": "1000",
                        "title": "utage-only",
                        "matched_charts": [
                            {
                                "fit_source_id": "1000",
                                "difficulty_index": 0,
                            }
                        ],
                    }
                ]
            return {"content": [{"type": "text", "text": json.dumps({"songs": songs})}]}

        try:
            song_rank._call_maimai_local_search_tool = fake_search
            music_id, level_index = song_rank._resolve_optional_song_target_from_arguments(
                {"songQuery": "utage-only", "levelIndex": 3},
                3,
            )
            self.assertEqual(music_id, 1000)
            self.assertEqual(level_index, 0)
            self.assertEqual(calls[0]["difficulty"], "Master")
            self.assertNotIn("difficulty", calls[1])
        finally:
            song_rank._call_maimai_local_search_tool = original_search

    def test_song_query_without_level_uses_highest_search_difficulty(self) -> None:
        original_search = song_rank._call_maimai_local_search_tool
        calls: list[dict] = []

        def fake_search(tool_name: str, arguments: dict, *, timeout_ms: int) -> dict:
            del tool_name, timeout_ms
            calls.append(dict(arguments))
            songs = [
                {
                    "id": "1000",
                    "title": "has-remaster",
                    "matched_charts": [
                        {"fit_source_id": "1000", "difficulty_index": 0},
                        {"fit_source_id": "1000", "difficulty_index": 3},
                        {"fit_source_id": "1000", "difficulty_index": 4},
                    ],
                }
            ]
            return {"content": [{"type": "text", "text": json.dumps({"songs": songs})}]}

        try:
            song_rank._call_maimai_local_search_tool = fake_search
            music_id, level_index = song_rank._resolve_optional_song_target_from_arguments(
                {"songQuery": "has-remaster"},
                None,
            )
            self.assertEqual(music_id, 1000)
            self.assertEqual(level_index, 4)
            self.assertNotIn("difficulty", calls[0])
        finally:
            song_rank._call_maimai_local_search_tool = original_search

    def test_song_rank_without_level_uses_highest_cached_level(self) -> None:
        from player_cache import store as player_cache_store

        previous_song_cache_dir = os.environ.get("GROUP_SONG_CACHE_DIR")
        previous_player_cache_dir = os.environ.get("PLAYER_CACHE_DIR")
        with tempfile.TemporaryDirectory() as song_tmp, tempfile.TemporaryDirectory() as player_tmp:
            try:
                os.environ["GROUP_SONG_CACHE_DIR"] = song_tmp
                os.environ["PLAYER_CACHE_DIR"] = player_tmp
                group_id = "song-highest-level"

                def records_doc(qq: str, white_achievements: float, master_achievements: float) -> dict:
                    doc = fake_records_doc(qq)
                    master = dict(doc["records"][0])
                    master["level_index"] = 3
                    master["level_label"] = "Master"
                    master["achievements"] = master_achievements
                    master["ra"] = 500
                    remaster = dict(master)
                    remaster["level_index"] = 4
                    remaster["level_label"] = "Re:Master"
                    remaster["achievements"] = white_achievements
                    remaster["ra"] = 100
                    doc["records"] = [master, remaster]
                    return doc

                for qq, white_achievements, master_achievements in (
                    ("10001", 98.0, 100.0),
                    ("10002", 99.0, 100.5),
                ):
                    player_cache_store.write_player_records(
                        qq,
                        records_doc(qq, white_achievements, master_achievements),
                    )

                song_rank.song_write_cache(
                    group_id,
                    {
                        "groupId": group_id,
                        "feature": song_rank.SONG_FEATURE,
                        "fetchedAt": datetime.now(timezone.utc).isoformat(),
                        "results": [
                            {"userId": "10001", "displayName": "low", "ok": True, "rating": 14000},
                            {"userId": "10002", "displayName": "high", "ok": True, "rating": 15000},
                        ],
                    },
                )

                result = song_rank.group_song_score_report({"groupId": group_id, "musicId": 1000})
                self.assertEqual(result["levelIndex"], 4)
                self.assertEqual(result["matchedCount"], 2)
                self.assertIn("难度=Re:Master", result["text"])
                self.assertTrue(all(row["record"]["levelIndex"] == 4 for row in result["rows"]))
            finally:
                if previous_song_cache_dir is None:
                    os.environ.pop("GROUP_SONG_CACHE_DIR", None)
                else:
                    os.environ["GROUP_SONG_CACHE_DIR"] = previous_song_cache_dir
                if previous_player_cache_dir is None:
                    os.environ.pop("PLAYER_CACHE_DIR", None)
                else:
                    os.environ["PLAYER_CACHE_DIR"] = previous_player_cache_dir

    def test_song_rank_single_cached_level_fallback_overrides_requested_level(self) -> None:
        from player_cache import store as player_cache_store

        previous_song_cache_dir = os.environ.get("GROUP_SONG_CACHE_DIR")
        previous_player_cache_dir = os.environ.get("PLAYER_CACHE_DIR")
        with tempfile.TemporaryDirectory() as song_tmp, tempfile.TemporaryDirectory() as player_tmp:
            try:
                os.environ["GROUP_SONG_CACHE_DIR"] = song_tmp
                os.environ["PLAYER_CACHE_DIR"] = player_tmp
                group_id = "song-single-level"

                def records_doc(qq: str, achievements: float) -> dict:
                    doc = fake_records_doc(qq)
                    doc["records"][0]["level_index"] = 0
                    doc["records"][0]["level_label"] = "Basic"
                    doc["records"][0]["achievements"] = achievements
                    doc["records"][0]["ra"] = int(achievements * 3)
                    return doc

                for qq, achievements in (("10001", 99.0), ("10002", 100.0)):
                    player_cache_store.write_player_records(qq, records_doc(qq, achievements))

                song_rank.song_write_cache(
                    group_id,
                    {
                        "groupId": group_id,
                        "feature": song_rank.SONG_FEATURE,
                        "fetchedAt": datetime.now(timezone.utc).isoformat(),
                        "results": [
                            {"userId": "10001", "displayName": "low", "ok": True, "rating": 14000},
                            {"userId": "10002", "displayName": "high", "ok": True, "rating": 15000},
                        ],
                    },
                )

                result = song_rank.group_song_score_report(
                    {"groupId": group_id, "musicId": 1000, "levelIndex": 3}
                )
                self.assertEqual(result["levelIndex"], 0)
                self.assertEqual(result["matchedCount"], 2)
                self.assertIn("难度=Basic", result["text"])
            finally:
                if previous_song_cache_dir is None:
                    os.environ.pop("GROUP_SONG_CACHE_DIR", None)
                else:
                    os.environ["GROUP_SONG_CACHE_DIR"] = previous_song_cache_dir
                if previous_player_cache_dir is None:
                    os.environ.pop("PLAYER_CACHE_DIR", None)
                else:
                    os.environ["PLAYER_CACHE_DIR"] = previous_player_cache_dir

    def test_song_rank_domain_error_is_returned_as_tool_error(self) -> None:
        original_member_rank = song_rank.group_song_score_member_rank

        class GroupB50Error(Exception):
            code = "SONG_NOT_FOUND"

            def to_dict(self) -> dict:
                return {"code": self.code, "message": str(self)}

        try:
            def raise_other_module_error(arguments):
                del arguments
                raise GroupB50Error("maimai-local-search 没有找到曲目：不存在")

            song_rank.group_song_score_member_rank = raise_other_module_error
            response = server.handle_tool_call(
                1,
                {
                    "name": song_rank.GROUP_SONG_SCORE_MEMBER_RANK_TOOL["name"],
                    "arguments": {"groupId": "123", "qq": "10001", "songQuery": "不存在"},
                },
            )

            result = response["result"]
            self.assertTrue(result["isError"])
            self.assertIn("没有找到曲目", result["content"][0]["text"])
            self.assertEqual(result["structuredContent"]["error"]["code"], "SONG_NOT_FOUND")
        finally:
            song_rank.group_song_score_member_rank = original_member_rank


if __name__ == "__main__":
    unittest.main()
