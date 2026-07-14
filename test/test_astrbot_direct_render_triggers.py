from __future__ import annotations

import asyncio
import json
import re
import sys
import tempfile
import types
import unittest
from pathlib import Path
from typing import Any
from unittest.mock import patch

from deploy.astrbot.plugins.astrbot_plugin_maimai_auto_send_images.direct_render import (
    COMMAND_HELP_TEXT,
    DirectCommand,
    TargetContext,
    format_search_candidates,
    handle_direct_command,
    parse_direct_render_command,
)


class Plain:
    type = "Plain"

    def __init__(self, text: str):
        self.text = text


class At:
    type = "At"

    def __init__(self, qq: str):
        self.qq = qq


def install_astrbot_stubs() -> None:
    if "mcp.types" not in sys.modules:
        mcp_module = types.ModuleType("mcp")
        mcp_types = types.ModuleType("mcp.types")
        mcp_types.CallToolResult = type("CallToolResult", (), {})
        mcp_types.TextContent = type("TextContent", (), {})
        sys.modules.setdefault("mcp", mcp_module)
        sys.modules["mcp.types"] = mcp_types

    astrbot_module = sys.modules.setdefault("astrbot", types.ModuleType("astrbot"))
    api_module = sys.modules.setdefault("astrbot.api", types.ModuleType("astrbot.api"))
    api_module.AstrBotConfig = dict
    event_module = sys.modules.setdefault("astrbot.api.event", types.ModuleType("astrbot.api.event"))
    event_module.AstrMessageEvent = type("AstrMessageEvent", (), {})

    filter_module = types.ModuleType("filter")

    class EventMessageType:
        ALL = "ALL"

    def decorator_factory(*args: Any, **kwargs: Any):
        del args, kwargs

        def decorator(func: Any) -> Any:
            return func

        return decorator

    filter_module.EventMessageType = EventMessageType
    filter_module.event_message_type = decorator_factory
    filter_module.on_using_llm_tool = decorator_factory
    filter_module.on_llm_tool_respond = decorator_factory
    filter_module.on_decorating_result = decorator_factory
    event_module.filter = filter_module

    star_module = sys.modules.setdefault("astrbot.api.star", types.ModuleType("astrbot.api.star"))

    class Star:
        def __init__(self, context: Any):
            self.context = context

    def register(*args: Any, **kwargs: Any):
        del args, kwargs

        def decorator(cls: Any) -> Any:
            return cls

        return decorator

    star_module.Context = type("Context", (), {})
    star_module.Star = Star
    star_module.register = register

    components_module = sys.modules.setdefault(
        "astrbot.api.message_components",
        types.ModuleType("astrbot.api.message_components"),
    )
    components_module.Plain = Plain
    components_module.At = At

    core_module = sys.modules.setdefault("astrbot.core", types.ModuleType("astrbot.core"))
    agent_module = sys.modules.setdefault("astrbot.core.agent", types.ModuleType("astrbot.core.agent"))
    tool_module = sys.modules.setdefault("astrbot.core.agent.tool", types.ModuleType("astrbot.core.agent.tool"))
    tool_module.FunctionTool = type("FunctionTool", (), {})
    run_context_module = sys.modules.setdefault(
        "astrbot.core.agent.run_context",
        types.ModuleType("astrbot.core.agent.run_context"),
    )

    class ContextWrapper:
        def __init__(self, context: Any, tool_call_timeout: int = 120):
            self.context = context
            self.tool_call_timeout = tool_call_timeout

    run_context_module.ContextWrapper = ContextWrapper
    del astrbot_module, core_module, agent_module


install_astrbot_stubs()

from deploy.astrbot.plugins.astrbot_plugin_maimai_auto_send_images.main import (  # noqa: E402
    AstrBotToolMcpClient,
    MaimaiAutoSendImagesPlugin,
    _is_ambiguous_napcat_send_timeout,
)


SENDER_QQ = "111111"
SELF_QQ = "999999"
GROUP_ID = "23456"


def context(*mentions: str) -> TargetContext:
    return TargetContext(sender_qq=SENDER_QQ, mention_qqs=mentions, self_qq=SELF_QQ)


def group_context(*mentions: str, private: bool = False) -> TargetContext:
    return TargetContext(
        sender_qq=SENDER_QQ,
        mention_qqs=mentions,
        self_qq=SELF_QQ,
        group_id="" if private else GROUP_ID,
        is_private=private,
    )


def parse(text: str, *mentions: str) -> DirectCommand:
    for mention in mentions:
        if mention.isdigit():
            text = re.sub(rf"@{re.escape(mention)}", f"[CQ:at,qq={mention}]", text)
    command = parse_direct_render_command(text, context(*mentions))
    assert command is not None, text
    return command


def parse_group(text: str, *mentions: str) -> DirectCommand:
    for mention in mentions:
        if mention.isdigit():
            text = re.sub(rf"@{re.escape(mention)}", f"[CQ:at,qq={mention}]", text)
    command = parse_direct_render_command(text, group_context(*mentions))
    assert command is not None, text
    return command


def parse_group_error(text: str, *mentions: str) -> DirectCommand:
    command = parse_group(text, *mentions)
    assert command.tool_name == "direct_render_syntax_error", text
    return command


class FakeMcpClient:
    def __init__(self, responses: dict[tuple[str, str], dict[str, Any]]):
        self.responses = responses
        self.calls: list[tuple[str, str, dict[str, Any]]] = []

    async def call_tool(self, server: str, tool_name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        self.calls.append((server, tool_name, arguments))
        return self.responses[(server, tool_name)]


class FakeAstrBotTool:
    def __init__(self, result: Any):
        self.result = result
        self.calls: list[tuple[int, dict[str, Any]]] = []

    async def call(self, context: Any, **kwargs: Any) -> Any:
        self.calls.append((context.tool_call_timeout, kwargs))
        return self.result


class FakeToolManager:
    def __init__(self, tools: dict[str, Any]):
        self.tools = tools

    def get_func(self, name: str) -> Any:
        return self.tools.get(name)


class FakeAstrBotContext:
    def __init__(self, manager: FakeToolManager):
        self.manager = manager

    def get_llm_tool_manager(self) -> FakeToolManager:
        return self.manager


class DummyEvent:
    def __init__(
        self,
        chain: list[Any],
        *,
        message_str: str = "",
        is_at_or_wake: bool = True,
        is_wake_up: bool = False,
        private: bool = False,
        group_id: str = "",
        unified_msg_origin: str = "",
        role: str = "",
    ):
        self._chain = chain
        self._message_str = message_str
        self._is_at_or_wake = is_at_or_wake
        self._is_wake_up = is_wake_up
        self._private = private
        self._group_id = group_id
        self.unified_msg_origin = unified_msg_origin
        self.role = role

    def get_messages(self) -> list[Any]:
        return self._chain

    def get_message_str(self) -> str:
        return self._message_str

    def get_self_id(self) -> str:
        return SELF_QQ

    def get_sender_id(self) -> str:
        return SENDER_QQ

    def is_at_or_wake_command(self) -> bool:
        return self._is_at_or_wake

    def is_wake_up(self) -> bool:
        return self._is_wake_up

    def is_private_chat(self) -> bool:
        return self._private

    def get_group_id(self) -> str:
        return self._group_id


class DirectRenderParserTest(unittest.TestCase):
    def test_b50_target_syntax(self) -> None:
        self.assertEqual(parse("b50").arguments, {"qq": SENDER_QQ})
        self.assertEqual(parse("b50 123456").arguments, {"username": "123456"})
        self.assertEqual(parse("b50123456").arguments, {"username": "123456"})
        self.assertEqual(parse("123456 b50").arguments, {"qq": "123456"})
        self.assertEqual(parse("b50 alice").arguments, {"username": "alice"})
        self.assertEqual(parse("B50alice").arguments, {"username": "alice"})
        self.assertEqual(parse("b50 示例玩家").arguments, {"username": "示例玩家"})
        self.assertEqual(parse("b50x").arguments, {"username": "x"})
        self.assertEqual(parse("b50 @222222", "222222").arguments, {"qq": "222222"})
        self.assertEqual(parse("b50@222222", "222222").arguments, {"qq": "222222"})
        self.assertEqual(parse("@222222 b50", "222222").arguments, {"qq": "222222"})
        self.assertEqual(parse("@222222b50", "222222").arguments, {"qq": "222222"})
        self.assertIsNone(parse_direct_render_command("b50 @999999", context(SELF_QQ)))
        self.assertIsNone(parse_direct_render_command("b50 @all", context("all")))
        self.assertIsNone(parse_direct_render_command("b50图片", context("222222")))
        self.assertIsNone(parse_direct_render_command("alice b50", context()))
        self.assertIsNone(parse_direct_render_command("自算b50", context("222222")))
        self.assertIsNone(parse_direct_render_command("计算b50", context("222222")))

    def test_plain_context_mention_is_not_a_player_target(self) -> None:
        self.assertEqual(parse("b50", SELF_QQ).arguments, {"qq": SENDER_QQ})
        self.assertEqual(parse("b50", "222222").arguments, {"qq": SENDER_QQ})
        self.assertEqual(parse("b50 alice", "222222").arguments, {"username": "alice"})
        self.assertEqual(parse("b50 123456", SELF_QQ, "222222").arguments, {"username": "123456"})

    def test_today_maimai_direct_syntax(self) -> None:
        default = parse("今日舞萌")
        self.assertEqual(default.server, "search")
        self.assertEqual(default.tool_name, "today_maimai")
        self.assertEqual(default.arguments, {"qq": SENDER_QQ})

        self.assertEqual(parse("今日mai").arguments, {"qq": SENDER_QQ})
        self.assertEqual(parse("今日运势").arguments, {"qq": SENDER_QQ})
        self.assertEqual(parse("@222222 今日舞萌", "222222").arguments, {"qq": "222222"})
        self.assertEqual(parse("123456 今日舞萌").arguments, {"qq": "123456"})
        self.assertEqual(parse("今日舞萌 123456").arguments, {"qq": "123456"})
        multiple_targets = parse("@222222 @333333 今日舞萌", "222222", "333333")
        self.assertEqual(multiple_targets.tool_name, "direct_render_syntax_error")
        self.assertIn("今日舞萌一次只能指定一个 QQ", multiple_targets.error_text)

    def test_maimai_update_workflow_direct_syntax(self) -> None:
        bind = parse("mai bind import-token-abc")
        self.assertEqual(bind.server, "upload")
        self.assertEqual(bind.tool_name, "maimai_bind_import_token")
        self.assertEqual(bind.arguments, {"qq": SENDER_QQ, "importToken": "import-token-abc"})

        update = parse("mai update SGWCSDGB2606171200000123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef --keyship A63E01D90630000 --logoutid 1 --title-ver 1.55.00")
        self.assertEqual(update.server, "upload")
        self.assertEqual(update.tool_name, "maimai_update_records")
        self.assertEqual(
            update.arguments,
            {
                "qq": SENDER_QQ,
                "qrContent": "SGWCSDGB2606171200000123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "keyship": "A63E01D90630000",
                "logoutid": 1,
                "titleVer": "1.55.00",
            },
        )

        bad_logoutid = parse("mai update SGWCSDGB2606171200000123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef --logoutid 3")
        self.assertEqual(bad_logoutid.tool_name, "direct_render_syntax_error")
        self.assertIn("--logoutid", bad_logoutid.error_text)

    def test_minfo_target_syntax(self) -> None:
        self.assertEqual(
            parse("minfo 系ぎて").arguments,
            {"qq": SENDER_QQ, "query": "系ぎて"},
        )
        self.assertEqual(
            parse("minfo系ぎて").arguments,
            {"qq": SENDER_QQ, "query": "系ぎて"},
        )
        self.assertEqual(
            parse("info 123456 系ぎて").arguments,
            {"username": "系ぎて", "query": "123456"},
        )
        self.assertEqual(
            parse("info123456 系ぎて").arguments,
            {"username": "系ぎて", "query": "123456"},
        )
        self.assertEqual(
            parse("123456 minfo 系ぎて").arguments,
            {"qq": "123456", "query": "系ぎて"},
        )
        self.assertEqual(
            parse("minfo @222222 系ぎて", "222222").arguments,
            {"qq": "222222", "query": "系ぎて"},
        )
        self.assertEqual(
            parse("minfo@222222系ぎて", "222222").arguments,
            {"qq": "222222", "query": "系ぎて"},
        )
        self.assertEqual(
            parse("@222222 minfo 系ぎて", "222222").arguments,
            {"qq": "222222", "query": "系ぎて"},
        )
        self.assertEqual(
            parse("@222222minfo系ぎて", "222222").arguments,
            {"qq": "222222", "query": "系ぎて"},
        )
        self.assertEqual(
            parse("minfo 系ぎて @222222", "222222").arguments,
            {"qq": "222222", "query": "系ぎて"},
        )
        self.assertEqual(
            parse("minfo @222222 系ぎて").arguments,
            {"qq": SENDER_QQ, "query": "@222222 系ぎて"},
        )
        self.assertEqual(
            parse("minfo 系ぎて alice").arguments,
            {"username": "alice", "query": "系ぎて"},
        )
        self.assertEqual(
            parse("minfo 系ぎて 示例玩家").arguments,
            {"username": "示例玩家", "query": "系ぎて"},
        )
        self.assertEqual(
            parse("minfo 系ぎて x").arguments,
            {"username": "x", "query": "系ぎて"},
        )

    def test_ginfo_and_song_info_lookup(self) -> None:
        ginfo = parse("ginfo 紫 系ぎて")
        self.assertEqual(ginfo.tool_name, "render_maimai_music_global_stats")
        self.assertEqual(ginfo.arguments, {"query": "系ぎて", "difficulty": "紫"})

        ginfo_attached_diff = parse("ginfo紫 系ぎて")
        self.assertEqual(ginfo_attached_diff.tool_name, "render_maimai_music_global_stats")
        self.assertEqual(ginfo_attached_diff.arguments, {"query": "系ぎて", "difficulty": "紫"})

        ginfo_attached_query = parse("ginfo系ぎて")
        self.assertEqual(ginfo_attached_query.tool_name, "render_maimai_music_global_stats")
        self.assertEqual(ginfo_attached_query.arguments, {"query": "系ぎて"})

        song = parse("系ぎて是什么歌")
        self.assertTrue(song.needs_search)
        self.assertEqual(song.search_arguments, {"query": "系ぎて", "limit": 5, "format": "json"})
        self.assertEqual(song.render_tool_name, "render_maimai_music_info")

        by_id = parse("id296")
        self.assertTrue(by_id.needs_search)
        self.assertEqual(by_id.search_arguments, {"id": "296", "limit": 5, "format": "json"})

        by_spaced_id = parse("id 296")
        self.assertTrue(by_spaced_id.needs_search)
        self.assertEqual(by_spaced_id.search_arguments, {"id": "296", "limit": 5, "format": "json"})
        self.assertEqual(by_spaced_id.render_arguments, {"qq": SENDER_QQ})

        by_id_target = parse("id296 @222222", "222222")
        self.assertTrue(by_id_target.needs_search)
        self.assertEqual(by_id_target.render_arguments, {"qq": "222222"})

        by_id_target_attached = parse("id296@222222", "222222")
        self.assertTrue(by_id_target_attached.needs_search)
        self.assertEqual(by_id_target_attached.render_arguments, {"qq": "222222"})

        by_id_prefix_target = parse("@222222 id296", "222222")
        self.assertTrue(by_id_prefix_target.needs_search)
        self.assertEqual(by_id_prefix_target.render_arguments, {"qq": "222222"})

    def test_text_search_filter_lookup(self) -> None:
        artist = parse("曲师查歌かめりあ")
        self.assertEqual(artist.server, "search")
        self.assertEqual(artist.tool_name, "search_maimai_songs")
        self.assertEqual(artist.arguments, {"artist": "かめりあ", "limit": 20, "format": "compact"})

        charter = parse("谱师查歌 Jack")
        self.assertEqual(charter.server, "search")
        self.assertEqual(charter.arguments, {"charter": "Jack", "limit": 20, "format": "compact"})

        bpm = parse("bpm查歌180～200")
        self.assertEqual(bpm.server, "search")
        self.assertEqual(bpm.arguments, {"bpm": "180-200", "limit": 20, "format": "compact"})

        search_alias = parse("search artist sasakure")
        self.assertEqual(search_alias.server, "search")
        self.assertEqual(search_alias.arguments, {"artist": "sasakure", "limit": 20, "format": "compact"})

        self.assertIsNone(parse_direct_render_command("search 系ぎて", context()))
        self.assertIn("需要曲师名", parse("曲师查歌").error_text)

    def test_random_music_info_lookup(self) -> None:
        level = parse("随个13+")
        self.assertTrue(level.needs_search)
        self.assertEqual(level.search_tool_name, "random_maimai_songs")
        self.assertEqual(level.search_arguments, {"count": 1, "format": "json", "level": "13+"})
        self.assertEqual(level.render_tool_name, "render_maimai_music_info")
        self.assertEqual(level.render_arguments, {"qq": SENDER_QQ})

        dx_master = parse("随个dx紫13")
        self.assertEqual(
            dx_master.search_arguments,
            {"count": 1, "format": "json", "song_type": "dx", "difficulty": "紫", "level": "13"},
        )
        self.assertEqual(dx_master.render_arguments, {"qq": SENDER_QQ, "songType": "dx"})

        standard_basic = parse("随个 标准 绿 13 @222222", "222222")
        self.assertEqual(
            standard_basic.search_arguments,
            {"count": 1, "format": "json", "song_type": "standard", "difficulty": "绿", "level": "13"},
        )
        self.assertEqual(standard_basic.render_arguments, {"qq": "222222", "songType": "standard"})

        ds = parse("随个紫13.4")
        self.assertEqual(ds.search_arguments, {"count": 1, "format": "json", "difficulty": "紫", "ds": "13.4"})

        any_song = parse("随个歌")
        self.assertEqual(any_song.search_arguments, {"count": 1, "format": "json"})
        spaced_any_song = parse("随个 歌")
        self.assertEqual(spaced_any_song.search_arguments, {"count": 1, "format": "json"})
        self.assertEqual(spaced_any_song.render_arguments, {"qq": SENDER_QQ})

        self.assertIsNone(parse_direct_render_command("随个不存在条件", context()))

    def test_tables_progress_lists_and_rise_score(self) -> None:
        self.assertEqual(
            parse("13+定数表").arguments,
            {"qq": SENDER_QQ, "rating": "13+"},
        )
        self.assertEqual(
            parse("13+定数表 @222222", "222222").arguments,
            {"qq": "222222", "rating": "13+"},
        )
        self.assertEqual(
            parse("13+定数表@222222", "222222").arguments,
            {"qq": "222222", "rating": "13+"},
        )
        self.assertEqual(
            parse("@222222 13+定数表", "222222").arguments,
            {"qq": "222222", "rating": "13+"},
        )
        self.assertEqual(
            parse("13+定数表 alice").arguments,
            {"username": "alice", "rating": "13+"},
        )
        self.assertEqual(
            parse("13+定数表alice").arguments,
            {"username": "alice", "rating": "13+"},
        )
        self.assertEqual(
            parse("13+定数表 123456").arguments,
            {"username": "123456", "rating": "13+"},
        )
        self.assertEqual(
            parse("13+完成表123456").arguments,
            {"username": "123456", "rating": "13+"},
        )
        self.assertEqual(
            parse("13+定数表 示例玩家").arguments,
            {"username": "示例玩家", "rating": "13+"},
        )

        plate = parse("桃极完成表 alice")
        self.assertEqual(plate.tool_name, "render_maimai_plate")
        self.assertEqual(
            plate.arguments,
            {"username": "alice", "version": "桃", "plan": "极"},
        )
        attached_plate = parse("桃极完成表alice")
        self.assertEqual(attached_plate.tool_name, "render_maimai_plate")
        self.assertEqual(
            attached_plate.arguments,
            {"username": "alice", "version": "桃", "plan": "极"},
        )

        batch = parse("桃极完成表 熊将完成表")
        self.assertEqual(batch.tool_name, "render_maimai_plate_batch")
        self.assertEqual(
            batch.arguments,
            {
                "qq": SENDER_QQ,
                "items": [
                    {"version": "桃", "plan": "极"},
                    {"version": "熊", "plan": "将"},
                ],
            },
        )

        typo_batch = parse("桃将完成表 星神完成表 真级完成表")
        self.assertEqual(typo_batch.tool_name, "render_maimai_plate_batch")
        self.assertEqual(
            typo_batch.arguments,
            {
                "qq": SENDER_QQ,
                "items": [
                    {"version": "桃", "plan": "将"},
                    {"version": "星", "plan": "神"},
                    {"version": "真", "plan": "极"},
                ],
            },
        )

        self.assertEqual(
            parse("桃极进度").arguments,
            {"qq": SENDER_QQ, "version": "桃", "plan": "极"},
        )
        self.assertEqual(
            parse("12+sss进度").arguments,
            {"qq": SENDER_QQ, "level": "12+", "plan": "sss"},
        )
        self.assertEqual(
            parse("12+sss完成表").arguments,
            {"qq": SENDER_QQ, "level": "12+", "plan": "sss"},
        )
        self.assertEqual(
            parse("13+sss未完成进度 2").arguments,
            {"qq": SENDER_QQ, "level": "13+", "plan": "sss", "category": "unfinished", "page": 2},
        )
        self.assertEqual(
            parse("13+sss未完成表 2").arguments,
            {"qq": SENDER_QQ, "level": "13+", "plan": "sss", "category": "unfinished", "page": 2},
        )
        self.assertEqual(
            parse("12+sss进度 20").arguments,
            {"qq": SENDER_QQ, "level": "12+", "plan": "sss", "page": 20},
        )
        self.assertEqual(
            parse("13+分数列表").arguments,
            {"qq": SENDER_QQ, "level": "13+"},
        )
        self.assertEqual(
            parse("13+分数列表 @222222", "222222").arguments,
            {"qq": "222222", "level": "13+"},
        )
        self.assertEqual(
            parse("@22222213+分数列表", "222222").arguments,
            {"qq": "222222", "level": "13+"},
        )
        self.assertEqual(
            parse("14.2分数列表 2").arguments,
            {"qq": SENDER_QQ, "ds": "14.2", "page": 2},
        )
        self.assertEqual(
            parse("14.2分数列表 20").arguments,
            {"qq": SENDER_QQ, "ds": "14.2", "page": 20},
        )
        self.assertEqual(
            parse("我要在13+加5分 alice").arguments,
            {"username": "alice", "level": "13+", "score": 5},
        )
        self.assertEqual(
            parse("我要上分").arguments,
            {"qq": SENDER_QQ},
        )
        self.assertEqual(
            parse("我要上积分").arguments,
            {"qq": SENDER_QQ},
        )
        self.assertEqual(
            parse("我要上5分").arguments,
            {"qq": SENDER_QQ, "score": 5},
        )
        self.assertEqual(
            parse("我要在13上分").arguments,
            {"qq": SENDER_QQ, "level": "13"},
        )
        self.assertEqual(
            parse("我要在13+上分").arguments,
            {"qq": SENDER_QQ, "level": "13+"},
        )
        self.assertEqual(
            parse("我要在13家上分").arguments,
            {"qq": SENDER_QQ, "level": "13+"},
        )
        self.assertEqual(
            parse("我要在13家上5分").arguments,
            {"qq": SENDER_QQ, "level": "13+", "score": 5},
        )
        self.assertEqual(
            parse("我要上分 alice").arguments,
            {"username": "alice"},
        )
        self.assertEqual(
            parse("我要上5分 alice").arguments,
            {"username": "alice", "score": 5},
        )
        self.assertEqual(
            parse("13+完成表 alice").arguments,
            {"username": "alice", "rating": "13+"},
        )
        self.assertEqual(
            parse("桃极完成表 熊将完成表 alice").arguments,
            {
                "username": "alice",
                "items": [
                    {"version": "桃", "plan": "极"},
                    {"version": "熊", "plan": "将"},
                ],
            },
        )
        self.assertEqual(
            parse("桃极完成表 熊将完成表 示例玩家").arguments,
            {
                "username": "示例玩家",
                "items": [
                    {"version": "桃", "plan": "极"},
                    {"version": "熊", "plan": "将"},
                ],
            },
        )
        self.assertEqual(
            parse("桃极进度 alice").arguments,
            {"username": "alice", "version": "桃", "plan": "极"},
        )
        self.assertEqual(
            parse("桃极进度alice").arguments,
            {"username": "alice", "version": "桃", "plan": "极"},
        )
        self.assertEqual(
            parse("12+sss进度 alice").arguments,
            {"username": "alice", "level": "12+", "plan": "sss"},
        )
        self.assertEqual(
            parse("12+sss进度alice").arguments,
            {"username": "alice", "level": "12+", "plan": "sss"},
        )
        self.assertEqual(
            parse("12+sss进度2").arguments,
            {"qq": SENDER_QQ, "level": "12+", "plan": "sss", "page": 2},
        )
        self.assertEqual(
            parse("12+sss完成表 alice").arguments,
            {"username": "alice", "level": "12+", "plan": "sss"},
        )
        self.assertEqual(
            parse("12+sss完成表alice").arguments,
            {"username": "alice", "level": "12+", "plan": "sss"},
        )
        self.assertEqual(
            parse("13+sss未完成进度 2 alice").arguments,
            {"username": "alice", "level": "13+", "plan": "sss", "category": "unfinished", "page": 2},
        )
        self.assertEqual(
            parse("13+sss未完成进度2").arguments,
            {"qq": SENDER_QQ, "level": "13+", "plan": "sss", "category": "unfinished", "page": 2},
        )
        self.assertEqual(
            parse("13+sss未完成表 2 alice").arguments,
            {"username": "alice", "level": "13+", "plan": "sss", "category": "unfinished", "page": 2},
        )
        self.assertEqual(
            parse("13+sss未完成表alice").arguments,
            {"username": "alice", "level": "13+", "plan": "sss", "category": "unfinished"},
        )
        self.assertEqual(
            parse("13+sss未完成进度 21").arguments,
            {"username": "21", "level": "13+", "plan": "sss", "category": "unfinished"},
        )
        self.assertEqual(
            parse("13+sss完成表 21").arguments,
            {"username": "21", "level": "13+", "plan": "sss"},
        )
        self.assertEqual(
            parse("13+sss未完成进度 2 1").arguments,
            {"username": "1", "level": "13+", "plan": "sss", "category": "unfinished", "page": 2},
        )
        self.assertEqual(
            parse("13+sss完成表 2 1").arguments,
            {"username": "1", "level": "13+", "plan": "sss", "page": 2},
        )
        self.assertEqual(
            parse("13+分数列表 alice").arguments,
            {"username": "alice", "level": "13+"},
        )
        self.assertEqual(
            parse("13+分数列表alice").arguments,
            {"username": "alice", "level": "13+"},
        )
        self.assertEqual(
            parse("13+分数列表2").arguments,
            {"qq": SENDER_QQ, "level": "13+", "page": 2},
        )
        self.assertEqual(
            parse("13+分数列表 123456").arguments,
            {"username": "123456", "level": "13+"},
        )
        self.assertEqual(
            parse("13+分数列表21").arguments,
            {"username": "21", "level": "13+"},
        )
        self.assertEqual(
            parse("14.2分数列表 2 alice").arguments,
            {"username": "alice", "ds": "14.2", "page": 2},
        )
        self.assertEqual(
            parse("14.2分数列表 2 示例玩家").arguments,
            {"username": "示例玩家", "ds": "14.2", "page": 2},
        )
        self.assertEqual(
            parse("14.2分数列表 21").arguments,
            {"username": "21", "ds": "14.2"},
        )
        self.assertEqual(
            parse("14.2分数列表21").arguments,
            {"username": "21", "ds": "14.2"},
        )
        self.assertEqual(
            parse("14.2分数列表 2 1").arguments,
            {"username": "1", "ds": "14.2", "page": 2},
        )
        self.assertIsNone(parse_direct_render_command("查我14s进度", context()))

    def test_excluded_triggers_are_not_direct(self) -> None:
        for text in ("b50图", "查歌 系ぎて", "系ぎて信息图", "系ぎて成绩图", "系ぎて全服统计", "桃极"):
            self.assertIsNone(parse_direct_render_command(text, context()))

    def test_group_rank_direct_syntax(self) -> None:
        command = parse_group("rank")
        self.assertEqual(command.server, "group")
        self.assertEqual(command.tool_name, "group_b50_member_rank")
        self.assertEqual(
            command.arguments,
            {"groupId": GROUP_ID, "qq": SENDER_QQ, "outputMode": "rating", "contextSize": 3},
        )

        prefix_qq = parse_group("1000000001 rank")
        self.assertEqual(prefix_qq.tool_name, "group_b50_member_rank")
        self.assertEqual(prefix_qq.arguments["qq"], "1000000001")

        mention = parse_group("ra@222222nk", "222222")
        self.assertEqual(mention.tool_name, "group_b50_member_rank")
        self.assertEqual(mention.arguments["qq"], "222222")

        report = parse_group("rank 10")
        self.assertEqual(report.tool_name, "group_b50_report")
        self.assertEqual(
            report.arguments,
            {"groupId": GROUP_ID, "sortOrder": "desc", "outputMode": "rating", "outputLimit": 10},
        )

        attached_report = parse_group("rank10")
        self.assertEqual(attached_report.tool_name, "group_b50_report")
        self.assertEqual(attached_report.arguments["outputLimit"], 10)

        range_report = parse_group("rank31-60")
        self.assertEqual(range_report.tool_name, "group_b50_report")
        self.assertEqual(
            range_report.arguments,
            {"groupId": GROUP_ID, "sortOrder": "desc", "outputMode": "rating", "startRank": 31, "endRank": 60},
        )

        reverse_report = parse_group("rank 倒序 20")
        self.assertEqual(reverse_report.tool_name, "group_b50_report")
        self.assertEqual(reverse_report.arguments["sortOrder"], "asc")
        self.assertEqual(reverse_report.arguments["outputLimit"], 20)

        attached_reverse_report = parse_group("rank倒序20")
        self.assertEqual(attached_reverse_report.tool_name, "group_b50_report")
        self.assertEqual(attached_reverse_report.arguments["sortOrder"], "asc")
        self.assertEqual(attached_reverse_report.arguments["outputLimit"], 20)

        attached_reverse_range = parse_group("rank倒序31-60")
        self.assertEqual(attached_reverse_range.tool_name, "group_b50_report")
        self.assertEqual(attached_reverse_range.arguments["sortOrder"], "asc")
        self.assertEqual(attached_reverse_range.arguments["startRank"], 31)
        self.assertEqual(attached_reverse_range.arguments["endRank"], 60)

        suffix_qq = parse_group_error("rank 1000000001")
        self.assertIn("rank 只支持", suffix_qq.error_text)

        target_with_limit = parse_group_error("1000000001 rank 10")
        self.assertIn("带 N 时不能同时指定玩家目标", target_with_limit.error_text)
        self.assertNotIn("`", target_with_limit.error_text)

        multiple_targets = parse_group_error("@222222 @333333 rank", "222222", "333333")
        self.assertIn("只能指定一个玩家目标", multiple_targets.error_text)

        no_group = parse_direct_render_command("rank", context())
        self.assertIsNotNone(no_group)
        self.assertEqual(no_group.tool_name, "direct_render_syntax_error")
        self.assertIn("无法识别群号", no_group.error_text)

    def test_group_musicrank_direct_syntax(self) -> None:
        command = parse_group("musicrank id11451")
        self.assertEqual(command.server, "group")
        self.assertEqual(command.tool_name, "group_song_score_member_rank")
        self.assertEqual(
            command.arguments,
            {"groupId": GROUP_ID, "musicId": 11451, "qq": SENDER_QQ, "contextSize": 3},
        )

        spaced_id = parse_group("musicrank id 11451")
        self.assertEqual(spaced_id.arguments["musicId"], 11451)
        self.assertNotIn("levelIndex", spaced_id.arguments)

        alias = parse_group("musicrank 系ぎて")
        self.assertEqual(alias.tool_name, "group_song_score_member_rank")
        self.assertEqual(alias.arguments["songQuery"], "系ぎて")
        self.assertEqual(alias.arguments["qq"], SENDER_QQ)
        self.assertNotIn("levelIndex", alias.arguments)

        attached_alias = parse_group("musicrank系ぎて")
        self.assertEqual(attached_alias.tool_name, "group_song_score_member_rank")
        self.assertEqual(attached_alias.arguments["songQuery"], "系ぎて")
        self.assertNotIn("levelIndex", attached_alias.arguments)

        attached_difficulty = parse_group("musicrank紫白系")
        self.assertEqual(attached_difficulty.tool_name, "group_song_score_member_rank")
        self.assertEqual(attached_difficulty.arguments["songQuery"], "白系")
        self.assertEqual(attached_difficulty.arguments["levelIndex"], 3)

        spaced_difficulty_token = parse_group("musicrank 白 白系")
        self.assertEqual(spaced_difficulty_token.arguments["songQuery"], "白系")
        self.assertEqual(spaced_difficulty_token.arguments["levelIndex"], 4)

        spaced_compact_difficulty = parse_group("musicrank 紫白系")
        self.assertEqual(spaced_compact_difficulty.arguments["songQuery"], "白系")
        self.assertEqual(spaced_compact_difficulty.arguments["levelIndex"], 3)

        spaced_white_prefix = parse_group("musicrank 白系")
        self.assertEqual(spaced_white_prefix.arguments["songQuery"], "系")
        self.assertEqual(spaced_white_prefix.arguments["levelIndex"], 4)

        attached_song_with_number = parse_group("musicrank系ぎて10")
        self.assertEqual(attached_song_with_number.tool_name, "group_song_score_member_rank")
        self.assertEqual(attached_song_with_number.arguments["songQuery"], "系ぎて10")
        self.assertNotIn("levelIndex", attached_song_with_number.arguments)

        middle_at = parse_group("musicrank 系@222222ぎて", "222222")
        self.assertEqual(middle_at.tool_name, "group_song_score_member_rank")
        self.assertEqual(middle_at.arguments["songQuery"], "系ぎて")
        self.assertEqual(middle_at.arguments["qq"], "222222")
        self.assertNotIn("levelIndex", middle_at.arguments)

        prefix_qq = parse_group("1000000001 musicrank 系ぎて")
        self.assertEqual(prefix_qq.arguments["qq"], "1000000001")
        self.assertEqual(prefix_qq.arguments["songQuery"], "系ぎて")
        self.assertNotIn("levelIndex", prefix_qq.arguments)

        report = parse_group("musicrank 系ぎて 10")
        self.assertEqual(report.tool_name, "group_song_score_report")
        self.assertEqual(report.arguments, {"groupId": GROUP_ID, "songQuery": "系ぎて", "sortOrder": "desc", "outputLimit": 10})

        difficulty_report = parse_group("musicrank红 系ぎて 10")
        self.assertEqual(difficulty_report.tool_name, "group_song_score_report")
        self.assertEqual(difficulty_report.arguments, {"groupId": GROUP_ID, "songQuery": "系ぎて", "levelIndex": 2, "sortOrder": "desc", "outputLimit": 10})

        attached_report = parse_group("musicrank系ぎて 10")
        self.assertEqual(attached_report.tool_name, "group_song_score_report")
        self.assertEqual(attached_report.arguments, {"groupId": GROUP_ID, "songQuery": "系ぎて", "sortOrder": "desc", "outputLimit": 10})

        range_report = parse_group("musicrank系ぎて 31-60")
        self.assertEqual(range_report.tool_name, "group_song_score_report")
        self.assertEqual(
            range_report.arguments,
            {"groupId": GROUP_ID, "songQuery": "系ぎて", "sortOrder": "desc", "startRank": 31, "endRank": 60},
        )

        attached_song_range_stays_query = parse_group("musicrank系ぎて31-60")
        self.assertEqual(attached_song_range_stays_query.tool_name, "group_song_score_member_rank")
        self.assertEqual(attached_song_range_stays_query.arguments["songQuery"], "系ぎて31-60")
        self.assertNotIn("levelIndex", attached_song_range_stays_query.arguments)

        reverse_report = parse_group("musicrank 倒序 id11451 10")
        self.assertEqual(reverse_report.tool_name, "group_song_score_report")
        self.assertEqual(reverse_report.arguments, {"groupId": GROUP_ID, "musicId": 11451, "sortOrder": "asc", "outputLimit": 10})

        reverse_range = parse_group("musicrank倒序 id11451 31-60")
        self.assertEqual(reverse_range.tool_name, "group_song_score_report")
        self.assertEqual(reverse_range.arguments, {"groupId": GROUP_ID, "musicId": 11451, "sortOrder": "asc", "startRank": 31, "endRank": 60})

        suffix_qq_stays_song_query = parse_group("musicrank 系ぎて 1000000001")
        self.assertEqual(suffix_qq_stays_song_query.tool_name, "group_song_score_member_rank")
        self.assertEqual(suffix_qq_stays_song_query.arguments["songQuery"], "系ぎて 1000000001")
        self.assertNotIn("levelIndex", suffix_qq_stays_song_query.arguments)

        target_with_limit = parse_group_error("1000000001 musicrank 系ぎて 10")
        self.assertIn("带 N 时不能同时指定玩家目标", target_with_limit.error_text)
        self.assertNotIn("`", target_with_limit.error_text)

    def test_divingfishrank_direct_syntax(self) -> None:
        default = parse("divingfishrank")
        self.assertEqual(default.tool_name, "render_maimai_rating_ranking")
        self.assertEqual(default.arguments, {"qq": SENDER_QQ})

        interval = parse("divingfishrank 31-60")
        self.assertEqual(interval.arguments, {"startRank": 31, "endRank": 60})

        dots = parse("divingfishrank 31..60")
        self.assertEqual(dots.arguments, {"startRank": 31, "endRank": 60})

        single = parse("divingfishrank 第123名")
        self.assertEqual(single.arguments, {"startRank": 123, "endRank": 123})

        username = parse("divingfishrank 123456")
        self.assertEqual(username.arguments, {"username": "123456"})

        prefix_qq = parse("1000000001 divingfishrank")
        self.assertEqual(prefix_qq.arguments, {"qq": "1000000001"})

        mention = parse("diving@222222fishrank", "222222")
        self.assertEqual(mention.arguments, {"qq": "222222"})

        too_many = parse("divingfishrank 1-31")
        self.assertEqual(too_many.tool_name, "direct_render_syntax_error")
        self.assertIn("最多输出 30 人", too_many.error_text)

    def test_player_mention_target_positions_cover_all_direct_renderers(self) -> None:
        cases: list[tuple[str, str, dict[str, Any], tuple[str, ...]]] = [
            ("@222222 b50", "render_maimai_b50", {"qq": "222222"}, ("222222",)),
            ("b50 @222222", "render_maimai_b50", {"qq": "222222"}, ("222222",)),
            ("@222222b50", "render_maimai_b50", {"qq": "222222"}, ("222222",)),
            ("b50@222222", "render_maimai_b50", {"qq": "222222"}, ("222222",)),
            ("@222222 拟合b50", "render_maimai_b50", {"qq": "222222", "computeFromRecords": True}, ("222222",)),
            ("b50拟合 @222222", "render_maimai_b50", {"qq": "222222", "computeFromRecords": True}, ("222222",)),
            ("@222222 minfo 系ぎて", "render_maimai_music_score", {"qq": "222222", "query": "系ぎて"}, ("222222",)),
            ("@222222minfo系ぎて", "render_maimai_music_score", {"qq": "222222", "query": "系ぎて"}, ("222222",)),
            ("minfo 系ぎて @222222", "render_maimai_music_score", {"qq": "222222", "query": "系ぎて"}, ("222222",)),
            ("@222222 13+定数表", "render_maimai_rating", {"qq": "222222", "rating": "13+"}, ("222222",)),
            ("13+定数表 @222222", "render_maimai_rating", {"qq": "222222", "rating": "13+"}, ("222222",)),
            ("@22222213+定数表", "render_maimai_rating", {"qq": "222222", "rating": "13+"}, ("222222",)),
            ("13+定数表@222222", "render_maimai_rating", {"qq": "222222", "rating": "13+"}, ("222222",)),
            ("@222222 桃极完成表", "render_maimai_plate", {"qq": "222222", "version": "桃", "plan": "极"}, ("222222",)),
            ("桃极完成表 @222222", "render_maimai_plate", {"qq": "222222", "version": "桃", "plan": "极"}, ("222222",)),
            ("@222222桃极完成表", "render_maimai_plate", {"qq": "222222", "version": "桃", "plan": "极"}, ("222222",)),
            ("桃极完成表@222222", "render_maimai_plate", {"qq": "222222", "version": "桃", "plan": "极"}, ("222222",)),
            (
                "@222222 桃极完成表 熊将完成表",
                "render_maimai_plate_batch",
                {
                    "qq": "222222",
                    "items": [{"version": "桃", "plan": "极"}, {"version": "熊", "plan": "将"}],
                },
                ("222222",),
            ),
            (
                "桃极完成表 熊将完成表 @222222",
                "render_maimai_plate_batch",
                {
                    "qq": "222222",
                    "items": [{"version": "桃", "plan": "极"}, {"version": "熊", "plan": "将"}],
                },
                ("222222",),
            ),
            ("@222222 我要在13+加5分", "render_maimai_rise_score", {"qq": "222222", "level": "13+", "score": 5}, ("222222",)),
            ("@222222 我要上分", "render_maimai_rise_score", {"qq": "222222"}, ("222222",)),
            ("@222222 我要上5分", "render_maimai_rise_score", {"qq": "222222", "score": 5}, ("222222",)),
            ("@222222 我要在13家上分", "render_maimai_rise_score", {"qq": "222222", "level": "13+"}, ("222222",)),
            ("我要在13+加5分 @222222", "render_maimai_rise_score", {"qq": "222222", "level": "13+", "score": 5}, ("222222",)),
            ("@222222我要在13+加5分", "render_maimai_rise_score", {"qq": "222222", "level": "13+", "score": 5}, ("222222",)),
            ("我要在13+加5分@222222", "render_maimai_rise_score", {"qq": "222222", "level": "13+", "score": 5}, ("222222",)),
            ("@222222 桃极进度", "render_maimai_plate_progress", {"qq": "222222", "version": "桃", "plan": "极"}, ("222222",)),
            ("桃极进度 @222222", "render_maimai_plate_progress", {"qq": "222222", "version": "桃", "plan": "极"}, ("222222",)),
            ("@222222桃极进度", "render_maimai_plate_progress", {"qq": "222222", "version": "桃", "plan": "极"}, ("222222",)),
            ("桃极进度@222222", "render_maimai_plate_progress", {"qq": "222222", "version": "桃", "plan": "极"}, ("222222",)),
            ("@222222 12+sss进度", "render_maimai_progress", {"qq": "222222", "level": "12+", "plan": "sss"}, ("222222",)),
            ("12+sss进度 @222222", "render_maimai_progress", {"qq": "222222", "level": "12+", "plan": "sss"}, ("222222",)),
            ("@22222212+sss进度", "render_maimai_progress", {"qq": "222222", "level": "12+", "plan": "sss"}, ("222222",)),
            ("12+sss进度@222222", "render_maimai_progress", {"qq": "222222", "level": "12+", "plan": "sss"}, ("222222",)),
            ("@222222 12+sss完成表", "render_maimai_progress", {"qq": "222222", "level": "12+", "plan": "sss"}, ("222222",)),
            ("12+sss完成表 @222222", "render_maimai_progress", {"qq": "222222", "level": "12+", "plan": "sss"}, ("222222",)),
            ("@22222212+sss完成表", "render_maimai_progress", {"qq": "222222", "level": "12+", "plan": "sss"}, ("222222",)),
            ("12+sss完成表@222222", "render_maimai_progress", {"qq": "222222", "level": "12+", "plan": "sss"}, ("222222",)),
            (
                "@222222 13+sss未完成进度 2",
                "render_maimai_progress",
                {"qq": "222222", "level": "13+", "plan": "sss", "category": "unfinished", "page": 2},
                ("222222",),
            ),
            (
                "13+sss未完成进度 2 @222222",
                "render_maimai_progress",
                {"qq": "222222", "level": "13+", "plan": "sss", "category": "unfinished", "page": 2},
                ("222222",),
            ),
            (
                "@222222 13+sss未完成表 2",
                "render_maimai_progress",
                {"qq": "222222", "level": "13+", "plan": "sss", "category": "unfinished", "page": 2},
                ("222222",),
            ),
            (
                "13+sss未完成表 2 @222222",
                "render_maimai_progress",
                {"qq": "222222", "level": "13+", "plan": "sss", "category": "unfinished", "page": 2},
                ("222222",),
            ),
            ("@222222 13+分数列表", "render_maimai_score_list", {"qq": "222222", "level": "13+"}, ("222222",)),
            ("13+分数列表 @222222", "render_maimai_score_list", {"qq": "222222", "level": "13+"}, ("222222",)),
            ("@22222213+分数列表", "render_maimai_score_list", {"qq": "222222", "level": "13+"}, ("222222",)),
            ("13+分数列表@222222", "render_maimai_score_list", {"qq": "222222", "level": "13+"}, ("222222",)),
            ("@222222 14.2分数列表 2", "render_maimai_score_list", {"qq": "222222", "ds": "14.2", "page": 2}, ("222222",)),
            ("14.2分数列表 2 @222222", "render_maimai_score_list", {"qq": "222222", "ds": "14.2", "page": 2}, ("222222",)),
        ]
        for text, tool_name, arguments, mentions in cases:
            with self.subTest(text=text):
                command = parse(text, *mentions)
                self.assertEqual(command.tool_name, tool_name)
                self.assertEqual(command.arguments, arguments)

    def test_plain_qq_target_positions_cover_account_renderers(self) -> None:
        cases: list[tuple[str, str, dict[str, Any]]] = [
            ("123456 b50", "render_maimai_b50", {"qq": "123456"}),
            ("123456 minfo 系ぎて", "render_maimai_music_score", {"qq": "123456", "query": "系ぎて"}),
            ("123456 13+定数表", "render_maimai_rating", {"qq": "123456", "rating": "13+"}),
            ("123456 桃极完成表", "render_maimai_plate", {"qq": "123456", "version": "桃", "plan": "极"}),
            (
                "123456 桃极完成表 熊将完成表",
                "render_maimai_plate_batch",
                {
                    "qq": "123456",
                    "items": [{"version": "桃", "plan": "极"}, {"version": "熊", "plan": "将"}],
                },
            ),
            ("123456 我要在13+加5分", "render_maimai_rise_score", {"qq": "123456", "level": "13+", "score": 5}),
            ("123456 我要上分", "render_maimai_rise_score", {"qq": "123456"}),
            ("123456 我要上5分", "render_maimai_rise_score", {"qq": "123456", "score": 5}),
            ("123456 我要在13家上分", "render_maimai_rise_score", {"qq": "123456", "level": "13+"}),
            ("123456 桃极进度", "render_maimai_plate_progress", {"qq": "123456", "version": "桃", "plan": "极"}),
            ("123456 12+sss进度", "render_maimai_progress", {"qq": "123456", "level": "12+", "plan": "sss"}),
            ("123456 12+sss完成表", "render_maimai_progress", {"qq": "123456", "level": "12+", "plan": "sss"}),
            (
                "123456 13+sss未完成进度 2",
                "render_maimai_progress",
                {"qq": "123456", "level": "13+", "plan": "sss", "category": "unfinished", "page": 2},
            ),
            (
                "123456 13+sss未完成表 2",
                "render_maimai_progress",
                {"qq": "123456", "level": "13+", "plan": "sss", "category": "unfinished", "page": 2},
            ),
            ("123456 13+分数列表", "render_maimai_score_list", {"qq": "123456", "level": "13+"}),
            ("123456 14.2分数列表 2", "render_maimai_score_list", {"qq": "123456", "ds": "14.2", "page": 2}),
        ]
        for text, tool_name, arguments in cases:
            with self.subTest(text=text):
                command = parse(text)
                self.assertEqual(command.tool_name, tool_name)
                self.assertEqual(command.arguments, arguments)

    def test_numeric_username_suffix_positions_cover_account_renderers(self) -> None:
        cases: list[tuple[str, str, dict[str, Any]]] = [
            ("b50 123456", "render_maimai_b50", {"username": "123456"}),
            ("b50 示例玩家", "render_maimai_b50", {"username": "示例玩家"}),
            ("拟合b50 123456", "render_maimai_b50", {"username": "123456", "computeFromRecords": True}),
            ("123456 拟合b50", "render_maimai_b50", {"qq": "123456", "computeFromRecords": True}),
            ("minfo 系ぎて 123456", "render_maimai_music_score", {"username": "123456", "query": "系ぎて"}),
            ("minfo 系ぎて 示例玩家", "render_maimai_music_score", {"username": "示例玩家", "query": "系ぎて"}),
            ("13+定数表 123456", "render_maimai_rating", {"username": "123456", "rating": "13+"}),
            (
                "桃极完成表 123456",
                "render_maimai_plate",
                {"username": "123456", "version": "桃", "plan": "极"},
            ),
            (
                "桃极完成表 熊将完成表 123456",
                "render_maimai_plate_batch",
                {
                    "username": "123456",
                    "items": [{"version": "桃", "plan": "极"}, {"version": "熊", "plan": "将"}],
                },
            ),
            ("我要在13+加5分 123456", "render_maimai_rise_score", {"username": "123456", "level": "13+", "score": 5}),
            ("我要在13+加5分 示例玩家", "render_maimai_rise_score", {"username": "示例玩家", "level": "13+", "score": 5}),
            ("我要上分 123456", "render_maimai_rise_score", {"username": "123456"}),
            ("我要上5分 示例玩家", "render_maimai_rise_score", {"username": "示例玩家", "score": 5}),
            ("我要在13家上分 示例玩家", "render_maimai_rise_score", {"username": "示例玩家", "level": "13+"}),
            ("桃极进度 123456", "render_maimai_plate_progress", {"username": "123456", "version": "桃", "plan": "极"}),
            ("桃极进度 示例玩家", "render_maimai_plate_progress", {"username": "示例玩家", "version": "桃", "plan": "极"}),
            ("12+sss进度 123456", "render_maimai_progress", {"username": "123456", "level": "12+", "plan": "sss"}),
            ("12+sss进度 示例玩家", "render_maimai_progress", {"username": "示例玩家", "level": "12+", "plan": "sss"}),
            ("12+sss完成表 123456", "render_maimai_progress", {"username": "123456", "level": "12+", "plan": "sss"}),
            ("12+sss完成表 示例玩家", "render_maimai_progress", {"username": "示例玩家", "level": "12+", "plan": "sss"}),
            (
                "13+sss未完成表 2 123456",
                "render_maimai_progress",
                {"username": "123456", "level": "13+", "plan": "sss", "category": "unfinished", "page": 2},
            ),
            ("13+分数列表 123456", "render_maimai_score_list", {"username": "123456", "level": "13+"}),
            ("13+分数列表 示例玩家", "render_maimai_score_list", {"username": "示例玩家", "level": "13+"}),
            ("14.2分数列表 2 123456", "render_maimai_score_list", {"username": "123456", "ds": "14.2", "page": 2}),
            ("14.2分数列表 2 示例玩家", "render_maimai_score_list", {"username": "示例玩家", "ds": "14.2", "page": 2}),
        ]
        for text, tool_name, arguments in cases:
            with self.subTest(text=text):
                command = parse(text)
                self.assertEqual(command.tool_name, tool_name)
                self.assertEqual(command.arguments, arguments)

    def test_player_mention_target_positions_cover_search_then_render(self) -> None:
        cases: list[tuple[str, dict[str, Any], dict[str, Any], tuple[str, ...]]] = [
            ("@222222 id296", {"id": "296", "limit": 5, "format": "json"}, {"qq": "222222"}, ("222222",)),
            ("id296 @222222", {"id": "296", "limit": 5, "format": "json"}, {"qq": "222222"}, ("222222",)),
            ("@222222id296", {"id": "296", "limit": 5, "format": "json"}, {"qq": "222222"}, ("222222",)),
            ("id296@222222", {"id": "296", "limit": 5, "format": "json"}, {"qq": "222222"}, ("222222",)),
            (
                "@222222 系ぎて是什么歌",
                {"query": "系ぎて", "limit": 5, "format": "json"},
                {"qq": "222222"},
                ("222222",),
            ),
            (
                "系ぎて是什么歌 @222222",
                {"query": "系ぎて", "limit": 5, "format": "json"},
                {"qq": "222222"},
                ("222222",),
            ),
            (
                "@222222系ぎて是什么歌",
                {"query": "系ぎて", "limit": 5, "format": "json"},
                {"qq": "222222"},
                ("222222",),
            ),
            (
                "系ぎて是什么歌@222222",
                {"query": "系ぎて", "limit": 5, "format": "json"},
                {"qq": "222222"},
                ("222222",),
            ),
        ]
        for text, search_arguments, render_arguments, mentions in cases:
            with self.subTest(text=text):
                command = parse(text, *mentions)
                self.assertTrue(command.needs_search)
                self.assertEqual(command.search_arguments, search_arguments)
                self.assertEqual(command.render_arguments, render_arguments)

    def test_plain_qq_target_positions_cover_search_then_render(self) -> None:
        cases: list[tuple[str, dict[str, Any], dict[str, Any]]] = [
            ("123456 id296", {"id": "296", "limit": 5, "format": "json"}, {"qq": "123456"}),
            ("123456 系ぎて是什么歌", {"query": "系ぎて", "limit": 5, "format": "json"}, {"qq": "123456"}),
        ]
        for text, search_arguments, render_arguments in cases:
            with self.subTest(text=text):
                command = parse(text)
                self.assertTrue(command.needs_search)
                self.assertEqual(command.search_arguments, search_arguments)
                self.assertEqual(command.render_arguments, render_arguments)

    def test_suffix_username_positions_cover_search_then_render(self) -> None:
        cases: list[tuple[str, dict[str, Any], dict[str, Any]]] = [
            ("id296 alice", {"id": "296", "limit": 5, "format": "json"}, {"username": "alice"}),
            ("id296 123456", {"id": "296", "limit": 5, "format": "json"}, {"username": "123456"}),
            ("id296 示例玩家", {"id": "296", "limit": 5, "format": "json"}, {"username": "示例玩家"}),
            ("id 296 123456", {"id": "296", "limit": 5, "format": "json"}, {"username": "123456"}),
            ("系ぎて是什么歌 alice", {"query": "系ぎて", "limit": 5, "format": "json"}, {"username": "alice"}),
            ("系ぎて是什么歌 123456", {"query": "系ぎて", "limit": 5, "format": "json"}, {"username": "123456"}),
            ("系ぎて是什么歌 示例玩家", {"query": "系ぎて", "limit": 5, "format": "json"}, {"username": "示例玩家"}),
        ]
        for text, search_arguments, render_arguments in cases:
            with self.subTest(text=text):
                command = parse(text)
                self.assertTrue(command.needs_search)
                self.assertEqual(command.search_arguments, search_arguments)
                self.assertEqual(command.render_arguments, render_arguments)

    def test_ginfo_does_not_accept_player_qq_target(self) -> None:
        self.assertIsNone(parse_direct_render_command("123456 ginfo 系ぎて", context()))
        command = parse("ginfo 123456 系ぎて")
        self.assertEqual(command.tool_name, "render_maimai_music_global_stats")
        self.assertEqual(command.arguments, {"query": "123456 系ぎて"})

    def test_middle_mentions_are_player_targets(self) -> None:
        cases: list[tuple[str, str, dict[str, Any], tuple[str, ...]]] = [
            ("minfo @222222 系ぎて", "render_maimai_music_score", {"qq": "222222", "query": "系ぎて"}, ("222222",)),
            ("minfo114 [CQ:at,qq=222222] 51", "render_maimai_music_score", {"qq": "222222", "query": "11451"}, ("222222",)),
            ("系ぎ[CQ:at,qq=222222]て是什么歌", "", {"qq": "222222"}, ("222222",)),
            ("系ぎ [CQ:at,qq=222222] て是什么歌", "", {"qq": "222222"}, ("222222",)),
            ("id @222222 296", "", {"qq": "222222"}, ("222222",)),
            ("13+分数列表 @222222 2", "render_maimai_score_list", {"qq": "222222", "level": "13+", "page": 2}, ("222222",)),
            ("雪[CQ:at,qq=222222]峰神完成表", "render_maimai_plate", {"qq": "222222", "version": "雪峰", "plan": "神"}, ("222222",)),
            ("雪 @222222 峰神完成表", "render_maimai_plate", {"qq": "222222", "version": "雪峰", "plan": "神"}, ("222222",)),
        ]
        for text, tool_name, arguments, mentions in cases:
            with self.subTest(text=text):
                command = parse(text, *mentions)
                if command.needs_search:
                    self.assertEqual(command.render_arguments, arguments)
                else:
                    self.assertEqual(command.tool_name, tool_name)
                    self.assertEqual(command.arguments, arguments)

    def test_plain_at_text_is_not_player_target(self) -> None:
        self.assertEqual(
            parse("minfo @222222 系ぎて").arguments,
            {"qq": SENDER_QQ, "query": "@222222 系ぎて"},
        )
        command = parse("@222222是什么歌")
        self.assertTrue(command.needs_search)
        self.assertEqual(command.search_arguments, {"query": "@222222", "limit": 5, "format": "json"})
        self.assertEqual(command.render_arguments, {"qq": SENDER_QQ})

    def test_literal_cq_text_without_message_chain_mention_is_not_player_target(self) -> None:
        command = parse("[CQ:at,qq=222222]是什么歌")

        self.assertTrue(command.needs_search)
        self.assertEqual(command.search_arguments, {"query": "[CQ:at,qq=222222]", "limit": 5, "format": "json"})
        self.assertEqual(command.render_arguments, {"qq": SENDER_QQ})

    def test_middle_message_chain_mention_inside_song_lookup_compacts_query(self) -> None:
        command = parse("系ぎ [CQ:at,qq=222222] て是什么歌", "222222")

        self.assertTrue(command.needs_search)
        self.assertEqual(command.search_arguments, {"query": "系ぎて", "limit": 5, "format": "json"})
        self.assertEqual(command.render_arguments, {"qq": "222222"})

    def test_invalid_plain_prefix_falls_back_to_agent(self) -> None:
        self.assertIsNone(parse_direct_render_command("alice b50", context()))
        self.assertIsNone(parse_direct_render_command("查我14s进度", context()))

    def test_help_command_returns_command_description(self) -> None:
        for text in ("help", "帮助", "命令说明", "指令说明", "菜单"):
            with self.subTest(text=text):
                command = parse(text)
                self.assertEqual(command.tool_name, "maimai_command_help")
                self.assertEqual(command.text_response, COMMAND_HELP_TEXT)
        self.assertNotIn("`", COMMAND_HELP_TEXT)
        self.assertIsNone(re.search(r"(?m)^\s*[-*]\s+", COMMAND_HELP_TEXT))


class DirectRenderAstrBotEventTest(unittest.TestCase):
    def plugin(self):
        plugin = object.__new__(MaimaiAutoSendImagesPlugin)
        plugin.config = {}
        plugin.context = types.SimpleNamespace(get_config=lambda umo="": {"admins_id": [SENDER_QQ]})
        plugin._sent_by_event = {}
        plugin._recent_path_keys = {}
        return plugin

    def test_napcat_ack_timeout_is_delivery_unknown(self) -> None:
        class NapCatSendTimeoutError(Exception):
            retcode = 1200
            message = "Timeout: NTEvent serviceAndMethod:NodeIKernelMsgService/sendMsg"
            wording = message

        self.assertTrue(_is_ambiguous_napcat_send_timeout(NapCatSendTimeoutError("send timeout")))
        self.assertFalse(_is_ambiguous_napcat_send_timeout(TimeoutError("ordinary timeout")))

    def test_direct_render_ack_timeout_does_not_fall_back_or_retry(self) -> None:
        class NapCatSendTimeoutError(Exception):
            retcode = 1200
            message = "Timeout: NTEvent serviceAndMethod:NodeIKernelMsgService/sendMsg"
            wording = message

        with tempfile.TemporaryDirectory() as temp_dir:
            first = Path(temp_dir) / "first.png"
            second = Path(temp_dir) / "second.png"
            first.write_bytes(b"\x89PNG\r\n\x1a\n")
            second.write_bytes(b"\x89PNG\r\n\x1a\n")
            plugin = self.plugin()
            plugin._direct_mcp_client = lambda: object()
            plugin._direct_command_text = lambda _event: "歌曲信息"
            attempted: list[str] = []

            async def timeout_after_submit(_event: Any, path: str) -> None:
                attempted.append(path)
                raise NapCatSendTimeoutError("send timeout")

            plugin._send_image = timeout_after_submit
            event = DummyEvent([Plain("歌曲信息")])
            command = DirectCommand("render_maimai_music_info", {})

            async def fake_handle(*_args: Any, **_kwargs: Any) -> Any:
                return types.SimpleNamespace(
                    image_paths=(str(first), str(second)),
                    text="不应发送的失败回退文本",
                )

            with (
                patch(
                    "deploy.astrbot.plugins.astrbot_plugin_maimai_auto_send_images.main.parse_direct_render_command",
                    return_value=command,
                ),
                patch(
                    "deploy.astrbot.plugins.astrbot_plugin_maimai_auto_send_images.main.handle_direct_command",
                    side_effect=fake_handle,
                ),
            ):
                asyncio.run(plugin.on_direct_render_message(event))

            self.assertEqual(attempted, [str(first), str(second)])
            state = plugin._sent_by_event[plugin._event_key(event)]
            self.assertEqual(state.paths, [str(first), str(second)])

    def test_tool_result_ack_timeout_is_recorded_and_suppressed(self) -> None:
        class NapCatSendTimeoutError(Exception):
            retcode = 1200
            message = "Timeout: NTEvent serviceAndMethod:NodeIKernelMsgService/sendMsg"
            wording = message

        with tempfile.TemporaryDirectory() as temp_dir:
            image = Path(temp_dir) / "result.png"
            image.write_bytes(b"\x89PNG\r\n\x1a\n")
            plugin = self.plugin()

            async def timeout_after_submit(_event: Any, _path: str) -> None:
                raise NapCatSendTimeoutError("send timeout")

            plugin._send_image = timeout_after_submit
            event = DummyEvent([Plain("b50")])
            tool = types.SimpleNamespace(name="render_maimai_b50")
            result = types.SimpleNamespace(
                isError=False,
                structuredContent={"imagePath": str(image)},
                content=[types.SimpleNamespace(text=str(image))],
            )

            asyncio.run(plugin.on_llm_tool_respond(event, tool, {}, result))

            state = plugin._sent_by_event[plugin._event_key(event)]
            self.assertEqual(state.paths, [str(image)])
            self.assertNotIn(str(image), result.content[0].text)

    def test_maimai_subagent_results_are_handled_by_default(self) -> None:
        plugin = self.plugin()

        self.assertTrue(plugin._should_handle_tool("transfer_to_maimai"))
        self.assertFalse(plugin._should_handle_tool("transfer_to_other"))

    def test_suppress_tool_result_paths_replaces_sent_image_path(self) -> None:
        plugin = self.plugin()
        path = "/AstrBot/data/maimai-images/1000000001_020.png"
        result = types.SimpleNamespace(
            content=[
                types.SimpleNamespace(
                    text=f"B50 图片已生成成功。\n![B50图](file://{path})\n已发送群内~"
                )
            ]
        )

        plugin._suppress_tool_result_paths(result, [path])

        self.assertNotIn(path, result.content[0].text)
        self.assertIn("图片已由插件自动发送", result.content[0].text)
        self.assertIn("不要再次调用 send_message_to_user", result.content[0].text)

    def test_bot_at_is_wake_not_target(self) -> None:
        plugin = self.plugin()
        event = DummyEvent([At(SELF_QQ), Plain(" b50")])

        command_text = plugin._direct_command_text(event)
        mentions = tuple(plugin._mention_qqs(event))
        command = parse_direct_render_command(
            command_text or "",
            TargetContext(sender_qq=plugin._sender_id(event), mention_qqs=mentions, self_qq=plugin._self_id(event)),
        )

        self.assertEqual(command_text, "b50")
        self.assertIsNotNone(command)
        self.assertEqual(command.arguments, {"qq": SENDER_QQ})

    def test_user_at_before_command_is_target(self) -> None:
        plugin = self.plugin()
        event = DummyEvent([At(SELF_QQ), Plain(" "), At("222222"), Plain(" b50")])

        command_text = plugin._direct_command_text(event)
        mentions = tuple(plugin._mention_qqs(event))
        command = parse_direct_render_command(
            command_text or "",
            TargetContext(sender_qq=plugin._sender_id(event), mention_qqs=mentions, self_qq=plugin._self_id(event)),
        )

        self.assertEqual(command_text, "[CQ:at,qq=222222] b50")
        self.assertIsNotNone(command)
        self.assertEqual(command.arguments, {"qq": "222222"})

    def test_user_at_after_command_is_target(self) -> None:
        plugin = self.plugin()
        event = DummyEvent([At(SELF_QQ), Plain(" b50 "), At("222222")])

        command_text = plugin._direct_command_text(event)
        mentions = tuple(plugin._mention_qqs(event))
        command = parse_direct_render_command(
            command_text or "",
            TargetContext(sender_qq=plugin._sender_id(event), mention_qqs=mentions, self_qq=plugin._self_id(event)),
        )

        self.assertEqual(command_text, "b50 [CQ:at,qq=222222]")
        self.assertIsNotNone(command)
        self.assertEqual(command.arguments, {"qq": "222222"})

    def test_adapter_wake_prefix_uses_processed_message_str(self) -> None:
        plugin = self.plugin()
        event = DummyEvent([Plain("Player b50")], message_str="b50")

        command_text = plugin._direct_command_text(event)
        command = parse_direct_render_command(
            command_text or "",
            TargetContext(sender_qq=plugin._sender_id(event), mention_qqs=(), self_qq=plugin._self_id(event)),
        )

        self.assertEqual(command_text, "b50")
        self.assertIsNotNone(command)
        self.assertEqual(command.arguments, {"qq": SENDER_QQ})

    def test_processed_message_str_keeps_trailing_chain_at_target(self) -> None:
        plugin = self.plugin()
        event = DummyEvent([Plain("Player b50 "), At("222222")], message_str="b50")

        command_text = plugin._direct_command_text(event)
        mentions = tuple(plugin._mention_qqs(event))
        command = parse_direct_render_command(
            command_text or "",
            TargetContext(sender_qq=plugin._sender_id(event), mention_qqs=mentions, self_qq=plugin._self_id(event)),
        )

        self.assertEqual(command_text, "b50 [CQ:at,qq=222222]")
        self.assertIsNotNone(command)
        self.assertEqual(command.arguments, {"qq": "222222"})

    def test_processed_message_str_keeps_leading_chain_at_target(self) -> None:
        plugin = self.plugin()
        event = DummyEvent([Plain("Player "), At("222222"), Plain(" b50")], message_str="b50")

        command_text = plugin._direct_command_text(event)
        mentions = tuple(plugin._mention_qqs(event))
        command = parse_direct_render_command(
            command_text or "",
            TargetContext(sender_qq=plugin._sender_id(event), mention_qqs=mentions, self_qq=plugin._self_id(event)),
        )

        self.assertEqual(command_text, "[CQ:at,qq=222222] b50")
        self.assertIsNotNone(command)
        self.assertEqual(command.arguments, {"qq": "222222"})

    def test_processed_message_str_keeps_middle_chain_at_target(self) -> None:
        plugin = self.plugin()
        event = DummyEvent([Plain("Player minfo "), At("222222"), Plain(" 系ぎて")], message_str="minfo 系ぎて")

        command_text = plugin._direct_command_text(event)
        mentions = tuple(plugin._mention_qqs(event))
        command = parse_direct_render_command(
            command_text or "",
            TargetContext(sender_qq=plugin._sender_id(event), mention_qqs=mentions, self_qq=plugin._self_id(event)),
        )

        self.assertEqual(command_text, "minfo [CQ:at,qq=222222] 系ぎて")
        self.assertIsNotNone(command)
        self.assertEqual(command.arguments, {"qq": "222222", "query": "系ぎて"})

    def test_processed_message_str_maps_display_mention_back_to_chain_at(self) -> None:
        plugin = self.plugin()
        event = DummyEvent(
            [Plain("minf "), At(SELF_QQ), Plain(" o114 "), At("1000000001"), Plain(" 51")],
            message_str="minfo114 @ExampleUser(1000000001) 51",
        )

        command_text = plugin._direct_command_text(event)
        mentions = tuple(plugin._mention_qqs(event))
        command = parse_direct_render_command(
            command_text or "",
            TargetContext(sender_qq=plugin._sender_id(event), mention_qqs=mentions, self_qq=plugin._self_id(event)),
        )

        self.assertEqual(command_text, "minf o114 [CQ:at,qq=1000000001] 51")
        self.assertIsNotNone(command)
        self.assertEqual(command.arguments, {"qq": "1000000001", "query": "11451"})

    def test_middle_bot_at_is_wake_only_and_not_target(self) -> None:
        plugin = self.plugin()
        event = DummyEvent([Plain("雪 "), At(SELF_QQ), Plain(" 峰神完成表")], message_str="雪峰神完成表")

        command_text = plugin._direct_command_text(event)
        mentions = tuple(plugin._mention_qqs(event))
        command = parse_direct_render_command(
            command_text or "",
            TargetContext(sender_qq=plugin._sender_id(event), mention_qqs=mentions, self_qq=plugin._self_id(event)),
        )

        self.assertEqual(command_text, "雪峰神完成表")
        self.assertIsNotNone(command)
        self.assertEqual(command.arguments, {"qq": SENDER_QQ, "version": "雪峰", "plan": "神"})

    def test_processed_message_str_keeps_middle_chain_at_inside_plate_name(self) -> None:
        plugin = self.plugin()
        event = DummyEvent(
            [Plain("Player 雪 "), At("222222"), Plain(" 峰神完成表")],
            message_str="雪峰神完成表",
        )

        command_text = plugin._direct_command_text(event)
        mentions = tuple(plugin._mention_qqs(event))
        command = parse_direct_render_command(
            command_text or "",
            TargetContext(sender_qq=plugin._sender_id(event), mention_qqs=mentions, self_qq=plugin._self_id(event)),
        )

        self.assertEqual(command_text, "雪 [CQ:at,qq=222222] 峰神完成表")
        self.assertIsNotNone(command)
        self.assertEqual(command.arguments, {"qq": "222222", "version": "雪峰", "plan": "神"})

    def test_processed_message_str_keeps_attached_middle_chain_at_inside_song_query(self) -> None:
        plugin = self.plugin()
        event = DummyEvent(
            [Plain("Player 系ぎ"), At("222222"), Plain("て是什么歌")],
            message_str="系ぎて是什么歌",
        )

        command_text = plugin._direct_command_text(event)
        mentions = tuple(plugin._mention_qqs(event))
        command = parse_direct_render_command(
            command_text or "",
            TargetContext(sender_qq=plugin._sender_id(event), mention_qqs=mentions, self_qq=plugin._self_id(event)),
        )

        self.assertEqual(command_text, "系ぎ[CQ:at,qq=222222]て是什么歌")
        self.assertIsNotNone(command)
        self.assertTrue(command.needs_search)
        self.assertEqual(command.search_arguments, {"query": "系ぎて", "limit": 5, "format": "json"})
        self.assertEqual(command.render_arguments, {"qq": "222222"})

    def test_processed_message_str_keeps_attached_middle_chain_at_inside_plate_name(self) -> None:
        plugin = self.plugin()
        event = DummyEvent(
            [Plain("Player 雪"), At("222222"), Plain("峰神完成表")],
            message_str="雪峰神完成表",
        )

        command_text = plugin._direct_command_text(event)
        mentions = tuple(plugin._mention_qqs(event))
        command = parse_direct_render_command(
            command_text or "",
            TargetContext(sender_qq=plugin._sender_id(event), mention_qqs=mentions, self_qq=plugin._self_id(event)),
        )

        self.assertEqual(command_text, "雪[CQ:at,qq=222222]峰神完成表")
        self.assertIsNotNone(command)
        self.assertEqual(command.arguments, {"qq": "222222", "version": "雪峰", "plan": "神"})

    def test_listener_wake_flag_alone_does_not_trigger_direct_route(self) -> None:
        plugin = self.plugin()
        event = DummyEvent(
            [Plain("b50")],
            message_str="b50",
            is_at_or_wake=False,
            is_wake_up=True,
        )

        self.assertIsNone(plugin._direct_command_text(event))

    def test_help_requires_same_wake_flow_as_other_direct_commands(self) -> None:
        plugin = self.plugin()
        asleep = DummyEvent([Plain("help")], message_str="help", is_at_or_wake=False)
        self.assertIsNone(plugin._direct_command_text(asleep))

        awakened = DummyEvent([At(SELF_QQ), Plain(" help")])
        command_text = plugin._direct_command_text(awakened)
        command = parse_direct_render_command(
            command_text or "",
            TargetContext(
                sender_qq=plugin._sender_id(awakened),
                mention_qqs=tuple(plugin._mention_qqs(awakened)),
                self_qq=plugin._self_id(awakened),
            ),
        )

        self.assertEqual(command_text, "help")
        self.assertIsNotNone(command)
        self.assertEqual(command.text_response, COMMAND_HELP_TEXT)

    def test_group_rank_event_context_includes_group_id(self) -> None:
        plugin = self.plugin()
        event = DummyEvent([At(SELF_QQ), Plain(" rank")], group_id=GROUP_ID)

        command_text = plugin._direct_command_text(event)
        mentions = tuple(plugin._mention_qqs(event))
        command = parse_direct_render_command(
            command_text or "",
            TargetContext(
                sender_qq=plugin._sender_id(event),
                mention_qqs=mentions,
                self_qq=plugin._self_id(event),
                group_id=plugin._group_id(event),
                is_private=plugin._is_private_chat(event),
            ),
        )

        self.assertEqual(command_text, "rank")
        self.assertIsNotNone(command)
        self.assertEqual(command.tool_name, "group_b50_member_rank")
        self.assertEqual(command.arguments["groupId"], GROUP_ID)

    def test_group_id_falls_back_to_unified_origin(self) -> None:
        plugin = self.plugin()
        event = DummyEvent(
            [Plain("rank")],
            group_id="",
            unified_msg_origin=f"napcat2:GroupMessage:{GROUP_ID}",
        )

        self.assertEqual(plugin._group_id(event), GROUP_ID)

    def test_whitelist_command_adds_group_and_allows_no_wake_direct_command(self) -> None:
        plugin = self.plugin()
        with tempfile.TemporaryDirectory() as tmp:
            whitelist_file = Path(tmp) / "direct-render-group-whitelist.json"
            plugin.config = {"direct_render_group_whitelist_file": str(whitelist_file)}
            event = DummyEvent(
                [At(SELF_QQ), Plain(f" whitelist add napcat2 {GROUP_ID}")],
                group_id=GROUP_ID,
                unified_msg_origin=f"napcat2:GroupMessage:{GROUP_ID}",
            )

            command_text = plugin._direct_command_text(event)
            reply = plugin._handle_whitelist_command(event, command_text or "")

            self.assertEqual(command_text, f"whitelist add napcat2 {GROUP_ID}")
            self.assertEqual(reply, f"已加入直连群白名单：napcat2 {GROUP_ID}")
            self.assertEqual(json.loads(whitelist_file.read_text(encoding="utf-8"))["items"], {"napcat2": [GROUP_ID]})

            no_wake_event = DummyEvent(
                [Plain("b50")],
                message_str="b50",
                is_at_or_wake=False,
                group_id=GROUP_ID,
                unified_msg_origin=f"napcat2:GroupMessage:{GROUP_ID}",
            )
            direct_text = plugin._direct_command_text(no_wake_event)
            command = parse_direct_render_command(
                direct_text or "",
                TargetContext(
                    sender_qq=plugin._sender_id(no_wake_event),
                    mention_qqs=tuple(plugin._mention_qqs(no_wake_event)),
                    self_qq=plugin._self_id(no_wake_event),
                    group_id=plugin._group_id(no_wake_event),
                ),
            )

            self.assertEqual(direct_text, "b50")
            self.assertIsNotNone(command)
            self.assertEqual(command.tool_name, "render_maimai_b50")

    def test_whitelist_command_requires_wake_even_in_whitelisted_group(self) -> None:
        plugin = self.plugin()
        with tempfile.TemporaryDirectory() as tmp:
            whitelist_file = Path(tmp) / "direct-render-group-whitelist.json"
            whitelist_file.write_text(
                json.dumps({"version": 1, "items": {"napcat2": [GROUP_ID]}}),
                encoding="utf-8",
            )
            plugin.config = {"direct_render_group_whitelist_file": str(whitelist_file)}
            event = DummyEvent(
                [Plain(f"whitelist add napcat2 {GROUP_ID}")],
                message_str=f"whitelist add napcat2 {GROUP_ID}",
                is_at_or_wake=False,
                group_id=GROUP_ID,
                unified_msg_origin=f"napcat2:GroupMessage:{GROUP_ID}",
            )

            command_text = plugin._direct_command_text(event)

            self.assertIsNone(command_text)

    def test_whitelist_command_rejects_cross_adapter_and_non_admin(self) -> None:
        plugin = self.plugin()
        with tempfile.TemporaryDirectory() as tmp:
            whitelist_file = Path(tmp) / "direct-render-group-whitelist.json"
            plugin.config = {"direct_render_group_whitelist_file": str(whitelist_file)}
            event = DummyEvent(
                [At(SELF_QQ), Plain(f" whitelist add napcat {GROUP_ID}")],
                group_id=GROUP_ID,
                unified_msg_origin=f"napcat2:GroupMessage:{GROUP_ID}",
            )

            command_text = plugin._direct_command_text(event)
            self.assertEqual(
                plugin._handle_whitelist_command(event, command_text or ""),
                "只能管理当前接收适配器 napcat2 的群白名单。",
            )

            plugin.context = types.SimpleNamespace(get_config=lambda umo="": {"admins_id": ["222222"]})
            non_admin_event = DummyEvent(
                [At(SELF_QQ), Plain(f" whitelist add napcat2 {GROUP_ID}")],
                group_id=GROUP_ID,
                unified_msg_origin=f"napcat2:GroupMessage:{GROUP_ID}",
            )

            command_text = plugin._direct_command_text(non_admin_event)
            self.assertEqual(
                plugin._handle_whitelist_command(non_admin_event, command_text or ""),
                "只有当前适配器配置文件里的管理员可以修改直连群白名单。",
            )
            self.assertFalse(whitelist_file.exists())

    def test_whitelist_list_and_delete_group(self) -> None:
        plugin = self.plugin()
        with tempfile.TemporaryDirectory() as tmp:
            whitelist_file = Path(tmp) / "direct-render-group-whitelist.json"
            whitelist_file.write_text(
                json.dumps({"version": 1, "items": {"napcat2": [GROUP_ID, "12345"]}}),
                encoding="utf-8",
            )
            plugin.config = {"direct_render_group_whitelist_file": str(whitelist_file)}
            list_event = DummyEvent(
                [At(SELF_QQ), Plain(" whitelist list napcat2")],
                group_id=GROUP_ID,
                unified_msg_origin=f"napcat2:GroupMessage:{GROUP_ID}",
            )

            command_text = plugin._direct_command_text(list_event)
            self.assertEqual(
                plugin._handle_whitelist_command(list_event, command_text or ""),
                f"直连群白名单 napcat2:\n12345\n{GROUP_ID}",
            )

            del_event = DummyEvent(
                [At(SELF_QQ), Plain(f" whitelist del napcat2 {GROUP_ID}")],
                group_id=GROUP_ID,
                unified_msg_origin=f"napcat2:GroupMessage:{GROUP_ID}",
            )
            command_text = plugin._direct_command_text(del_event)
            self.assertEqual(
                plugin._handle_whitelist_command(del_event, command_text or ""),
                f"已移出直连群白名单：napcat2 {GROUP_ID}",
            )
            self.assertEqual(json.loads(whitelist_file.read_text(encoding="utf-8"))["items"], {"napcat2": ["12345"]})

    def test_whitelist_command_allows_private_admin_management(self) -> None:
        plugin = self.plugin()
        with tempfile.TemporaryDirectory() as tmp:
            whitelist_file = Path(tmp) / "direct-render-group-whitelist.json"
            plugin.config = {"direct_render_group_whitelist_file": str(whitelist_file)}
            event = DummyEvent(
                [Plain(f"whitelist add napcat2 {GROUP_ID}")],
                message_str=f"whitelist add napcat2 {GROUP_ID}",
                private=True,
                unified_msg_origin=f"napcat2:FriendMessage:{SENDER_QQ}",
            )

            command_text = plugin._direct_command_text(event)
            reply = plugin._handle_whitelist_command(event, command_text or "")

            self.assertEqual(command_text, f"whitelist add napcat2 {GROUP_ID}")
            self.assertEqual(reply, f"已加入直连群白名单：napcat2 {GROUP_ID}")
            self.assertEqual(json.loads(whitelist_file.read_text(encoding="utf-8"))["items"], {"napcat2": [GROUP_ID]})

    def test_whitelist_old_syntax_returns_usage(self) -> None:
        plugin = self.plugin()
        event = DummyEvent(
            [At(SELF_QQ), Plain(f" whitelist napcat2 {GROUP_ID}")],
            group_id=GROUP_ID,
            unified_msg_origin=f"napcat2:GroupMessage:{GROUP_ID}",
        )

        command_text = plugin._direct_command_text(event)

        self.assertEqual(command_text, f"whitelist napcat2 {GROUP_ID}")
        self.assertEqual(
            plugin._handle_whitelist_command(event, command_text or ""),
            "用法：whitelist add 适配器名 群号 / whitelist del 适配器名 群号 / whitelist list [适配器名]",
        )


class DirectRenderHandleTest(unittest.IsolatedAsyncioTestCase):
    async def test_help_returns_text_without_mcp_call(self) -> None:
        client = FakeMcpClient({})

        result = await handle_direct_command(parse("help"), client)

        self.assertFalse(result.image_paths)
        self.assertEqual(result.text, COMMAND_HELP_TEXT)
        self.assertEqual(client.calls, [])

    async def test_today_maimai_passes_configured_offset(self) -> None:
        client = FakeMcpClient(
            {
                ("search", "today_maimai"): {
                    "content": [{"type": "text", "text": "今日人品值：32"}],
                    "isError": False,
                }
            }
        )

        result = await handle_direct_command(parse("今日舞萌"), client, today_offset=7)

        self.assertEqual(result.text, "今日人品值：32")
        self.assertEqual(client.calls, [("search", "today_maimai", {"qq": SENDER_QQ, "offset": 7})])

    async def test_text_search_filter_lookup_returns_search_text(self) -> None:
        client = FakeMcpClient(
            {
                ("search", "search_maimai_songs"): {
                    "content": [{"type": "text", "text": "查歌结果"}],
                    "isError": False,
                }
            }
        )

        result = await handle_direct_command(parse("谱师查歌Jack"), client)

        self.assertFalse(result.image_paths)
        self.assertEqual(result.text, "查歌结果")
        self.assertEqual(
            client.calls,
            [("search", "search_maimai_songs", {"charter": "Jack", "limit": 20, "format": "compact"})],
        )

    async def test_syntax_error_returns_text_without_mcp_call(self) -> None:
        client = FakeMcpClient({})

        result = await handle_direct_command(parse("divingfishrank 1-31"), client)

        self.assertFalse(result.image_paths)
        self.assertIn("最多输出 30 人", result.text)
        self.assertEqual(client.calls, [])

    async def test_group_rank_tool_returns_text_from_group_server(self) -> None:
        client = FakeMcpClient(
            {
                ("group", "group_b50_member_rank"): {
                    "isError": False,
                    "text": "群内排名文本",
                }
            }
        )

        result = await handle_direct_command(parse_group("rank"), client)

        self.assertFalse(result.image_paths)
        self.assertEqual(result.text, "群内排名文本")
        self.assertEqual(
            client.calls,
            [
                (
                    "group",
                    "group_b50_member_rank",
                    {"groupId": GROUP_ID, "qq": SENDER_QQ, "outputMode": "rating", "contextSize": 3},
                )
            ],
        )

    async def test_group_member_rank_direct_text_is_compact(self) -> None:
        client = FakeMcpClient(
            {
                ("group", "group_b50_member_rank"): {
                    "isError": False,
                    "groupId": GROUP_ID,
                    "qq": SENDER_QQ,
                    "found": True,
                    "member": {
                        "userId": SENDER_QQ,
                        "displayName": "test-player",
                        "rating": 15625,
                        "fitIndex": {
                            "available": True,
                            "virtualRatio": 1.86,
                            "virtualRating": 290.0,
                            "label": "明显虚高",
                        },
                    },
                    "rank": {"rankDesc": 3, "rankAsc": 26, "totalRanked": 28},
                    "text": "缓存状态: 本次使用一天内缓存\n正序文件: /tmp/a\n附近排名（排名正序）:\n| table |",
                }
            }
        )

        result = await handle_direct_command(parse_group("rank"), client)

        self.assertIn("test-player B50 群内排名", result.text)
        self.assertIn("倒序: 3 / 28", result.text)
        self.assertIn("虚高指数 +1.86% +290.0 ra 明显虚高", result.text)
        self.assertNotIn("正序文件", result.text)
        self.assertNotIn("附近排名", result.text)

    async def test_group_refresh_direct_text_is_compact(self) -> None:
        client = FakeMcpClient(
            {
                ("group", "group_b50_report"): {
                    "isError": False,
                    "groupId": GROUP_ID,
                    "job": {
                        "status": "running",
                        "processedCount": 12,
                        "totalCount": 46,
                        "cachedCount": 8,
                        "skippedCount": 2,
                    },
                    "text": f"群 {GROUP_ID} B50 缓存不存在、已过期或被要求刷新，已启动后台刷新任务。\n缓存路径: /AstrBot/data/group-b50-cache/{GROUP_ID}/cache.json\n稍后调用 group_b50_job_status 读取进度。",
                    "data": None,
                }
            }
        )

        result = await handle_direct_command(parse_group("rank 10"), client)

        self.assertEqual(result.text, f"群 {GROUP_ID} B50榜缓存正在后台刷新，进度 12/46，已缓存 8，跳过 2。稍后再试。")
        self.assertNotIn("缓存路径", result.text)
        self.assertNotIn("group_b50_job_status", result.text)

    async def test_group_song_member_rank_direct_text_is_compact(self) -> None:
        client = FakeMcpClient(
            {
                ("group", "group_song_score_member_rank"): {
                    "isError": False,
                    "groupId": GROUP_ID,
                    "qq": SENDER_QQ,
                    "musicId": 11451,
                    "found": True,
                    "target": {
                        "userId": SENDER_QQ,
                        "displayName": "test-player",
                        "record": {
                            "title": "系ぎて",
                            "levelLabel": "Master",
                            "level": "14",
                            "achievements": 100.5,
                            "rate": "sssp",
                            "ra": 320,
                            "dxScore": 1234,
                        },
                    },
                    "rankInfo": {"rankDesc": 2, "rankAsc": 27, "totalRanked": 28},
                    "text": "附近排名（达成率排名正序）:\n| table |\n附近排名（达成率升序）:\n| table |",
                }
            }
        )

        result = await handle_direct_command(parse_group("musicrank id11451"), client)

        self.assertIn("test-player 单曲群内排名：系ぎて / Master 14", result.text)
        self.assertIn("达成率倒序: 2 / 28；正序: 27 / 28", result.text)
        self.assertNotIn("附近排名", result.text)

    async def test_send_message_tool_wrapper_skips_already_sent_image(self) -> None:
        plugin = object.__new__(MaimaiAutoSendImagesPlugin)
        plugin.config = {}
        plugin._sent_by_event = {}
        plugin._recent_path_keys = {}
        event = DummyEvent([Plain("b50")])
        path = "/AstrBot/data/maimai-images/1000000001_021.png"
        plugin._remember_sent_paths(event, [path])

        class FakeSendMessageTool:
            name = "send_message_to_user"

            def __init__(self):
                self.calls: list[dict[str, Any]] = []

            async def call(self, context_wrapper: Any, **kwargs: Any) -> str:
                del context_wrapper
                self.calls.append(kwargs)
                return "sent"

        tool = FakeSendMessageTool()
        plugin._ensure_send_message_tool_wrapped(tool)  # type: ignore[arg-type]
        context_wrapper = types.SimpleNamespace(context=types.SimpleNamespace(event=event))

        result = await tool.call(
            context_wrapper,
            messages=[
                {"type": "plain", "text": "caption"},
                {"type": "image", "path": path},
            ],
        )

        self.assertIn("already sent", result)
        self.assertEqual(tool.calls, [])

    async def test_astrbot_tool_client_reuses_registered_mcp_tool(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            image = Path(tmp) / "b50.png"
            image.write_bytes(b"\x89PNG\r\n\x1a\n")
            text_item = type("TextItem", (), {"type": "text", "text": ""})()
            tool_result = type(
                "ToolResult",
                (),
                {
                    "isError": False,
                    "structuredContent": {"imagePath": str(image)},
                    "content": [text_item],
                },
            )()
            tool = FakeAstrBotTool(tool_result)
            fallback = FakeMcpClient({})
            client = AstrBotToolMcpClient(
                FakeAstrBotContext(FakeToolManager({"render_maimai_b50": tool})),
                {"direct_render_timeout_seconds": 7},
                fallback,  # type: ignore[arg-type]
            )

            result = await handle_direct_command(parse("b50"), client)

            self.assertEqual(result.image_paths, (str(image),))
            self.assertEqual(tool.calls, [(7, {"qq": SENDER_QQ})])
            self.assertEqual(fallback.calls, [])

    async def test_upload_server_bypasses_registered_astrbot_tool(self) -> None:
        tool_result = type(
            "ToolResult",
            (),
            {
                "isError": False,
                "structuredContent": {"text": "wrong"},
                "content": [type("TextItem", (), {"type": "text", "text": "wrong"})()],
            },
        )()
        tool = FakeAstrBotTool(tool_result)
        fallback = FakeMcpClient(
            {
                (
                    "upload",
                    "maimai_bind_import_token",
                ): {"isError": False, "content": [{"type": "text", "text": "bound"}]},
            }
        )
        client = AstrBotToolMcpClient(
            FakeAstrBotContext(FakeToolManager({"maimai_bind_import_token": tool})),
            {"direct_render_timeout_seconds": 7},
            fallback,  # type: ignore[arg-type]
        )

        result = await handle_direct_command(parse("mai bind import-token-abc"), client)

        self.assertEqual(result.text, "bound")
        self.assertEqual(tool.calls, [])
        self.assertEqual(
            fallback.calls,
            [
                (
                    "upload",
                    "maimai_bind_import_token",
                    {"qq": SENDER_QQ, "importToken": "import-token-abc"},
                )
            ],
        )

    async def test_search_unique_song_then_renders_music_info(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            image = Path(tmp) / "song.png"
            image.write_bytes(b"\x89PNG\r\n\x1a\n")
            search_result = {
                "content": [
                    {
                        "type": "text",
                        "text": json.dumps(
                            {
                                "songs": [
                                    {
                                        "id": "296",
                                        "title": "系ぎて",
                                        "artist": "test",
                                        "image_name": "cover.png",
                                    }
                                ]
                            },
                            ensure_ascii=False,
                        ),
                    }
                ],
                "isError": False,
            }
            render_result = {
                "content": [{"type": "text", "text": json.dumps({"imagePath": str(image)})}],
                "isError": False,
            }
            client = FakeMcpClient(
                {
                    ("search", "search_maimai_songs"): search_result,
                    ("render", "render_maimai_music_info"): render_result,
                }
            )

            result = await handle_direct_command(parse("id296"), client)

            self.assertEqual(result.image_paths, (str(image),))
            self.assertEqual(
                client.calls,
                [
                    ("search", "search_maimai_songs", {"id": "296", "limit": 5, "format": "json"}),
                    (
                        "render",
                        "render_maimai_music_info",
                        {"qq": SENDER_QQ, "query": "296", "music_id": "296", "image_name": "cover.png"},
                    ),
                ],
            )

    async def test_random_song_then_renders_music_info(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            image = Path(tmp) / "random_song.png"
            image.write_bytes(b"\x89PNG\r\n\x1a\n")
            random_result = {
                "content": [
                    {
                        "type": "text",
                        "text": json.dumps(
                            {
                                "songs": [
                                    {
                                        "id": "11451",
                                        "title": "系ぎて",
                                        "artist": "test",
                                        "image_name": "cover.png",
                                        "matched_charts": [{"chart_type": "dx", "difficulty_index": 3}],
                                    }
                                ]
                            },
                            ensure_ascii=False,
                        ),
                    }
                ],
                "isError": False,
            }
            render_result = {
                "content": [{"type": "text", "text": json.dumps({"imagePath": str(image)})}],
                "isError": False,
            }
            client = FakeMcpClient(
                {
                    ("search", "random_maimai_songs"): random_result,
                    ("render", "render_maimai_music_info"): render_result,
                }
            )

            result = await handle_direct_command(parse("随个dx紫13"), client)

            self.assertEqual(result.image_paths, (str(image),))
            self.assertEqual(
                client.calls,
                [
                    (
                        "search",
                        "random_maimai_songs",
                        {"count": 1, "format": "json", "song_type": "dx", "difficulty": "紫", "level": "13"},
                    ),
                    (
                        "render",
                        "render_maimai_music_info",
                        {
                            "qq": SENDER_QQ,
                            "songType": "dx",
                            "query": "系ぎて",
                            "music_id": "11451",
                            "image_name": "cover.png",
                        },
                    ),
                ],
            )

    async def test_random_song_without_candidates_returns_text(self) -> None:
        random_result = {
            "content": [{"type": "text", "text": json.dumps({"songs": []})}],
            "isError": False,
        }
        client = FakeMcpClient({("search", "random_maimai_songs"): random_result})

        result = await handle_direct_command(parse("随个白15"), client)

        self.assertFalse(result.image_paths)
        self.assertEqual(result.text, "没有找到符合条件的随机曲目。")
        self.assertEqual(
            client.calls,
            [("search", "random_maimai_songs", {"count": 1, "format": "json", "difficulty": "白", "level": "15"})],
        )

    async def test_search_dx_offset_id_keeps_music_id_for_render(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            image = Path(tmp) / "song.png"
            image.write_bytes(b"\x89PNG\r\n\x1a\n")
            search_result = {
                "content": [
                    {
                        "type": "text",
                        "text": json.dumps(
                            {
                                "songs": [
                                    {
                                        "id": "835",
                                        "title": "Believe the Rainbow",
                                        "artist": "test",
                                        "image_name": "cover.png",
                                        "available_chart_types": ["dx", "standard"],
                                    }
                                ]
                            },
                            ensure_ascii=False,
                        ),
                    }
                ],
                "isError": False,
            }
            render_result = {
                "content": [{"type": "text", "text": json.dumps({"imagePath": str(image)})}],
                "isError": False,
            }
            client = FakeMcpClient(
                {
                    ("search", "search_maimai_songs"): search_result,
                    ("render", "render_maimai_music_info"): render_result,
                }
            )

            result = await handle_direct_command(parse("id10835"), client)

            self.assertEqual(result.image_paths, (str(image),))
            self.assertEqual(
                client.calls,
                [
                    ("search", "search_maimai_songs", {"id": "10835", "limit": 5, "format": "json"}),
                    (
                        "render",
                        "render_maimai_music_info",
                        {"qq": SENDER_QQ, "query": "10835", "music_id": "10835", "image_name": "cover.png"},
                    ),
                ],
            )

    async def test_search_missing_id_falls_back_to_music_id_render(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            image = Path(tmp) / "cover_only.png"
            image.write_bytes(b"\x89PNG\r\n\x1a\n")
            search_result = {
                "content": [{"type": "text", "text": json.dumps({"songs": []})}],
                "isError": False,
            }
            render_result = {
                "content": [{"type": "text", "text": json.dumps({"imagePath": str(image)})}],
                "isError": False,
            }
            client = FakeMcpClient(
                {
                    ("search", "search_maimai_songs"): search_result,
                    ("render", "render_maimai_music_info"): render_result,
                }
            )

            result = await handle_direct_command(parse("id1000 @222222", "222222"), client)

            self.assertEqual(result.image_paths, (str(image),))
            self.assertEqual(
                client.calls,
                [
                    ("search", "search_maimai_songs", {"id": "1000", "limit": 5, "format": "json"}),
                    (
                        "render",
                        "render_maimai_music_info",
                        {"qq": "222222", "music_id": "1000"},
                    ),
                ],
            )

    async def test_search_missing_title_returns_text_without_rendering(self) -> None:
        search_result = {
            "content": [{"type": "text", "text": json.dumps({"songs": []})}],
            "isError": False,
        }
        client = FakeMcpClient({("search", "search_maimai_songs"): search_result})

        result = await handle_direct_command(parse("不存在是什么歌"), client)

        self.assertFalse(result.image_paths)
        self.assertEqual(result.text, "没有找到匹配曲目。")
        self.assertEqual(
            client.calls,
            [("search", "search_maimai_songs", {"query": "不存在", "limit": 5, "format": "json"})],
        )

    async def test_search_multi_candidate_returns_text_without_rendering(self) -> None:
        search_result = {
            "content": [
                {
                    "type": "text",
                    "text": json.dumps(
                        {
                            "songs": [
                                {"id": "1", "title": "alpha", "artist": "a"},
                                {"id": "2", "title": "beta", "artist": "b"},
                            ]
                        },
                        ensure_ascii=False,
                    ),
                }
            ],
            "isError": False,
        }
        client = FakeMcpClient({("search", "search_maimai_songs"): search_result})

        result = await handle_direct_command(parse("a是什么歌"), client)

        self.assertFalse(result.image_paths)
        self.assertIn("匹配到多个曲目", result.text)
        self.assertEqual(len(client.calls), 1)

    async def test_search_multi_candidate_text_uses_dx_chart_ids(self) -> None:
        search_result = {
            "content": [
                {
                    "type": "text",
                    "text": json.dumps(
                        {
                            "songs": [
                                {
                                    "id": "203",
                                    "title": "Help me, ERINNNNNN!!（Band ver.）",
                                    "artist": "ビートまりお(COOL＆CREATE)",
                                    "available_chart_types": ["standard"],
                                },
                                {
                                    "id": "1853",
                                    "title": "Help me, ERINNNNNN!!",
                                    "artist": "ビートまりお",
                                    "available_chart_types": ["dx"],
                                    "matched_charts": [{"chart_type": "dx", "internal_id": 11853}],
                                },
                            ]
                        },
                        ensure_ascii=False,
                    ),
                }
            ],
            "isError": False,
        }

        text = format_search_candidates(search_result)

        self.assertIn("1. Help me, ERINNNNNN!!（Band ver.） | ID 203", text)
        self.assertIn("2. Help me, ERINNNNNN!! | ID 11853", text)
        self.assertNotIn("ID 1853", text)


if __name__ == "__main__":
    unittest.main()
