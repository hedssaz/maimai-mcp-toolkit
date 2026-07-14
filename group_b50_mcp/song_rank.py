"""群内单曲成绩排行榜（group-rank-mcp 的 song-score feature）。

数据流：
  group_song_score_report(groupId, songQuery|musicId, ...)
    └─ resolve song → musicId (subprocess 调 maimai-local-search)
    └─ build_group_song_cache（缺/过期才跑）
         ├─ 群成员加载（复用 server.load_group_members_from_identity_cache / fetch_group_members）
         └─ 每人：先查 player_cache.records 缓存，缺/过期才 subprocess 调
                 diving_fish_api(operation=maimai_dev_player_records_get, query={qq})
                 拿到完整成绩 → 写 player_cache.records
    └─ 读 player_cache.records，按 musicId/levelIndex/songType 过滤每人最优成绩
    └─ 排序输出表格

缓存层级：
  player-cache/records/<qq>.json   ← 单人完整成绩，跨群复用（写入由 diving_fish_api 路径触发或本模块触发）
  group-song-cache/<groupId>/cache.json   ← 只存"哪些人是这个群的、什么时候刷的"指针，不复制成绩本体
"""

from __future__ import annotations

import json
import os
import shutil
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

from player_cache import (
    DAILY_RESET_HOUR_UTC,
    is_player_records_fresh,
    read_player_records,
    write_player_records,
)
from qq_identity_mcp.store import get_identity, resolve_identities

from . import job_runner
from .job_runner import (
    StaleRefreshJob,
    ensure_current_refresh_job,
    now_iso,
    read_job_status as _read_job_status,
    write_job_status as _write_job_status,
    write_refresh_progress as _write_refresh_progress,
)


SONG_FEATURE = "song_score"


class _RecordsBatchRequest:
    def __init__(
        self,
        qqs: list[str],
        *,
        timeout_ms: int,
        query_delay_ms: int,
        max_concurrency: int,
    ) -> None:
        self.qqs = qqs
        self.timeout_ms = timeout_ms
        self.query_delay_ms = query_delay_ms
        self.max_concurrency = max_concurrency
        self.done = threading.Event()
        self.results: dict[str, dict[str, Any]] = {}


_RECORDS_BATCH_LOCK = threading.Lock()
_RECORDS_BATCH_ACTIVE = False
_RECORDS_BATCH_REQUESTS: list[_RecordsBatchRequest] = []


def _daily_reset_time(now: datetime) -> datetime:
    reset = now.replace(hour=DAILY_RESET_HOUR_UTC, minute=0, second=0, microsecond=0)
    if now < reset:
        reset -= timedelta(days=1)
    return reset


# ============== 工具 schema ==============


GROUP_SONG_SCORE_REPORT_TOOL = {
    "name": "group_song_score_report",
    "description": (
        "按群号和某首歌输出群内单曲成绩排行榜。songQuery（曲名/别名）或 musicId 通常提供一个；"
        "仅 forceRefresh/预热单曲缓存时可以只传 groupId。"
        "提供 songQuery 时会先 subprocess 调 maimai-local-search 解析 music_id，再批量调"
        " diving_fish_api(maimai_dev_player_records_get) 拉全员完整成绩。缓存默认 1 天，"
        "和群 B50 缓存独立、不互踩。"
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "groupId": {"type": "string", "description": "QQ群号。"},
            "songQuery": {"type": "string", "description": "曲名、别名或曲目 ID。和 musicId 二选一。"},
            "musicId": {
                "oneOf": [{"type": "integer"}, {"type": "string"}],
                "description": "Diving-Fish music_id。和 songQuery 二选一。",
            },
            "levelIndex": {
                "type": "integer",
                "minimum": 0,
                "maximum": 4,
                "description": "可选，限定难度。0=Basic, 1=Advanced, 2=Expert, 3=Master, 4=Re:Master。不填时默认取该曲已有谱面的最高难度。",
            },
            "songType": {
                "type": "string",
                "enum": ["DX", "SD", "Standard"],
                "description": "可选，限定 DX/SD 谱面类型。不填不限制。",
            },
            "sortOrder": {
                "type": "string",
                "enum": ["asc", "desc"],
                "description": "排序方向，默认 desc（高分在前）。",
            },
            "sortBy": {
                "type": "string",
                "enum": ["achievements", "ra", "dxScore"],
                "description": "排序字段，默认 achievements。",
            },
            "outputLimit": {
                "type": "integer",
                "minimum": 1,
                "description": "筛选/排序后最多输出多少人。",
            },
            "startRank": {
                "type": "integer",
                "minimum": 1,
                "description": "可选，筛选/排序后从第几名开始输出，1-based，需和 endRank 一起使用。",
            },
            "endRank": {
                "type": "integer",
                "minimum": 1,
                "description": "可选，筛选/排序后输出到第几名，包含该名次，需和 startRank 一起使用。",
            },
            "achievementsMin": {"type": "number", "description": "可选，达成率下限。"},
            "achievementsMax": {"type": "number", "description": "可选，达成率上限。"},
            "forceRefresh": {
                "type": "boolean",
                "description": "忽略缓存重新拉取（一天内默认走缓存）。",
            },
            "napcatBaseUrl": {"type": "string", "description": "覆盖 NAPCAT_BASE_URL。"},
            "noCache": {"type": "boolean", "description": "传给 NapCat get_group_member_list 的 no_cache，默认 true。"},
            "timeoutMs": {
                "type": "integer",
                "minimum": 1000,
                "maximum": 60000,
                "description": "单次 HTTP 请求超时，默认 10000ms。",
            },
            "queryDelayMs": {
                "type": "integer",
                "minimum": 0,
                "maximum": 10000,
                "description": "每个水鱼查询之间的等待时间，默认 250ms。",
            },
            "maxConcurrency": {
                "type": "integer",
                "minimum": 1,
                "maximum": 20,
                "description": "完整成绩远端查询并发数，默认 3。多个群同时刷新时会先合并去重再按该并发数查询唯一 QQ。",
            },
            "batchSize": {
                "type": "integer",
                "minimum": 1,
                "maximum": 100,
                "description": "每批进度更新粒度，默认 5。",
            },
            "maxMembers": {
                "type": "integer",
                "minimum": 1,
                "description": "最多查询多少个群成员（测试/限流用）。",
            },
            "searchLimit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 20,
                "description": "maimai-local-search 检索 limit，默认 5。",
            },
        },
        "required": ["groupId"],
        "additionalProperties": False,
    },
}

GROUP_SONG_SCORE_MEMBER_RANK_TOOL = {
    "name": "group_song_score_member_rank",
    "description": (
        "查询某 QQ 或昵称在群内某首歌的成绩排名。复用 group_song_score_report 同一份群缓存。"
        "未提供群号时会尝试用 QQ 身份缓存推断唯一群。"
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "groupId": {"type": "string", "description": "QQ群号。可选。"},
            "qq": {"type": "string", "description": "要查询群内排名的 QQ 号。"},
            "target": {
                "type": "string",
                "description": "QQ号、QQ昵称、群昵称/群名片或水鱼昵称。未提供 qq 时会先用 QQ 身份缓存反查 QQ。",
            },
            "songQuery": GROUP_SONG_SCORE_REPORT_TOOL["inputSchema"]["properties"]["songQuery"],
            "musicId": GROUP_SONG_SCORE_REPORT_TOOL["inputSchema"]["properties"]["musicId"],
            "levelIndex": GROUP_SONG_SCORE_REPORT_TOOL["inputSchema"]["properties"]["levelIndex"],
            "songType": GROUP_SONG_SCORE_REPORT_TOOL["inputSchema"]["properties"]["songType"],
            "contextSize": {
                "type": "integer",
                "minimum": 0,
                "maximum": 10,
                "description": "目标排名上下展示多少人，默认 3。",
            },
            "forceRefresh": GROUP_SONG_SCORE_REPORT_TOOL["inputSchema"]["properties"]["forceRefresh"],
            "napcatBaseUrl": GROUP_SONG_SCORE_REPORT_TOOL["inputSchema"]["properties"]["napcatBaseUrl"],
            "noCache": GROUP_SONG_SCORE_REPORT_TOOL["inputSchema"]["properties"]["noCache"],
            "timeoutMs": GROUP_SONG_SCORE_REPORT_TOOL["inputSchema"]["properties"]["timeoutMs"],
            "queryDelayMs": GROUP_SONG_SCORE_REPORT_TOOL["inputSchema"]["properties"]["queryDelayMs"],
            "maxConcurrency": GROUP_SONG_SCORE_REPORT_TOOL["inputSchema"]["properties"]["maxConcurrency"],
            "batchSize": GROUP_SONG_SCORE_REPORT_TOOL["inputSchema"]["properties"]["batchSize"],
            "maxMembers": GROUP_SONG_SCORE_REPORT_TOOL["inputSchema"]["properties"]["maxMembers"],
            "searchLimit": GROUP_SONG_SCORE_REPORT_TOOL["inputSchema"]["properties"]["searchLimit"],
        },
        "required": [],
        "additionalProperties": False,
    },
}

GROUP_SONG_SCORE_CACHE_STATUS_TOOL = {
    "name": "group_song_score_cache_status",
    "description": "查看群单曲成绩缓存是否存在、是否超过 1 天、缓存路径。",
    "inputSchema": {
        "type": "object",
        "properties": {"groupId": {"type": "string", "description": "QQ群号。"}},
        "required": ["groupId"],
        "additionalProperties": False,
    },
}

GROUP_SONG_SCORE_JOB_STATUS_TOOL = {
    "name": "group_song_score_job_status",
    "description": "查看群单曲成绩后台刷新进度；完成后调本工具可直接读榜。",
    "inputSchema": {
        "type": "object",
        "properties": {"groupId": {"type": "string", "description": "QQ群号。"}},
        "required": ["groupId"],
        "additionalProperties": False,
    },
}

CLEAR_GROUP_SONG_SCORE_CACHE_TOOL = {
    "name": "clear_group_song_score_cache",
    "description": "清除指定群的单曲成绩缓存（不影响 player_cache.records 单人级缓存）。",
    "inputSchema": {
        "type": "object",
        "properties": {"groupId": {"type": "string", "description": "QQ群号。"}},
        "required": ["groupId"],
        "additionalProperties": False,
    },
}

TOOLS = [
    GROUP_SONG_SCORE_REPORT_TOOL,
    GROUP_SONG_SCORE_MEMBER_RANK_TOOL,
    GROUP_SONG_SCORE_CACHE_STATUS_TOOL,
    GROUP_SONG_SCORE_JOB_STATUS_TOOL,
    CLEAR_GROUP_SONG_SCORE_CACHE_TOOL,
]


# ============== 缓存路径 ==============


def get_song_cache_dir() -> Path:
    configured = os.environ.get("GROUP_SONG_CACHE_DIR")
    if configured:
        return Path(configured).expanduser().resolve()
    return (Path.cwd() / "group-song-cache").resolve()


def song_group_cache_dir(group_id: str) -> Path:
    return get_song_cache_dir() / group_id


def song_cache_json_path(group_id: str) -> Path:
    return song_group_cache_dir(group_id) / "cache.json"


def song_job_status_path(group_id: str) -> Path:
    return song_group_cache_dir(group_id) / "job_status.json"


def song_read_cache(group_id: str) -> dict[str, Any] | None:
    path = song_cache_json_path(group_id)
    if not path.exists():
        return None
    try:
        parsed = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return None
    return parsed if isinstance(parsed, dict) else None


def song_write_cache(group_id: str, cache: dict[str, Any]) -> None:
    job_runner.write_json_atomic(song_cache_json_path(group_id), cache)


def song_read_job_status(group_id: str) -> dict[str, Any] | None:
    return _read_job_status(song_job_status_path(group_id))


def song_write_job_status(group_id: str, status: dict[str, Any]) -> None:
    _write_job_status(song_job_status_path(group_id), status)


def song_clear_cache(group_id: str) -> dict[str, Any]:
    base = song_group_cache_dir(group_id)
    with job_runner.STATE_LOCK:
        existed = base.exists()
        if existed:
            shutil.rmtree(base)
    return {"groupId": group_id, "feature": SONG_FEATURE, "cleared": existed, "cacheDir": str(base)}


def song_cache_age_seconds(cache: dict[str, Any] | None) -> float | None:
    if not cache:
        return None
    fetched_at = cache.get("fetchedAt")
    if not isinstance(fetched_at, str):
        return None
    try:
        parsed = datetime.fromisoformat(fetched_at)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    return (datetime.now(timezone.utc) - parsed).total_seconds()


def song_is_cache_fresh(cache: dict[str, Any] | None) -> bool:
    if not cache:
        return False
    fetched_at = cache.get("fetchedAt")
    if not isinstance(fetched_at, str):
        return False
    try:
        parsed = datetime.fromisoformat(fetched_at)
    except ValueError:
        return False
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    return parsed >= _daily_reset_time(datetime.now(timezone.utc))


# ============== 参数归一化 ==============


def _normalize_identifier(value: Any, field_name: str | None = None) -> str:
    if isinstance(value, int):
        value = str(value)
    if not isinstance(value, str) or not value.strip():
        if field_name:
            from .server import GroupB50Error
            raise GroupB50Error(f"必须提供 {field_name}。", code="INVALID_INPUT")
        return ""
    return value.strip()


def _normalize_optional_str(value: Any) -> str | None:
    if isinstance(value, int):
        value = str(value)
    if not isinstance(value, str) or not value.strip():
        return None
    return value.strip()


def _normalize_optional_int(value: Any, field_name: str, *, low: int | None = None, high: int | None = None) -> int | None:
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, int):
        from .server import GroupB50Error
        raise GroupB50Error(f"{field_name} 必须是整数。", code="INVALID_INPUT")
    if low is not None and value < low:
        from .server import GroupB50Error
        raise GroupB50Error(f"{field_name} 必须 >= {low}。", code="INVALID_INPUT")
    if high is not None and value > high:
        from .server import GroupB50Error
        raise GroupB50Error(f"{field_name} 必须 <= {high}。", code="INVALID_INPUT")
    return value


def _normalize_music_id(value: Any) -> int | None:
    if value is None or value == "":
        return None
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value if value > 0 else None
    if isinstance(value, str):
        text = value.strip().lstrip("Ii").lstrip("Dd")
        try:
            mid = int(text)
        except ValueError:
            return None
        return mid if mid > 0 else None
    return None


# ============== 工具入口 ==============


def group_song_score_report(arguments: dict[str, Any]) -> dict[str, Any]:
    from .server import GroupB50Error

    group_id = _normalize_identifier(arguments.get("groupId"), "groupId")
    level_index = _normalize_optional_int(arguments.get("levelIndex"), "levelIndex", low=0, high=4)
    music_id, level_index = _resolve_optional_song_target_from_arguments(arguments, level_index)

    sort_by = arguments.get("sortBy") or "achievements"
    if sort_by not in {"achievements", "ra", "dxScore"}:
        raise GroupB50Error("sortBy 必须是 achievements / ra / dxScore。", code="INVALID_INPUT")
    sort_order = arguments.get("sortOrder") or "desc"
    if sort_order not in {"asc", "desc"}:
        raise GroupB50Error("sortOrder 必须是 asc 或 desc。", code="INVALID_INPUT")

    song_type = _normalize_song_type(arguments.get("songType"))
    output_limit = _normalize_optional_int(arguments.get("outputLimit"), "outputLimit", low=1)
    start_rank = _normalize_optional_int(arguments.get("startRank"), "startRank", low=1)
    end_rank = _normalize_optional_int(arguments.get("endRank"), "endRank", low=1)
    if (start_rank is None) != (end_rank is None):
        raise GroupB50Error("startRank 和 endRank 必须同时提供。", code="INVALID_INPUT")
    if start_rank is not None and end_rank is not None and start_rank > end_rank:
        raise GroupB50Error("startRank 不能大于 endRank。", code="INVALID_INPUT")
    ach_min = arguments.get("achievementsMin")
    ach_max = arguments.get("achievementsMax")
    if ach_min is not None and not isinstance(ach_min, (int, float)):
        raise GroupB50Error("achievementsMin 必须是数字。", code="INVALID_INPUT")
    if ach_max is not None and not isinstance(ach_max, (int, float)):
        raise GroupB50Error("achievementsMax 必须是数字。", code="INVALID_INPUT")

    cache = song_read_cache(group_id)
    force_refresh = arguments.get("forceRefresh") is True
    refresh_reason: str | None = None
    if force_refresh:
        song_clear_cache(group_id)
        cache = None
        refresh_reason = "forceRefresh"
    elif cache is not None and not song_is_cache_fresh(cache):
        song_clear_cache(group_id)
        cache = None
        refresh_reason = "stale"

    if cache is None:
        job = _start_song_refresh_job(group_id, arguments, refresh_reason=refresh_reason or "miss")
        status = song_cache_status({"groupId": group_id})
        text = _format_song_job_started_text(group_id, music_id, job, status)
        return {
            "groupId": group_id,
            "musicId": music_id,
            "levelIndex": level_index,
            "songType": song_type,
            "sortBy": sort_by,
            "sortOrder": sort_order,
            "outputLimit": output_limit,
            "startRank": start_rank,
            "endRank": end_rank,
            "cache": status,
            "job": job,
            "cacheRefreshReason": refresh_reason or "miss",
            "text": text,
            "data": None,
        }

    if music_id is None:
        status = song_cache_status({"groupId": group_id})
        cache_refresh_reason = cache.get("cacheRefreshReason") or "hit"
        return {
            "groupId": group_id,
            "musicId": None,
            "levelIndex": level_index,
            "songType": song_type,
            "sortBy": sort_by,
            "sortOrder": sort_order,
            "outputLimit": output_limit,
            "startRank": start_rank,
            "endRank": end_rank,
            "cache": status,
            "cacheRefreshReason": cache_refresh_reason,
            "text": _format_song_cache_only_text(group_id, cache, cache_refresh_reason),
            "data": cache,
        }

    cache_refresh_reason = cache.get("cacheRefreshReason") or "hit"
    rows, level_index = _collect_song_rows_with_auto_level(
        cache,
        music_id=music_id,
        level_index=level_index,
        song_type=song_type,
        achievements_min=ach_min,
        achievements_max=ach_max,
    )
    rows = _sort_song_rows(rows, sort_by=sort_by, sort_order=sort_order)
    matched = len(rows)
    rows = _apply_rank_window(rows, output_limit, start_rank=start_rank, end_rank=end_rank)

    text = _format_song_report_text(
        cache,
        rows=rows,
        music_id=music_id,
        level_index=level_index,
        song_type=song_type,
        sort_by=sort_by,
        sort_order=sort_order,
        matched_count=matched,
        output_limit=output_limit,
        start_rank=start_rank,
        end_rank=end_rank,
        cache_refresh_reason=cache_refresh_reason,
    )
    return {
        "groupId": group_id,
        "musicId": music_id,
        "levelIndex": level_index,
        "songType": song_type,
        "sortBy": sort_by,
        "sortOrder": sort_order,
        "outputLimit": output_limit,
        "startRank": start_rank,
        "endRank": end_rank,
        "matchedCount": matched,
        "rows": rows,
        "cache": song_cache_status({"groupId": group_id}),
        "cacheRefreshReason": cache_refresh_reason,
        "text": text,
        "data": cache,
    }


def group_song_score_member_rank(arguments: dict[str, Any]) -> dict[str, Any]:
    from .server import GroupB50Error

    qq = _normalize_optional_str(arguments.get("qq"))
    target = _normalize_optional_str(arguments.get("target"))
    group_id = _normalize_optional_str(arguments.get("groupId"))

    if not qq and target:
        resolved = _resolve_target_to_qq(target, group_id=group_id)
        qq = resolved["qq"]
        if not group_id:
            group_id = resolved["groupId"]

    if not qq:
        raise GroupB50Error("必须提供 qq 或 target。", code="INVALID_INPUT")
    if not group_id:
        group_id = _infer_group_id(qq)

    level_index = _normalize_optional_int(arguments.get("levelIndex"), "levelIndex", low=0, high=4)
    music_id, level_index = _resolve_song_target_from_arguments(arguments, level_index)
    song_type = _normalize_song_type(arguments.get("songType"))
    context_size = _normalize_optional_int(arguments.get("contextSize"), "contextSize", low=0, high=10) or 3

    cache = song_read_cache(group_id)
    force_refresh = arguments.get("forceRefresh") is True
    refresh_reason: str | None = None
    if force_refresh:
        song_clear_cache(group_id)
        cache = None
        refresh_reason = "forceRefresh"
    elif cache is not None and not song_is_cache_fresh(cache):
        song_clear_cache(group_id)
        cache = None
        refresh_reason = "stale"

    if cache is None:
        job = _start_song_refresh_job(group_id, arguments, refresh_reason=refresh_reason or "miss")
        status = song_cache_status({"groupId": group_id})
        text = _format_member_rank_job_started_text(group_id, qq, music_id, job, status)
        return {
            "groupId": group_id,
            "qq": qq,
            "musicId": music_id,
            "levelIndex": level_index,
            "songType": song_type,
            "contextSize": context_size,
            "cache": status,
            "job": job,
            "cacheRefreshReason": refresh_reason or "miss",
            "text": text,
            "data": None,
        }

    rows, level_index = _collect_song_rows_with_auto_level(
        cache,
        music_id=music_id,
        level_index=level_index,
        song_type=song_type,
        achievements_min=None,
        achievements_max=None,
    )
    desc_rows = _sort_song_rows(rows, sort_by="achievements", sort_order="desc")
    asc_rows = _sort_song_rows(rows, sort_by="achievements", sort_order="asc")
    target_desc_index = next((i for i, r in enumerate(desc_rows) if str(r.get("userId")) == qq), None)
    target_asc_index = next((i for i, r in enumerate(asc_rows) if str(r.get("userId")) == qq), None)
    if target_desc_index is None or target_asc_index is None:
        text = _format_member_rank_missing_text(cache, qq, music_id)
        return {
            "groupId": group_id,
            "qq": qq,
            "musicId": music_id,
            "found": False,
            "totalRanked": len(desc_rows),
            "text": text,
            "cache": song_cache_status({"groupId": group_id}),
            "data": None,
        }
    rank_desc = target_desc_index + 1
    rank_asc = target_asc_index + 1
    desc_lo = max(0, target_desc_index - context_size)
    desc_hi = min(len(desc_rows), target_desc_index + context_size + 1)
    asc_lo = max(0, target_asc_index - context_size)
    asc_hi = min(len(asc_rows), target_asc_index + context_size + 1)
    context = [{**desc_rows[i], "_rank": i + 1} for i in range(desc_lo, desc_hi)]
    reverse_context = [{**asc_rows[i], "_rank": i + 1} for i in range(asc_lo, asc_hi)]
    total_ranked = len(desc_rows)
    rank_info = {
        "rankDesc": rank_desc,
        "rankAsc": rank_asc,
        "totalRanked": total_ranked,
        "higherCount": rank_desc - 1,
        "lowerCount": total_ranked - rank_desc,
    }
    text = _format_member_rank_text(
        cache,
        desc_rows[target_desc_index],
        rank_info,
        context,
        music_id,
    )
    return {
        "groupId": group_id,
        "qq": qq,
        "musicId": music_id,
        "found": True,
        "rank": rank_desc,
        "reverseRank": rank_asc,
        "rankInfo": rank_info,
        "totalRanked": total_ranked,
        "target": desc_rows[target_desc_index],
        "context": context,
        "reverseContext": reverse_context,
        "cache": song_cache_status({"groupId": group_id}),
        "text": text,
        "data": {"target": desc_rows[target_desc_index], "rank": rank_desc, "rankInfo": rank_info},
    }


def song_cache_status(arguments: dict[str, Any]) -> dict[str, Any]:
    group_id = _normalize_identifier(arguments.get("groupId"), "groupId")
    cache = song_read_cache(group_id)
    age = song_cache_age_seconds(cache)
    return {
        "groupId": group_id,
        "feature": SONG_FEATURE,
        "cacheExists": cache is not None,
        "fresh": song_is_cache_fresh(cache),
        "ageSeconds": age,
        "nextResetAt": (_daily_reset_time(datetime.now(timezone.utc)) + timedelta(days=1)).isoformat(),
        "fetchedAt": cache.get("fetchedAt") if cache else None,
        "memberCount": cache.get("memberCount") if cache else None,
        "successCount": cache.get("successCount") if cache else None,
        "skippedCount": cache.get("skippedCount") if cache else None,
        "cacheHitCount": cache.get("cacheHitCount") if cache else None,
        "sharedFetchCount": cache.get("sharedFetchCount") if cache else None,
        "job": song_read_job_status(group_id),
        "cachePath": str(song_cache_json_path(group_id)),
    }


def song_job_status(arguments: dict[str, Any]) -> dict[str, Any]:
    group_id = _normalize_identifier(arguments.get("groupId"), "groupId")
    job = song_read_job_status(group_id)
    cache = song_read_cache(group_id)
    return {
        "groupId": group_id,
        "feature": SONG_FEATURE,
        "job": job,
        "cache": song_cache_status({"groupId": group_id}),
        "text": _format_job_status_text(group_id, job, cache),
        "data": cache,
    }


# ============== 后台刷新 ==============


def _start_song_refresh_job(
    group_id: str,
    arguments: dict[str, Any],
    *,
    refresh_reason: str,
) -> dict[str, Any]:
    def runner(job_id: str) -> None:
        _run_song_refresh_job(group_id, job_id, arguments, refresh_reason)

    return job_runner.start_refresh_job(
        feature=SONG_FEATURE,
        group_id=group_id,
        status_path=song_job_status_path(group_id),
        refresh_reason=refresh_reason,
        runner=runner,
        start_message="单曲成绩榜后台刷新已启动，正在拉群成员并等待跨群合并去重后并发调水鱼 /dev/player/records。",
        thread_name=f"group-rank-refresh-{SONG_FEATURE}-{group_id}",
    )


def _run_song_refresh_job(
    group_id: str,
    job_id: str,
    arguments: dict[str, Any],
    refresh_reason: str,
) -> None:
    from .server import (
        GroupB50Error,
        fetch_group_members,
        is_transient_b50_error,
        load_group_members_from_identity_cache,
        normalize_napcat_base_url,
        update_identity_cache_from_members,
    )

    try:
        timeout_ms = arguments.get("timeoutMs") if isinstance(arguments.get("timeoutMs"), int) else 10000
        query_delay_ms = arguments.get("queryDelayMs") if isinstance(arguments.get("queryDelayMs"), int) else 250
        max_concurrency = _normalize_song_max_concurrency(arguments.get("maxConcurrency"))
        batch_size = arguments.get("batchSize") if isinstance(arguments.get("batchSize"), int) else 5
        max_members = arguments.get("maxMembers") if isinstance(arguments.get("maxMembers"), int) else None

        members = load_group_members_from_identity_cache(group_id)
        if members is None:
            members = fetch_group_members(
                group_id,
                napcat_base_url=normalize_napcat_base_url(arguments.get("napcatBaseUrl")),
                no_cache=arguments.get("noCache") is not False,
                timeout_ms=timeout_ms,
            )
        if max_members is not None:
            members = members[:max_members]
        update_identity_cache_from_members(group_id, members)

        qqs = [str(m["userId"]) for m in members]
        member_by_qq = {str(m["userId"]): m for m in members}
        results: list[dict[str, Any]] = []
        skipped_count = 0
        cache_hit_count = 0
        shared_fetch_count = 0
        transient_failures: list[dict[str, Any]] = []
        status_path = song_job_status_path(group_id)

        _write_refresh_progress(
            status_path=status_path,
            job_id=job_id,
            processed=0,
            total=len(qqs),
            cached_count=0,
            skipped_count=0,
            transient_failure_count=0,
            message="已拉取群成员，等待与其他群刷新任务合并去重后并发查询完整成绩。",
        )

        records_by_qq = _get_player_records_batch_for_group_refresh(
            qqs,
            timeout_ms=timeout_ms,
            query_delay_ms=query_delay_ms,
            max_concurrency=max_concurrency,
        )

        processed = 0
        for qq in qqs:
            records_item = records_by_qq.get(qq)
            if not isinstance(records_item, dict):
                skipped_count += 1
                processed += 1
                continue
            if records_item.get("ok") is not True:
                error = records_item.get("error")
                exc = error if isinstance(error, GroupB50Error) else None
                if exc is None:
                    exc = GroupB50Error(
                        str(error) if error else "完整成绩查询失败。",
                        code="SONG_RECORDS_ERROR",
                    )
                if is_transient_b50_error({"code": exc.code, "status": exc.status}):
                    transient_failures.append(
                        {
                            "userId": qq,
                            "displayName": member_by_qq.get(qq, {}).get("displayName"),
                            "error": {"code": exc.code, "message": str(exc), "status": exc.status},
                        }
                    )
                else:
                    skipped_count += 1
                processed += 1
                continue

            records_doc = records_item.get("records")
            if not isinstance(records_doc, dict):
                skipped_count += 1
                processed += 1
                continue

            records_source = str(records_item.get("source") or "remote")
            member = member_by_qq.get(qq, {"userId": qq, "displayName": qq})
            entry = _build_member_result(qq, member, records_doc, group_id, cache_hit=records_source == "cache")
            if entry is not None:
                results.append(entry)
                if records_source == "cache":
                    cache_hit_count += 1
                elif records_source == "shared":
                    shared_fetch_count += 1
            else:
                skipped_count += 1

            processed += 1
            if processed % batch_size == 0 or processed == len(qqs):
                _write_refresh_progress(
                    status_path=status_path,
                    job_id=job_id,
                    processed=processed,
                    total=len(qqs),
                    cached_count=len(results),
                    skipped_count=skipped_count,
                    transient_failure_count=len(transient_failures),
                    message=(
                        f"已完成 {processed}/{len(qqs)} 人（缓存命中 {cache_hit_count}，"
                        f"跨群复用 {shared_fetch_count}）。"
                    ),
                )
        if transient_failures:
            raise GroupB50Error(
                f"单曲成绩刷新存在 {len(transient_failures)} 个临时失败，已放弃缓存。请稍后 forceRefresh 重试。",
                code="SONG_TRANSIENT_FAILURE",
            )

        cache = {
            "groupId": group_id,
            "feature": SONG_FEATURE,
            "fetchedAt": now_iso(),
            "nextResetAt": (_daily_reset_time(datetime.now(timezone.utc)) + timedelta(days=1)).isoformat(),
            "cacheScope": "group_members_with_full_records_ref",
            "memberCount": len(members),
            "successCount": len(results),
            "failureCount": 0,
            "skippedCount": skipped_count,
            "cacheHitCount": cache_hit_count,
            "sharedFetchCount": shared_fetch_count,
            "cacheRefreshReason": refresh_reason,
            "members": members,
            "results": results,
        }

        with job_runner.STATE_LOCK:
            current = ensure_current_refresh_job(song_job_status_path(group_id), job_id)
            song_write_cache(group_id, cache)
            song_write_job_status(
                group_id,
                {
                    "jobId": job_id,
                    "feature": SONG_FEATURE,
                    "groupId": group_id,
                    "status": "completed",
                    "startedAt": current.get("startedAt"),
                    "finishedAt": now_iso(),
                    "refreshReason": refresh_reason,
                    "message": "单曲成绩榜后台刷新已完成。",
                    "memberCount": cache["memberCount"],
                    "successCount": cache["successCount"],
                    "skippedCount": cache["skippedCount"],
                    "cacheHitCount": cache["cacheHitCount"],
                    "sharedFetchCount": cache["sharedFetchCount"],
                },
            )
    except StaleRefreshJob:
        return
    except Exception as exc:
        from .server import GroupB50Error
        try:
            with job_runner.STATE_LOCK:
                current = ensure_current_refresh_job(song_job_status_path(group_id), job_id)
                song_write_job_status(
                    group_id,
                    {
                        "jobId": job_id,
                        "feature": SONG_FEATURE,
                        "groupId": group_id,
                        "status": "failed",
                        "startedAt": current.get("startedAt"),
                        "finishedAt": now_iso(),
                        "refreshReason": refresh_reason,
                        "message": str(exc),
                        "error": exc.to_dict() if isinstance(exc, GroupB50Error) else {"code": "UNKNOWN_ERROR", "message": str(exc)},
                    },
                )
        except StaleRefreshJob:
            return


def _normalize_song_max_concurrency(value: Any) -> int:
    if value is None:
        configured = os.environ.get("GROUP_SONG_MAX_CONCURRENCY")
        if configured and configured.isdigit():
            return max(1, min(int(configured), 20))
        return 3
    if not isinstance(value, int) or value < 1 or value > 20:
        from .server import GroupB50Error
        raise GroupB50Error("maxConcurrency 必须是 1 到 20 之间的整数。", code="INVALID_INPUT")
    return value


def _records_batch_window_ms() -> int:
    configured = os.environ.get("GROUP_SONG_RECORDS_BATCH_WINDOW_MS")
    if configured and configured.isdigit():
        return max(0, min(int(configured), 10000))
    return 1000


def _get_player_records_batch_for_group_refresh(
    qqs: list[str],
    *,
    timeout_ms: int,
    query_delay_ms: int,
    max_concurrency: int,
) -> dict[str, dict[str, Any]]:
    """提交本群待查 QQ，和其他群刷新请求合并后并发查询唯一 QQ。"""
    cached_results: dict[str, dict[str, Any]] = {}
    pending: list[str] = []
    seen: set[str] = set()
    for qq in qqs:
        if qq in seen:
            continue
        seen.add(qq)
        cached = _fresh_records_doc(qq)
        if cached is not None:
            cached_results[qq] = {"ok": True, "records": cached, "source": "cache"}
        else:
            pending.append(qq)

    if not pending:
        return cached_results

    request = _RecordsBatchRequest(
        pending,
        timeout_ms=timeout_ms,
        query_delay_ms=query_delay_ms,
        max_concurrency=max_concurrency,
    )
    _submit_records_batch_request(request)
    request.done.wait()
    return {**cached_results, **request.results}


def _submit_records_batch_request(request: _RecordsBatchRequest) -> None:
    global _RECORDS_BATCH_ACTIVE
    with _RECORDS_BATCH_LOCK:
        _RECORDS_BATCH_REQUESTS.append(request)
        if _RECORDS_BATCH_ACTIVE:
            return
        _RECORDS_BATCH_ACTIVE = True
        thread = threading.Thread(
            target=_records_batch_worker,
            name="group-rank-song-records-batch",
            daemon=True,
        )
        thread.start()


def _records_batch_worker() -> None:
    global _RECORDS_BATCH_ACTIVE
    while True:
        window_ms = _records_batch_window_ms()
        if window_ms > 0:
            time.sleep(window_ms / 1000)

        with _RECORDS_BATCH_LOCK:
            requests = list(_RECORDS_BATCH_REQUESTS)
            _RECORDS_BATCH_REQUESTS.clear()

        if not requests:
            with _RECORDS_BATCH_LOCK:
                if not _RECORDS_BATCH_REQUESTS:
                    _RECORDS_BATCH_ACTIVE = False
                    return
            continue

        try:
            batch_results = _fetch_records_for_requests(requests)
        except BaseException as exc:
            for request in requests:
                request.results = {
                    qq: {"ok": False, "error": exc}
                    for qq in request.qqs
                }
                request.done.set()
        else:
            remote_seen: set[str] = set()
            for request in requests:
                request_results: dict[str, dict[str, Any]] = {}
                for qq in request.qqs:
                    item = dict(batch_results.get(qq) or {})
                    if item.get("source") == "remote":
                        if qq in remote_seen:
                            item["source"] = "shared"
                        else:
                            remote_seen.add(qq)
                    request_results[qq] = item
                request.results = request_results
                request.done.set()

        with _RECORDS_BATCH_LOCK:
            if not _RECORDS_BATCH_REQUESTS:
                _RECORDS_BATCH_ACTIVE = False
                return


def _fetch_records_for_requests(
    requests: list[_RecordsBatchRequest],
) -> dict[str, dict[str, Any]]:
    unique_qqs: list[str] = []
    seen: set[str] = set()
    timeout_ms = 10000
    query_delay_ms = 0
    max_concurrency = 1
    for request in requests:
        timeout_ms = max(timeout_ms, request.timeout_ms)
        query_delay_ms = max(query_delay_ms, request.query_delay_ms)
        max_concurrency = max(max_concurrency, request.max_concurrency)
        for qq in request.qqs:
            if qq not in seen:
                seen.add(qq)
                unique_qqs.append(qq)

    results: dict[str, dict[str, Any]] = {}
    remote_qqs: list[str] = []
    for qq in unique_qqs:
        cached = _fresh_records_doc(qq)
        if cached is not None:
            results[qq] = {"ok": True, "records": cached, "source": "cache"}
        else:
            remote_qqs.append(qq)

    if not remote_qqs:
        return results

    def fetch_one(qq: str) -> tuple[str, dict[str, Any]]:
        try:
            records_doc = _fetch_player_records_via_diving_fish(qq, timeout_ms=timeout_ms)
            if isinstance(records_doc, dict):
                try:
                    existing = read_player_records(qq)
                    if not is_player_records_fresh(existing):
                        write_player_records(qq, records_doc)
                except Exception:
                    pass
                return qq, {"ok": True, "records": records_doc, "source": "remote"}
            return qq, {
                "ok": False,
                "error": _group_error("水鱼 /dev/player/records 没有返回 data 字典。", "INVALID_JSON"),
            }
        except BaseException as exc:
            return qq, {"ok": False, "error": exc}

    with ThreadPoolExecutor(max_workers=max_concurrency) as executor:
        future_to_qq = {}
        for index, qq in enumerate(remote_qqs):
            future_to_qq[executor.submit(fetch_one, qq)] = qq
            if query_delay_ms > 0 and index < len(remote_qqs) - 1:
                time.sleep(query_delay_ms / 1000)
        for future in as_completed(future_to_qq):
            qq, item = future.result()
            results[qq] = item

    return results


def _group_error(message: str, code: str) -> BaseException:
    from .server import GroupB50Error
    return GroupB50Error(message, code=code)


def _fresh_records_doc(qq: str) -> dict[str, Any] | None:
    cache_entry = read_player_records(qq)
    if not is_player_records_fresh(cache_entry):
        return None
    records_doc = (cache_entry or {}).get("records") if isinstance(cache_entry, dict) else None
    return records_doc if isinstance(records_doc, dict) else None


def _fetch_player_records_via_diving_fish(qq: str, *, timeout_ms: int) -> dict[str, Any]:
    """同进程直调 diving_fish_b50_mcp.call_diving_fish_api 拿一个 QQ 的全部成绩。"""
    from .server import GroupB50Error
    from diving_fish_b50_mcp.server import DivingFishError, call_diving_fish_api

    try:
        api_result = call_diving_fish_api(
            {
                "operation": "maimai_dev_player_records_get",
                "query": {"qq": qq},
                "timeoutMs": min(timeout_ms, 30000),
            }
        )
    except DivingFishError as exc:
        raise GroupB50Error(
            f"diving_fish_api 调用失败：{exc}",
            code=str(exc.code) if exc.code else "B50_MCP_ERROR",
            status=exc.status,
            body=exc.body,
        ) from exc

    data = api_result.get("data") if isinstance(api_result, dict) else None
    if not isinstance(data, dict):
        raise GroupB50Error("水鱼 /dev/player/records 没有返回 data 字典。", code="INVALID_JSON")
    return data


def _build_member_result(
    qq: str,
    member: dict[str, Any],
    records_doc: dict[str, Any],
    group_id: str,
    *,
    cache_hit: bool,
) -> dict[str, Any] | None:
    rating = records_doc.get("rating")
    if not isinstance(rating, (int, float)) or rating <= 0:
        # /dev/player/records 这种数据缺失时通常 records 也是空
        records_list = records_doc.get("records") if isinstance(records_doc.get("records"), list) else []
        if not records_list:
            return None
    try:
        identity = get_identity(qq, group_id)
    except Exception:
        identity = None
    return {
        "userId": qq,
        "displayName": member.get("displayName"),
        "nickname": member.get("nickname"),
        "card": member.get("card"),
        "identity": identity,
        "ok": True,
        "rating": rating if isinstance(rating, (int, float)) else None,
        "playerNickname": records_doc.get("nickname"),
        "plate": records_doc.get("plate"),
        "fromCache": cache_hit,
        "error": None,
    }


# ============== songQuery 解析 ==============


def _resolve_song_target_from_arguments(
    arguments: dict[str, Any],
    level_index: int | None,
) -> tuple[int, int | None]:
    from .server import GroupB50Error

    music_id, level_index = _resolve_optional_song_target_from_arguments(arguments, level_index)
    if music_id:
        return music_id, level_index
    raise GroupB50Error("必须提供 songQuery 或 musicId。", code="INVALID_INPUT")


def _resolve_optional_song_target_from_arguments(
    arguments: dict[str, Any],
    level_index: int | None,
) -> tuple[int | None, int | None]:
    from .server import GroupB50Error

    music_id = _normalize_music_id(arguments.get("musicId"))
    if music_id:
        return music_id, level_index

    song_query = _normalize_optional_str(arguments.get("songQuery"))
    if not song_query:
        return None, level_index

    search_limit = _normalize_optional_int(arguments.get("searchLimit"), "searchLimit", low=1, high=20) or 5
    timeout_ms = arguments.get("timeoutMs") if isinstance(arguments.get("timeoutMs"), int) else 10000

    music_ids, searched_level_indexes = _resolve_song_query_via_maimai_search(
        song_query,
        search_limit=search_limit,
        difficulty=_difficulty_from_level_index(level_index),
        song_type=_song_type_for_search(arguments.get("songType")),
        timeout_ms=timeout_ms,
    )
    if not music_ids and level_index is not None:
        fallback_ids, fallback_level_indexes = _resolve_song_query_via_maimai_search(
            song_query,
            search_limit=search_limit,
            difficulty=None,
            song_type=_song_type_for_search(arguments.get("songType")),
            timeout_ms=timeout_ms,
        )
        unique_level_indexes = _unique_level_indexes(fallback_level_indexes)
        if fallback_ids and len(unique_level_indexes) == 1:
            music_ids = fallback_ids
            level_index = unique_level_indexes[0]
            searched_level_indexes = unique_level_indexes
    if not music_ids:
        raise GroupB50Error(f"maimai-local-search 没有找到曲目：{song_query}", code="SONG_NOT_FOUND")
    if level_index is None:
        level_index = _highest_level_index(searched_level_indexes)
    # 优先选第一个匹配（已经被本地搜索排序）
    return music_ids[0], level_index


def _unique_level_indexes(values: list[Any]) -> list[int]:
    unique: list[int] = []
    for value in values:
        if isinstance(value, bool):
            continue
        if isinstance(value, int):
            item = value
        elif isinstance(value, str) and value.strip().isdigit():
            item = int(value.strip())
        else:
            continue
        if not 0 <= item <= 4:
            continue
        if item not in unique:
            unique.append(item)
    return unique


def _highest_level_index(values: list[Any]) -> int | None:
    level_indexes = _unique_level_indexes(values)
    if not level_indexes:
        return None
    return max(level_indexes)


def _difficulty_from_level_index(level_index: Any) -> str | None:
    if not isinstance(level_index, int):
        return None
    return {0: "Basic", 1: "Advanced", 2: "Expert", 3: "Master", 4: "Re:Master"}.get(level_index)


def _song_type_for_search(song_type: Any) -> str | None:
    text = _normalize_song_type(song_type)
    if text is None:
        return None
    return "dx" if text == "DX" else "standard"


def _normalize_song_type(value: Any) -> str | None:
    if value is None:
        return None
    if isinstance(value, str):
        v = value.strip().casefold()
        if v in {"dx"}:
            return "DX"
        if v in {"sd", "standard", "std"}:
            return "SD"
    return None


def _resolve_song_query_via_maimai_search(
    song_query: str,
    *,
    search_limit: int,
    difficulty: str | None,
    song_type: str | None,
    timeout_ms: int,
) -> tuple[list[int], list[int]]:
    """Subprocess 调 maimai-local-search MCP 的 search_maimai_songs，提取 music_id。"""
    from .server import GroupB50Error

    args: dict[str, Any] = {"query": song_query, "limit": search_limit, "format": "json"}
    if difficulty:
        args["difficulty"] = difficulty
    if song_type:
        args["song_type"] = song_type

    response = _call_maimai_local_search_tool("search_maimai_songs", args, timeout_ms=timeout_ms)
    if response.get("isError"):
        raise GroupB50Error(
            "maimai-local-search 查询失败。",
            code="SONG_SEARCH_FAILED",
        )
    content = response.get("content")
    text = ""
    if isinstance(content, list) and content:
        first = content[0]
        if isinstance(first, dict) and isinstance(first.get("text"), str):
            text = first["text"]
    try:
        parsed = json.loads(text)
    except json.JSONDecodeError as exc:
        raise GroupB50Error("maimai-local-search 未返回可解析的 JSON。", code="SONG_SEARCH_INVALID") from exc
    if not isinstance(parsed, dict):
        raise GroupB50Error("maimai-local-search 返回结构不是对象。", code="SONG_SEARCH_INVALID")

    songs = parsed.get("songs") if isinstance(parsed.get("songs"), list) else []
    music_ids: list[int] = []
    level_indexes: list[int] = []
    for song in songs:
        if not isinstance(song, dict):
            continue
        # 优先 matched_charts 里的 fit_source_id（水鱼 music_id）
        for chart in song.get("matched_charts") or []:
            if isinstance(chart, dict):
                _add_int_id(music_ids, chart.get("fit_source_id"))
                _add_level_index(level_indexes, chart.get("difficulty_index"))
        if not music_ids:
            _add_int_id(music_ids, song.get("id"))
        if music_ids:
            break  # 取第一个有 ID 的曲目
    return music_ids, level_indexes


def _add_int_id(values: list[int], value: Any) -> None:
    if isinstance(value, bool):
        return
    if isinstance(value, int):
        v = value
    elif isinstance(value, str) and value.strip().isdigit():
        v = int(value.strip())
    else:
        return
    if v > 0 and v not in values:
        values.append(v)


def _add_level_index(values: list[int], value: Any) -> None:
    if isinstance(value, bool):
        return
    if isinstance(value, int):
        v = value
    elif isinstance(value, str) and value.strip().isdigit():
        v = int(value.strip())
    else:
        return
    if 0 <= v <= 4 and v not in values:
        values.append(v)


def _call_maimai_local_search_tool(tool_name: str, arguments: dict[str, Any], *, timeout_ms: int) -> dict[str, Any]:
    """复用 diving_fish_b50_mcp 里的 MaimaiLocalSearchClient 一次性调用。"""
    from diving_fish_b50_mcp.server import MaimaiLocalSearchClient

    with MaimaiLocalSearchClient(timeout_ms=max(timeout_ms, 30000)) as client:
        return client.call_tool(tool_name, arguments)


# ============== 按歌过滤 + 排序 ==============


def _collect_song_rows_with_auto_level(
    cache: dict[str, Any],
    *,
    music_id: int,
    level_index: int | None,
    song_type: str | None,
    achievements_min: float | None,
    achievements_max: float | None,
) -> tuple[list[dict[str, Any]], int | None]:
    if level_index is None:
        highest_level_index = _highest_level_index(
            _available_level_indexes_from_cache(
                cache,
                music_id=music_id,
                song_type=song_type,
                achievements_min=achievements_min,
                achievements_max=achievements_max,
            )
        )
        if highest_level_index is None:
            rows = _collect_song_rows(
                cache,
                music_id=music_id,
                level_index=None,
                song_type=song_type,
                achievements_min=achievements_min,
                achievements_max=achievements_max,
            )
            return rows, level_index
        return _collect_song_rows(
            cache,
            music_id=music_id,
            level_index=highest_level_index,
            song_type=song_type,
            achievements_min=achievements_min,
            achievements_max=achievements_max,
        ), highest_level_index

    rows = _collect_song_rows(
        cache,
        music_id=music_id,
        level_index=level_index,
        song_type=song_type,
        achievements_min=achievements_min,
        achievements_max=achievements_max,
    )
    if rows:
        return rows, level_index

    level_indexes = _unique_level_indexes(
        _available_level_indexes_from_cache(
            cache,
            music_id=music_id,
            song_type=song_type,
            achievements_min=achievements_min,
            achievements_max=achievements_max,
        )
    )
    if len(level_indexes) != 1:
        return rows, level_index
    only_level_index = level_indexes[0]
    return _collect_song_rows(
        cache,
        music_id=music_id,
        level_index=only_level_index,
        song_type=song_type,
        achievements_min=achievements_min,
        achievements_max=achievements_max,
    ), only_level_index


def _available_level_indexes_from_cache(
    cache: dict[str, Any],
    *,
    music_id: int,
    song_type: str | None,
    achievements_min: float | None,
    achievements_max: float | None,
) -> list[int]:
    level_indexes: list[int] = []
    for member_entry in cache.get("results") or []:
        if not isinstance(member_entry, dict) or not member_entry.get("ok"):
            continue
        qq = str(member_entry.get("userId") or "")
        if not qq:
            continue
        records_doc_entry = read_player_records(qq)
        if not isinstance(records_doc_entry, dict):
            continue
        records_doc = records_doc_entry.get("records") if isinstance(records_doc_entry.get("records"), dict) else None
        if not isinstance(records_doc, dict):
            continue
        records = records_doc.get("records") if isinstance(records_doc.get("records"), list) else []
        for record in records:
            if not _record_matches_song(record, music_id=music_id, song_type=song_type):
                continue
            ach = record.get("achievements")
            if achievements_min is not None and (not isinstance(ach, (int, float)) or ach < achievements_min):
                continue
            if achievements_max is not None and (not isinstance(ach, (int, float)) or ach > achievements_max):
                continue
            _add_level_index(level_indexes, record.get("level_index"))
    return level_indexes


def _collect_song_rows(
    cache: dict[str, Any],
    *,
    music_id: int,
    level_index: int | None,
    song_type: str | None,
    achievements_min: float | None,
    achievements_max: float | None,
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for member_entry in cache.get("results") or []:
        if not isinstance(member_entry, dict) or not member_entry.get("ok"):
            continue
        qq = str(member_entry.get("userId") or "")
        if not qq:
            continue
        records_doc_entry = read_player_records(qq)
        if not isinstance(records_doc_entry, dict):
            continue
        records_doc = records_doc_entry.get("records") if isinstance(records_doc_entry.get("records"), dict) else None
        if not isinstance(records_doc, dict):
            continue
        records = records_doc.get("records") if isinstance(records_doc.get("records"), list) else []
        match = _find_best_song_record(
            records,
            music_id=music_id,
            level_index=level_index,
            song_type=song_type,
        )
        if match is None:
            continue
        ach = match.get("achievements")
        if achievements_min is not None and (not isinstance(ach, (int, float)) or ach < achievements_min):
            continue
        if achievements_max is not None and (not isinstance(ach, (int, float)) or ach > achievements_max):
            continue
        rows.append(
            {
                "userId": qq,
                "displayName": member_entry.get("displayName"),
                "nickname": member_entry.get("nickname"),
                "card": member_entry.get("card"),
                "identity": member_entry.get("identity"),
                "playerNickname": member_entry.get("playerNickname"),
                "rating": member_entry.get("rating"),
                "record": _normalize_record(match),
            }
        )
    return rows


def _find_best_song_record(
    records: list[Any],
    *,
    music_id: int,
    level_index: int | None,
    song_type: str | None,
) -> dict[str, Any] | None:
    candidates: list[dict[str, Any]] = []
    for r in records:
        if not _record_matches_song(r, music_id=music_id, song_type=song_type):
            continue
        if level_index is not None and r.get("level_index") != level_index:
            continue
        candidates.append(r)
    if not candidates:
        return None
    # 取 ra 最高的；并列时取 achievements 最高
    candidates.sort(
        key=lambda r: (
            r.get("ra") if isinstance(r.get("ra"), (int, float)) else -1,
            r.get("achievements") if isinstance(r.get("achievements"), (int, float)) else -1,
        ),
        reverse=True,
    )
    return candidates[0]


def _record_matches_song(record: Any, *, music_id: int, song_type: str | None) -> bool:
    if not isinstance(record, dict):
        return False
    sid = record.get("song_id")
    if isinstance(sid, str) and sid.isdigit():
        sid = int(sid)
    if sid != music_id:
        return False
    if song_type is not None:
        rt = str(record.get("type") or "").upper()
        if song_type == "DX" and rt != "DX":
            return False
        if song_type == "SD" and rt not in {"SD", "STANDARD", "STD"}:
            return False
    return True


def _normalize_record(r: dict[str, Any]) -> dict[str, Any]:
    return {
        "title": r.get("title"),
        "type": r.get("type"),
        "level": r.get("level"),
        "levelLabel": r.get("level_label"),
        "levelIndex": r.get("level_index"),
        "ds": r.get("ds"),
        "achievements": r.get("achievements"),
        "dxScore": r.get("dxScore"),
        "fc": r.get("fc"),
        "fs": r.get("fs"),
        "ra": r.get("ra"),
        "rate": r.get("rate"),
        "songId": r.get("song_id"),
    }


def _sort_song_rows(rows: list[dict[str, Any]], *, sort_by: str, sort_order: str) -> list[dict[str, Any]]:
    reverse = sort_order == "desc"
    field_map = {"achievements": "achievements", "ra": "ra", "dxScore": "dxScore"}
    field = field_map[sort_by]

    def key(row: dict[str, Any]) -> tuple[Any, ...]:
        record = row.get("record") or {}
        value = record.get(field)
        primary = value if isinstance(value, (int, float)) else float("-inf")
        ach_tie = record.get("achievements") if isinstance(record.get("achievements"), (int, float)) else float("-inf")
        return (primary, ach_tie, row.get("userId") or "")

    return sorted(rows, key=key, reverse=reverse)


def _apply_rank_window(
    rows: list[dict[str, Any]],
    output_limit: int | None,
    *,
    start_rank: int | None = None,
    end_rank: int | None = None,
) -> list[dict[str, Any]]:
    if start_rank is not None and end_rank is not None:
        start_index = start_rank - 1
        end_index = min(len(rows), end_rank)
        return [{**row, "_rank": index + 1} for index, row in enumerate(rows[start_index:end_index], start=start_index)]
    if output_limit is not None:
        return [{**row, "_rank": index + 1} for index, row in enumerate(rows[:output_limit])]
    return rows


def _rank_window_label(output_limit: int | None, start_rank: int | None, end_rank: int | None) -> str:
    if start_rank is not None and end_rank is not None:
        return f"第 {start_rank}-{end_rank} 名"
    if output_limit is not None:
        return f"前 {output_limit} 人"
    return "无上限"


# ============== 文本输出 ==============


def _format_song_report_text(
    cache: dict[str, Any],
    *,
    rows: list[dict[str, Any]],
    music_id: int,
    level_index: int | None,
    song_type: str | None,
    sort_by: str,
    sort_order: str,
    matched_count: int,
    output_limit: int | None,
    start_rank: int | None,
    end_rank: int | None,
    cache_refresh_reason: str,
) -> str:
    status = "本次使用一天内缓存，未重新拉取" if cache_refresh_reason == "hit" else "本次已重新拉取并刷新缓存"
    title_bits = []
    if rows:
        first_record = rows[0].get("record") or {}
        if first_record.get("title"):
            title_bits.append(f"{first_record['title']}")
        if first_record.get("level"):
            title_bits.append(f"{first_record['levelLabel'] or ''} {first_record['level']}")
    header = [
        f"群 {cache.get('groupId')} 单曲成绩榜（music_id={music_id}{' '.join(title_bits) and '，' + ' / '.join(title_bits) or ''}）",
        f"缓存状态: {status}，缓存生成时间: {_format_display_time(cache.get('fetchedAt'))}",
        f"筛选: 难度={_level_index_label(level_index)}, 谱面={song_type or '不限'}；排序: {sort_by} {sort_order}；输出: {_rank_window_label(output_limit, start_rank, end_rank)}；匹配: {matched_count}，本次展示: {len(rows)}",
        f"群成员: {cache.get('memberCount', 0)}，已缓存: {cache.get('successCount', 0)}（缓存命中 {cache.get('cacheHitCount', 0)}，跨群复用 {cache.get('sharedFetchCount', 0)}），跳过: {cache.get('skippedCount', 0)}",
    ]
    if not rows:
        return "\n".join(header + ["", "群内没有人有这首歌的成绩。"])

    table = [
        "",
        "| 排名 | QQ | QQ昵称 | QQ群昵称 | 水鱼昵称 | 达成率 | Rate | FC | FS | DX Score | ra |",
        "| --- | --- | --- | --- | --- | ---: | --- | --- | --- | ---: | ---: |",
    ]
    for index, row in enumerate(rows, start=1):
        absolute_rank = row.get("_rank") if isinstance(row.get("_rank"), int) else index
        record = row.get("record") or {}
        identity = row.get("identity") if isinstance(row.get("identity"), dict) else {}
        preferred_group = identity.get("preferredGroup") if isinstance(identity.get("preferredGroup"), dict) else {}
        table.append(
            "| " + " | ".join(
                [
                    str(absolute_rank),
                    str(row.get("userId") or ""),
                    _escape(str(identity.get("qqNickname") or row.get("nickname") or "")),
                    _escape(str(preferred_group.get("groupNickname") or row.get("card") or row.get("displayName") or "")),
                    _escape(str(row.get("playerNickname") or identity.get("waterfishNickname") or "")),
                    f"{record.get('achievements')}%" if isinstance(record.get("achievements"), (int, float)) else "",
                    str(record.get("rate") or "").upper(),
                    str(record.get("fc") or "").upper(),
                    str(record.get("fs") or "").upper(),
                    str(record.get("dxScore")) if record.get("dxScore") is not None else "",
                    str(record.get("ra")) if record.get("ra") is not None else "",
                ]
            ) + " |"
        )
    return "\n".join(header + table)


def _format_song_cache_only_text(
    group_id: str,
    cache: dict[str, Any],
    cache_refresh_reason: str,
) -> str:
    status = "本次使用一天内缓存，未重新拉取" if cache_refresh_reason == "hit" else "本次已重新拉取并刷新缓存"
    return "\n".join(
        [
            f"群 {group_id} 单曲成绩缓存已就绪。",
            f"缓存状态: {status}，缓存生成时间: {_format_display_time(cache.get('fetchedAt'))}",
            f"群成员: {cache.get('memberCount', 0)}，已缓存: {cache.get('successCount', 0)}（缓存命中 {cache.get('cacheHitCount', 0)}，跨群复用 {cache.get('sharedFetchCount', 0)}），跳过: {cache.get('skippedCount', 0)}",
            "未指定 songQuery 或 musicId，本次只刷新/预热缓存，不生成单曲排行榜。",
        ]
    )


def _format_member_rank_text(
    cache: dict[str, Any],
    target: dict[str, Any],
    rank_info: dict[str, Any],
    context: list[dict[str, Any]],
    music_id: int,
) -> str:
    record = target.get("record") or {}
    identity = target.get("identity") if isinstance(target.get("identity"), dict) else {}
    lines = [
        f"群 {cache.get('groupId')} QQ {target.get('userId')} 在 music_id={music_id} 的群内排名",
        f"缓存生成时间: {_format_display_time(cache.get('fetchedAt'))}",
        f"曲目: {record.get('title') or '?'} - {record.get('levelLabel') or ''} / 定数 {record.get('ds')}",
        f"达成率: {record.get('achievements')}% / Rate: {(record.get('rate') or '').upper()} / FC: {(record.get('fc') or '').upper()} / FS: {(record.get('fs') or '').upper()} / DX Score: {record.get('dxScore')} / ra: {record.get('ra')}",
        f"达成率倒序排名: {rank_info.get('rankDesc')} / {rank_info.get('totalRanked')}（前面 {rank_info.get('higherCount')} 人，后面 {rank_info.get('lowerCount')} 人）",
        f"达成率正序排名: {rank_info.get('rankAsc')} / {rank_info.get('totalRanked')}",
        "",
        "附近排名（达成率排名正序）:",
        "| 排名 | QQ | QQ昵称 | QQ群昵称 | 水鱼昵称 | 达成率 | ra | 标记 |",
        "| --- | --- | --- | --- | --- | ---: | ---: | --- |",
    ]
    for row in context:
        r = row.get("record") or {}
        ident = row.get("identity") if isinstance(row.get("identity"), dict) else {}
        preferred_group = ident.get("preferredGroup") if isinstance(ident.get("preferredGroup"), dict) else {}
        marker = "目标" if str(row.get("userId")) == str(target.get("userId")) else ""
        lines.append(
            "| " + " | ".join(
                [
                    str(row.get("_rank")),
                    str(row.get("userId") or ""),
                    _escape(str(ident.get("qqNickname") or row.get("nickname") or "")),
                    _escape(str(preferred_group.get("groupNickname") or row.get("card") or row.get("displayName") or "")),
                    _escape(str(row.get("playerNickname") or ident.get("waterfishNickname") or "")),
                    f"{r.get('achievements')}%" if isinstance(r.get("achievements"), (int, float)) else "",
                    str(r.get("ra")) if r.get("ra") is not None else "",
                    marker,
                ]
            ) + " |"
        )
    return "\n".join(lines)


def _format_member_rank_missing_text(cache: dict[str, Any], qq: str, music_id: int) -> str:
    return "\n".join(
        [
            f"群 {cache.get('groupId')} QQ {qq} 在 music_id={music_id} 上没有可用成绩。",
            "可能原因：没玩过这首歌、对方隐私设置导致 records 拿不到、records 缓存里没有该曲。",
            f"缓存生成时间: {_format_display_time(cache.get('fetchedAt'))}",
        ]
    )


def _format_song_job_started_text(
    group_id: str,
    music_id: int | None,
    job: dict[str, Any],
    status: dict[str, Any],
) -> str:
    target_line = (
        f"music_id={music_id}。"
        if music_id is not None
        else "未指定曲目，本次只刷新/预热全群完整成绩缓存。"
    )
    return "\n".join(
        [
            f"群 {group_id} 单曲成绩缓存不存在/已过期/被要求刷新，已启动后台任务。",
            f"任务状态: {job.get('status')}，启动时间: {_format_display_time(job.get('startedAt'))}",
            f"{target_line}会等待短批处理窗口合并多个群的重复 QQ，再并发调水鱼 /dev/player/records。",
            "稍后调 group_song_score_job_status 看进度，或重新调 group_song_score_report 读结果。",
            f"缓存路径: {status.get('cachePath')}",
        ]
    )


def _format_member_rank_job_started_text(
    group_id: str,
    qq: str,
    music_id: int,
    job: dict[str, Any],
    status: dict[str, Any],
) -> str:
    return "\n".join(
        [
            f"群 {group_id} QQ {qq} 在 music_id={music_id} 的群内排名需要完整成绩缓存，已启动后台刷新。",
            f"任务状态: {job.get('status')}，启动时间: {_format_display_time(job.get('startedAt'))}",
            "稍后再次调用 group_song_score_member_rank 读取排名。",
            f"缓存路径: {status.get('cachePath')}",
        ]
    )


def _format_job_status_text(group_id: str, job: dict[str, Any] | None, cache: dict[str, Any] | None) -> str:
    if not job:
        return f"群 {group_id} 当前没有单曲成绩后台刷新任务。\n缓存存在: {cache is not None}"
    lines = [
        f"群 {group_id} 单曲成绩刷新任务: {job.get('status')}",
        f"启动: {_format_display_time(job.get('startedAt'))}",
        f"完成: {_format_display_time(job.get('finishedAt'), missing='未完成')}",
        f"说明: {job.get('message')}",
    ]
    if job.get("status") == "running":
        lines.append(
            f"进度: {job.get('processedCount', 0)}/{job.get('totalCount', '?')}，"
            f"已缓存: {job.get('cachedCount', 0)}，跳过: {job.get('skippedCount', 0)}，"
            f"临时失败: {job.get('transientFailureCount', 0)}"
        )
    if job.get("status") == "completed":
        lines.append(
            f"缓存命中: {job.get('cacheHitCount', 0)}，跨群复用: {job.get('sharedFetchCount', 0)}"
        )
    if job.get("status") == "failed" and job.get("error"):
        lines.append(f"错误: {job['error'].get('message')}")
    return "\n".join(lines)


def _level_index_label(level_index: int | None) -> str:
    if level_index is None:
        return "不限"
    return {0: "Basic", 1: "Advanced", 2: "Expert", 3: "Master", 4: "Re:Master"}.get(level_index, str(level_index))


def _format_display_time(value: Any, *, missing: str = "未知") -> str:
    if not isinstance(value, str) or not value.strip():
        return missing
    from .server import format_display_time
    return format_display_time(value, missing=missing)


def _escape(value: str) -> str:
    return value.replace("|", "\\|").replace("\n", " ")


# ============== 身份反查 ==============


def _resolve_target_to_qq(target: str, *, group_id: str | None) -> dict[str, str]:
    from .server import GroupB50Error, resolve_rank_target
    return resolve_rank_target(target, group_id=group_id)


def _infer_group_id(qq: str) -> str:
    from .server import infer_rank_group_id
    return infer_rank_group_id(qq)
