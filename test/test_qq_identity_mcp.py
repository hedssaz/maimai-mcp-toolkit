from __future__ import annotations

import os
import tempfile
import unittest

from qq_identity_mcp import server
from qq_identity_mcp import store
import diving_fish_b50_mcp.server as b50_server
import group_b50_mcp.server as group_server


class QqIdentityMcpTests(unittest.TestCase):
    def test_refresh_builds_identity_cache_without_friend_remarks(self) -> None:
        previous_cache_dir = os.environ.get("QQ_IDENTITY_CACHE_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["QQ_IDENTITY_CACHE_DIR"] = tmpdir
            original_friends = server.fetch_friend_list
            original_groups = server.fetch_group_list
            original_members = server.fetch_group_members
            try:
                server.fetch_friend_list = lambda **_kwargs: [
                    {"userId": "10001", "nickname": "qq-name", "remark": "do-not-save"}
                ]
                server.fetch_group_list = lambda **_kwargs: [
                    {"groupId": "20001", "groupName": "mai-group", "memberCount": 2}
                ]
                server.fetch_group_members = lambda *_args, **_kwargs: [
                    {"userId": "10001", "nickname": "qq-name", "card": "group-card"},
                    {"userId": "10002", "nickname": "same-name", "card": ""},
                ]

                cache = server.build_identity_cache(
                    napcat_base_url="http://napcat:3000",
                    no_cache=True,
                    timeout_ms=1000,
                    group_delay_ms=0,
                    max_groups=None,
                )
                cache["fetchedAt"] = store.now_iso()
                store.write_cache(cache)

                user = store.read_cache()["users"]["10001"]
                self.assertEqual(user["friendNickname"], "qq-name")
                self.assertNotIn("remark", user)
                self.assertEqual(user["groups"]["20001"]["groupNickname"], "group-card")

                identity = store.get_identity("10001", "20001")
                self.assertEqual(identity["qqNickname"], "qq-name")
                self.assertEqual(identity["preferredGroup"]["groupNickname"], "group-card")
            finally:
                server.fetch_friend_list = original_friends
                server.fetch_group_list = original_groups
                server.fetch_group_members = original_members
                if previous_cache_dir is None:
                    os.environ.pop("QQ_IDENTITY_CACHE_DIR", None)
                else:
                    os.environ["QQ_IDENTITY_CACHE_DIR"] = previous_cache_dir

    def test_resolve_identity_reports_ambiguous_duplicate_names(self) -> None:
        previous_cache_dir = os.environ.get("QQ_IDENTITY_CACHE_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["QQ_IDENTITY_CACHE_DIR"] = tmpdir
            try:
                cache = store.empty_cache()
                cache["fetchedAt"] = store.now_iso()
                store.upsert_group_member(
                    cache,
                    group_id="20001",
                    group_name="mai-group",
                    qq="10001",
                    nickname="dup",
                    card="same",
                )
                store.upsert_group_member(
                    cache,
                    group_id="20001",
                    group_name="mai-group",
                    qq="10002",
                    nickname="other",
                    card="same",
                )
                store.write_cache(cache)

                result = store.resolve_identities("same", group_id="20001")
                self.assertTrue(result["ambiguous"])
                self.assertEqual([item["qq"] for item in result["matches"]], ["10001", "10002"])
            finally:
                if previous_cache_dir is None:
                    os.environ.pop("QQ_IDENTITY_CACHE_DIR", None)
                else:
                    os.environ["QQ_IDENTITY_CACHE_DIR"] = previous_cache_dir

    def test_b50_and_group_tools_resolve_target_without_agent_handoff(self) -> None:
        previous_cache_dir = os.environ.get("QQ_IDENTITY_CACHE_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["QQ_IDENTITY_CACHE_DIR"] = tmpdir
            try:
                cache = store.empty_cache()
                cache["fetchedAt"] = store.now_iso()
                store.upsert_group_member(
                    cache,
                    group_id="20001",
                    group_name="mai-group",
                    qq="10001",
                    nickname="qq-name",
                    card="group-card",
                )
                store.write_cache(cache)

                self.assertEqual(
                    b50_server.validate_lookup({"target": "group-card", "groupId": "20001"}),
                    {"qq": "10001"},
                )
                options = group_server.normalize_rank_options(
                    {"groupId": "20001", "target": "qq-name", "queryDelayMs": 0}
                )
                self.assertEqual(options["qq"], "10001")
                self.assertEqual(options["groupId"], "20001")

                inferred = group_server.normalize_rank_options(
                    {"target": "group-card", "queryDelayMs": 0}
                )
                self.assertEqual(inferred["qq"], "10001")
                self.assertEqual(inferred["groupId"], "20001")
            finally:
                if previous_cache_dir is None:
                    os.environ.pop("QQ_IDENTITY_CACHE_DIR", None)
                else:
                    os.environ["QQ_IDENTITY_CACHE_DIR"] = previous_cache_dir

    def test_group_rank_requires_group_choice_when_qq_is_in_multiple_groups(self) -> None:
        previous_cache_dir = os.environ.get("QQ_IDENTITY_CACHE_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["QQ_IDENTITY_CACHE_DIR"] = tmpdir
            try:
                cache = store.empty_cache()
                cache["fetchedAt"] = store.now_iso()
                store.upsert_group_member(cache, group_id="20001", group_name="g1", qq="10001", nickname="n", card="c1")
                store.upsert_group_member(cache, group_id="20002", group_name="g2", qq="10001", nickname="n", card="c2")
                store.write_cache(cache)

                with self.assertRaises(group_server.GroupB50Error) as raised:
                    group_server.normalize_rank_options({"qq": "10001", "queryDelayMs": 0})
                self.assertEqual(raised.exception.code, "AMBIGUOUS_GROUP")
            finally:
                if previous_cache_dir is None:
                    os.environ.pop("QQ_IDENTITY_CACHE_DIR", None)
                else:
                    os.environ["QQ_IDENTITY_CACHE_DIR"] = previous_cache_dir

    def test_group_rank_resolves_multiple_field_hits_for_same_qq(self) -> None:
        previous_cache_dir = os.environ.get("QQ_IDENTITY_CACHE_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["QQ_IDENTITY_CACHE_DIR"] = tmpdir
            try:
                cache = store.empty_cache()
                cache["fetchedAt"] = store.now_iso()
                store.upsert_group_member(cache, group_id="20001", group_name="g1", qq="10001", nickname="same", card="same")
                store.write_cache(cache)
                store.upsert_waterfish_profile("10001", nickname="same", username="same", rating=15000)

                options = group_server.normalize_rank_options({"target": "same", "queryDelayMs": 0})
                self.assertEqual(options["qq"], "10001")
                self.assertEqual(options["groupId"], "20001")
            finally:
                if previous_cache_dir is None:
                    os.environ.pop("QQ_IDENTITY_CACHE_DIR", None)
                else:
                    os.environ["QQ_IDENTITY_CACHE_DIR"] = previous_cache_dir

    def test_group_rank_does_not_choose_group_nickname_when_name_hits_another_user(self) -> None:
        previous_cache_dir = os.environ.get("QQ_IDENTITY_CACHE_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["QQ_IDENTITY_CACHE_DIR"] = tmpdir
            try:
                cache = store.empty_cache()
                cache["fetchedAt"] = store.now_iso()
                store.upsert_group_member(cache, group_id="20001", group_name="g1", qq="10001", nickname="n1", card="same")
                store.upsert_group_member(cache, group_id="20002", group_name="g2", qq="10002", nickname="same", card="c2")
                store.write_cache(cache)

                with self.assertRaises(group_server.GroupB50Error) as raised:
                    group_server.normalize_rank_options({"target": "same", "queryDelayMs": 0})
                self.assertEqual(raised.exception.code, "AMBIGUOUS_IDENTITY")
            finally:
                if previous_cache_dir is None:
                    os.environ.pop("QQ_IDENTITY_CACHE_DIR", None)
                else:
                    os.environ["QQ_IDENTITY_CACHE_DIR"] = previous_cache_dir

    def test_b50_target_falls_back_to_username_when_qq_identity_is_unknown(self) -> None:
        previous_cache_dir = os.environ.get("QQ_IDENTITY_CACHE_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["QQ_IDENTITY_CACHE_DIR"] = tmpdir
            try:
                store.write_cache(store.empty_cache())
                self.assertEqual(
                    b50_server.validate_lookup({"target": "waterfish-name"}),
                    {"username": "waterfish-name"},
                )
                self.assertEqual(
                    b50_server.validate_lookup({"target": "123456789"}),
                    {"username": "123456789"},
                )

                cache = store.empty_cache()
                cache["fetchedAt"] = store.now_iso()
                store.upsert_group_member(cache, group_id="20001", group_name="g", qq="123456789", nickname="n", card="")
                store.write_cache(cache)
                self.assertEqual(
                    b50_server.validate_lookup({"target": "123456789"}),
                    {"qq": "123456789"},
                )
            finally:
                if previous_cache_dir is None:
                    os.environ.pop("QQ_IDENTITY_CACHE_DIR", None)
                else:
                    os.environ["QQ_IDENTITY_CACHE_DIR"] = previous_cache_dir

    def test_status_text_displays_utc_times_in_local_timezone(self) -> None:
        previous_cache_dir = os.environ.get("QQ_IDENTITY_CACHE_DIR")
        previous_tz = os.environ.get("QQ_IDENTITY_DISPLAY_TZ")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["QQ_IDENTITY_CACHE_DIR"] = tmpdir
            os.environ["QQ_IDENTITY_DISPLAY_TZ"] = "Asia/Shanghai"
            try:
                cache = store.empty_cache()
                cache["fetchedAt"] = "2026-05-27T16:58:31.395064+00:00"
                store.write_cache(cache)
                server.write_job_status(
                    {
                        "status": "completed",
                        "startedAt": "2026-05-27T16:58:29.420017+00:00",
                        "finishedAt": "2026-05-27T16:58:31.395064+00:00",
                        "refreshReason": "forceRefresh",
                        "message": "QQ 身份缓存刷新完成。",
                        "stats": {"friendCount": 3, "groupCount": 4, "uniqueUsers": 53},
                    }
                )

                status_text = server.format_cache_status_text(server.cache_status())
                self.assertIn("生成时间: 2026-05-28 00:58:31 +08:00", status_text)

                job_text = server.format_job_status_text(server.read_job_status())
                self.assertIn("启动时间: 2026-05-28 00:58:29 +08:00", job_text)
                self.assertIn("完成时间: 2026-05-28 00:58:31 +08:00", job_text)
            finally:
                if previous_cache_dir is None:
                    os.environ.pop("QQ_IDENTITY_CACHE_DIR", None)
                else:
                    os.environ["QQ_IDENTITY_CACHE_DIR"] = previous_cache_dir
                if previous_tz is None:
                    os.environ.pop("QQ_IDENTITY_DISPLAY_TZ", None)
                else:
                    os.environ["QQ_IDENTITY_DISPLAY_TZ"] = previous_tz

    def test_fresh_refresh_call_reports_recent_completed_job(self) -> None:
        previous_cache_dir = os.environ.get("QQ_IDENTITY_CACHE_DIR")
        previous_tz = os.environ.get("QQ_IDENTITY_DISPLAY_TZ")
        with tempfile.TemporaryDirectory() as tmpdir:
            os.environ["QQ_IDENTITY_CACHE_DIR"] = tmpdir
            os.environ["QQ_IDENTITY_DISPLAY_TZ"] = "Asia/Shanghai"
            try:
                cache = store.empty_cache()
                cache["fetchedAt"] = store.now_iso()
                store.write_cache(cache)
                server.write_job_status(
                    {
                        "status": "completed",
                        "startedAt": "2026-05-27T16:58:29.420017+00:00",
                        "finishedAt": "2026-05-27T16:58:31.395064+00:00",
                        "refreshReason": "forceRefresh",
                        "message": "QQ 身份缓存刷新完成。",
                        "stats": {"friendCount": 3, "groupCount": 4, "uniqueUsers": 53},
                    }
                )

                result = server.refresh_qq_identity_cache({"forceRefresh": False})
                self.assertFalse(result["started"])
                self.assertEqual(result["job"]["status"], "completed")
                self.assertIn("最近一次 QQ 身份缓存刷新已完成", result["text"])
                self.assertIn("最近刷新任务:", result["text"])
                self.assertIn("完成时间: 2026-05-28 00:58:31 +08:00", result["text"])
            finally:
                if previous_cache_dir is None:
                    os.environ.pop("QQ_IDENTITY_CACHE_DIR", None)
                else:
                    os.environ["QQ_IDENTITY_CACHE_DIR"] = previous_cache_dir
                if previous_tz is None:
                    os.environ.pop("QQ_IDENTITY_DISPLAY_TZ", None)
                else:
                    os.environ["QQ_IDENTITY_DISPLAY_TZ"] = previous_tz


if __name__ == "__main__":
    unittest.main()
