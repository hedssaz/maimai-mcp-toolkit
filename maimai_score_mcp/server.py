from __future__ import annotations

import json
import sys
import traceback
from datetime import datetime, timezone
from typing import Any

from . import __version__


SERVER_NAME = "maimai-score-query-mcp"


class ScoreLookupError(Exception):
    def __init__(
        self,
        message: str,
        *,
        code: str = "SCORE_LOOKUP_ERROR",
        data: Any = None,
    ) -> None:
        super().__init__(message)
        self.code = code
        self.data = data

    def to_dict(self) -> dict[str, Any]:
        return {"code": self.code, "message": str(self), "data": self.data}


QUERY_SCORE_BY_SONG_TOOL = {
    "name": "query_maimai_score_by_song",
    "description": (
        "用曲名、别名或曲目 ID 先调用 maimai-local-search 查出 Diving-Fish music_id，"
        "再调用水鱼 MCP 查询玩家 maimai 单曲成绩。底层按 ID 查分工具仅由本 MCP 内部使用。"
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "qq": {"type": "string", "description": "QQ 号。和 username/target 三选一。"},
            "username": {"type": "string", "description": "明确的水鱼 username。和 qq/target 三选一。"},
            "target": {
                "type": "string",
                "description": "QQ号、QQ昵称、群昵称/群名片、水鱼昵称或水鱼 username。和 qq/username 三选一。",
            },
            "songQuery": {"type": "string", "description": "曲名、别名或曲目 ID。"},
            "groupId": {"type": "string", "description": "可选，当前群号；用于 target 解析和身份显示。"},
            "difficulty": {
                "type": "string",
                "description": "可选，用于本地曲库筛选，例如 Master、Expert、紫、红。",
            },
            "songType": {
                "type": "string",
                "enum": ["standard", "SD", "sd", "dx", "DX", "utage", "宴"],
                "description": "可选，筛选 SD/DX/宴 谱面；不传时会查询该曲可用的全部 music_id。",
            },
            "searchLimit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 20,
                "description": "本地曲库最多返回多少个候选，默认 5。",
            },
            "timeoutMs": {
                "type": "integer",
                "minimum": 1000,
                "maximum": 30000,
                "description": "子 MCP 调用超时时间，默认 10000ms。",
            },
            "includeRaw": {"type": "boolean", "description": "是否保留水鱼单曲成绩原始响应。"},
        },
        "required": ["songQuery"],
        "additionalProperties": False,
    },
}

TOOLS = [QUERY_SCORE_BY_SONG_TOOL]


def normalize_identifier(value: Any) -> str | None:
    if isinstance(value, int):
        value = str(value)
    if not isinstance(value, str) or not value.strip():
        return None
    return value.strip()


def normalize_timeout_ms(value: Any) -> int:
    if value is None:
        return 10000
    if not isinstance(value, int) or value < 1000 or value > 30000:
        raise ScoreLookupError("timeoutMs 必须是 1000 到 30000 之间的整数。", code="INVALID_INPUT")
    return value


def normalize_search_limit(value: Any) -> int:
    if value is None:
        return 5
    if not isinstance(value, int) or value < 1 or value > 20:
        raise ScoreLookupError("searchLimit 必须是 1 到 20 之间的整数。", code="INVALID_INPUT")
    return value


def validate_lookup(arguments: dict[str, Any]) -> dict[str, str]:
    qq = normalize_identifier(arguments.get("qq"))
    username = normalize_identifier(arguments.get("username"))
    target = normalize_identifier(arguments.get("target"))
    provided = [value is not None for value in (qq, username, target)]
    if sum(provided) != 1:
        raise ScoreLookupError("必须且只能提供 qq、username、target 其中一个。", code="INVALID_INPUT")
    if qq:
        return {"qq": qq}
    if username:
        return {"username": username}
    return {"target": target or ""}


def query_maimai_score_by_song(arguments: dict[str, Any]) -> dict[str, Any]:
    lookup = validate_lookup(arguments)
    song_query = normalize_identifier(arguments.get("songQuery"))
    if not song_query:
        raise ScoreLookupError("必须提供 songQuery。", code="INVALID_INPUT")

    timeout_ms = normalize_timeout_ms(arguments.get("timeoutMs"))
    search_result = search_song(song_query, arguments, timeout_ms=timeout_ms)
    selected_song, music_ids = select_song_and_music_ids(search_result)

    score_results = []
    for music_id in music_ids:
        score_args: dict[str, Any] = {
            **lookup,
            "musicId": music_id,
            "timeoutMs": timeout_ms,
            "includeRaw": arguments.get("includeRaw") is True,
        }
        group_id = normalize_identifier(arguments.get("groupId"))
        if group_id:
            score_args["groupId"] = group_id
        from diving_fish_b50_mcp.server import (
            DivingFishError,
            query_maimai_song_score as _query_song_score_impl,
        )

        try:
            structured = _query_song_score_impl(score_args)
            score_results.append(
                {
                    "musicId": music_id,
                    "ok": True,
                    "result": structured,
                    "error": None,
                    "text": None,
                }
            )
        except DivingFishError as exc:
            score_results.append(
                {
                    "musicId": music_id,
                    "ok": False,
                    "result": None,
                    "error": exc.to_dict(),
                    "text": str(exc),
                }
            )

    success_count = sum(1 for item in score_results if item["ok"])
    result = {
        "source": "maimai-score-query",
        "requestedAt": datetime.now(timezone.utc).isoformat(),
        "lookup": lookup,
        "songQuery": song_query,
        "selectedSong": summarize_song(selected_song),
        "selection": song_selection_summary(search_result, selected_song),
        "musicIds": music_ids,
        "counts": {"requested": len(score_results), "success": success_count, "failure": len(score_results) - success_count},
        "scores": score_results,
    }
    if success_count == 0:
        raise ScoreLookupError("水鱼 MCP 未返回任何成功的单曲成绩。", code="SCORE_QUERY_FAILED", data=result)
    result["text"] = format_score_lookup(result)
    return result


def search_song(song_query: str, arguments: dict[str, Any], *, timeout_ms: int) -> dict[str, Any]:
    """复用 diving_fish_b50_mcp 已经封装好的 MaimaiLocalSearchClient 来调
    maimai-local-search 子进程，免去重复实现 JSON-RPC 桥接。"""
    from diving_fish_b50_mcp.server import MaimaiLocalSearchClient, extract_mcp_text

    search_args: dict[str, Any] = {
        "query": song_query,
        "limit": normalize_search_limit(arguments.get("searchLimit")),
        "format": "json",
    }
    difficulty = normalize_identifier(arguments.get("difficulty"))
    if difficulty:
        search_args["difficulty"] = difficulty
    song_type = normalize_identifier(arguments.get("songType"))
    if song_type:
        search_args["song_type"] = song_type

    with MaimaiLocalSearchClient(timeout_ms=max(timeout_ms, 30000)) as client:
        response = client.call_tool("search_maimai_songs", search_args)
    if response.get("isError"):
        raise ScoreLookupError(
            f"maimai-local-search 查询失败：{extract_mcp_text(response)}",
            code="SONG_SEARCH_FAILED",
        )
    text = extract_mcp_text(response)
    try:
        parsed = json.loads(text)
    except json.JSONDecodeError as exc:
        raise ScoreLookupError("maimai-local-search 未返回可解析的 JSON。", code="SONG_SEARCH_INVALID") from exc
    if not isinstance(parsed, dict):
        raise ScoreLookupError("maimai-local-search 返回结构不是对象。", code="SONG_SEARCH_INVALID")
    return parsed


def select_song_and_music_ids(search_result: dict[str, Any]) -> tuple[dict[str, Any], list[int]]:
    songs = search_result.get("songs")
    if not isinstance(songs, list) or not songs:
        raise ScoreLookupError("没有找到匹配曲目。", code="SONG_NOT_FOUND")
    invalid_candidates = []
    for song in songs:
        if not isinstance(song, dict):
            invalid_candidates.append(song)
            continue
        music_ids = extract_music_ids(song)
        if music_ids:
            return song, music_ids
        invalid_candidates.append(summarize_song(song))
    if invalid_candidates:
        raise ScoreLookupError("匹配曲目没有可用于水鱼查询的 music_id。", code="MUSIC_ID_NOT_FOUND", data=invalid_candidates[:10])
    raise ScoreLookupError("曲目结果结构无效。", code="SONG_SEARCH_INVALID")


def extract_music_ids(song: dict[str, Any]) -> list[int]:
    ids: list[int] = []
    for chart in song.get("matched_charts") or []:
        if not isinstance(chart, dict):
            continue
        add_music_id(ids, chart.get("fit_source_id"))
    if not ids:
        add_music_id(ids, song.get("id"))
    return ids[:10]


def add_music_id(values: list[int], value: Any) -> None:
    if isinstance(value, int):
        music_id = value
    elif isinstance(value, str) and value.strip().isdigit():
        music_id = int(value.strip())
    else:
        return
    if music_id > 0 and music_id not in values:
        values.append(music_id)


def summarize_song(song: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": song.get("id"),
        "sourceId": song.get("source_id"),
        "title": song.get("title"),
        "artist": song.get("artist"),
        "source": song.get("source"),
        "availableChartTypes": song.get("available_chart_types"),
        "aliases": (song.get("aliases") or [])[:20] if isinstance(song.get("aliases"), list) else [],
    }


def song_selection_summary(search_result: dict[str, Any], selected_song: dict[str, Any]) -> dict[str, Any]:
    songs = [song for song in search_result.get("songs") or [] if isinstance(song, dict)]
    selected_id = normalize_identifier(selected_song.get("id"))
    selected_rank = None
    for index, song in enumerate(songs, start=1):
        if normalize_identifier(song.get("id")) == selected_id:
            selected_rank = index
            break
    total_matches = search_result.get("total_matches")
    if not isinstance(total_matches, int):
        total_matches = len(songs)
    return {
        "autoSelected": total_matches > 1,
        "selectedRank": selected_rank,
        "totalMatches": total_matches,
        "truncated": search_result.get("truncated") is True,
        "candidates": [summarize_song(song) for song in songs[:5]],
    }


def format_score_lookup(result: dict[str, Any]) -> str:
    song = result.get("selectedSong") if isinstance(result.get("selectedSong"), dict) else {}
    selection = result.get("selection") if isinstance(result.get("selection"), dict) else {}
    lookup = result.get("lookup") if isinstance(result.get("lookup"), dict) else {}
    target = lookup.get("qq") or lookup.get("username") or lookup.get("target") or "未知"
    lines = [
        "maimai 单曲成绩查询",
        f"目标: {target}",
        f"曲目: {song.get('title') or result.get('songQuery')} / ID: {', '.join(str(item) for item in result.get('musicIds') or [])}",
        f"成功: {result.get('counts', {}).get('success', 0)}，失败: {result.get('counts', {}).get('failure', 0)}",
    ]
    if selection.get("autoSelected"):
        lines.insert(
            3,
            f"匹配到 {selection.get('totalMatches')} 首，已自动选择最可能结果（第 {selection.get('selectedRank') or 1} 个候选）。",
        )
    for item in result.get("scores") or []:
        if not isinstance(item, dict):
            continue
        lines.append("")
        lines.extend(format_score_item(item))
    return "\n".join(lines)


def format_score_item(item: dict[str, Any]) -> list[str]:
    music_id = item.get("musicId")
    if not item.get("ok"):
        error_info = item.get("error") if isinstance(item.get("error"), dict) else {}
        message = error_info.get("message") or item.get("text") or "未知错误"
        return [f"music_id {music_id}: 查询失败 - {message}"]

    score = item.get("result") if isinstance(item.get("result"), dict) else {}
    player = score.get("player") if isinstance(score.get("player"), dict) else {}
    records = score.get("records") if isinstance(score.get("records"), list) else []
    lines = [f"music_id {music_id}: 返回 {len(records)} 条成绩"]
    player_bits = [
        f"昵称: {player.get('nickname')}" if player.get("nickname") else None,
        f"Rating: {player.get('rating')}" if player.get("rating") is not None else None,
        f"牌子: {player.get('plate')}" if player.get("plate") else None,
    ]
    player_line = " / ".join(item for item in player_bits if item)
    if player_line:
        lines.append(player_line)
    if not records:
        lines.append("未返回该曲成绩数据。")
        return lines
    for index, record in enumerate(records, start=1):
        if isinstance(record, dict):
            lines.append(format_score_record(record, index))
    return lines


def format_score_record(record: dict[str, Any], index: int) -> str:
    parts = [
        record.get("levelLabel"),
        record.get("level"),
        f"定数 {record.get('ds')}" if record.get("ds") is not None else None,
        f"{record.get('achievements')}%" if record.get("achievements") is not None else None,
        f"ra {record.get('ra')}" if record.get("ra") is not None else None,
        str(record.get("rate")).upper() if record.get("rate") else None,
        str(record.get("fc")).upper() if record.get("fc") else None,
        str(record.get("fs")).upper() if record.get("fs") else None,
        f"DX Score {record.get('dxScore')}" if record.get("dxScore") is not None else None,
    ]
    suffix = " / ".join(str(item) for item in parts if item)
    if suffix:
        suffix = f" - {suffix}"
    return f"{index}. [{record.get('type') or '?'}] {record.get('title') or '未知歌曲'}{suffix}"


def write_message(message: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(message, ensure_ascii=False, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def success(message_id: Any, result: dict[str, Any]) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": message_id, "result": result}


def error(message_id: Any, code: int, message: str, data: Any = None) -> dict[str, Any]:
    payload: dict[str, Any] = {"jsonrpc": "2.0", "id": message_id, "error": {"code": code, "message": message}}
    if data is not None:
        payload["error"]["data"] = data
    return payload


def handle_initialize(message_id: Any, params: dict[str, Any] | None) -> dict[str, Any]:
    requested_version = (params or {}).get("protocolVersion") or "2024-11-05"
    return success(
        message_id,
        {
            "protocolVersion": requested_version,
            "capabilities": {"tools": {"listChanged": False}},
            "serverInfo": {"name": SERVER_NAME, "version": __version__},
        },
    )


def handle_tool_call(message_id: Any, params: dict[str, Any] | None) -> dict[str, Any]:
    params = params or {}
    arguments = params.get("arguments") or {}
    if params.get("name") != QUERY_SCORE_BY_SONG_TOOL["name"]:
        return error(message_id, -32602, f"Unknown tool: {params.get('name')}")
    if not isinstance(arguments, dict):
        return success(message_id, {"content": [{"type": "text", "text": "arguments 必须是对象。"}], "isError": True})
    try:
        result = query_maimai_score_by_song(arguments)
        return success(
            message_id,
            {
                "content": [{"type": "text", "text": result["text"]}],
                "structuredContent": result,
                "isError": False,
            },
        )
    except ScoreLookupError as exc:
        return success(
            message_id,
            {
                "content": [{"type": "text", "text": str(exc)}],
                "structuredContent": {"error": exc.to_dict()},
                "isError": True,
            },
        )


def handle_request(message: dict[str, Any]) -> dict[str, Any] | None:
    message_id = message.get("id")
    method = message.get("method")
    params = message.get("params")
    if method == "initialize":
        return handle_initialize(message_id, params)
    if method == "tools/list":
        return success(message_id, {"tools": TOOLS})
    if method == "tools/call":
        return handle_tool_call(message_id, params)
    if method == "prompts/list":
        return success(message_id, {"prompts": []})
    if method == "resources/list":
        return success(message_id, {"resources": []})
    if method == "ping":
        return success(message_id, {})
    if message_id is None:
        return None
    return error(message_id, -32601, f"Method not found: {method}")


def main() -> None:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
            response = handle_request(message)
            if response is not None:
                write_message(response)
        except Exception as exc:
            traceback.print_exc(file=sys.stderr)
            write_message(error(None, -32603, "Internal error", str(exc)))


if __name__ == "__main__":
    main()
