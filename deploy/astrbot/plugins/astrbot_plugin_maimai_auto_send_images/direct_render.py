from __future__ import annotations

import asyncio
import json
import os
import re
import shlex
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Protocol

from .image_paths import (
    collect_paths_from_mapping,
    collect_paths_from_text,
    parse_prefix_mappings,
    unique_existing_paths,
)


DEFAULT_DATA_DIR = "/AstrBot/data"
DEFAULT_PROJECT_DIR = f"{DEFAULT_DATA_DIR}/maimai-mcp"
DEFAULT_IMAGE_DIR = f"{DEFAULT_DATA_DIR}/maimai-images"
DEFAULT_COVER_DIR = f"{DEFAULT_DATA_DIR}/maimai-covers"
DEFAULT_STATIC_DIR = f"{DEFAULT_DATA_DIR}/maimai-yuzu-static/Resource/static"
DEFAULT_SECRET_FILE = f"{DEFAULT_DATA_DIR}/maimai-config/.diving-fish-mcp-secrets.json"
DEFAULT_PLAYER_CACHE_DIR = f"{DEFAULT_DATA_DIR}/player-cache"
DEFAULT_QQ_IDENTITY_CACHE_DIR = f"{DEFAULT_DATA_DIR}/qq-identity-cache"

QQ_RE = re.compile(r"\d{5,12}")
MENTION_TOKEN_RE = re.compile(r"(?:@|\[CQ:at,qq=)(\d{5,12}|all)(?:\])?", re.IGNORECASE)
LEVEL_RE = r"(?:[1-9]|1[0-5])\+?"
RISE_LEVEL_RE = r"(?:[1-9]|1[0-5])(?:\+|加|家)?"
DS_RE = r"(?:[1-9]|1[0-5])(?:\.\d)?"
RATING_TARGET_RE = r"(?:sss\+|sss|ss\+|ss|s\+|s|ap|fc|fsd)"
PLATE_TARGET_RE = r"(?:舞舞|极|级|将|神)"
PLATE_TARGET_ALIASES = {
    "级": "极",
}
DIFFICULTY_ALIASES = {
    "basic",
    "bas",
    "advanced",
    "adv",
    "expert",
    "exp",
    "master",
    "mas",
    "remaster",
    "re:master",
    "remas",
    "green",
    "yellow",
    "red",
    "purple",
    "white",
    "绿",
    "黄",
    "红",
    "紫",
    "白",
}
MUSICRANK_DIFFICULTY_LEVEL_INDEX = {
    "绿": 0,
    "黄": 1,
    "红": 2,
    "紫": 3,
    "白": 4,
}
RANDOM_MUSIC_TYPE_ALIASES: tuple[tuple[str, str], ...] = (
    ("standard", "standard"),
    ("标准", "standard"),
    ("dx", "dx"),
    ("sd", "standard"),
    ("st", "standard"),
    ("标", "standard"),
)
TODAY_MAIMAI_COMMANDS = {
    "今日舞萌",
    "今日mai",
    "今日maimai",
    "今日mai运势",
    "今日maimai运势",
    "今日运势",
    "jrm",
    "jrrp",
}
TODAY_MAIMAI_COMMAND_FOLDS = {command.casefold() for command in TODAY_MAIMAI_COMMANDS}
COMMAND_HELP_TEXT = """命令说明

群聊先 @机器人、用唤醒词或回复机器人；私聊默认可直接发。
查自己默认用发送者 QQ；查别人优先用真实 @某人，也可把 QQ 放在命令最前面。

今日：
1. 今日舞萌
2. @某人 今日舞萌
3. 123456 今日mai
4. 今日舞萌 123456

成绩与玩家图：
1. b50
2. @某人 b50
3. 123456 b50
4. b50 username
5. 拟合b50 / b50拟合：用拟合定数重算单曲 rating 后重排 B50
6. minfo 歌名：玩家单曲成绩图，例如 minfo 系ぎて
7. ginfo紫 歌名：全服统计图，例如 ginfo紫 系ぎて

查歌与随机：
1. 歌名是什么歌
2. id296
3. 曲师查歌 かめりあ
4. 谱师查歌 Jack
5. bpm查歌 180-200
6. 随个13+
7. 随个dx紫13
8. 随个标准绿13
9. 随个紫13.4

完成表与列表：
1. 13+定数表
2. 桃极完成表
3. 真将进度
4. 13+sss进度
5. 13+sss未完成表 2
6. 14+分数列表 2
7. 14.2分数列表

上分推荐：
1. 我要上分
2. 我要在13+上分
3. 我要上5分

成绩导入：
1. mai bind <水鱼成绩导入token>
2. mai update <二维码解析内容>
3. mai update <二维码解析内容> --keyship <keyship> --logoutid 2 --title-ver 1.55.00

群排行：
1. rank
2. rank10
3. rank 倒序 10
4. rank 31-60
5. musicrank 歌名
6. musicrank紫白系
7. musicrank id11451 10
8. divingfishrank 1-30
9. divingfishrank 第123名

注意：
1. QQ 通常只放命令最前面；今日舞萌额外支持 QQ 放最后。
2. 命令后面的纯数字通常按页码、名次、曲目 ID 或 username 理解。
3. 支持 username 的命令可把水鱼 username 放在最后，例如 b50 username。"""


@dataclass(frozen=True)
class TargetContext:
    sender_qq: str = ""
    mention_qqs: tuple[str, ...] = ()
    self_qq: str = ""
    group_id: str = ""
    is_private: bool = False


@dataclass(frozen=True)
class DirectCommand:
    tool_name: str
    arguments: dict[str, Any]
    server: str = "render"
    search_tool_name: str = "search_maimai_songs"
    search_arguments: dict[str, Any] | None = None
    render_tool_name: str = ""
    render_arguments: dict[str, Any] = field(default_factory=dict)
    search_query: str = ""
    error_text: str = ""
    text_response: str = ""

    @property
    def needs_search(self) -> bool:
        return self.search_arguments is not None


@dataclass(frozen=True)
class RankOutputWindow:
    sort_order: str = "desc"
    output_limit: int | None = None
    start_rank: int | None = None
    end_rank: int | None = None

    @property
    def is_report(self) -> bool:
        return self.output_limit is not None or self.start_rank is not None


@dataclass(frozen=True)
class DirectMcpConfig:
    python_command: tuple[str, ...] = ("python",)
    project_cwd: str = DEFAULT_PROJECT_DIR
    render_module: str = "maimaidx_render_mcp.server"
    search_module: str = "maimai_mcp.server"
    group_module: str = "group_b50_mcp.server"
    upload_module: str = "maimai_update_mcp.server"
    timeout_seconds: float = 90.0
    upload_timeout_seconds: float = 300.0
    env: dict[str, str] = field(default_factory=dict)


@dataclass(frozen=True)
class DirectHandleResult:
    image_paths: tuple[str, ...] = ()
    text: str = ""


@dataclass(frozen=True)
class RankTargetParse:
    spaced_body: str
    compact_body: str
    targets: tuple[str, ...] = ()


class DirectRenderError(Exception):
    pass


class DirectMcpProtocol(Protocol):
    async def call_tool(self, server: str, tool_name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        ...


def parse_direct_render_command(text: str, context: TargetContext) -> DirectCommand | None:
    body = normalize_command_text(_space_context_mention_tokens(text, context))
    if not body or _is_explicitly_excluded(body):
        return None

    for parser in (
        _parse_help,
        _parse_maimai_update_workflow,
        _parse_today_maimai,
        _parse_musicrank,
        _parse_rank,
        _parse_divingfishrank,
        _parse_search_filter_lookup,
        _parse_random_music_info,
        _parse_song_info_lookup,
        _parse_b50,
        _parse_minfo,
        _parse_ginfo,
        _parse_rise_score,
        _parse_plate_batch,
        _parse_plate_table,
        _parse_plate_progress,
        _parse_rating_table,
        _parse_level_progress,
        _parse_score_list,
    ):
        command = parser(body, context)
        if command is not None:
            return command
    return None


async def handle_direct_command(
    command: DirectCommand,
    client: DirectMcpProtocol,
    *,
    path_prefix_mappings: Any = "",
    max_images: int = 8,
    today_offset: int | str | None = None,
) -> DirectHandleResult:
    if command.text_response:
        return DirectHandleResult(text=command.text_response)
    if command.error_text:
        return DirectHandleResult(text=command.error_text)

    if command.needs_search:
        search_result = await client.call_tool("search", command.search_tool_name, command.search_arguments or {})
        if command.search_tool_name == "random_maimai_songs":
            song = select_random_search_song(search_result)
            if song is None:
                return DirectHandleResult(text=format_random_candidates(search_result))
            render_args = build_random_music_info_render_arguments(
                song,
                command.render_arguments,
                command.search_arguments,
            )
        else:
            song = select_unique_search_song(search_result, command.search_query)
            if song is None:
                render_args = build_missing_search_music_info_render_arguments(command)
                if render_args is None:
                    return DirectHandleResult(text=format_search_candidates(search_result))
            else:
                render_args = build_music_info_render_arguments(
                    song,
                    command.search_query,
                    command.render_arguments,
                    command.search_arguments,
                )
        result = await client.call_tool("render", command.render_tool_name, render_args)
    else:
        arguments = command.arguments
        if command.tool_name == "today_maimai" and today_offset not in (None, ""):
            arguments = dict(arguments)
            arguments.setdefault("offset", today_offset)
        result = await client.call_tool(command.server, command.tool_name, arguments)

    if result.get("isError"):
        return DirectHandleResult(text=extract_mcp_text(result).strip() or "MCP 调用失败。")

    mappings = parse_prefix_mappings(path_prefix_mappings)
    paths = extract_existing_image_paths(result, mappings)[: max(1, max_images)]
    text = extract_mcp_text(result).strip()
    text = compact_direct_group_text(command, result, text)
    if paths:
        return DirectHandleResult(image_paths=tuple(paths))
    return DirectHandleResult(text=text)


def compact_direct_group_text(command: DirectCommand, result: dict[str, Any], text: str) -> str:
    if command.server != "group":
        return text
    if result.get("isError"):
        return text
    job_text = _compact_direct_group_job_text(command.tool_name, result)
    if job_text:
        return job_text
    if command.tool_name == "group_b50_member_rank":
        return _compact_direct_b50_member_rank(result) or _strip_direct_group_noise(text)
    if command.tool_name == "group_b50_rank_at":
        return _compact_direct_b50_rank_at(result) or _strip_direct_group_noise(text)
    if command.tool_name == "group_song_score_member_rank":
        return _compact_direct_song_member_rank(result) or _strip_direct_group_noise(text)
    if command.tool_name in {"group_b50_report", "group_b50_job_status", "group_song_score_report", "group_song_score_job_status"}:
        return _strip_direct_group_noise(text)
    return text


def _compact_direct_group_job_text(tool_name: str, result: dict[str, Any]) -> str:
    job = result.get("job") if isinstance(result.get("job"), dict) else None
    if not job:
        return ""
    status = str(job.get("status") or "")
    if status not in {"running", "pending"}:
        return ""
    group_id = result.get("groupId") or job.get("groupId") or _nested_get(result, ("cache", "groupId")) or "当前群"
    if tool_name.startswith("group_song"):
        feature = "单曲成绩"
    else:
        feature = "B50"
    bits = []
    processed = job.get("processedCount")
    total = job.get("totalCount") or job.get("memberCount") or _nested_get(result, ("cache", "memberCount"))
    if isinstance(processed, int) and total not in (None, ""):
        bits.append(f"进度 {processed}/{total}")
    cached = job.get("cachedCount") or job.get("successCount")
    skipped = job.get("skippedCount")
    if isinstance(cached, int):
        bits.append(f"已缓存 {cached}")
    if isinstance(skipped, int) and skipped:
        bits.append(f"跳过 {skipped}")
    detail = "，" + "，".join(bits) if bits else ""
    return f"群 {group_id} {feature}榜缓存正在后台刷新{detail}。稍后再试。"


def _compact_direct_b50_member_rank(result: dict[str, Any]) -> str:
    group_id = result.get("groupId") or "当前群"
    qq = str(result.get("qq") or "")
    if result.get("found") is False:
        total = result.get("totalRanked")
        suffix = f"当前可排名 {total} 人。" if isinstance(total, int) else ""
        return f"群 {group_id} QQ {qq} 暂无可输出的 B50 排名。{suffix}".strip()
    target = _direct_target_from_result(result, "member")
    rank = result.get("rank") if isinstance(result.get("rank"), dict) else _nested_get(result, ("data", "rank"))
    if not isinstance(target, dict) or not isinstance(rank, dict):
        return ""
    total = rank.get("totalRanked")
    lines = [
        f"{_direct_member_name(target)} B50 群内排名",
        f"倒序: {rank.get('rankDesc')} / {total}；正序: {rank.get('rankAsc')} / {total}",
    ]
    rating = target.get("rating")
    fit = _format_direct_fit_index(target.get("fitIndex"))
    details = []
    if rating not in (None, ""):
        details.append(f"Rating {rating}")
    if fit:
        details.append(f"虚高指数 {fit}")
    if details:
        lines.append("；".join(details))
    return "\n".join(lines)


def _compact_direct_b50_rank_at(result: dict[str, Any]) -> str:
    group_id = result.get("groupId") or "当前群"
    if result.get("found") is False:
        rank = result.get("rank")
        return f"群 {group_id} 没有第 {rank} 名。"
    target = _direct_target_from_result(result, "member")
    info = result.get("rankInfo") if isinstance(result.get("rankInfo"), dict) else _nested_get(result, ("data", "rank"))
    if not isinstance(target, dict) or not isinstance(info, dict):
        return ""
    label = "正序" if info.get("sortOrder") == "asc" else "倒序"
    requested = info.get("requestedRank") or result.get("rank")
    total = info.get("totalRanked")
    lines = [
        f"群 {group_id} rating {label}第 {requested} 名：{_direct_member_name(target)}",
        f"Rating {target.get('rating')}；倒序 {info.get('rankDesc')} / {total}；正序 {info.get('rankAsc')} / {total}",
    ]
    fit = _format_direct_fit_index(target.get("fitIndex"))
    if fit:
        lines.append(f"虚高指数 {fit}")
    return "\n".join(lines)


def _compact_direct_song_member_rank(result: dict[str, Any]) -> str:
    group_id = result.get("groupId") or "当前群"
    qq = str(result.get("qq") or "")
    music_id = result.get("musicId")
    if result.get("found") is False:
        return f"群 {group_id} QQ {qq} 在 music_id={music_id} 上没有可用成绩。"
    target = _direct_target_from_result(result, "target")
    rank = result.get("rankInfo") if isinstance(result.get("rankInfo"), dict) else _nested_get(result, ("data", "rankInfo"))
    if not isinstance(target, dict) or not isinstance(rank, dict):
        return ""
    record = target.get("record") if isinstance(target.get("record"), dict) else {}
    title = record.get("title") or f"music_id={music_id}"
    level = " ".join(str(v) for v in (record.get("levelLabel"), record.get("level")) if v not in (None, ""))
    lines = [
        f"{_direct_member_name(target)} 单曲群内排名：{title}" + (f" / {level}" if level else ""),
        f"达成率倒序: {rank.get('rankDesc')} / {rank.get('totalRanked')}；正序: {rank.get('rankAsc')} / {rank.get('totalRanked')}",
    ]
    score_bits = []
    achievements = record.get("achievements")
    if isinstance(achievements, (int, float)):
        score_bits.append(f"{achievements}%")
    if record.get("rate"):
        score_bits.append(str(record.get("rate")).upper())
    if record.get("ra") is not None:
        score_bits.append(f"ra {record.get('ra')}")
    if record.get("dxScore") is not None:
        score_bits.append(f"DX {record.get('dxScore')}")
    if score_bits:
        lines.append("；".join(score_bits))
    return "\n".join(lines)


def _strip_direct_group_noise(text: str) -> str:
    if not text:
        return text
    noisy_prefixes = (
        "缓存状态:",
        "缓存内容:",
        "缓存生成时间:",
        "任务状态:",
        "启动时间:",
        "完成时间:",
        "说明:",
        "缓存路径:",
        "正序文件:",
        "倒序文件:",
    )
    source_lines = text.splitlines()
    has_markdown_title = any(line.startswith("# ") for line in source_lines)
    lines: list[str] = []
    for raw in source_lines:
        line = raw.rstrip()
        stripped = line.strip()
        if not stripped:
            if lines and lines[-1] != "":
                lines.append("")
            continue
        if stripped.startswith(noisy_prefixes):
            continue
        if stripped.startswith("筛选:") and "匹配:" in stripped:
            continue
        if stripped.startswith("群成员:") and ("已缓存" in stripped or "跳过" in stripped):
            continue
        if has_markdown_title and stripped.startswith("群 ") and "B50 榜单" in stripped:
            continue
        lines.append(line)
    while lines and lines[-1] == "":
        lines.pop()
    return "\n".join(lines).strip() or text


def _direct_target_from_result(result: dict[str, Any], key: str) -> dict[str, Any] | None:
    value = result.get(key)
    if isinstance(value, dict):
        return value
    nested = _nested_get(result, ("data", "target"))
    return nested if isinstance(nested, dict) else None


def _nested_get(mapping: dict[str, Any], path: tuple[str, ...]) -> Any:
    value: Any = mapping
    for key in path:
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    return value


def _direct_member_name(item: dict[str, Any]) -> str:
    identity = item.get("identity") if isinstance(item.get("identity"), dict) else {}
    preferred_group = identity.get("preferredGroup") if isinstance(identity.get("preferredGroup"), dict) else {}
    player = item.get("player") if isinstance(item.get("player"), dict) else {}
    candidates = [
        preferred_group.get("groupNickname"),
        item.get("card"),
        item.get("displayName"),
        player.get("nickname"),
        item.get("playerNickname"),
        identity.get("waterfishNickname"),
        identity.get("qqNickname"),
        item.get("nickname"),
        item.get("userId"),
    ]
    for value in candidates:
        if isinstance(value, str) and value.strip():
            return value.strip()
    return str(item.get("userId") or "未知玩家")


def _format_direct_fit_index(fit_index: Any) -> str:
    if not isinstance(fit_index, dict) or not fit_index.get("available"):
        return ""
    parts = []
    ratio = fit_index.get("virtualRatio")
    rating = fit_index.get("virtualRating")
    label = fit_index.get("label")
    if isinstance(ratio, (int, float)):
        parts.append(f"{ratio:+.2f}%")
    if isinstance(rating, (int, float)):
        parts.append(f"{rating:+.1f} ra")
    if isinstance(label, str) and label.strip():
        parts.append(label.strip())
    return " ".join(parts)


def normalize_command_text(text: str) -> str:
    text = (text or "").strip()
    text = text.strip(" \t\r\n:：,，")
    text = _space_mention_tokens(text)
    text = re.sub(r"\s+", " ", text)
    return text.strip()


def _space_mention_tokens(text: str) -> str:
    return text


def _space_context_mention_tokens(text: str, context: TargetContext) -> str:
    del context
    return text


def _strip_leading_command(body: str, *commands: str) -> str | None:
    lowered = body.casefold()
    for command in commands:
        key = command.casefold()
        if lowered == key:
            return ""
        if lowered.startswith(key):
            return normalize_command_text(body[len(command) :])
    return None


def _is_explicitly_excluded(body: str) -> bool:
    lowered = body.casefold()
    if lowered in {"b50图", "b50图片", "查分图"}:
        return True
    if lowered.startswith("search ") and not lowered.startswith(("search artist", "search charter", "search bpm")):
        return True
    if lowered.startswith(("查歌", "分数线")):
        return True
    if any(token in lowered for token in ("rating排行榜", "查分器排行榜", "rating排名")):
        return True
    if body.endswith(("信息图", "歌曲信息图", "成绩图", "单曲成绩图", "全服统计", "达成率分布")):
        return True
    if re.fullmatch(rf".+{PLATE_TARGET_RE}", body):
        return True
    return False


def _parse_song_info_lookup(body: str, context: TargetContext) -> DirectCommand | None:
    for allow_username_suffix in (False, True):
        target_body, target_args = _strip_player_target(
            body,
            context,
            allow_username_suffix=allow_username_suffix,
            allow_numeric_username_suffix=True,
            compact_mention_gaps=True,
        )
        command = _parse_song_info_lookup_body(target_body, target_args)
        if command is None:
            command = _parse_song_info_lookup_body(_compact_command_text(target_body), target_args)
        if command is not None:
            return command
    return None


def _parse_help(body: str, context: TargetContext) -> DirectCommand | None:
    del context
    if body.casefold() not in {"help", "帮助", "命令说明", "指令说明", "菜单"}:
        return None
    return DirectCommand(
        tool_name="maimai_command_help",
        arguments={},
        text_response=COMMAND_HELP_TEXT,
    )


def _parse_maimai_update_workflow(body: str, context: TargetContext) -> DirectCommand | None:
    lowered = body.casefold()
    if lowered == "mai" or lowered.startswith("mai "):
        rest = normalize_command_text(body[3:])
    else:
        return None

    if not context.sender_qq:
        return _syntax_error("命令语法错误：当前会话无法识别发送者 QQ，不能绑定或上传成绩。")

    bind_rest = _strip_leading_command(rest, "bind")
    if bind_rest is not None:
        token = bind_rest.strip()
        if not token:
            return _syntax_error("用法：mai bind <水鱼成绩导入token>")
        if len(token.split()) != 1:
            return _syntax_error("命令语法错误：Import-Token 不能包含空格。")
        return _render(
            "maimai_bind_import_token",
            {"qq": context.sender_qq, "importToken": token},
            server="upload",
        )

    update_rest = _strip_leading_command(rest, "update")
    if update_rest is not None:
        return _parse_maimai_update_command(update_rest, context)

    return _syntax_error("用法：mai bind <水鱼成绩导入token> / mai update <二维码解析内容> [--keyship <keyship>] [--logoutid <1或2>] [--title-ver <版本>]")


def _parse_maimai_update_command(rest: str, context: TargetContext) -> DirectCommand:
    if not rest.strip():
        return _syntax_error("用法：mai update <二维码解析内容> [--keyship <keyship>] [--logoutid <1或2>] [--title-ver <版本>]")
    try:
        parts = shlex.split(rest)
    except ValueError as exc:
        return _syntax_error(f"命令语法错误：{exc}")
    if not parts:
        return _syntax_error("用法：mai update <二维码解析内容> [--keyship <keyship>] [--logoutid <1或2>] [--title-ver <版本>]")

    qr_content = parts[0]
    args: dict[str, Any] = {"qq": context.sender_qq, "qrContent": qr_content}
    index = 1
    while index < len(parts):
        option = parts[index]
        if option not in {"--keyship", "--keychip", "--logoutid", "--title-ver"}:
            return _syntax_error(f"命令语法错误：不支持的 mai update 参数 {option}")
        if index + 1 >= len(parts):
            return _syntax_error(f"命令语法错误：{option} 缺少参数值。")
        value = parts[index + 1]
        if option in {"--keyship", "--keychip"}:
            args["keyship"] = value
        elif option == "--logoutid":
            if value not in {"1", "2"}:
                return _syntax_error("命令语法错误：--logoutid 只能是 1 或 2。")
            args["logoutid"] = int(value)
        elif option == "--title-ver":
            args["titleVer"] = value
        index += 2
    return _render("maimai_update_records", args, server="upload")


def _parse_today_maimai(body: str, context: TargetContext) -> DirectCommand | None:
    target = _strip_direct_rank_targets(body, context, allow_prefix_qq=True)

    for target_body in (target.spaced_body, target.compact_body):
        command_body, suffix_qq = _strip_today_maimai_suffix_qq(target_body)
        if not _is_today_maimai_command(command_body):
            continue
        if len(target.targets) > 1:
            return _syntax_error("命令语法错误：今日舞萌一次只能指定一个 QQ。")
        qq = target.targets[0] if target.targets else suffix_qq or context.sender_qq
        if not qq:
            return _syntax_error("命令语法错误：当前会话无法识别发送者 QQ，不能生成今日舞萌。")
        return _render("today_maimai", {"qq": qq}, server="search")
    return None


def _strip_today_maimai_suffix_qq(body: str) -> tuple[str, str | None]:
    parts = body.split()
    if len(parts) == 2 and QQ_RE.fullmatch(parts[1]):
        return parts[0], parts[1]
    return body, None


def _is_today_maimai_command(body: str) -> bool:
    compact = _compact_command_text(body).casefold()
    return compact in TODAY_MAIMAI_COMMAND_FOLDS


def _parse_song_info_lookup_body(target_body: str, target_args: dict[str, Any]) -> DirectCommand | None:
    match = re.fullmatch(r"id\s*(\d+)", target_body, flags=re.IGNORECASE)
    if match:
        query = match.group(1)
        return _search_then_music_info({"id": query, "limit": 5, "format": "json"}, query, target_args)

    match = re.fullmatch(r"(.+?)是(?:什么|啥)歌", target_body)
    if not match:
        return None
    query = match.group(1).strip()
    if not query:
        return None
    return _search_then_music_info({"query": query, "limit": 5, "format": "json"}, query, target_args)


def _parse_search_filter_lookup(body: str, context: TargetContext) -> DirectCommand | None:
    del context
    cases: tuple[tuple[str, tuple[str, ...], str], ...] = (
        ("artist", ("曲师查歌", "曲師查歌", "search artist"), "曲师查歌需要曲师名，例如 曲师查歌 かめりあ。"),
        ("charter", ("谱师查歌", "譜師查歌", "search charter"), "谱师查歌需要谱师名，例如 谱师查歌 Jack。"),
        ("bpm", ("bpm查歌", "BPM查歌", "search bpm"), "bpm查歌需要 BPM 或范围，例如 bpm查歌 180-200。"),
    )
    for field, commands, error_text in cases:
        rest = _strip_leading_command(body, *commands)
        if rest is None:
            continue
        rest = rest.strip()
        if not rest:
            return _syntax_error(f"命令语法错误：{error_text}")
        value = _normalize_bpm_filter(rest) if field == "bpm" else rest
        return _render(
            "search_maimai_songs",
            {field: value, "limit": 20, "format": "compact"},
            server="search",
        )
    return None


def _normalize_bpm_filter(value: str) -> str:
    return value.replace("～", "-").replace("~", "-").strip()


def _parse_b50(body: str, context: TargetContext) -> DirectCommand | None:
    body, any_mention = _strip_any_mention_target(body, context)
    body, compute_from_records = _strip_computed_b50_marker(body)
    rest = _strip_leading_command(body, "b50")
    if rest is not None:
        if not rest:
            return _render("render_maimai_b50", _b50_arguments(context, any_mention, None, compute_from_records))
        parts = rest.split()
        if len(parts) != 1:
            return None
        other = parts[0]
        explicit_qq = any_mention or _mention_token_qq(other, context)
        username = other if not explicit_qq and _looks_username(other, allow_numeric=True) else None
        if not explicit_qq and not username:
            return None
        return _render("render_maimai_b50", _b50_arguments(context, explicit_qq, username, compute_from_records))

    parts = body.split()
    lowered = [part.casefold() for part in parts]
    if not parts or "b50" not in lowered or len(parts) > 2:
        return None
    if len(parts) == 1:
        return _render("render_maimai_b50", _b50_arguments(context, None, None, compute_from_records))

    command_first = lowered[0] == "b50"
    other = parts[1] if command_first else parts[0]
    if not command_first and not any_mention and not _mention_token_qq(other, context) and not QQ_RE.fullmatch(other):
        return None
    explicit_qq = any_mention or _mention_token_qq(other, context) or (None if command_first else (other if QQ_RE.fullmatch(other) else None))
    username = other if command_first and not explicit_qq and _looks_username(other, allow_numeric=True) else None
    if not explicit_qq and not username:
        return None
    return _render("render_maimai_b50", _b50_arguments(context, explicit_qq, username, compute_from_records))


def _strip_computed_b50_marker(body: str) -> tuple[str, bool]:
    updated = body
    computed = False
    for marker in ("拟合",):
        patterns = (
            rf"(?i){marker}\s*b50",
            rf"(?i)b50\s*{marker}",
        )
        for pattern in patterns:
            if re.search(pattern, updated):
                updated = re.sub(pattern, "b50", updated)
                computed = True
        parts = updated.split()
        if "b50" in [part.casefold() for part in parts] and marker in parts:
            updated = " ".join(part for part in parts if part != marker)
            computed = True
    return updated.strip(), computed


def _b50_arguments(
    context: TargetContext,
    explicit_qq: str | None = None,
    username: str | None = None,
    compute_from_records: bool = False,
) -> dict[str, Any]:
    args = _target_arguments(context, explicit_qq, username)
    if compute_from_records:
        args["computeFromRecords"] = True
    return args


def _parse_minfo(body: str, context: TargetContext) -> DirectCommand | None:
    for allow_username_suffix in (True, False):
        target_body, target_args = _strip_player_target(
            body,
            context,
            allow_username_suffix=allow_username_suffix,
            allow_numeric_username_suffix=True,
            compact_mention_gaps=True,
        )
        rest = _strip_leading_command(target_body, "minfo", "info")
        if rest is None:
            rest = _strip_leading_command(_compact_command_text(target_body), "minfo", "info")
        if rest is None or not rest:
            continue
        if allow_username_suffix and _contains_plain_at_token(rest, context):
            continue
        return _render(
            "render_maimai_music_score",
            {**target_args, "query": rest},
        )
    return None


def _parse_ginfo(body: str, context: TargetContext) -> DirectCommand | None:
    del context
    rest = _strip_leading_command(body, "ginfo")
    if rest is None:
        return None
    if not rest:
        return None
    difficulty = None
    query = rest
    tokens = rest.split(maxsplit=1)
    if len(tokens) == 2 and tokens[0].casefold() in DIFFICULTY_ALIASES:
        difficulty = tokens[0]
        query = tokens[1].strip()
    args: dict[str, Any] = {"query": query}
    if difficulty:
        args["difficulty"] = difficulty
    return _render("render_maimai_music_global_stats", args)


def _parse_rank(body: str, context: TargetContext) -> DirectCommand | None:
    target = _strip_direct_rank_targets(body, context, allow_prefix_qq=True)
    rest = _rank_command_rest(target.compact_body, "rank", attached_prefixes=("倒序",), attached_digit=True)
    if rest is None:
        rest = _rank_command_rest(target.spaced_body, "rank", attached_prefixes=("倒序",), attached_digit=True)
    if rest is None:
        return None
    if context.is_private:
        return _syntax_error(_group_rank_requires_group_text())
    if not context.group_id:
        return _syntax_error(_group_rank_missing_group_text())
    if len(target.targets) > 1:
        return _syntax_error(_rank_target_error_text())

    parsed = _parse_rank_suffix(rest)
    if parsed is None:
        return _syntax_error(_rank_syntax_error_text())
    if parsed.is_report:
        if target.targets:
            return _syntax_error(_rank_target_error_text())
        range_error = _direct_rank_window_error(parsed, "群 B50 排名")
        if range_error:
            return _syntax_error(range_error)
        args = {
            "groupId": context.group_id,
            "sortOrder": parsed.sort_order,
            "outputMode": "rating",
        }
        args.update(_rank_window_arguments(parsed))
        return _render(
            "group_b50_report",
            args,
            server="group",
        )

    qq = target.targets[0] if target.targets else context.sender_qq
    if not qq:
        return _syntax_error(_rank_missing_sender_text())
    return _render(
        "group_b50_member_rank",
        {
            "groupId": context.group_id,
            "qq": qq,
            "outputMode": "rating",
            "contextSize": 3,
        },
        server="group",
    )


def _parse_musicrank(body: str, context: TargetContext) -> DirectCommand | None:
    target = _strip_direct_rank_targets(body, context, allow_prefix_qq=True)
    rest = _rank_command_rest(target.compact_body, "musicrank", allow_attached=True)
    if rest is None:
        rest = _rank_command_rest(target.spaced_body, "musicrank", allow_attached=True)
    if rest is None:
        return None
    if context.is_private:
        return _syntax_error(_group_rank_requires_group_text())
    if not context.group_id:
        return _syntax_error(_group_rank_missing_group_text())
    if len(target.targets) > 1:
        return _syntax_error(_rank_target_error_text())

    sort_order = "desc"
    if rest == "倒序":
        return _syntax_error(_musicrank_syntax_error_text())
    if rest.startswith("倒序 "):
        sort_order = "asc"
        rest = normalize_command_text(rest[2:])
    if not rest:
        return _syntax_error(_musicrank_syntax_error_text())

    song_text, rank_window = _split_musicrank_song_and_window(rest)
    level_index, song_text = _split_musicrank_difficulty(song_text)
    if not song_text:
        return _syntax_error(_musicrank_syntax_error_text())
    if rank_window is not None and target.targets:
        return _syntax_error(_rank_target_error_text())

    song_args = _musicrank_song_arguments(song_text)
    if level_index is not None:
        song_args["levelIndex"] = level_index
    if rank_window is not None:
        range_error = _direct_rank_window_error(rank_window, "群单曲成绩排名")
        if range_error:
            return _syntax_error(range_error)
        args = {
            "groupId": context.group_id,
            **song_args,
            "sortOrder": sort_order,
        }
        args.update(_rank_window_arguments(rank_window))
        return _render(
            "group_song_score_report",
            args,
            server="group",
        )

    qq = target.targets[0] if target.targets else context.sender_qq
    if not qq:
        return _syntax_error(_rank_missing_sender_text())
    return _render(
        "group_song_score_member_rank",
        {
            "groupId": context.group_id,
            **song_args,
            "qq": qq,
            "contextSize": 3,
        },
        server="group",
    )


def _parse_divingfishrank(body: str, context: TargetContext) -> DirectCommand | None:
    target = _strip_direct_rank_targets(body, context, allow_prefix_qq=True)
    rest = _rank_command_rest(target.compact_body, "divingfishrank")
    if rest is None:
        rest = _rank_command_rest(target.spaced_body, "divingfishrank")
    if rest is None:
        return None
    if len(target.targets) > 1:
        return _syntax_error(_rank_target_error_text())

    if target.targets:
        if rest:
            return _syntax_error(_divingfishrank_syntax_error_text())
        return _render("render_maimai_rating_ranking", {"qq": target.targets[0]})

    if not rest:
        if not context.sender_qq:
            return _syntax_error(_rank_missing_sender_text())
        return _render("render_maimai_rating_ranking", {"qq": context.sender_qq})

    rank_range = _parse_divingfish_rank_range(rest)
    if rank_range is not None:
        start_rank, end_rank = rank_range
        if end_rank - start_rank + 1 > 30:
            return _syntax_error("命令语法错误：Diving-Fish 公开排名一次最多输出 30 人。")
        return _render(
            "render_maimai_rating_ranking",
            {"startRank": start_rank, "endRank": end_rank},
        )

    if _looks_username(rest, allow_numeric=True):
        return _render("render_maimai_rating_ranking", {"username": rest})
    return _syntax_error(_divingfishrank_syntax_error_text())


def _parse_random_music_info(body: str, context: TargetContext) -> DirectCommand | None:
    for allow_username_suffix in (False, True):
        target_body, target_args = _strip_player_target(
            body,
            context,
            allow_username_suffix=allow_username_suffix,
            allow_numeric_username_suffix=False,
            compact_mention_gaps=True,
        )
        command = _parse_random_music_info_body(target_body, target_args)
        if command is None:
            command = _parse_random_music_info_body(_compact_command_text(target_body), target_args)
        if command is not None:
            return command
    return None


def _parse_random_music_info_body(target_body: str, target_args: dict[str, Any]) -> DirectCommand | None:
    rest = _strip_leading_command(target_body, "随个")
    if rest is None:
        return None
    args = _parse_random_music_filters(rest)
    if args is None:
        return None

    search_args = {"count": 1, "format": "json", **args}
    render_args = dict(target_args)
    song_type = args.get("song_type")
    if song_type in {"dx", "standard"}:
        render_args["songType"] = song_type
    return DirectCommand(
        tool_name="",
        arguments={},
        search_tool_name="random_maimai_songs",
        search_arguments=search_args,
        render_tool_name="render_maimai_music_info",
        render_arguments=render_args,
        search_query="随机曲目",
    )


def _parse_rise_score(body: str, context: TargetContext) -> DirectCommand | None:
    target_body, target_args = _strip_player_target(
        body,
        context,
        allow_username_suffix=True,
        allow_numeric_username_suffix=True,
    )
    command = _parse_rise_score_body(target_body, target_args)
    if command is None:
        command = _parse_rise_score_body(_compact_command_text(target_body), target_args)
    return command


def _parse_rise_score_body(target_body: str, target_args: dict[str, Any]) -> DirectCommand | None:
    match = re.fullmatch(rf"我要在({RISE_LEVEL_RE})(?:上|加)(\d+)分", target_body)
    if match:
        return _render(
            "render_maimai_rise_score",
            {
                **target_args,
                "level": _normalize_rise_level(match.group(1)),
                "score": int(match.group(2)),
            },
        )

    match = re.fullmatch(rf"我要在({RISE_LEVEL_RE})(?:上|加)分", target_body)
    if match:
        return _render(
            "render_maimai_rise_score",
            {**target_args, "level": _normalize_rise_level(match.group(1))},
        )

    match = re.fullmatch(r"我要(?:上|加)(\d+)分", target_body)
    if match:
        return _render("render_maimai_rise_score", {**target_args, "score": int(match.group(1))})

    if re.fullmatch(r"我要(?:上|加)(?:积?分)", target_body):
        return _render("render_maimai_rise_score", target_args)
    return None


def _parse_plate_batch(body: str, context: TargetContext) -> DirectCommand | None:
    for allow_username_suffix in (False, True):
        target_body, target_args = _strip_player_target(
            body,
            context,
            allow_username_suffix=allow_username_suffix,
            allow_numeric_username_suffix=True,
        )
        pieces = target_body.split()
        if len(pieces) < 2:
            continue
        items = []
        for piece in pieces:
            match = re.fullmatch(rf"(.+?)({PLATE_TARGET_RE})完成表", piece)
            if not match:
                items = []
                break
            items.append({"version": _normalize_plate_version(match.group(1)), "plan": _normalize_plate_target(match.group(2))})
        if items:
            return _render("render_maimai_plate_batch", {**target_args, "items": items})
    return None


def _parse_plate_table(body: str, context: TargetContext) -> DirectCommand | None:
    target_body, target_args = _strip_player_target(
        body,
        context,
        allow_username_suffix=True,
        allow_numeric_username_suffix=True,
    )
    match = re.fullmatch(rf"(.+?)({PLATE_TARGET_RE})完成表", target_body)
    args = dict(target_args)
    if not match:
        attached = _split_attached_suffix_after_keyword(target_body, ("完成表",))
        if attached and _can_use_attached_username(target_args, context) and _looks_username(attached[1], allow_numeric=True):
            match = re.fullmatch(rf"(.+?)({PLATE_TARGET_RE})完成表", attached[0])
            args = _with_username_target(target_args, context, attached[1])
    if not match:
        match = re.fullmatch(rf"(.+?)({PLATE_TARGET_RE})完成表", _compact_command_text(target_body))
    if not match:
        return None
    return _render(
        "render_maimai_plate",
        {**args, "version": _normalize_plate_version(match.group(1)), "plan": _normalize_plate_target(match.group(2))},
    )


def _parse_plate_progress(body: str, context: TargetContext) -> DirectCommand | None:
    target_body, target_args = _strip_player_target(
        body,
        context,
        allow_username_suffix=True,
        allow_numeric_username_suffix=True,
    )
    match = re.fullmatch(rf"(.+?)({PLATE_TARGET_RE})进度", target_body)
    args = dict(target_args)
    if not match:
        attached = _split_attached_suffix_after_keyword(target_body, ("进度",))
        if attached and _can_use_attached_username(target_args, context) and _looks_username(attached[1], allow_numeric=True):
            match = re.fullmatch(rf"(.+?)({PLATE_TARGET_RE})进度", attached[0])
            args = _with_username_target(target_args, context, attached[1])
    if not match:
        match = re.fullmatch(rf"(.+?)({PLATE_TARGET_RE})进度", _compact_command_text(target_body))
    if not match:
        return None
    return _render(
        "render_maimai_plate_progress",
        {**args, "version": _normalize_plate_version(match.group(1)), "plan": _normalize_plate_target(match.group(2))},
    )


def _parse_rating_table(body: str, context: TargetContext) -> DirectCommand | None:
    target_body, target_args = _strip_player_target(
        body,
        context,
        allow_username_suffix=True,
        allow_numeric_username_suffix=True,
    )
    match = re.fullmatch(rf"({LEVEL_RE})(?:定数表|完成表)", target_body)
    args = dict(target_args)
    if not match:
        attached = _split_attached_suffix_after_keyword(target_body, ("定数表", "完成表"))
        if attached and _can_use_attached_username(target_args, context) and _looks_username(attached[1], allow_numeric=True):
            match = re.fullmatch(rf"({LEVEL_RE})(?:定数表|完成表)", attached[0])
            args = _with_username_target(target_args, context, attached[1])
    if not match:
        match = re.fullmatch(rf"({LEVEL_RE})(?:定数表|完成表)", _compact_command_text(target_body))
    if not match:
        return None
    return _render("render_maimai_rating", {**args, "rating": match.group(1)})


def _parse_level_progress(body: str, context: TargetContext) -> DirectCommand | None:
    target_body, target_args = _strip_paged_player_target(
        body,
        context,
    )
    command = _parse_level_progress_body(target_body, target_args, context)
    if command is None:
        command = _parse_level_progress_body(_compact_command_text(target_body), target_args, context)
    return command


def _parse_level_progress_body(
    target_body: str,
    target_args: dict[str, Any],
    context: TargetContext,
) -> DirectCommand | None:
    pattern = rf"({LEVEL_RE})({RATING_TARGET_RE})(?:(未完成)(?:进度|表)|完成表|进度)(?:\s+(\d+))?"
    match = re.fullmatch(
        pattern,
        target_body,
        flags=re.IGNORECASE,
    )
    args: dict[str, Any]
    if match:
        args = {
            **target_args,
            "level": match.group(1),
            "plan": match.group(2).casefold(),
        }
        if match.group(3):
            args["category"] = "unfinished"
        if match.group(4):
            args["page"] = int(match.group(4))
        return _render("render_maimai_progress", args)

    attached = _split_attached_suffix_after_keyword(
        target_body,
        ("未完成进度", "未完成表", "完成表", "进度"),
    )
    if not attached:
        return None
    base, suffix = attached
    match = re.fullmatch(
        rf"({LEVEL_RE})({RATING_TARGET_RE})(?:(未完成)(?:进度|表)|完成表|进度)",
        base,
        flags=re.IGNORECASE,
    )
    if not match:
        return None
    args = {
        **target_args,
        "level": match.group(1),
        "plan": match.group(2).casefold(),
    }
    if match.group(3):
        args["category"] = "unfinished"
    if suffix.isdigit() and _is_page_number(suffix):
        args["page"] = int(suffix)
    elif _can_use_attached_username(target_args, context) and _looks_username(suffix, allow_numeric=True):
        args = _with_username_target(args, context, suffix)
    else:
        return None
    return _render("render_maimai_progress", args)


def _parse_score_list(body: str, context: TargetContext) -> DirectCommand | None:
    target_body, target_args = _strip_paged_player_target(
        body,
        context,
    )
    command = _parse_score_list_body(target_body, target_args, context)
    if command is None:
        command = _parse_score_list_body(_compact_command_text(target_body), target_args, context)
    return command


def _parse_score_list_body(
    target_body: str,
    target_args: dict[str, Any],
    context: TargetContext,
) -> DirectCommand | None:
    match = re.fullmatch(rf"({LEVEL_RE}|{DS_RE})分数列表(?:\s+(\d+))?", target_body)
    args = dict(target_args)
    if match:
        value = match.group(1)
        if "." in value:
            args["ds"] = value
        else:
            args["level"] = value
        if match.group(2):
            args["page"] = int(match.group(2))
        return _render("render_maimai_score_list", args)

    attached = _split_attached_suffix_after_keyword(target_body, ("分数列表",))
    if not attached:
        return None
    base, suffix = attached
    match = re.fullmatch(rf"({LEVEL_RE}|{DS_RE})分数列表", base)
    if not match:
        return None
    value = match.group(1)
    if "." in value:
        args["ds"] = value
    else:
        args["level"] = value
    if suffix.isdigit() and _is_page_number(suffix):
        args["page"] = int(suffix)
    elif _can_use_attached_username(target_args, context) and _looks_username(suffix, allow_numeric=True):
        args = _with_username_target(args, context, suffix)
    else:
        return None
    return _render("render_maimai_score_list", args)


def _render(tool_name: str, args: dict[str, Any], *, server: str = "render") -> DirectCommand:
    return DirectCommand(tool_name=tool_name, arguments=args, server=server)


def _syntax_error(text: str) -> DirectCommand:
    return DirectCommand(tool_name="direct_render_syntax_error", arguments={}, error_text=text)


def _group_rank_requires_group_text() -> str:
    return "命令语法错误：群排行榜只支持群聊。"


def _group_rank_missing_group_text() -> str:
    return "命令语法错误：当前会话无法识别群号，不能查询群排行榜。"


def _rank_missing_sender_text() -> str:
    return "命令语法错误：当前会话无法识别发送者 QQ，不能查询个人群内排名。"


def _rank_target_error_text() -> str:
    return (
        "命令语法错误：同一条排名命令只能指定一个玩家目标。\n"
        "QQ 必须放在最前面，例如 1000000001 rank、1000000001 musicrank 系ぎて；"
        "真实 @ 可以放在消息任意位置；水鱼 username 只用于 divingfishrank sample_user；"
        "群排行榜查人请使用 QQ 或真实 @。N 是群榜输出人数，带 N 时不能同时指定玩家目标。"
    )


def _rank_syntax_error_text() -> str:
    return (
        "命令语法错误：rank 只支持 rank、rank 倒序、rank 1-30、"
        "rank 倒序 1-30、rank 起始-结束，或前置 QQ / 真实 @ 查询个人名次。"
    )


def _musicrank_syntax_error_text() -> str:
    return (
        "命令语法错误：musicrank 需要曲名、别名或 id，例如 musicrank id11451、"
        "musicrank系ぎて、musicrank 系ぎて 10、musicrank 系ぎて 31-60。"
    )


def _divingfishrank_syntax_error_text() -> str:
    return (
        "命令语法错误：divingfishrank 支持空参数查自己、1-30、31-60、第123名、"
        "前置 QQ / 真实 @ 查询玩家，或后置水鱼 username。"
    )


def _normalize_plate_target(target: str) -> str:
    return PLATE_TARGET_ALIASES.get(target, target)


def _normalize_rise_level(level: str) -> str:
    return f"{level[:-1]}+" if level.endswith(("加", "家")) else level


def _normalize_plate_version(version: str) -> str:
    return re.sub(r"\s+", "", version)


def _search_then_music_info(
    search_args: dict[str, Any],
    query: str,
    render_args: dict[str, Any],
) -> DirectCommand:
    return DirectCommand(
        tool_name="",
        arguments={},
        search_arguments=search_args,
        render_tool_name="render_maimai_music_info",
        render_arguments=render_args,
        search_query=query,
    )


def _strip_any_mention_target(
    body: str,
    context: TargetContext,
    *,
    compact_gaps: bool = False,
) -> tuple[str, str | None]:
    explicit_qq = None
    self_qq = str(context.self_qq or "")
    mention_qqs = {str(raw or "").strip() for raw in context.mention_qqs}

    def remove_cq(match: re.Match[str]) -> str:
        nonlocal explicit_qq
        qq = match.group(1)
        if qq == "all" or qq == self_qq:
            return ""
        if qq not in mention_qqs:
            return match.group(0)
        if QQ_RE.fullmatch(qq) and explicit_qq is None:
            explicit_qq = qq
        return ""

    cq_pattern = r"\[CQ:at,qq=(\d{5,12}|all)\]"
    if compact_gaps:
        cq_pattern = rf"\s*{cq_pattern}\s*"
    body = re.sub(cq_pattern, remove_cq, body, flags=re.IGNORECASE)
    kept: list[str] = []
    for part in body.split():
        qq = _mention_token_qq(part, context)
        if qq:
            if explicit_qq is None:
                explicit_qq = qq
            continue
        kept.append(part)
    return normalize_command_text(" ".join(kept)), explicit_qq


def _strip_direct_rank_targets(
    body: str,
    context: TargetContext,
    *,
    allow_prefix_qq: bool,
) -> RankTargetParse:
    targets: list[str] = []
    self_qq = str(context.self_qq or "")
    mention_qqs = {str(raw or "").strip() for raw in context.mention_qqs}

    def remove_spaced(match: re.Match[str]) -> str:
        qq = match.group(1)
        if qq != "all" and qq != self_qq and qq in mention_qqs and QQ_RE.fullmatch(qq):
            targets.append(qq)
        return " "

    def remove_compact(match: re.Match[str]) -> str:
        qq = match.group(1)
        if qq != "all" and qq != self_qq and qq in mention_qqs and QQ_RE.fullmatch(qq):
            # Targets are collected by remove_spaced. This pass only builds
            # the text where an At inserted inside a word is joined back.
            pass
        return ""

    cq_pattern = r"\[CQ:at,qq=(\d{5,12}|all)\]"
    spaced_body = re.sub(cq_pattern, remove_spaced, body, flags=re.IGNORECASE)
    compact_body = re.sub(rf"\s*{cq_pattern}\s*", remove_compact, body, flags=re.IGNORECASE)

    spaced_body = normalize_command_text(spaced_body)
    compact_body = normalize_command_text(compact_body)

    if allow_prefix_qq:
        parts = spaced_body.split()
        if len(parts) >= 2 and QQ_RE.fullmatch(parts[0]):
            prefix_qq = parts[0]
            targets.append(prefix_qq)
            spaced_body = normalize_command_text(" ".join(parts[1:]))
            if compact_body.startswith(prefix_qq):
                compact_body = normalize_command_text(compact_body[len(prefix_qq) :])

    return RankTargetParse(
        spaced_body=spaced_body,
        compact_body=compact_body,
        targets=tuple(targets),
    )


def _rank_command_rest(
    body: str,
    command: str,
    *,
    allow_attached: bool = False,
    attached_prefixes: tuple[str, ...] = (),
    attached_digit: bool = False,
) -> str | None:
    body = normalize_command_text(body)
    lowered = body.casefold()
    key = command.casefold()
    if lowered == key:
        return ""
    if lowered.startswith(key + " "):
        return normalize_command_text(body[len(command) :])
    if lowered.startswith(key):
        rest = body[len(command) :]
        if not rest:
            return ""
        rest_folded = rest.casefold()
        if allow_attached or (attached_digit and rest[:1].isdigit()) or any(rest_folded.startswith(prefix.casefold()) for prefix in attached_prefixes):
            return normalize_command_text(rest)
    return None


def _parse_rank_suffix(rest: str) -> RankOutputWindow | None:
    rest = normalize_command_text(rest)
    if not rest:
        return RankOutputWindow(sort_order="desc")
    if rest == "倒序":
        return RankOutputWindow(sort_order="asc")
    sort_order = "desc"
    if rest.startswith("倒序"):
        sort_order = "asc"
        rest = normalize_command_text(rest[2:])
        if not rest:
            return RankOutputWindow(sort_order=sort_order)
    match = re.fullmatch(r"(\d+)", rest)
    if match:
        value = int(match.group(1))
        return RankOutputWindow(sort_order=sort_order, output_limit=value) if 1 <= value <= 30 else None
    rank_range = _parse_rank_range(rest, allow_single=True)
    if rank_range is not None:
        start_rank, end_rank = rank_range
        return RankOutputWindow(sort_order=sort_order, start_rank=start_rank, end_rank=end_rank)
    return None


def _split_musicrank_song_and_window(rest: str) -> tuple[str, RankOutputWindow | None]:
    rest = normalize_command_text(rest)
    parts = rest.split()
    if len(parts) >= 2:
        tail = parts[-1]
        if tail.isdigit():
            value = int(tail)
            if 1 <= value <= 30:
                return normalize_command_text(" ".join(parts[:-1])), RankOutputWindow(output_limit=value)
        rank_range = _parse_rank_range(tail, allow_single=False)
        if rank_range is not None:
            start_rank, end_rank = rank_range
            return normalize_command_text(" ".join(parts[:-1])), RankOutputWindow(start_rank=start_rank, end_rank=end_rank)
    return rest, None


def _split_musicrank_difficulty(song_text: str) -> tuple[int | None, str]:
    song_text = normalize_command_text(song_text)
    parts = song_text.split(maxsplit=1)
    if parts and parts[0] in MUSICRANK_DIFFICULTY_LEVEL_INDEX:
        rest = parts[1] if len(parts) >= 2 else ""
        return MUSICRANK_DIFFICULTY_LEVEL_INDEX[parts[0]], normalize_command_text(rest)

    if song_text[:1] in MUSICRANK_DIFFICULTY_LEVEL_INDEX:
        rest = song_text[1:].strip()
        if rest:
            return MUSICRANK_DIFFICULTY_LEVEL_INDEX[song_text[:1]], normalize_command_text(rest)

    return None, song_text


def _parse_random_music_filters(rest: str) -> dict[str, Any] | None:
    rest = normalize_command_text(rest)
    if rest in {"", "歌", "曲", "曲目", "一首", "一首歌"}:
        return {}

    text = re.sub(r"\s+", "", rest)
    args: dict[str, Any] = {}

    song_type, text = _consume_random_song_type(text)
    if song_type:
        args["song_type"] = song_type

    if text[:1] in MUSICRANK_DIFFICULTY_LEVEL_INDEX:
        args["difficulty"] = text[:1]
        text = text[1:]

    if text:
        if re.fullmatch(LEVEL_RE, text):
            args["level"] = text
            text = ""
        elif re.fullmatch(DS_RE, text) and "." in text:
            args["ds"] = text
            text = ""

    if text:
        return None
    return args


def _consume_random_song_type(text: str) -> tuple[str | None, str]:
    folded = text.casefold()
    for alias, song_type in RANDOM_MUSIC_TYPE_ALIASES:
        if folded.startswith(alias.casefold()):
            return song_type, text[len(alias) :]
    return None, text


def _rank_window_arguments(window: RankOutputWindow) -> dict[str, Any]:
    if window.start_rank is not None and window.end_rank is not None:
        return {"startRank": window.start_rank, "endRank": window.end_rank}
    if window.output_limit is not None:
        return {"outputLimit": window.output_limit}
    return {}


def _direct_rank_window_error(window: RankOutputWindow, label: str) -> str:
    if window.start_rank is not None and window.end_rank is not None and window.end_rank - window.start_rank + 1 > 30:
        return f"命令语法错误：{label}范围一次最多输出 30 人。"
    return ""


def _musicrank_song_arguments(song_text: str) -> dict[str, Any]:
    song_text = normalize_command_text(song_text)
    match = re.fullmatch(r"id\s*(\d+)", song_text, flags=re.IGNORECASE)
    if match:
        return {"musicId": int(match.group(1))}
    if song_text.isdigit():
        return {"musicId": int(song_text)}
    return {"songQuery": song_text}


def _parse_divingfish_rank_range(rest: str) -> tuple[int, int] | None:
    rest = normalize_command_text(rest)
    return _parse_rank_range(rest, allow_single=True)


def _parse_rank_range(rest: str, *, allow_single: bool) -> tuple[int, int] | None:
    rest = normalize_command_text(rest)
    match = re.fullmatch(r"(\d+)\s*(?:-|\.\.)\s*(\d+)", rest)
    if match:
        start_rank = int(match.group(1))
        end_rank = int(match.group(2))
        if start_rank >= 1 and end_rank >= start_rank:
            return start_rank, end_rank
        return None
    match = re.fullmatch(r"第\s*(\d+)\s*(?:到|至|-)\s*(\d+)\s*名?", rest)
    if match:
        start_rank = int(match.group(1))
        end_rank = int(match.group(2))
        if start_rank >= 1 and end_rank >= start_rank:
            return start_rank, end_rank
        return None
    if not allow_single:
        return None
    match = re.fullmatch(r"第\s*(\d+)\s*名", rest)
    if match:
        rank = int(match.group(1))
        return (rank, rank) if rank >= 1 else None
    return None


def _compact_command_text(body: str) -> str:
    parts = normalize_command_text(body).split()
    if not parts:
        return ""
    if len(parts) >= 2 and parts[-1].isdigit():
        return normalize_command_text(f"{''.join(parts[:-1])} {parts[-1]}")
    return normalize_command_text("".join(parts))


def _split_attached_suffix_after_keyword(body: str, keywords: tuple[str, ...]) -> tuple[str, str] | None:
    body = normalize_command_text(body)
    best: tuple[int, int, str, str] | None = None
    for keyword in keywords:
        start = body.rfind(keyword)
        if start < 0:
            continue
        end = start + len(keyword)
        suffix = body[end:].strip()
        prefix = body[:end].strip()
        if not prefix or not suffix:
            continue
        if best is None or end > best[0] or (end == best[0] and len(keyword) > best[1]):
            best = (end, len(keyword), prefix, suffix)
    if best is None:
        return None
    return best[2], best[3]


def _strip_player_target(
    body: str,
    context: TargetContext,
    *,
    allow_username_suffix: bool,
    allow_numeric_username_suffix: bool = False,
    numeric_username_min_length: int = 0,
    compact_mention_gaps: bool = False,
) -> tuple[str, dict[str, Any]]:
    body, explicit_qq = _strip_any_mention_target(body, context, compact_gaps=compact_mention_gaps)
    parts = body.split()
    username = None
    if not explicit_qq and len(parts) >= 2 and QQ_RE.fullmatch(parts[0]):
        explicit_qq = parts[0]
        body = " ".join(parts[1:])
    elif (
        not explicit_qq
        and allow_username_suffix
        and len(parts) >= 2
        and _looks_username(
            parts[-1],
            allow_numeric=allow_numeric_username_suffix,
            numeric_min_length=numeric_username_min_length,
        )
    ):
        username = parts[-1]
        body = " ".join(parts[:-1])
    return normalize_command_text(body), _target_arguments(context, explicit_qq, username)


def _strip_paged_player_target(
    body: str,
    context: TargetContext,
    *,
    page_max: int = 20,
) -> tuple[str, dict[str, Any]]:
    body, explicit_qq = _strip_any_mention_target(body, context)
    parts = body.split()
    username = None
    if not explicit_qq and len(parts) >= 2 and QQ_RE.fullmatch(parts[0]):
        explicit_qq = parts[0]
        parts = parts[1:]

    if not explicit_qq and len(parts) >= 2:
        suffix = parts[-1]
        if suffix.isdigit() and (
            not _is_page_number(suffix, page_max)
            or (len(parts) >= 3 and _is_page_number(parts[-2], page_max))
        ):
            username = suffix
            parts = parts[:-1]
        elif not suffix.isdigit() and _looks_username(suffix, allow_numeric=False):
            username = suffix
            parts = parts[:-1]

    return normalize_command_text(" ".join(parts)), _target_arguments(context, explicit_qq, username)


def _is_page_number(value: str, page_max: int = 20) -> bool:
    if not value.isdigit():
        return False
    page = int(value)
    return 1 <= page <= page_max


def _target_arguments(
    context: TargetContext,
    explicit_qq: str | None = None,
    username: str | None = None,
) -> dict[str, Any]:
    if explicit_qq:
        return {"qq": explicit_qq}
    if username:
        return {"username": username}
    return {"qq": context.sender_qq} if context.sender_qq else {}


def _can_use_attached_username(args: dict[str, Any], context: TargetContext) -> bool:
    if not args:
        return True
    return set(args) == {"qq"} and str(args.get("qq") or "") == str(context.sender_qq or "")


def _with_username_target(args: dict[str, Any], context: TargetContext, username: str) -> dict[str, Any]:
    next_args = dict(args)
    if str(next_args.get("qq") or "") == str(context.sender_qq or ""):
        next_args.pop("qq", None)
    next_args["username"] = username
    return next_args


def _mention_token_qq(token: str, context: TargetContext) -> str | None:
    token = token.strip()
    match = MENTION_TOKEN_RE.fullmatch(token)
    if not match:
        return None
    qq = match.group(1)
    if qq == "all" or qq == str(context.self_qq or ""):
        return None
    if not QQ_RE.fullmatch(qq):
        return None
    mention_qqs = {str(raw or "").strip() for raw in context.mention_qqs}
    return qq if qq in mention_qqs else None


def _contains_plain_at_token(value: str, context: TargetContext) -> bool:
    return any(
        MENTION_TOKEN_RE.fullmatch(part.strip()) and _mention_token_qq(part, context) is None
        for part in value.split()
    )


def _looks_username(value: str, *, allow_numeric: bool = False, numeric_min_length: int = 0) -> bool:
    value = (value or "").strip()
    if not value or any(ch.isspace() for ch in value):
        return False
    if value.startswith("@") or value.casefold().startswith("[cq:at,qq="):
        return False
    if value.isdigit():
        return allow_numeric and len(value) >= numeric_min_length
    return True


def extract_mcp_text(result: dict[str, Any]) -> str:
    texts: list[str] = []
    direct_text = result.get("text")
    if isinstance(direct_text, str):
        texts.append(direct_text)
    for item in result.get("content") or []:
        if isinstance(item, dict) and isinstance(item.get("text"), str):
            texts.append(item["text"])
    return "\n".join(texts)


def extract_existing_image_paths(result: dict[str, Any], mappings: Any = ()) -> list[str]:
    raw_paths: list[str] = []
    raw_paths.extend(collect_paths_from_mapping(result.get("structuredContent")))
    raw_paths.extend(collect_paths_from_mapping(result.get("structured_content")))
    raw_paths.extend(collect_paths_from_mapping(result))
    for item in result.get("content") or []:
        if isinstance(item, dict):
            text = item.get("text")
            if isinstance(text, str):
                raw_paths.extend(collect_paths_from_text(text))
    return unique_existing_paths(raw_paths, mappings)


def parse_json_content(result: dict[str, Any]) -> dict[str, Any]:
    text = extract_mcp_text(result).strip()
    if not text:
        return {}
    try:
        parsed = json.loads(text)
    except json.JSONDecodeError:
        return {}
    return parsed if isinstance(parsed, dict) else {}


def select_unique_search_song(result: dict[str, Any], query: str) -> dict[str, Any] | None:
    if result.get("isError"):
        return None
    payload = parse_json_content(result)
    songs = payload.get("songs") if isinstance(payload.get("songs"), list) else []
    songs = [song for song in songs if isinstance(song, dict)]
    if len(songs) == 1:
        return songs[0]

    exact = [song for song in songs if _song_matches_query_exactly(song, query)]
    return exact[0] if len(exact) == 1 else None


def select_random_search_song(result: dict[str, Any]) -> dict[str, Any] | None:
    if result.get("isError"):
        return None
    payload = parse_json_content(result)
    songs = payload.get("songs") if isinstance(payload.get("songs"), list) else []
    for song in songs:
        if isinstance(song, dict):
            return song
    return None


def _song_matches_query_exactly(song: dict[str, Any], query: str) -> bool:
    needle = _normalize_match_value(query)
    values: list[Any] = [
        song.get("id"),
        song.get("source_id"),
        song.get("title"),
    ]
    values.extend(song.get("aliases") or [])
    source_ids = song.get("source_ids") if isinstance(song.get("source_ids"), dict) else {}
    values.extend(source_ids.values())
    values.extend(_candidate_song_id_values(song))
    return any(_normalize_match_value(value) == needle for value in values if value not in (None, ""))


def _normalize_match_value(value: Any) -> str:
    return str(value or "").strip().casefold().replace(" ", "")


def _numeric_text(value: Any) -> str:
    try:
        numeric_id = int(str(value))
    except (TypeError, ValueError):
        return ""
    return str(numeric_id) if numeric_id > 0 else ""


def _candidate_chart_type(value: Any) -> str:
    text = str(value or "").strip().lower()
    if text in {"dx"}:
        return "dx"
    if text in {"standard", "st", "sd"}:
        return "standard"
    return ""


def _candidate_base_id(song: dict[str, Any]) -> str:
    for value in (song.get("id"), song.get("source_id")):
        numeric_id = _numeric_text(value)
        if numeric_id:
            return numeric_id
    source_ids = song.get("source_ids") if isinstance(song.get("source_ids"), dict) else {}
    for value in source_ids.values():
        numeric_id = _numeric_text(value)
        if numeric_id:
            return numeric_id
    return ""


def _candidate_chart_ids(song: dict[str, Any]) -> dict[str, str]:
    base_id = _candidate_base_id(song)
    ids: dict[str, str] = {}
    charts = song.get("matched_charts") if isinstance(song.get("matched_charts"), list) else []
    for chart in charts:
        if not isinstance(chart, dict):
            continue
        chart_type = _candidate_chart_type(chart.get("chart_type"))
        if not chart_type or chart_type in ids:
            continue
        chart_id = ""
        for key in ("chart_id", "music_id", "musicId", "internal_id"):
            chart_id = _numeric_text(chart.get(key))
            if chart_id:
                break
        if not chart_id and base_id:
            chart_id = str(int(base_id) + 10000) if chart_type == "dx" and int(base_id) < 10000 else base_id
        if chart_id and chart_type == "dx" and int(chart_id) < 10000:
            chart_id = str(int(chart_id) + 10000)
        if chart_id:
            ids[chart_type] = chart_id

    raw_types = song.get("available_chart_types") if isinstance(song.get("available_chart_types"), list) else []
    for raw_type in raw_types:
        chart_type = _candidate_chart_type(raw_type)
        if not chart_type or chart_type in ids or not base_id:
            continue
        ids[chart_type] = str(int(base_id) + 10000) if chart_type == "dx" and int(base_id) < 10000 else base_id
    return dict(sorted(ids.items(), key=lambda item: {"standard": 0, "dx": 1}.get(item[0], 9)))


def _candidate_song_id_values(song: dict[str, Any]) -> list[str]:
    values = [_candidate_base_id(song)]
    values.extend(_candidate_chart_ids(song).values())
    return [value for value in dict.fromkeys(values) if value]


def format_candidate_song_id(song: dict[str, Any]) -> str:
    chart_ids = _candidate_chart_ids(song)
    if chart_ids:
        unique_ids = list(dict.fromkeys(chart_ids.values()))
        if len(unique_ids) == 1:
            return unique_ids[0]
        labels = {"standard": "ST", "dx": "DX"}
        return " / ".join(f"{labels.get(chart_type, chart_type.upper())}#{chart_id}" for chart_type, chart_id in chart_ids.items())
    return _candidate_base_id(song) or str(song.get("id") or song.get("source_id") or "-")


def build_music_info_render_arguments(
    song: dict[str, Any],
    query: str,
    base_arguments: dict[str, Any],
    search_arguments: dict[str, Any] | None = None,
) -> dict[str, Any]:
    args = dict(base_arguments)
    args["query"] = query
    search_args = search_arguments or {}
    music_id = search_args.get("id") or search_args.get("music_id") or search_args.get("musicId")
    if music_id not in (None, "") and re.fullmatch(r"\d+", str(music_id).strip()):
        args["music_id"] = str(music_id).strip()
    image_name = song.get("image_name") or song.get("imageName")
    if image_name:
        args["image_name"] = image_name
    return args


def build_random_music_info_render_arguments(
    song: dict[str, Any],
    base_arguments: dict[str, Any],
    random_arguments: dict[str, Any] | None = None,
) -> dict[str, Any]:
    args = dict(base_arguments)
    query = str(song.get("title") or song.get("id") or song.get("source_id") or "").strip()
    if query:
        args["query"] = query
    music_id = _candidate_base_id(song)
    if music_id:
        args["music_id"] = music_id
    image_name = song.get("image_name") or song.get("imageName")
    if image_name:
        args["image_name"] = image_name
    random_args = random_arguments or {}
    song_type = random_args.get("song_type")
    if song_type in {"dx", "standard"}:
        args["songType"] = song_type
    return args


def build_missing_search_music_info_render_arguments(command: DirectCommand) -> dict[str, Any] | None:
    if command.render_tool_name != "render_maimai_music_info":
        return None
    search_args = command.search_arguments or {}
    music_id = search_args.get("id") or search_args.get("music_id") or search_args.get("musicId")
    if music_id in (None, ""):
        return None
    music_id_text = str(music_id).strip()
    if not re.fullmatch(r"\d+", music_id_text):
        return None
    args = dict(command.render_arguments)
    args["music_id"] = music_id_text
    return args


def format_search_candidates(result: dict[str, Any], *, limit: int = 5) -> str:
    if result.get("isError"):
        return extract_mcp_text(result).strip() or "查歌失败。"
    payload = parse_json_content(result)
    songs = payload.get("songs") if isinstance(payload.get("songs"), list) else []
    if not songs:
        return "没有找到匹配曲目。"

    lines = ["匹配到多个曲目，请指定更精确的曲名或 ID："]
    for index, song in enumerate([s for s in songs if isinstance(s, dict)][:limit], 1):
        sid = format_candidate_song_id(song)
        title = song.get("title") or "-"
        artist = song.get("artist") or "-"
        lines.append(f"{index}. {title} | ID {sid} | {artist}")
    return "\n".join(lines)


def format_random_candidates(result: dict[str, Any]) -> str:
    if result.get("isError"):
        return extract_mcp_text(result).strip() or "随机曲目失败。"
    return "没有找到符合条件的随机曲目。"


class DirectMcpClient:
    def __init__(self, config: DirectMcpConfig):
        self.config = config
        self._next_id = 1

    async def call_tool(self, server: str, tool_name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        if server == "search":
            module = self.config.search_module
            timeout = self.config.timeout_seconds
        elif server == "group":
            module = self.config.group_module
            timeout = self.config.timeout_seconds
        elif server == "upload":
            module = self.config.upload_module
            timeout = self.config.upload_timeout_seconds
        else:
            module = self.config.render_module
            timeout = self.config.timeout_seconds
        return await self._call_stdio(module, tool_name, arguments, timeout=timeout)

    async def _call_stdio(self, module: str, tool_name: str, arguments: dict[str, Any], *, timeout: float) -> dict[str, Any]:
        message_id = self._next_id
        self._next_id += 1
        message = {
            "jsonrpc": "2.0",
            "id": message_id,
            "method": "tools/call",
            "params": {"name": tool_name, "arguments": arguments},
        }
        command = [*self.config.python_command, "-m", module]
        env = self._process_env()
        try:
            process = await asyncio.create_subprocess_exec(
                *command,
                cwd=self.config.project_cwd,
                env=env,
                stdin=asyncio.subprocess.PIPE,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
        except FileNotFoundError as exc:
            raise DirectRenderError(f"启动 MCP 失败: {command[0]} 不存在") from exc

        payload = (json.dumps(message, ensure_ascii=False) + "\n").encode("utf-8")
        try:
            stdout, stderr = await asyncio.wait_for(
                process.communicate(payload),
                timeout=timeout,
            )
        except asyncio.TimeoutError as exc:
            process.kill()
            await process.wait()
            raise DirectRenderError(f"MCP 调用超时: {tool_name}") from exc

        response = _load_json_response(stdout, message_id)
        if response is None:
            err = stderr.decode("utf-8", errors="replace").strip()
            raise DirectRenderError(err or f"MCP 未返回 JSON 响应: {tool_name}")
        if "error" in response:
            error = response.get("error") or {}
            raise DirectRenderError(str(error.get("message") or error))
        result = response.get("result")
        if not isinstance(result, dict):
            raise DirectRenderError(f"MCP 返回格式异常: {tool_name}")
        return result

    def _process_env(self) -> dict[str, str]:
        env = os.environ.copy()
        env.update(default_mcp_env(self.config.project_cwd))
        env.update(self.config.env)
        return env


def _load_json_response(stdout: bytes, message_id: int) -> dict[str, Any] | None:
    for raw_line in stdout.decode("utf-8", errors="replace").splitlines():
        raw_line = raw_line.strip()
        if not raw_line:
            continue
        try:
            parsed = json.loads(raw_line)
        except json.JSONDecodeError:
            continue
        if isinstance(parsed, dict) and parsed.get("id") == message_id:
            return parsed
    return None


def direct_mcp_config_from_mapping(config: Any) -> DirectMcpConfig:
    data_dir = str(_config_value(config, "direct_render_data_dir", DEFAULT_DATA_DIR) or DEFAULT_DATA_DIR)
    project_cwd = str(_config_value(config, "direct_render_project_cwd", f"{data_dir}/maimai-mcp") or DEFAULT_PROJECT_DIR)
    timeout = float(_config_value(config, "direct_render_timeout_seconds", 90) or 90)
    upload_timeout = float(_config_value(config, "direct_render_upload_timeout_seconds", 300) or 300)
    python_value = str(_config_value(config, "direct_render_python", "python") or "python")
    python_command = tuple(shlex.split(python_value)) or ("python",)
    env = _config_env(config, data_dir, project_cwd)
    return DirectMcpConfig(
        python_command=python_command,
        project_cwd=project_cwd,
        render_module=str(_config_value(config, "direct_render_render_module", "maimaidx_render_mcp.server")),
        search_module=str(_config_value(config, "direct_render_search_module", "maimai_mcp.server")),
        group_module=str(_config_value(config, "direct_render_group_module", "group_b50_mcp.server")),
        upload_module=str(_config_value(config, "direct_render_upload_module", "maimai_update_mcp.server")),
        timeout_seconds=timeout,
        upload_timeout_seconds=upload_timeout,
        env=env,
    )


def default_mcp_env(project_cwd: str = DEFAULT_PROJECT_DIR) -> dict[str, str]:
    return {
        "DIVING_FISH_MCP_TOKEN_FILE": DEFAULT_SECRET_FILE,
        "PLAYER_CACHE_DIR": DEFAULT_PLAYER_CACHE_DIR,
        "QQ_IDENTITY_CACHE_DIR": DEFAULT_QQ_IDENTITY_CACHE_DIR,
        "MAIMAI_IMPORT_TOKEN_BINDINGS_FILE": f"{DEFAULT_DATA_DIR}/maimai-config/.maimai-import-token-bindings.json",
        "MAIMAI_UPDATE_RECORDS_OUTPUT_DIR": f"{DEFAULT_DATA_DIR}/maimai-record-imports",
        "MAIMAI_LOCAL_SEARCH_MCP_ARGS": json.dumps(["-m", "maimai_mcp.server"]),
        "MAIMAI_LOCAL_SEARCH_MCP_CWD": project_cwd,
        "MAIMAIDX_RENDER_OUTPUT_DIR": DEFAULT_IMAGE_DIR,
        "MAIMAIDX_COVER_CACHE_DIR": DEFAULT_COVER_DIR,
        "MAIMAIDX_STATIC_DIR": DEFAULT_STATIC_DIR,
    }


def _config_env(config: Any, data_dir: str, project_cwd: str) -> dict[str, str]:
    defaults = {
        "DIVING_FISH_MCP_TOKEN_FILE": f"{data_dir}/maimai-config/.diving-fish-mcp-secrets.json",
        "PLAYER_CACHE_DIR": f"{data_dir}/player-cache",
        "QQ_IDENTITY_CACHE_DIR": f"{data_dir}/qq-identity-cache",
        "MAIMAI_IMPORT_TOKEN_BINDINGS_FILE": f"{data_dir}/maimai-config/.maimai-import-token-bindings.json",
        "MAIMAI_UPDATE_RECORDS_OUTPUT_DIR": f"{data_dir}/maimai-record-imports",
        "MAIMAI_LOCAL_SEARCH_MCP_ARGS": json.dumps(["-m", "maimai_mcp.server"]),
        "MAIMAI_LOCAL_SEARCH_MCP_CWD": project_cwd,
        "MAIMAIDX_RENDER_OUTPUT_DIR": f"{data_dir}/maimai-images",
        "MAIMAIDX_COVER_CACHE_DIR": f"{data_dir}/maimai-covers",
        "MAIMAIDX_STATIC_DIR": f"{data_dir}/maimai-yuzu-static/Resource/static",
    }
    overrides = {
        "DIVING_FISH_MCP_TOKEN_FILE": "direct_render_token_file",
        "PLAYER_CACHE_DIR": "direct_render_player_cache_dir",
        "QQ_IDENTITY_CACHE_DIR": "direct_render_qq_identity_cache_dir",
        "MAIMAI_IMPORT_TOKEN_BINDINGS_FILE": "direct_render_import_token_bindings_file",
        "MAIMAI_UPDATE_RECORDS_OUTPUT_DIR": "direct_render_update_records_output_dir",
        "MAIMAIDX_RENDER_OUTPUT_DIR": "direct_render_output_dir",
        "MAIMAIDX_COVER_CACHE_DIR": "direct_render_cover_cache_dir",
        "MAIMAIDX_STATIC_DIR": "direct_render_static_dir",
    }
    for env_key, config_key in overrides.items():
        value = _config_value(config, config_key, "")
        if value:
            defaults[env_key] = str(value)
    extra = _config_value(config, "direct_render_extra_env_json", "")
    if extra:
        try:
            parsed = json.loads(str(extra))
        except json.JSONDecodeError:
            parsed = {}
        if isinstance(parsed, dict):
            for key, value in parsed.items():
                if isinstance(key, str) and value is not None:
                    defaults[key] = str(value)
    return defaults


def _config_value(config: Any, key: str, default: Any) -> Any:
    if isinstance(config, dict):
        return config.get(key, default)
    return getattr(config, key, default)


def project_python_command() -> tuple[str, ...]:
    return (sys.executable,) if sys.executable else ("python",)


def mcp_project_exists(project_cwd: str) -> bool:
    return Path(project_cwd).exists()
