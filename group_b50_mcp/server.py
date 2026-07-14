from __future__ import annotations

import json
import os
import shutil
import socket
import sys
import traceback
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Callable
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from diving_fish_b50_mcp.server import format_b50_summary
from player_cache import DAILY_RESET_HOUR_UTC, is_player_b50_fresh, read_player_b50
from qq_identity_mcp.store import (
    get_identity,
    is_cache_fresh as is_identity_cache_fresh,
    read_cache as read_identity_cache,
    resolve_identities,
    upsert_group_member,
    upsert_waterfish_profile,
    write_cache as write_identity_cache,
)
from . import __version__
from . import job_runner
from .job_runner import (
    StaleRefreshJob,
    ensure_current_refresh_job,
    now_iso,
    read_job_status as _read_job_status,
    write_job_status as _write_job_status,
    write_json_atomic,
    write_refresh_progress as _write_refresh_progress,
)


SERVER_NAME = "group-rank-mcp"
B50_FEATURE = "b50"
DEFAULT_NAPCAT_BASE_URL = "http://napcat:3000"
TRANSIENT_B50_ERROR_CODES = {"NETWORK_ERROR", "TIMEOUT", "RATE_LIMITED"}


def _daily_reset_time(now: datetime) -> datetime:
    """最近一次每日重置时刻（UTC），默认 14:00 UTC = 22:00 CST。"""
    reset = now.replace(hour=DAILY_RESET_HOUR_UTC, minute=0, second=0, microsecond=0)
    if now < reset:
        reset -= timedelta(days=1)
    return reset


def _next_daily_reset_iso() -> str:
    now = datetime.now(timezone.utc)
    reset = now.replace(hour=DAILY_RESET_HOUR_UTC, minute=0, second=0, microsecond=0)
    if now >= reset:
        reset += timedelta(days=1)
    return reset.isoformat()
# 对全局状态文件加写锁，沿用 job_runner 共享的那把，避免不同 feature 间死锁。
REFRESH_STATE_LOCK = job_runner.STATE_LOCK
DEFAULT_DISPLAY_TIMEZONE = "Asia/Shanghai"


class GroupB50Error(Exception):
    def __init__(
        self,
        message: str,
        *,
        code: str = "GROUP_B50_ERROR",
        status: int | None = None,
        body: str | None = None,
    ) -> None:
        super().__init__(message)
        self.code = code
        self.status = status
        self.body = body

    def to_dict(self) -> dict[str, Any]:
        return {
            "code": self.code,
            "message": str(self),
            "status": self.status,
            "body": self.body,
        }


# StaleRefreshJob 现由 job_runner 提供，这里只保留 re-export 以兼容旧用法。


GROUP_B50_REPORT_TOOL = {
    "name": "group_b50_report",
    "description": (
        "按群号从 NapCat 拉取群成员 QQ，调用 B50 MCP 的 query_b50_batch，生成 rating 正序/倒序"
        "两个缓存文件，并按请求输出 rating 榜或详细 B50。缓存超过 1 天会自动清理重拉。"
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "groupId": {
                "type": "string",
                "description": "QQ群号。",
            },
            "forceRefresh": {"type": "boolean", "description": "是否忽略缓存并重新拉取。"},
            "sortBy": {
                "type": "string",
                "enum": ["rating", "fitIndex"],
                "description": "排序字段。rating=按 rating；fitIndex=按 B50 虚高指数（虚高百分比）。默认 rating。",
            },
            "sortOrder": {
                "type": "string",
                "enum": ["asc", "desc"],
                "description": "返回内容排序，默认 asc。",
            },
            "fitIndexMin": {
                "type": "number",
                "description": "虚高指数下限（百分比）。例如 0.5 表示只看虚高 >= 0.5% 的人。",
            },
            "fitIndexMax": {
                "type": "number",
                "description": "虚高指数上限（百分比）。例如 -0.5 表示只看虚低 >= 0.5% 的人。",
            },
            "outputMode": {
                "type": "string",
                "enum": ["rating", "detail"],
                "description": "rating 只输出榜单；detail 输出对应排序的完整 B50。",
            },
            "ratingMin": {
                "type": "integer",
                "description": "筛选 rating 下限，包含该值。例如 14000。",
            },
            "ratingMax": {
                "type": "integer",
                "description": "筛选 rating 上限，包含该值。例如 15000 表示只看 15000 以下/以内。",
            },
            "outputLimit": {
                "type": "integer",
                "minimum": 1,
                "description": "筛选和排序之后最多输出多少人。例如前 10 或后 30。",
            },
            "startRank": {
                "type": "integer",
                "minimum": 1,
                "description": "可选，筛选和排序之后从第几名开始输出，1-based，需和 endRank 一起使用。",
            },
            "endRank": {
                "type": "integer",
                "minimum": 1,
                "description": "可选，筛选和排序之后输出到第几名，包含该名次，需和 startRank 一起使用。",
            },
            "napcatBaseUrl": {
                "type": "string",
                "description": "覆盖 NapCat OneBot HTTP API 地址，默认读 NAPCAT_BASE_URL。",
            },
            "noCache": {
                "type": "boolean",
                "description": "传给 NapCat get_group_member_list 的 no_cache，默认 true。",
            },
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
                "description": "每个水鱼查询之间的等待时间，默认读 GROUP_B50_QUERY_DELAY_MS 或 250ms。",
            },
            "maxConcurrency": {
                "type": "integer",
                "minimum": 1,
                "maximum": 20,
                "description": "传给 B50 MCP 批量查询的并发数，默认 3。",
            },
            "batchSize": {
                "type": "integer",
                "minimum": 1,
                "maximum": 100,
                "description": "每批提交给 B50 MCP 的 QQ 数量，默认 100；批内会按单个 QQ 完成进度更新。",
            },
            "maxMembers": {
                "type": "integer",
                "minimum": 1,
                "description": "最多查询多少个群成员，主要用于测试或临时限流；默认查全群。",
            },
        },
        "required": ["groupId"],
        "additionalProperties": False,
    },
}

GROUP_B50_CACHE_STATUS_TOOL = {
    "name": "group_b50_cache_status",
    "description": "查看指定群 B50 榜单缓存是否存在、是否超过 1 天、对应报告文件路径。",
    "inputSchema": {
        "type": "object",
        "properties": {
            "groupId": {"type": "string", "description": "QQ群号。"},
        },
        "required": ["groupId"],
        "additionalProperties": False,
    },
}

GROUP_B50_JOB_STATUS_TOOL = {
    "name": "group_b50_job_status",
    "description": "查看群 B50 后台刷新任务状态；任务完成后可按筛选/排序直接读取榜单。",
    "inputSchema": {
        "type": "object",
        "properties": {
            "groupId": {"type": "string", "description": "QQ群号。"},
            "sortBy": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["sortBy"],
            "sortOrder": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["sortOrder"],
            "outputMode": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["outputMode"],
            "ratingMin": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["ratingMin"],
            "ratingMax": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["ratingMax"],
            "fitIndexMin": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["fitIndexMin"],
            "fitIndexMax": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["fitIndexMax"],
            "outputLimit": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["outputLimit"],
        },
        "required": ["groupId"],
        "additionalProperties": False,
    },
}

GROUP_B50_MEMBER_RANK_TOOL = {
    "name": "group_b50_member_rank",
    "description": (
        "输入 QQ 或昵称，基于群 B50 缓存输出该 QQ 的群内 rating 排名信息；"
        "未提供群号时会尝试从 QQ 身份缓存推断唯一群。"
        "缓存超过 1 天或强制刷新时会启动后台刷新任务。"
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "groupId": {
                "type": "string",
                "description": "QQ群号。可选；未提供时会尝试从 QQ 身份缓存推断唯一群。",
            },
            "qq": {"type": "string", "description": "要查询群内排名的 QQ 号。"},
            "target": {
                "type": "string",
                "description": "QQ号、QQ昵称、群昵称/群名片或水鱼昵称。未提供 qq 时会用 QQ 身份缓存自动反查 QQ。",
            },
            "forceRefresh": {"type": "boolean", "description": "是否忽略缓存并重新拉取。"},
            "outputMode": {
                "type": "string",
                "enum": ["rating", "detail"],
                "description": "rating 只输出排名信息；detail 额外输出完整 B50。",
            },
            "contextSize": {
                "type": "integer",
                "minimum": 0,
                "maximum": 10,
                "description": "展示目标排名上下各多少人，默认 3。",
            },
            "napcatBaseUrl": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["napcatBaseUrl"],
            "noCache": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["noCache"],
            "timeoutMs": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["timeoutMs"],
            "queryDelayMs": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["queryDelayMs"],
            "maxConcurrency": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["maxConcurrency"],
            "batchSize": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["batchSize"],
            "maxMembers": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["maxMembers"],
        },
        "required": [],
        "additionalProperties": False,
    },
}

GROUP_B50_RANK_AT_TOOL = {
    "name": "group_b50_rank_at",
    "description": (
        "按群 B50 缓存查询指定名次是谁。默认 rating 倒序，所以 rank=1 是群内最高 rating；"
        "sortOrder=asc 时 rank=1 是最低 rating。缓存超过 1 天或强制刷新时会启动后台刷新任务。"
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "groupId": {"type": "string", "description": "QQ群号。"},
            "rank": {
                "type": "integer",
                "minimum": 1,
                "description": "要查询的名次，1 表示当前排序下第一名。",
            },
            "sortOrder": {
                "type": "string",
                "enum": ["asc", "desc"],
                "description": "排名方向，默认 desc。desc 表示最高 rating 第 1，asc 表示最低 rating 第 1。",
            },
            "outputMode": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["outputMode"],
            "ratingMin": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["ratingMin"],
            "ratingMax": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["ratingMax"],
            "fitIndexMin": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["fitIndexMin"],
            "fitIndexMax": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["fitIndexMax"],
            "forceRefresh": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["forceRefresh"],
            "napcatBaseUrl": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["napcatBaseUrl"],
            "noCache": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["noCache"],
            "timeoutMs": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["timeoutMs"],
            "queryDelayMs": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["queryDelayMs"],
            "maxConcurrency": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["maxConcurrency"],
            "batchSize": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["batchSize"],
            "maxMembers": GROUP_B50_REPORT_TOOL["inputSchema"]["properties"]["maxMembers"],
        },
        "required": ["groupId", "rank"],
        "additionalProperties": False,
    },
}

CLEAR_GROUP_B50_CACHE_TOOL = {
    "name": "clear_group_b50_cache",
    "description": "清除指定群的 B50 榜单缓存和报告文件。",
    "inputSchema": {
        "type": "object",
        "properties": {
            "groupId": {"type": "string", "description": "QQ群号。"},
        },
        "required": ["groupId"],
        "additionalProperties": False,
    },
}

from . import song_rank as _song_rank


TOOLS = [
    GROUP_B50_REPORT_TOOL,
    GROUP_B50_CACHE_STATUS_TOOL,
    GROUP_B50_JOB_STATUS_TOOL,
    GROUP_B50_MEMBER_RANK_TOOL,
    GROUP_B50_RANK_AT_TOOL,
    CLEAR_GROUP_B50_CACHE_TOOL,
    *_song_rank.TOOLS,
]


def normalize_identifier(value: Any, field_name: str) -> str:
    if isinstance(value, int):
        value = str(value)
    if not isinstance(value, str) or not value.strip():
        raise GroupB50Error(f"必须提供 {field_name}。", code="INVALID_INPUT")
    return value.strip()


def normalize_timeout_ms(value: Any) -> int:
    if value is None:
        return 10000
    if not isinstance(value, int) or value < 1000 or value > 60000:
        raise GroupB50Error("timeoutMs 必须是 1000 到 60000 之间的整数。", code="INVALID_INPUT")
    return value


def normalize_query_delay_ms(value: Any) -> int:
    if value is None:
        configured = os.environ.get("GROUP_B50_QUERY_DELAY_MS")
        if configured and configured.isdigit():
            return max(0, min(int(configured), 10000))
        return 250
    if not isinstance(value, int) or value < 0 or value > 10000:
        raise GroupB50Error("queryDelayMs 必须是 0 到 10000 之间的整数。", code="INVALID_INPUT")
    return value


def normalize_sort_order(value: Any) -> str:
    if value is None:
        return "asc"
    if value not in {"asc", "desc"}:
        raise GroupB50Error("sortOrder 必须是 asc 或 desc。", code="INVALID_INPUT")
    return str(value)


def normalize_sort_by(value: Any) -> str:
    if value is None:
        return "rating"
    if value not in {"rating", "fitIndex"}:
        raise GroupB50Error("sortBy 必须是 rating 或 fitIndex。", code="INVALID_INPUT")
    return str(value)


def normalize_fit_index_bound(value: Any, field_name: str) -> float | None:
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise GroupB50Error(f"{field_name} 必须是数字。", code="INVALID_INPUT")
    return float(value)


def normalize_output_mode(value: Any) -> str:
    if value is None:
        return "rating"
    if value not in {"rating", "detail"}:
        raise GroupB50Error("outputMode 必须是 rating 或 detail。", code="INVALID_INPUT")
    return str(value)


def normalize_rating_bound(value: Any, field_name: str) -> int | None:
    if value is None:
        return None
    if not isinstance(value, int) or value < 0:
        raise GroupB50Error(f"{field_name} 必须是非负整数。", code="INVALID_INPUT")
    return value


def normalize_output_limit(value: Any) -> int | None:
    if value is None:
        return None
    if not isinstance(value, int) or value < 1:
        raise GroupB50Error("outputLimit 必须是正整数。", code="INVALID_INPUT")
    return value


def normalize_optional_rank_bound(value: Any, field_name: str) -> int | None:
    if value is None:
        return None
    if not isinstance(value, int) or value < 1:
        raise GroupB50Error(f"{field_name} 必须是正整数。", code="INVALID_INPUT")
    return value


def normalize_rank_number(value: Any) -> int:
    if not isinstance(value, int) or value < 1:
        raise GroupB50Error("rank 必须是正整数。", code="INVALID_INPUT")
    return value


def normalize_rank_at_sort_order(value: Any) -> str:
    if value is None:
        return "desc"
    return normalize_sort_order(value)


def normalize_context_size(value: Any) -> int:
    if value is None:
        return 3
    if not isinstance(value, int) or value < 0 or value > 10:
        raise GroupB50Error("contextSize 必须是 0 到 10 之间的整数。", code="INVALID_INPUT")
    return value


def normalize_max_concurrency(value: Any) -> int:
    if value is None:
        configured = os.environ.get("GROUP_B50_MAX_CONCURRENCY")
        if configured and configured.isdigit():
            return max(1, min(int(configured), 20))
        return 3
    if not isinstance(value, int) or value < 1 or value > 20:
        raise GroupB50Error("maxConcurrency 必须是 1 到 20 之间的整数。", code="INVALID_INPUT")
    return value


def normalize_batch_size(value: Any) -> int:
    if value is None:
        configured = os.environ.get("GROUP_B50_BATCH_SIZE")
        if configured and configured.isdigit():
            return max(1, min(int(configured), 100))
        return 100
    if not isinstance(value, int) or value < 1 or value > 100:
        raise GroupB50Error("batchSize 必须是 1 到 100 之间的整数。", code="INVALID_INPUT")
    return value


def get_cache_dir() -> Path:
    configured = os.environ.get("GROUP_B50_CACHE_DIR")
    if configured:
        return Path(configured).expanduser().resolve()
    return (Path.cwd() / "group-b50-cache").resolve()


def group_cache_dir(group_id: str) -> Path:
    return get_cache_dir() / group_id


def cache_json_path(group_id: str) -> Path:
    return group_cache_dir(group_id) / "cache.json"


def job_status_path(group_id: str) -> Path:
    return group_cache_dir(group_id) / "job_status.json"


def report_paths(group_id: str) -> dict[str, Path]:
    base = group_cache_dir(group_id)
    return {
        "asc": base / "rating_asc.md",
        "desc": base / "rating_desc.md",
    }


# now_iso 现由 job_runner 提供并在 import 处 re-export，保留这个注释作为路标。


def display_timezone() -> timezone:
    configured = (
        os.environ.get("GROUP_B50_DISPLAY_TZ")
        or os.environ.get("MCP_DISPLAY_TZ")
        or DEFAULT_DISPLAY_TIMEZONE
    )
    try:
        return ZoneInfo(configured)
    except (ZoneInfoNotFoundError, ValueError):
        return timezone(timedelta(hours=8))


def format_display_time(value: Any, *, missing: str = "未知") -> str:
    if not isinstance(value, str) or not value.strip():
        return missing
    try:
        parsed = datetime.fromisoformat(value)
    except ValueError:
        return value
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    local = parsed.astimezone(display_timezone())
    offset = local.strftime("%z")
    if len(offset) == 5:
        offset = f"{offset[:3]}:{offset[3:]}"
    return f"{local:%Y-%m-%d %H:%M:%S} {offset}"


def parse_iso_timestamp(value: Any) -> datetime | None:
    if not isinstance(value, str):
        return None
    try:
        parsed = datetime.fromisoformat(value)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    return parsed


def cache_age_seconds(cache: dict[str, Any] | None) -> float | None:
    if not cache:
        return None
    fetched_at = parse_iso_timestamp(cache.get("fetchedAt"))
    if not fetched_at:
        return None
    return (datetime.now(timezone.utc) - fetched_at).total_seconds()


def is_cache_fresh(cache: dict[str, Any] | None) -> bool:
    fetched_at = parse_iso_timestamp(cache.get("fetchedAt")) if cache else None
    if not fetched_at:
        return False
    return fetched_at >= _daily_reset_time(datetime.now(timezone.utc))


def read_cache(group_id: str) -> dict[str, Any] | None:
    path = cache_json_path(group_id)
    if not path.exists():
        return None
    try:
        parsed = json.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:
        raise GroupB50Error(f"读取群缓存失败：{exc}", code="CACHE_ERROR") from exc
    return parsed if isinstance(parsed, dict) else None


def write_cache(group_id: str, cache: dict[str, Any]) -> None:
    path = cache_json_path(group_id)
    try:
        write_json_atomic(path, cache)
    except Exception as exc:
        raise GroupB50Error(f"写入群缓存失败：{exc}", code="CACHE_ERROR") from exc


def read_job_status(group_id: str) -> dict[str, Any] | None:
    return _read_job_status(job_status_path(group_id))


def write_job_status(group_id: str, status: dict[str, Any]) -> None:
    _write_job_status(job_status_path(group_id), status)


def clear_cache(group_id: str) -> dict[str, Any]:
    base = group_cache_dir(group_id)
    with REFRESH_STATE_LOCK:
        existed = base.exists()
        if existed:
            shutil.rmtree(base)
    return {
        "groupId": group_id,
        "cleared": existed,
        "cacheDir": str(base),
    }


def cache_status(arguments: dict[str, Any]) -> dict[str, Any]:
    group_id = normalize_identifier(arguments.get("groupId"), "groupId")
    cache = read_cache(group_id)
    paths = report_paths(group_id)
    age = cache_age_seconds(cache)
    return {
        "groupId": group_id,
        "cacheExists": cache is not None,
        "fresh": is_cache_fresh(cache),
        "ageSeconds": age,
        "nextResetAt": _next_daily_reset_iso(),
        "fetchedAt": cache.get("fetchedAt") if cache else None,
        "memberCount": cache.get("memberCount") if cache else None,
        "successCount": cache.get("successCount") if cache else None,
        "failureCount": cache.get("failureCount") if cache else None,
        "skippedCount": cache.get("skippedCount") if cache else None,
        "skipPolicy": cache.get("skipPolicy") if cache else None,
        "job": read_job_status(group_id),
        "cacheScope": "full_group_detailed_b50",
        "containsDetailedB50": cache_contains_detailed_b50(cache),
        "cachePath": str(cache_json_path(group_id)),
        "reportFiles": {key: str(path) for key, path in paths.items()},
        "reportFilesExist": {key: path.exists() for key, path in paths.items()},
    }


def group_b50_report(arguments: dict[str, Any]) -> dict[str, Any]:
    options = normalize_report_options(arguments)
    group_id = options["groupId"]

    cache = read_cache(group_id)
    cache_was_present = cache is not None
    force_refresh = arguments.get("forceRefresh") is True
    refresh_reason = None
    if force_refresh:
        clear_cache(group_id)
        cache = None
        refresh_reason = "forceRefresh"
    elif cache_was_present and not is_cache_fresh(cache):
        clear_cache(group_id)
        cache = None
        refresh_reason = "stale"

    if cache is None:
        job = start_refresh_job(
            group_id,
            arguments,
            refresh_reason=refresh_reason or "miss",
            timeout_ms=options["timeoutMs"],
            query_delay_ms=options["queryDelayMs"],
            max_concurrency=options["maxConcurrency"],
            batch_size=options["batchSize"],
            max_members=options["maxMembers"],
        )
        status = cache_status({"groupId": group_id})
        text = format_job_started_text(group_id, job, status)
        return {
            "groupId": group_id,
            "sortBy": options["sortBy"],
            "sortOrder": options["sortOrder"],
            "outputMode": options["outputMode"],
            "ratingMin": options["ratingMin"],
            "ratingMax": options["ratingMax"],
            "fitIndexMin": options["fitIndexMin"],
            "fitIndexMax": options["fitIndexMax"],
            "outputLimit": options["outputLimit"],
            "startRank": options["startRank"],
            "endRank": options["endRank"],
            "cache": status,
            "job": job,
            "cacheRefreshReason": refresh_reason or "miss",
            "text": text,
            "data": None,
        }

    return format_cached_report(cache, options)


def normalize_report_options(arguments: dict[str, Any]) -> dict[str, Any]:
    group_id = normalize_identifier(arguments.get("groupId"), "groupId")
    sort_by = normalize_sort_by(arguments.get("sortBy"))
    sort_order = normalize_sort_order(arguments.get("sortOrder"))
    output_mode = normalize_output_mode(arguments.get("outputMode"))
    rating_min = normalize_rating_bound(arguments.get("ratingMin"), "ratingMin")
    rating_max = normalize_rating_bound(arguments.get("ratingMax"), "ratingMax")
    if rating_min is not None and rating_max is not None and rating_min > rating_max:
        raise GroupB50Error("ratingMin 不能大于 ratingMax。", code="INVALID_INPUT")
    fit_index_min = normalize_fit_index_bound(arguments.get("fitIndexMin"), "fitIndexMin")
    fit_index_max = normalize_fit_index_bound(arguments.get("fitIndexMax"), "fitIndexMax")
    if fit_index_min is not None and fit_index_max is not None and fit_index_min > fit_index_max:
        raise GroupB50Error("fitIndexMin 不能大于 fitIndexMax。", code="INVALID_INPUT")
    output_limit = normalize_output_limit(arguments.get("outputLimit"))
    start_rank = normalize_optional_rank_bound(arguments.get("startRank"), "startRank")
    end_rank = normalize_optional_rank_bound(arguments.get("endRank"), "endRank")
    if (start_rank is None) != (end_rank is None):
        raise GroupB50Error("startRank 和 endRank 必须同时提供。", code="INVALID_INPUT")
    if start_rank is not None and end_rank is not None and start_rank > end_rank:
        raise GroupB50Error("startRank 不能大于 endRank。", code="INVALID_INPUT")
    timeout_ms = normalize_timeout_ms(arguments.get("timeoutMs"))
    query_delay_ms = normalize_query_delay_ms(arguments.get("queryDelayMs"))
    max_concurrency = normalize_max_concurrency(arguments.get("maxConcurrency"))
    batch_size = normalize_batch_size(arguments.get("batchSize"))
    max_members = arguments.get("maxMembers")
    if max_members is not None and (not isinstance(max_members, int) or max_members < 1):
        raise GroupB50Error("maxMembers 必须是正整数。", code="INVALID_INPUT")
    return {
        "groupId": group_id,
        "sortBy": sort_by,
        "sortOrder": sort_order,
        "outputMode": output_mode,
        "ratingMin": rating_min,
        "ratingMax": rating_max,
        "fitIndexMin": fit_index_min,
        "fitIndexMax": fit_index_max,
        "outputLimit": output_limit,
        "startRank": start_rank,
        "endRank": end_rank,
        "timeoutMs": timeout_ms,
        "queryDelayMs": query_delay_ms,
        "maxConcurrency": max_concurrency,
        "batchSize": batch_size,
        "maxMembers": max_members,
    }


def format_cached_report(
    cache: dict[str, Any],
    options: dict[str, Any],
    *,
    cache_refresh_reason: str = "hit",
) -> dict[str, Any]:
    display_cache = {**cache, "cacheRefreshReason": cache_refresh_reason}
    reports = write_reports(display_cache)
    text = format_response(
        display_cache,
        sort_by=options.get("sortBy", "rating"),
        sort_order=options["sortOrder"],
        output_mode=options["outputMode"],
        rating_min=options["ratingMin"],
        rating_max=options["ratingMax"],
        fit_index_min=options.get("fitIndexMin"),
        fit_index_max=options.get("fitIndexMax"),
        output_limit=options["outputLimit"],
        start_rank=options["startRank"],
        end_rank=options["endRank"],
    )
    status = cache_status({"groupId": options["groupId"]})
    return {
        "groupId": options["groupId"],
        "sortBy": options.get("sortBy", "rating"),
        "sortOrder": options["sortOrder"],
        "outputMode": options["outputMode"],
        "ratingMin": options["ratingMin"],
        "ratingMax": options["ratingMax"],
        "fitIndexMin": options.get("fitIndexMin"),
        "fitIndexMax": options.get("fitIndexMax"),
        "outputLimit": options["outputLimit"],
        "startRank": options["startRank"],
        "endRank": options["endRank"],
        "cache": status,
        "cacheRefreshReason": display_cache.get("cacheRefreshReason"),
        "reportFiles": {key: str(path) for key, path in reports.items()},
        "text": text,
        "data": display_cache,
    }


def group_b50_job_status(arguments: dict[str, Any]) -> dict[str, Any]:
    options = normalize_report_options({**arguments, "timeoutMs": arguments.get("timeoutMs", 10000)})
    group_id = options["groupId"]
    job = read_job_status(group_id)
    cache = read_cache(group_id)

    if job and job.get("status") == "completed" and cache:
        refresh_reason = cache.get("cacheRefreshReason") or job.get("refreshReason") or "completed"
        report = format_cached_report(cache, options, cache_refresh_reason=refresh_reason)
        report["job"] = job
        report["text"] = "后台刷新已完成。\n\n" + report["text"]
        return report

    if cache and not job:
        report = format_cached_report(cache, options)
        report["job"] = None
        return report

    text = format_job_status_text(group_id, job, cache_status({"groupId": group_id}))
    return {
        "groupId": group_id,
        "sortOrder": options["sortOrder"],
        "outputMode": options["outputMode"],
        "ratingMin": options["ratingMin"],
        "ratingMax": options["ratingMax"],
        "outputLimit": options["outputLimit"],
        "cache": cache_status({"groupId": group_id}),
        "job": job,
        "text": text,
        "data": cache,
    }


def group_b50_member_rank(arguments: dict[str, Any]) -> dict[str, Any]:
    options = normalize_rank_options(arguments)
    group_id = options["groupId"]
    qq = options["qq"]

    cache = read_cache(group_id)
    cache_was_present = cache is not None
    force_refresh = arguments.get("forceRefresh") is True
    refresh_reason = None
    if force_refresh:
        clear_cache(group_id)
        cache = None
        refresh_reason = "forceRefresh"
    elif cache_was_present and not is_cache_fresh(cache):
        clear_cache(group_id)
        cache = None
        refresh_reason = "stale"

    if cache is None:
        job = start_refresh_job(
            group_id,
            arguments,
            refresh_reason=refresh_reason or "miss",
            timeout_ms=options["timeoutMs"],
            query_delay_ms=options["queryDelayMs"],
            max_concurrency=options["maxConcurrency"],
            batch_size=options["batchSize"],
            max_members=options["maxMembers"],
        )
        status = cache_status({"groupId": group_id})
        text = format_member_rank_job_started_text(group_id, qq, job, status)
        return {
            "groupId": group_id,
            "qq": qq,
            "outputMode": options["outputMode"],
            "contextSize": options["contextSize"],
            "cache": status,
            "job": job,
            "cacheRefreshReason": refresh_reason or "miss",
            "text": text,
            "data": None,
        }

    cache["cacheRefreshReason"] = "hit"
    result = build_member_rank_result(
        cache,
        qq,
        output_mode=options["outputMode"],
        context_size=options["contextSize"],
    )
    result["cache"] = cache_status({"groupId": group_id})
    result["cacheRefreshReason"] = cache.get("cacheRefreshReason")
    return result


def group_b50_rank_at(arguments: dict[str, Any]) -> dict[str, Any]:
    options = normalize_rank_at_options(arguments)
    group_id = options["groupId"]

    cache = read_cache(group_id)
    cache_was_present = cache is not None
    force_refresh = arguments.get("forceRefresh") is True
    refresh_reason = None
    if force_refresh:
        clear_cache(group_id)
        cache = None
        refresh_reason = "forceRefresh"
    elif cache_was_present and not is_cache_fresh(cache):
        clear_cache(group_id)
        cache = None
        refresh_reason = "stale"

    if cache is None:
        job = start_refresh_job(
            group_id,
            arguments,
            refresh_reason=refresh_reason or "miss",
            timeout_ms=options["timeoutMs"],
            query_delay_ms=options["queryDelayMs"],
            max_concurrency=options["maxConcurrency"],
            batch_size=options["batchSize"],
            max_members=options["maxMembers"],
        )
        status = cache_status({"groupId": group_id})
        text = format_rank_at_job_started_text(group_id, options["rank"], options["sortOrder"], job, status)
        return {
            "groupId": group_id,
            "rank": options["rank"],
            "sortOrder": options["sortOrder"],
            "outputMode": options["outputMode"],
            "ratingMin": options["ratingMin"],
            "ratingMax": options["ratingMax"],
            "cache": status,
            "job": job,
            "cacheRefreshReason": refresh_reason or "miss",
            "text": text,
            "data": None,
        }

    cache["cacheRefreshReason"] = "hit"
    result = build_rank_at_result(
        cache,
        rank=options["rank"],
        sort_order=options["sortOrder"],
        output_mode=options["outputMode"],
        rating_min=options["ratingMin"],
        rating_max=options["ratingMax"],
        fit_index_min=options.get("fitIndexMin"),
        fit_index_max=options.get("fitIndexMax"),
    )
    result["cache"] = cache_status({"groupId": group_id})
    result["cacheRefreshReason"] = cache.get("cacheRefreshReason")
    return result


def normalize_rank_at_options(arguments: dict[str, Any]) -> dict[str, Any]:
    group_id = normalize_identifier(arguments.get("groupId"), "groupId")
    rank = normalize_rank_number(arguments.get("rank"))
    sort_order = normalize_rank_at_sort_order(arguments.get("sortOrder"))
    output_mode = normalize_output_mode(arguments.get("outputMode"))
    rating_min = normalize_rating_bound(arguments.get("ratingMin"), "ratingMin")
    rating_max = normalize_rating_bound(arguments.get("ratingMax"), "ratingMax")
    if rating_min is not None and rating_max is not None and rating_min > rating_max:
        raise GroupB50Error("ratingMin 不能大于 ratingMax。", code="INVALID_INPUT")
    fit_index_min = normalize_fit_index_bound(arguments.get("fitIndexMin"), "fitIndexMin")
    fit_index_max = normalize_fit_index_bound(arguments.get("fitIndexMax"), "fitIndexMax")
    if fit_index_min is not None and fit_index_max is not None and fit_index_min > fit_index_max:
        raise GroupB50Error("fitIndexMin 不能大于 fitIndexMax。", code="INVALID_INPUT")
    timeout_ms = normalize_timeout_ms(arguments.get("timeoutMs"))
    query_delay_ms = normalize_query_delay_ms(arguments.get("queryDelayMs"))
    max_concurrency = normalize_max_concurrency(arguments.get("maxConcurrency"))
    batch_size = normalize_batch_size(arguments.get("batchSize"))
    max_members = arguments.get("maxMembers")
    if max_members is not None and (not isinstance(max_members, int) or max_members < 1):
        raise GroupB50Error("maxMembers 必须是正整数。", code="INVALID_INPUT")
    return {
        "groupId": group_id,
        "rank": rank,
        "sortOrder": sort_order,
        "outputMode": output_mode,
        "ratingMin": rating_min,
        "ratingMax": rating_max,
        "fitIndexMin": fit_index_min,
        "fitIndexMax": fit_index_max,
        "timeoutMs": timeout_ms,
        "queryDelayMs": query_delay_ms,
        "maxConcurrency": max_concurrency,
        "batchSize": batch_size,
        "maxMembers": max_members,
    }


def normalize_rank_options(arguments: dict[str, Any]) -> dict[str, Any]:
    group_id = normalize_optional_identifier(arguments.get("groupId"))
    qq = normalize_optional_identifier(arguments.get("qq"))
    target = normalize_optional_identifier(arguments.get("target"))
    if not qq and target:
        resolved = resolve_rank_target(target, group_id=group_id)
        qq = resolved["qq"]
        group_id = resolved["groupId"]
    if not qq:
        raise GroupB50Error("必须提供 qq 或 target。", code="INVALID_INPUT")
    if not group_id:
        group_id = infer_rank_group_id(qq)
    output_mode = normalize_output_mode(arguments.get("outputMode"))
    timeout_ms = normalize_timeout_ms(arguments.get("timeoutMs"))
    query_delay_ms = normalize_query_delay_ms(arguments.get("queryDelayMs"))
    max_concurrency = normalize_max_concurrency(arguments.get("maxConcurrency"))
    batch_size = normalize_batch_size(arguments.get("batchSize"))
    context_size = normalize_context_size(arguments.get("contextSize"))
    max_members = arguments.get("maxMembers")
    if max_members is not None and (not isinstance(max_members, int) or max_members < 1):
        raise GroupB50Error("maxMembers 必须是正整数。", code="INVALID_INPUT")
    return {
        "groupId": group_id,
        "qq": qq,
        "outputMode": output_mode,
        "timeoutMs": timeout_ms,
        "queryDelayMs": query_delay_ms,
        "maxConcurrency": max_concurrency,
        "batchSize": batch_size,
        "contextSize": context_size,
        "maxMembers": max_members,
    }


def normalize_optional_identifier(value: Any) -> str | None:
    if isinstance(value, int):
        value = str(value)
    if not isinstance(value, str) or not value.strip():
        return None
    return value.strip()


def resolve_rank_target(target: str, *, group_id: str | None) -> dict[str, str]:
    result = resolve_identities(target, group_id=group_id, max_results=20)
    matches = result.get("matches") if isinstance(result.get("matches"), list) else []
    if not matches:
        raise GroupB50Error(
            f"没有从 QQ 身份缓存中找到：{target}",
            code="IDENTITY_NOT_FOUND",
        )

    if group_id:
        if result.get("ambiguous") or len(matches) > 1 and matches[0].get("matchScore") == matches[1].get("matchScore"):
            raise_ambiguous_identity(target, matches)
        qq = matches[0].get("qq")
        if not isinstance(qq, str) or not qq:
            raise GroupB50Error(f"昵称“{target}”没有可用 QQ。", code="IDENTITY_NOT_FOUND")
        return {"qq": qq, "groupId": group_id}

    exact_group_matches = unique_rank_pairs(group_name_matches(matches, target, exact_only=True))
    exact_user_matches = unique_identity_matches(user_name_matches(matches, target, exact_only=True))
    if exact_group_matches:
        group_qqs = {pair.get("qq") for pair in exact_group_matches}
        user_qqs = {item.get("qq") for item in exact_user_matches}
        if len(group_qqs) == 1 and (not user_qqs or user_qqs <= group_qqs):
            return resolve_unique_qq_from_group_pairs(target, exact_group_matches, next(iter(group_qqs)))
        raise_ambiguous_identity(target, matches)

    if exact_user_matches:
        if len(exact_user_matches) == 1:
            qq = exact_user_matches[0].get("qq")
            if not isinstance(qq, str) or not qq:
                raise GroupB50Error(f"昵称“{target}”没有可用 QQ。", code="IDENTITY_NOT_FOUND")
            return {"qq": qq, "groupId": infer_rank_group_id(qq, identity=exact_user_matches[0])}
        raise_ambiguous_identity(target, exact_user_matches)

    fuzzy_group_matches = unique_rank_pairs(group_name_matches(matches, target, exact_only=False))
    fuzzy_user_matches = unique_identity_matches(user_name_matches(matches, target, exact_only=False))
    if fuzzy_group_matches:
        group_qqs = {pair.get("qq") for pair in fuzzy_group_matches}
        user_qqs = {item.get("qq") for item in fuzzy_user_matches}
        if len(group_qqs) == 1 and (not user_qqs or user_qqs <= group_qqs):
            return resolve_unique_qq_from_group_pairs(target, fuzzy_group_matches, next(iter(group_qqs)))
        raise_ambiguous_identity(target, matches)

    if fuzzy_user_matches:
        if len(fuzzy_user_matches) == 1:
            qq = fuzzy_user_matches[0].get("qq")
            if not isinstance(qq, str) or not qq:
                raise GroupB50Error(f"昵称“{target}”没有可用 QQ。", code="IDENTITY_NOT_FOUND")
            return {"qq": qq, "groupId": infer_rank_group_id(qq, identity=fuzzy_user_matches[0])}
        raise_ambiguous_identity(target, fuzzy_user_matches)

    if result.get("ambiguous") or len(matches) > 1 and matches[0].get("matchScore") == matches[1].get("matchScore"):
        raise_ambiguous_identity(target, matches)

    qq = matches[0].get("qq")
    if not isinstance(qq, str) or not qq:
        raise GroupB50Error(f"昵称“{target}”没有可用 QQ。", code="IDENTITY_NOT_FOUND")
    return {"qq": qq, "groupId": infer_rank_group_id(qq, identity=matches[0])}


def infer_rank_group_id(qq: str, identity: dict[str, Any] | None = None) -> str:
    identity = identity or get_identity(qq)
    if not isinstance(identity, dict):
        raise GroupB50Error(
            f"QQ {qq} 不在 QQ 身份缓存中，无法判断要查哪个群的排名。",
            code="GROUP_NOT_FOUND",
        )
    groups = [group for group in identity.get("groups") or [] if isinstance(group, dict) and group.get("groupId")]
    if len(groups) == 1:
        return str(groups[0]["groupId"])
    if not groups:
        raise GroupB50Error(
            f"QQ {qq} 没有群成员缓存记录，无法判断要查哪个群的排名。",
            code="GROUP_NOT_FOUND",
        )
    raise_ambiguous_group(
        str(qq),
        [{"qq": qq, "groupId": str(group.get("groupId")), "groupName": group.get("groupName"), "groupNickname": group.get("groupNickname")} for group in groups],
    )
    raise AssertionError("unreachable")


def resolve_unique_qq_from_group_pairs(target: str, pairs: list[dict[str, str]], qq: Any) -> dict[str, str]:
    if not isinstance(qq, str) or not qq:
        raise GroupB50Error(f"昵称“{target}”没有可用 QQ。", code="IDENTITY_NOT_FOUND")
    if len(pairs) == 1:
        return pairs[0]
    unique_group_ids = {pair.get("groupId") for pair in pairs if pair.get("groupId")}
    if len(unique_group_ids) == 1:
        return pairs[0]
    raise_ambiguous_group(target, pairs)
    raise AssertionError("unreachable")


def user_name_matches(matches: list[dict[str, Any]], target: str, *, exact_only: bool) -> list[dict[str, Any]]:
    matched = []
    for item in matches:
        qq = item.get("qq")
        if not isinstance(qq, str) or not qq:
            continue
        if target_matches(qq, target, exact_only=exact_only):
            matched.append(item)
            continue
        values = [
            item.get("qqNickname"),
            item.get("friendNickname"),
            item.get("waterfishNickname"),
            item.get("waterfishUsername"),
        ]
        if any(target_matches(value, target, exact_only=exact_only) for value in values):
            matched.append(item)
    return matched


def group_name_matches(matches: list[dict[str, Any]], target: str, *, exact_only: bool) -> list[dict[str, str]]:
    pairs = []
    for item in matches:
        qq = item.get("qq")
        if not isinstance(qq, str) or not qq:
            continue
        for group in item.get("groups") or []:
            if not isinstance(group, dict) or not group.get("groupId"):
                continue
            values = [group.get("groupNickname"), group.get("card"), group.get("nickname")]
            if any(target_matches(value, target, exact_only=exact_only) for value in values):
                pairs.append(
                    {
                        "qq": qq,
                        "groupId": str(group["groupId"]),
                        "groupName": group.get("groupName"),
                        "groupNickname": group.get("groupNickname"),
                    }
                )
    return pairs


def target_matches(value: Any, target: str, *, exact_only: bool) -> bool:
    if not isinstance(value, str) or not value.strip():
        return False
    normalized_value = value.strip().casefold()
    normalized_target = target.strip().casefold()
    if exact_only:
        return normalized_value == normalized_target
    return normalized_target in normalized_value


def unique_identity_matches(matches: list[dict[str, Any]]) -> list[dict[str, Any]]:
    seen = set()
    unique = []
    for item in matches:
        qq = item.get("qq")
        if not isinstance(qq, str) or not qq or qq in seen:
            continue
        seen.add(qq)
        unique.append(item)
    return unique


def unique_rank_pairs(pairs: list[dict[str, Any]]) -> list[dict[str, str]]:
    seen = set()
    unique = []
    for pair in pairs:
        key = (pair.get("qq"), pair.get("groupId"))
        if key in seen:
            continue
        seen.add(key)
        unique.append(pair)
    return unique


def raise_ambiguous_identity(target: str, matches: list[dict[str, Any]]) -> None:
    candidates = [
        {
            "qq": item.get("qq"),
            "qqNickname": item.get("qqNickname"),
            "waterfishNickname": item.get("waterfishNickname"),
            "groups": item.get("groups"),
            "matchedFields": item.get("matchedFields"),
        }
        for item in matches[:10]
    ]
    raise GroupB50Error(
        f"昵称“{target}”匹配到多个 QQ，请让用户选择具体 QQ。",
        code="AMBIGUOUS_IDENTITY",
        body=json.dumps(candidates, ensure_ascii=False),
    )


def raise_ambiguous_group(target: str, groups: list[dict[str, Any]]) -> None:
    raise GroupB50Error(
        f"“{target}”对应多个群，无法判断要查哪个群的排名，请让用户选择群号。",
        code="AMBIGUOUS_GROUP",
        body=json.dumps(groups[:20], ensure_ascii=False),
    )


def start_refresh_job(
    group_id: str,
    arguments: dict[str, Any],
    *,
    refresh_reason: str,
    timeout_ms: int,
    query_delay_ms: int,
    max_concurrency: int,
    batch_size: int,
    max_members: int | None,
) -> dict[str, Any]:
    def runner(job_id: str) -> None:
        run_refresh_job(
            group_id,
            job_id,
            arguments,
            refresh_reason,
            timeout_ms,
            query_delay_ms,
            max_concurrency,
            batch_size,
            max_members,
        )

    return job_runner.start_refresh_job(
        feature=B50_FEATURE,
        group_id=group_id,
        status_path=job_status_path(group_id),
        refresh_reason=refresh_reason,
        runner=runner,
        start_message="后台刷新已启动，正在拉群成员并批量查询 B50。",
        thread_name=f"group-rank-refresh-{B50_FEATURE}-{group_id}",
    )


def run_refresh_job(
    group_id: str,
    job_id: str,
    arguments: dict[str, Any],
    refresh_reason: str,
    timeout_ms: int,
    query_delay_ms: int,
    max_concurrency: int,
    batch_size: int,
    max_members: int | None,
) -> None:
    try:
        cache = build_group_cache(
            group_id,
            job_id,
            arguments,
            timeout_ms=timeout_ms,
            query_delay_ms=query_delay_ms,
            max_concurrency=max_concurrency,
            batch_size=batch_size,
            max_members=max_members,
        )
        cache["cacheRefreshReason"] = refresh_reason
        with REFRESH_STATE_LOCK:
            current = ensure_current_refresh_job(job_status_path(group_id), job_id)
            write_cache(group_id, cache)
            reports = write_reports(cache)
            write_job_status(
                group_id,
                {
                    "jobId": job_id,
                    "groupId": group_id,
                    "status": "completed",
                    "startedAt": current.get("startedAt"),
                    "finishedAt": now_iso(),
                    "refreshReason": refresh_reason,
                    "message": "后台刷新已完成，可读取缓存榜单。",
                    "memberCount": cache.get("memberCount"),
                    "successCount": cache.get("successCount"),
                    "skippedCount": cache.get("skippedCount"),
                    "reportFiles": {key: str(path) for key, path in reports.items()},
                },
            )
    except StaleRefreshJob:
        return
    except Exception as exc:
        try:
            with REFRESH_STATE_LOCK:
                current = ensure_current_refresh_job(job_status_path(group_id), job_id)
                write_job_status(
                    group_id,
                    {
                        "jobId": job_id,
                        "groupId": group_id,
                        "status": "failed",
                        "startedAt": current.get("startedAt"),
                        "finishedAt": now_iso(),
                        "refreshReason": refresh_reason,
                        "message": str(exc),
                        "error": exc.to_dict()
                        if isinstance(exc, GroupB50Error)
                        else {"code": "UNKNOWN_ERROR", "message": str(exc)},
                    },
                )
        except StaleRefreshJob:
            return


def format_job_started_text(group_id: str, job: dict[str, Any], status: dict[str, Any]) -> str:
    return "\n".join(
        [
            f"群 {group_id} B50 缓存不存在、已过期或被要求刷新，已启动后台刷新任务。",
            f"任务状态: {job.get('status')}，启动时间: {format_display_time(job.get('startedAt'))}，原因: {job.get('refreshReason')}",
            "这次不会阻塞等待完整群查询，避免超过 AstrBot 30 秒 MCP 超时。",
            "稍后调用 group_b50_job_status 读取进度；完成后会返回正序/倒序文件路径和榜单。",
            f"缓存路径: {status.get('cachePath')}",
        ]
    )


def format_member_rank_job_started_text(
    group_id: str,
    qq: str,
    job: dict[str, Any],
    status: dict[str, Any],
) -> str:
    return "\n".join(
        [
            f"群 {group_id} 的 QQ {qq} 排名需要完整群榜缓存，当前已启动后台刷新任务。",
            f"任务状态: {job.get('status')}，启动时间: {format_display_time(job.get('startedAt'))}，原因: {job.get('refreshReason')}",
            "稍后再次调用 group_b50_member_rank 读取该 QQ 的排名；也可调用 group_b50_job_status 查看进度。",
            f"缓存路径: {status.get('cachePath')}",
        ]
    )


def format_rank_at_job_started_text(
    group_id: str,
    rank: int,
    sort_order: str,
    job: dict[str, Any],
    status: dict[str, Any],
) -> str:
    label = "倒序" if sort_order == "desc" else "正序"
    return "\n".join(
        [
            f"群 {group_id} 的 rating {label}第 {rank} 名需要完整群榜缓存，当前已启动后台刷新任务。",
            f"任务状态: {job.get('status')}，启动时间: {format_display_time(job.get('startedAt'))}，原因: {job.get('refreshReason')}",
            "稍后再次调用 group_b50_rank_at 读取该名次；也可调用 group_b50_job_status 查看进度。",
            f"缓存路径: {status.get('cachePath')}",
        ]
    )


def format_job_status_text(group_id: str, job: dict[str, Any] | None, status: dict[str, Any]) -> str:
    if not job:
        return f"群 {group_id} 当前没有后台刷新任务。\n缓存存在: {status.get('cacheExists')}，未过期: {status.get('fresh')}"
    lines = [
        f"群 {group_id} 后台刷新任务状态: {job.get('status')}",
        f"启动时间: {format_display_time(job.get('startedAt'))}",
        f"完成时间: {format_display_time(job.get('finishedAt'), missing='未完成')}",
        f"说明: {job.get('message')}",
    ]
    if job.get("status") == "running":
        lines.append(
            f"进度: {job.get('processedCount', 0)}/{job.get('totalCount', '?')}，"
            f"已缓存: {job.get('cachedCount', 0)}，跳过: {job.get('skippedCount', 0)}，"
            f"临时失败: {job.get('transientFailureCount', 0)}"
        )
    if job.get("status") == "failed" and job.get("error"):
        lines.append(f"错误: {job['error'].get('message')}")
    return "\n".join(lines)


def build_group_cache(
    group_id: str,
    job_id: str,
    arguments: dict[str, Any],
    *,
    timeout_ms: int,
    query_delay_ms: int,
    max_concurrency: int,
    batch_size: int,
    max_members: int | None,
) -> dict[str, Any]:
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
    member_by_qq = {str(member["userId"]): member for member in members}
    results: list[dict[str, Any]] = []
    skipped_count = 0
    transient_failures: list[dict[str, Any]] = []
    batch_counts = {"requested": 0, "success": 0, "failure": 0}
    cache_hit_count = 0
    qqs = [str(member["userId"]) for member in members]

    # 先扫一遍 player_cache：刚有人查过 B50 的成员直接复用，跳过水鱼调用。
    # 同时仍把命中的成员塞进 results、走和 query_b50_batch 一致的归一化路径。
    qqs_to_query: list[str] = []
    for qq in qqs:
        entry = read_player_b50(qq)
        if not is_player_b50_fresh(entry):
            qqs_to_query.append(qq)
            continue
        b50 = (entry or {}).get("b50") if isinstance(entry, dict) else None
        if not isinstance(b50, dict):
            qqs_to_query.append(qq)
            continue
        player = b50.get("player") if isinstance(b50.get("player"), dict) else {}
        rating = player.get("rating")
        if not isinstance(rating, (int, float)) or rating <= 0:
            # 缓存里有但 rating 无效（隐私/0），按现有 skipPolicy 不计入榜单。
            skipped_count += 1
            continue
        fit_index = summarize_fit_index_for_group(b50.get("fitIndex"))
        if not fit_index or not fit_index.get("available"):
            # 早期 B50 绘图为了速度会写入 chartMetadata.skipped=true 的轻量缓存。
            # 群榜需要虚高指数，不能把这种缓存当完整结果复用。
            qqs_to_query.append(qq)
            continue
        member = member_by_qq.get(qq, {"userId": qq, "displayName": qq})
        results.append(
            {
                "userId": qq,
                "displayName": member.get("displayName"),
                "nickname": member.get("nickname"),
                "card": member.get("card"),
                "identity": get_identity(qq, group_id),
                "ok": True,
                "rating": rating,
                "b50Rating": (b50.get("ratingBreakdown") or {}).get("total"),
                "fitIndex": fit_index,
                "player": player,
                "b50": b50,
                "error": None,
            }
        )
        cache_hit_count += 1

    write_refresh_progress(
        group_id,
        job_id,
        cache_hit_count + skipped_count,
        len(qqs),
        len(results),
        skipped_count,
        0,
        f"已从 player_cache 复用 {cache_hit_count} 人成绩，剩余 {len(qqs_to_query)} 人需调水鱼。",
    )

    for start in range(0, len(qqs_to_query), batch_size):
        batch_qqs = qqs_to_query[start : start + batch_size]
        batch = query_b50_batch_via_mcp(
            batch_qqs,
            group_id=group_id,
            timeout_ms=timeout_ms,
            query_delay_ms=query_delay_ms,
            max_concurrency=max_concurrency,
            progress_callback=make_b50_batch_progress_callback(
                group_id=group_id,
                job_id=job_id,
                total_members=len(qqs),
                already_processed=cache_hit_count + skipped_count + start,
                cache_hit_count=cache_hit_count,
                results=results,
                skipped_count=skipped_count,
                transient_failures=transient_failures,
            ),
        )
        counts = batch.get("counts") if isinstance(batch.get("counts"), dict) else {}
        for key in batch_counts:
            batch_counts[key] += int(counts.get(key) or 0)
        for item in batch.get("results") or []:
            qq = str(item.get("qq") or "")
            member = member_by_qq.get(qq, {"userId": qq, "displayName": qq})
            if item.get("ok"):
                b50 = item.get("result") if isinstance(item.get("result"), dict) else {}
                player = b50.get("player") if isinstance(b50.get("player"), dict) else {}
                rating = item.get("rating", player.get("rating"))
                if not isinstance(rating, (int, float)) or rating <= 0:
                    skipped_count += 1
                    continue
                update_identity_cache_from_b50(qq, b50, group_id=group_id)
                results.append(
                    {
                        "userId": qq,
                        "displayName": member.get("displayName"),
                        "nickname": member.get("nickname"),
                        "card": member.get("card"),
                        "identity": get_identity(qq, group_id),
                        "ok": True,
                        "rating": rating,
                        "b50Rating": item.get("b50Rating", (b50.get("ratingBreakdown") or {}).get("total")),
                        "fitIndex": summarize_fit_index_for_group(
                            item.get("fitIndex") or b50.get("fitIndex")
                        ),
                        "player": player,
                        "b50": b50,
                        "error": None,
                    }
                )
            else:
                error_info = item.get("error") if isinstance(item.get("error"), dict) else {}
                if is_transient_b50_error(error_info):
                    transient_failures.append(
                        {
                            "userId": qq,
                            "displayName": member.get("displayName"),
                            "error": {
                                "code": error_info.get("code"),
                                "message": error_info.get("message"),
                                "status": error_info.get("status"),
                            },
                        }
                    )
                else:
                    skipped_count += 1
        processed_in_batch = min(start + len(batch_qqs), len(qqs_to_query))
        processed = cache_hit_count + skipped_count + processed_in_batch
        write_refresh_progress(
            group_id,
            job_id,
            processed,
            len(qqs),
            len(results),
            skipped_count,
            len(transient_failures),
            f"已完成 {processed}/{len(qqs)} 个 QQ（缓存命中 {cache_hit_count}）。",
        )

    if transient_failures:
        raise GroupB50Error(
            format_transient_b50_failure_message(transient_failures),
            code="B50_TRANSIENT_FAILURE",
        )

    success_count = sum(1 for item in results if item.get("ok"))
    return {
        "groupId": group_id,
        "fetchedAt": now_iso(),
        "nextResetAt": _next_daily_reset_iso(),
        "cacheScope": "full_group_detailed_b50",
        "memberCount": len(members),
        "successCount": success_count,
        "failureCount": 0,
        "skippedCount": skipped_count,
        "cacheHitCount": cache_hit_count,
        "skipPolicy": "unqueryable_or_rating_lte_zero_not_cached",
        "members": members,
        "results": results,
        "b50BatchCounts": batch_counts,
    }


def load_group_members_from_identity_cache(group_id: str) -> list[dict[str, Any]] | None:
    try:
        cache = read_identity_cache()
    except Exception:
        return None
    if not is_identity_cache_fresh(cache):
        return None
    users = cache.get("users") if isinstance(cache.get("users"), dict) else {}
    members = []
    for qq, user in users.items():
        if not isinstance(user, dict):
            continue
        groups = user.get("groups") if isinstance(user.get("groups"), dict) else {}
        group = groups.get(group_id)
        if not isinstance(group, dict):
            continue
        nickname = user.get("qqNickname") or group.get("nickname")
        card = group.get("card")
        display_name = group.get("groupNickname") or card or nickname or qq
        members.append(
            {
                "groupId": group_id,
                "userId": str(qq),
                "nickname": nickname,
                "card": card,
                "displayName": display_name,
            }
        )
    if not members:
        return None
    members.sort(key=lambda item: str(item.get("userId") or ""))
    return members


def update_identity_cache_from_members(group_id: str, members: list[dict[str, Any]]) -> None:
    try:
        cache = read_identity_cache()
        for member in members:
            upsert_group_member(
                cache,
                group_id=group_id,
                qq=member.get("userId"),
                nickname=member.get("nickname"),
                card=member.get("card"),
            )
        write_identity_cache(cache)
    except Exception:
        pass


def update_identity_cache_from_b50(qq: str, b50: dict[str, Any], *, group_id: str) -> None:
    try:
        player = b50.get("player") if isinstance(b50.get("player"), dict) else {}
        upsert_waterfish_profile(
            qq,
            nickname=player.get("nickname"),
            username=player.get("username"),
            rating=player.get("rating"),
        )
    except Exception:
        pass


def summarize_fit_index_for_group(fit_index: Any) -> dict[str, Any] | None:
    if not isinstance(fit_index, dict):
        return None
    if "virtualRatio" in fit_index or "virtualRating" in fit_index:
        return {
            "available": bool(fit_index.get("available", fit_index.get("counted"))),
            "label": fit_index.get("label"),
            "virtualRating": fit_index.get("virtualRating"),
            "virtualRatio": fit_index.get("virtualRatio"),
            "counted": fit_index.get("counted") or 0,
            "missing": fit_index.get("missing") or 0,
        }
    b50 = fit_index.get("b50") if isinstance(fit_index.get("b50"), dict) else {}
    return {
        "available": bool(fit_index.get("available") and b50.get("counted")),
        "label": fit_index.get("label"),
        "virtualRating": b50.get("virtualRating"),
        "virtualRatio": b50.get("virtualRatio"),
        "counted": b50.get("counted") or 0,
        "missing": b50.get("missing") or 0,
    }


def write_refresh_progress(
    group_id: str,
    job_id: str,
    processed: int,
    total: int,
    cached_count: int,
    skipped_count: int,
    transient_failure_count: int,
    message: str,
) -> None:
    _write_refresh_progress(
        status_path=job_status_path(group_id),
        job_id=job_id,
        processed=processed,
        total=total,
        cached_count=cached_count,
        skipped_count=skipped_count,
        transient_failure_count=transient_failure_count,
        message=message,
    )


def make_b50_batch_progress_callback(
    *,
    group_id: str,
    job_id: str,
    total_members: int,
    already_processed: int,
    cache_hit_count: int,
    results: list[dict[str, Any]],
    skipped_count: int,
    transient_failures: list[dict[str, Any]],
) -> Callable[[dict[str, Any]], None]:
    def callback(progress: dict[str, Any]) -> None:
        completed = progress.get("completed")
        if not isinstance(completed, int):
            return
        processed = min(total_members, already_processed + completed)
        metadata_cache_size = progress.get("metadataCacheSize")
        metadata_note = (
            f"，metadata 复用池 {metadata_cache_size} 条"
            if isinstance(metadata_cache_size, int) and metadata_cache_size > 0
            else ""
        )
        try:
            write_refresh_progress(
                group_id,
                job_id,
                processed,
                total_members,
                len(results),
                skipped_count,
                len(transient_failures),
                f"已收到 {processed}/{total_members} 个 QQ 查询结果（缓存命中 {cache_hit_count}{metadata_note}），正在整理 B50。",
            )
        except Exception:
            pass

    return callback


def is_transient_b50_error(error_info: dict[str, Any]) -> bool:
    code = error_info.get("code")
    if code in TRANSIENT_B50_ERROR_CODES:
        return True
    status = error_info.get("status")
    return isinstance(status, int) and (status == 429 or status >= 500)


def format_transient_b50_failure_message(errors: list[dict[str, Any]]) -> str:
    samples = []
    for error in errors[:5]:
        detail = error.get("error") if isinstance(error.get("error"), dict) else {}
        samples.append(
            f"{error.get('userId')}/{error.get('displayName')}: "
            f"{detail.get('code') or 'ERROR'} {detail.get('message') or ''}".strip()
        )
    return (
        f"B50 批量查询存在 {len(errors)} 个临时网络或限流失败，已放弃写入缓存，"
        "避免把不完整榜单缓存 1 天。请稍后 forceRefresh 重试。"
        + (" 示例：" + "；".join(samples) if samples else "")
    )


def query_b50_batch_via_mcp(
    qqs: list[str],
    *,
    group_id: str,
    timeout_ms: int,
    query_delay_ms: int,
    max_concurrency: int,
    progress_callback: Callable[[dict[str, Any]], None] | None = None,
) -> dict[str, Any]:
    """同进程直调 diving_fish_b50_mcp.query_b50_batch。

    早期版本走 stdio 子进程，但因为 group-rank 和 diving-fish 跑在同一个 Python
    进程里，subprocess 只是多 ~100ms 启动 + JSON 序列化开销，没有实际隔离意义。
    现在改成 import 直调，并把 DivingFishError 转成 GroupB50Error 维持现有 API。
    """
    from diving_fish_b50_mcp.server import (
        DivingFishError,
        query_b50_batch as _query_b50_batch_impl,
    )

    try:
        query_args: dict[str, Any] = {
            "qqs": qqs,
            "topN": 50,
            "section": "b50",
            "includeRaw": False,
            "includeSummaries": False,
            "includeChartMetadata": True,
            "timeoutMs": min(timeout_ms, 30000),
            "queryDelayMs": query_delay_ms,
            "maxConcurrency": max_concurrency,
            "groupId": group_id,
        }
        if progress_callback is not None:
            query_args["_progressCallback"] = progress_callback
        return _query_b50_batch_impl(query_args)
    except DivingFishError as exc:
        raise GroupB50Error(
            f"B50 批量查询失败：{exc}",
            code=str(exc.code) if exc.code else "B50_MCP_ERROR",
            status=exc.status,
            body=exc.body,
        ) from exc


def normalize_napcat_base_url(value: Any) -> str:
    if isinstance(value, str) and value.strip():
        base_url = value.strip()
    else:
        base_url = os.environ.get("NAPCAT_BASE_URL", DEFAULT_NAPCAT_BASE_URL).strip()
    return base_url.rstrip("/")


def fetch_group_members(
    group_id: str,
    *,
    napcat_base_url: str,
    no_cache: bool,
    timeout_ms: int,
) -> list[dict[str, Any]]:
    payload = {"group_id": int(group_id) if group_id.isdigit() else group_id, "no_cache": no_cache}
    response = post_json(
        f"{napcat_base_url}/get_group_member_list",
        payload,
        timeout_ms=timeout_ms,
        service_name="NapCat",
    )
    raw_members = extract_member_list(response)
    members = [normalize_member(member) for member in raw_members if isinstance(member, dict)]
    members = [member for member in members if member is not None]
    if not members:
        raise GroupB50Error("NapCat 没有返回可用的群成员 QQ。", code="NO_MEMBERS")
    return members


def post_json(
    url: str,
    payload: dict[str, Any],
    *,
    timeout_ms: int,
    service_name: str,
) -> Any:
    body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=body,
        method="POST",
        headers={
            "Accept": "application/json",
            "Content-Type": "application/json",
            "User-Agent": f"{SERVER_NAME}/{__version__}",
        },
    )

    try:
        with urllib.request.urlopen(request, timeout=timeout_ms / 1000) as response:
            response_body = response.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as exc:
        error_body = exc.read().decode("utf-8", errors="replace")
        hint = ""
        if service_name == "NapCat" and exc.code == 404:
            hint = "。请确认 NapCat OneBot HTTP 已启用，且 NAPCAT_BASE_URL 指向该端口"
        raise GroupB50Error(
            f"{service_name} 请求失败：HTTP {exc.code}{hint}",
            code="HTTP_ERROR",
            status=exc.code,
            body=error_body,
        ) from exc
    except socket.timeout as exc:
        raise GroupB50Error(f"{service_name} 请求超时（{timeout_ms}ms）。", code="TIMEOUT") from exc
    except urllib.error.URLError as exc:
        reason = getattr(exc, "reason", exc)
        raise GroupB50Error(f"{service_name} 请求失败：{reason}", code="NETWORK_ERROR") from exc

    try:
        return json.loads(response_body)
    except json.JSONDecodeError as exc:
        raise GroupB50Error(
            f"{service_name} 返回了非 JSON 内容。",
            code="INVALID_JSON",
            body=response_body,
        ) from exc


def extract_member_list(response: Any) -> list[Any]:
    if isinstance(response, list):
        return response
    if not isinstance(response, dict):
        raise GroupB50Error("NapCat 返回的 JSON 结构不是对象或数组。", code="INVALID_JSON")
    if response.get("retcode") not in (None, 0):
        raise GroupB50Error(
            f"NapCat 返回错误：{response.get('message') or response.get('wording') or response.get('retcode')}",
            code="NAPCAT_ERROR",
            body=json.dumps(response, ensure_ascii=False),
        )
    data = response.get("data")
    if isinstance(data, list):
        return data
    raise GroupB50Error("NapCat 返回中没有 data 成员列表。", code="INVALID_JSON")


def normalize_member(member: dict[str, Any]) -> dict[str, Any] | None:
    user_id = member.get("user_id", member.get("userId"))
    if isinstance(user_id, int):
        user_id = str(user_id)
    if not isinstance(user_id, str) or not user_id.strip():
        return None
    nickname = optional_string(member.get("nickname"))
    card = optional_string(member.get("card"))
    display_name = card or nickname or user_id.strip()
    return {
        "groupId": member.get("group_id", member.get("groupId")),
        "userId": user_id.strip(),
        "nickname": nickname,
        "card": card,
        "displayName": display_name,
    }


def optional_string(value: Any) -> str | None:
    return value.strip() if isinstance(value, str) and value.strip() else None


def cache_contains_detailed_b50(cache: dict[str, Any] | None) -> bool:
    if not cache:
        return False
    for item in cache.get("results") or []:
        if not item.get("ok"):
            continue
        b50 = item.get("b50")
        charts = b50.get("charts") if isinstance(b50, dict) else None
        if not isinstance(charts, dict):
            return False
        if not isinstance(charts.get("sd"), list) or not isinstance(charts.get("dx"), list):
            return False
    return True


def sorted_results(
    cache: dict[str, Any],
    sort_order: str,
    *,
    rating_min: int | None = None,
    rating_max: int | None = None,
    sort_by: str = "rating",
    fit_index_min: float | None = None,
    fit_index_max: float | None = None,
) -> list[dict[str, Any]]:
    reverse = sort_order == "desc"
    # 排序前先用 player_cache.b50 叠加最新数据：刚查过自己 B50 的人在群榜里立刻看到
    # 新 rating，不用等群缓存重刷。筛选/排序都基于 overlay 后的数值。
    results = _overlay_results_with_player_cache(cache.get("results") or [])
    ok = [item for item in results if item.get("ok")]
    ok = [item for item in ok if rating_matches(item.get("rating"), rating_min, rating_max)]
    ok = [item for item in ok if fit_index_matches(item.get("fitIndex"), fit_index_min, fit_index_max)]
    if sort_by == "fitIndex":
        ok.sort(
            key=lambda item: (
                _fit_index_sort_value(item),
                item.get("rating") if isinstance(item.get("rating"), (int, float)) else -1,
                item.get("displayName") or "",
                item.get("userId") or "",
            ),
            reverse=reverse,
        )
    else:
        ok.sort(
            key=lambda item: (
                item.get("rating") if isinstance(item.get("rating"), (int, float)) else -1,
                item.get("displayName") or "",
                item.get("userId") or "",
            ),
            reverse=reverse,
        )
    return ok


def rank_sorted_results(cache: dict[str, Any], sort_order: str) -> list[dict[str, Any]]:
    return sorted_results(cache, sort_order, rating_min=None, rating_max=None)


def _overlay_results_with_player_cache(results: list[Any]) -> list[dict[str, Any]]:
    """对 ok=True 的 result 叠加 player_cache.b50 里更新的数据。

    覆盖五个字段：rating / b50Rating / fitIndex / player / b50。其他字段（群成员关系、
    身份、显示名）保留群缓存原值。fresh 判定走 player_cache 默认 TTL（1 天）。
    rating<=0 的 fresh 数据不覆盖——避免某人隐私设置改了之后污染榜单。
    """
    overlaid: list[dict[str, Any]] = []
    for item in results:
        if not isinstance(item, dict):
            continue
        if not item.get("ok"):
            overlaid.append(item)
            continue
        qq = str(item.get("userId") or "")
        if not qq:
            overlaid.append(item)
            continue
        fresh_entry = read_player_b50(qq)
        if not is_player_b50_fresh(fresh_entry):
            overlaid.append(item)
            continue
        fresh_b50 = fresh_entry.get("b50") if isinstance(fresh_entry, dict) else None
        if not isinstance(fresh_b50, dict):
            overlaid.append(item)
            continue
        player = fresh_b50.get("player") if isinstance(fresh_b50.get("player"), dict) else {}
        rating = player.get("rating")
        if not isinstance(rating, (int, float)) or rating <= 0:
            overlaid.append(item)
            continue
        rating_breakdown = fresh_b50.get("ratingBreakdown") if isinstance(fresh_b50.get("ratingBreakdown"), dict) else {}
        fresh_fit_index = summarize_fit_index_for_group(fresh_b50.get("fitIndex"))
        updated = {
            **item,
            "rating": rating,
            "b50Rating": rating_breakdown.get("total"),
            "fitIndex": fresh_fit_index or item.get("fitIndex"),
            "player": player,
            "b50": fresh_b50,
        }
        overlaid.append(updated)
    return overlaid


def rating_matches(rating: Any, rating_min: int | None, rating_max: int | None) -> bool:
    if rating_min is None and rating_max is None:
        return True
    if not isinstance(rating, (int, float)):
        return False
    if rating_min is not None and rating < rating_min:
        return False
    if rating_max is not None and rating > rating_max:
        return False
    return True


def fit_index_matches(fit_index: Any, low: float | None, high: float | None) -> bool:
    if low is None and high is None:
        return True
    if not isinstance(fit_index, dict) or not fit_index.get("available"):
        return False
    value = fit_index.get("virtualRatio")
    if not isinstance(value, (int, float)):
        return False
    if low is not None and value < low:
        return False
    if high is not None and value > high:
        return False
    return True


def _fit_index_sort_value(item: dict[str, Any]) -> float:
    fit_index = item.get("fitIndex") if isinstance(item.get("fitIndex"), dict) else None
    if not fit_index or not fit_index.get("available"):
        return float("-inf")
    value = fit_index.get("virtualRatio")
    return float(value) if isinstance(value, (int, float)) else float("-inf")


def write_reports(cache: dict[str, Any]) -> dict[str, Path]:
    paths = report_paths(str(cache["groupId"]))
    for sort_order, path in paths.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        text = format_group_report(cache, sort_order=sort_order, output_mode="detail")
        path.write_text(text + "\n", encoding="utf-8")
    return paths


def format_response(
    cache: dict[str, Any],
    *,
    sort_order: str,
    output_mode: str,
    rating_min: int | None,
    rating_max: int | None,
    output_limit: int | None,
    start_rank: int | None = None,
    end_rank: int | None = None,
    sort_by: str = "rating",
    fit_index_min: float | None = None,
    fit_index_max: float | None = None,
) -> str:
    filtered_rows = sorted_results(
        cache,
        sort_order,
        rating_min=rating_min,
        rating_max=rating_max,
        sort_by=sort_by,
        fit_index_min=fit_index_min,
        fit_index_max=fit_index_max,
    )
    rows = apply_rank_window(filtered_rows, output_limit, start_rank=start_rank, end_rank=end_rank)
    header = format_header(
        cache,
        sort_order=sort_order,
        rating_min=rating_min,
        rating_max=rating_max,
        visible_count=len(rows),
        matched_count=len(filtered_rows),
        output_limit=output_limit,
        start_rank=start_rank,
        end_rank=end_rank,
        sort_by=sort_by,
        fit_index_min=fit_index_min,
        fit_index_max=fit_index_max,
    )
    body = format_group_report(
        cache,
        sort_order=sort_order,
        output_mode=output_mode,
        rating_min=rating_min,
        rating_max=rating_max,
        rows=rows,
        sort_by=sort_by,
        fit_index_min=fit_index_min,
        fit_index_max=fit_index_max,
    )
    return header + "\n\n" + body


def format_header(
    cache: dict[str, Any],
    *,
    sort_order: str,
    rating_min: int | None,
    rating_max: int | None,
    visible_count: int,
    matched_count: int,
    output_limit: int | None,
    start_rank: int | None = None,
    end_rank: int | None = None,
    sort_by: str = "rating",
    fit_index_min: float | None = None,
    fit_index_max: float | None = None,
) -> str:
    paths = report_paths(str(cache["groupId"]))
    status = (
        "本次使用一天内缓存，未重新拉取"
        if cache.get("cacheRefreshReason") == "hit"
        else "本次已重新拉取并刷新缓存"
    )
    sort_field_label = "虚高指数" if sort_by == "fitIndex" else "rating"
    return "\n".join(
        [
            f"群 {cache.get('groupId')} B50 榜单（{sort_field_label} {'正序' if sort_order == 'asc' else '倒序'}）",
            f"缓存状态: {status}，缓存生成时间: {format_display_time(cache.get('fetchedAt'))}",
            f"筛选: rating {format_rating_filter(rating_min, rating_max)}，虚高指数 {format_fit_index_filter(fit_index_min, fit_index_max)}，输出: {format_rank_window_label(output_limit, start_rank, end_rank)}，匹配: {matched_count} 条，本次展示: {visible_count} 条",
            f"缓存内容: 仅保存可查询且 rating>0 的完整 B50 歌曲明细；群成员: {cache.get('memberCount', 0)}，已缓存: {cache.get('successCount', 0)}，跳过: {cache.get('skippedCount', 0)}",
            f"正序文件: {paths['asc']}",
            f"倒序文件: {paths['desc']}",
        ]
    )


def format_fit_index_filter(low: float | None, high: float | None) -> str:
    if low is None and high is None:
        return "无"
    if low is not None and high is not None:
        return f"{low:+.2f}% 到 {high:+.2f}%"
    if low is not None:
        return f"{low:+.2f}% 及以上"
    return f"{high:+.2f}% 及以下"


def format_rating_filter(rating_min: int | None, rating_max: int | None) -> str:
    if rating_min is None and rating_max is None:
        return "无"
    if rating_min is not None and rating_max is not None:
        return f"{rating_min} 到 {rating_max}"
    if rating_min is not None:
        return f"{rating_min} 及以上"
    return f"{rating_max} 及以下"


def format_rank_window_label(output_limit: int | None, start_rank: int | None, end_rank: int | None) -> str:
    if start_rank is not None and end_rank is not None:
        return f"第 {start_rank}-{end_rank} 名"
    if output_limit is not None:
        return f"前 {output_limit} 人"
    return "无上限"


def apply_rank_window(
    rows: list[dict[str, Any]],
    output_limit: int | None,
    *,
    start_rank: int | None = None,
    end_rank: int | None = None,
) -> list[dict[str, Any]]:
    if start_rank is not None and end_rank is not None:
        start_index = start_rank - 1
        end_index = min(len(rows), end_rank)
        return [{**item, "_rank": index + 1} for index, item in enumerate(rows[start_index:end_index], start=start_index)]
    if output_limit is not None:
        return [{**item, "_rank": index + 1} for index, item in enumerate(rows[:output_limit])]
    return rows


def format_group_report(
    cache: dict[str, Any],
    *,
    sort_order: str,
    output_mode: str,
    rating_min: int | None = None,
    rating_max: int | None = None,
    rows: list[dict[str, Any]] | None = None,
    sort_by: str = "rating",
    fit_index_min: float | None = None,
    fit_index_max: float | None = None,
) -> str:
    if rows is None:
        rows = sorted_results(
            cache,
            sort_order,
            rating_min=rating_min,
            rating_max=rating_max,
            sort_by=sort_by,
            fit_index_min=fit_index_min,
            fit_index_max=fit_index_max,
        )
    lines = [format_rating_table(cache, rows, sort_order=sort_order, sort_by=sort_by)]
    if output_mode == "detail":
        for index, item in enumerate([row for row in rows if row.get("ok")], start=1):
            absolute_rank = item.get("_rank") if isinstance(item.get("_rank"), int) else index
            lines.extend(["", f"## {absolute_rank}. {format_member_name(item)}"])
            b50 = item.get("b50")
            if isinstance(b50, dict):
                lines.append(format_b50_summary(b50, 50, "b50"))
    return "\n".join(lines)


def format_rating_table(
    cache: dict[str, Any],
    rows: list[dict[str, Any]],
    *,
    sort_order: str,
    sort_by: str = "rating",
) -> str:
    sort_field_label = "虚高指数" if sort_by == "fitIndex" else "Rating"
    lines = [
        f"# 群 {cache.get('groupId')} {sort_field_label} {'正序' if sort_order == 'asc' else '倒序'}",
        "",
        "| 排名 | QQ | QQ昵称 | QQ群昵称 | 水鱼昵称 | Rating | 虚高指数 | 状态 |",
        "| --- | --- | --- | --- | --- | ---: | --- | --- |",
    ]
    for index, item in enumerate(rows, start=1):
        absolute_rank = item.get("_rank") if isinstance(item.get("_rank"), int) else index
        rating = item.get("rating") if item.get("rating") is not None else ""
        status = "OK" if item.get("ok") else format_error_short(item.get("error"))
        identity = identity_for_group_item(cache, item)
        preferred_group = identity.get("preferredGroup") if isinstance(identity.get("preferredGroup"), dict) else {}
        player = item.get("player") if isinstance(item.get("player"), dict) else {}
        lines.append(
            "| "
            + " | ".join(
                [
                    str(absolute_rank),
                    str(item.get("userId") or ""),
                    escape_markdown_table(str(identity.get("qqNickname") or item.get("nickname") or "")),
                    escape_markdown_table(str(preferred_group.get("groupNickname") or item.get("card") or item.get("displayName") or "")),
                    escape_markdown_table(str(player.get("nickname") or identity.get("waterfishNickname") or "")),
                    str(rating),
                    escape_markdown_table(format_fit_index_cell(item.get("fitIndex"))),
                    escape_markdown_table(status),
                ]
            )
            + " |"
        )
    return "\n".join(lines)


def format_fit_index_cell(fit_index: Any) -> str:
    if not isinstance(fit_index, dict):
        return ""
    if not fit_index.get("available"):
        return "数据不足"
    ratio = fit_index.get("virtualRatio")
    rating = fit_index.get("virtualRating")
    label = fit_index.get("label")
    parts = []
    if isinstance(ratio, (int, float)):
        parts.append(f"{ratio:+.2f}%")
    if isinstance(rating, (int, float)):
        parts.append(f"{rating:+.1f} ra")
    if label:
        parts.append(str(label))
    return " ".join(parts)


def identity_for_group_item(cache: dict[str, Any], item: dict[str, Any]) -> dict[str, Any]:
    identity = item.get("identity") if isinstance(item.get("identity"), dict) else None
    if identity:
        return identity
    try:
        loaded = get_identity(str(item.get("userId") or ""), cache.get("groupId"))
    except Exception:
        loaded = None
    return loaded if isinstance(loaded, dict) else {}


def build_member_rank_result(
    cache: dict[str, Any],
    qq: str,
    *,
    output_mode: str,
    context_size: int,
) -> dict[str, Any]:
    desc_rows = rank_sorted_results(cache, "desc")
    asc_rows = rank_sorted_results(cache, "asc")
    target = next((item for item in desc_rows if str(item.get("userId")) == qq), None)
    member = find_cached_member(cache, qq)
    reports = write_reports(cache)

    if target is None:
        text = format_member_rank_missing_text(cache, qq, member)
        return {
            "groupId": cache.get("groupId"),
            "qq": qq,
            "found": False,
            "member": member,
            "totalRanked": len(desc_rows),
            "outputMode": output_mode,
            "contextSize": context_size,
            "reportFiles": {key: str(path) for key, path in reports.items()},
            "text": text,
            "data": None,
        }

    desc_rank = next(index for index, item in enumerate(desc_rows, start=1) if str(item.get("userId")) == qq)
    asc_rank = next(index for index, item in enumerate(asc_rows, start=1) if str(item.get("userId")) == qq)
    context_start = max(0, desc_rank - 1 - context_size)
    context_end = min(len(desc_rows), desc_rank + context_size)
    context_rows = [
        {**item, "_rank": context_start + offset}
        for offset, item in enumerate(desc_rows[context_start:context_end], start=1)
    ]
    rank_info = {
        "rankDesc": desc_rank,
        "rankAsc": asc_rank,
        "totalRanked": len(desc_rows),
        "higherCount": desc_rank - 1,
        "lowerCount": len(desc_rows) - desc_rank,
    }
    text = format_member_rank_text(
        cache,
        target,
        rank_info=rank_info,
        context_rows=context_rows,
        output_mode=output_mode,
    )
    return {
        "groupId": cache.get("groupId"),
        "qq": qq,
        "found": True,
        "member": target,
        "rank": rank_info,
        "context": context_rows,
        "outputMode": output_mode,
        "contextSize": context_size,
        "reportFiles": {key: str(path) for key, path in reports.items()},
        "text": text,
        "data": {"target": target, "rank": rank_info, "context": context_rows},
    }


def build_rank_at_result(
    cache: dict[str, Any],
    *,
    rank: int,
    sort_order: str,
    output_mode: str,
    rating_min: int | None,
    rating_max: int | None,
    fit_index_min: float | None = None,
    fit_index_max: float | None = None,
) -> dict[str, Any]:
    rows = sorted_results(
        cache,
        sort_order,
        rating_min=rating_min,
        rating_max=rating_max,
        fit_index_min=fit_index_min,
        fit_index_max=fit_index_max,
    )
    reports = write_reports(cache)
    target = rows[rank - 1] if rank <= len(rows) else None

    rank_info = {
        "requestedRank": rank,
        "sortOrder": sort_order,
        "matchedCount": len(rows),
        "ratingMin": rating_min,
        "ratingMax": rating_max,
        "fitIndexMin": fit_index_min,
        "fitIndexMax": fit_index_max,
    }

    if target is None:
        text = format_rank_at_missing_text(cache, rank_info)
        return {
            "groupId": cache.get("groupId"),
            "rank": rank,
            "sortOrder": sort_order,
            "found": False,
            "outputMode": output_mode,
            "ratingMin": rating_min,
            "ratingMax": rating_max,
            "matchedCount": len(rows),
            "reportFiles": {key: str(path) for key, path in reports.items()},
            "text": text,
            "data": None,
        }

    qq = str(target.get("userId") or "")
    desc_rows = rank_sorted_results(cache, "desc")
    asc_rows = rank_sorted_results(cache, "asc")
    desc_rank = next((index for index, item in enumerate(desc_rows, start=1) if str(item.get("userId")) == qq), None)
    asc_rank = next((index for index, item in enumerate(asc_rows, start=1) if str(item.get("userId")) == qq), None)
    rank_info.update(
        {
            "rankDesc": desc_rank,
            "rankAsc": asc_rank,
            "totalRanked": len(desc_rows),
        }
    )
    text = format_rank_at_text(
        cache,
        target,
        rank_info=rank_info,
        output_mode=output_mode,
    )
    return {
        "groupId": cache.get("groupId"),
        "rank": rank,
        "sortOrder": sort_order,
        "found": True,
        "member": target,
        "rankInfo": rank_info,
        "outputMode": output_mode,
        "ratingMin": rating_min,
        "ratingMax": rating_max,
        "matchedCount": len(rows),
        "reportFiles": {key: str(path) for key, path in reports.items()},
        "text": text,
        "data": {"target": target, "rank": rank_info},
    }


def find_cached_member(cache: dict[str, Any], qq: str) -> dict[str, Any] | None:
    for member in cache.get("members") or []:
        if str(member.get("userId")) == qq:
            return member
    return None


def format_member_rank_missing_text(
    cache: dict[str, Any],
    qq: str,
    member: dict[str, Any] | None,
) -> str:
    paths = report_paths(str(cache["groupId"]))
    status = (
        "本次使用一天内缓存，未重新拉取"
        if cache.get("cacheRefreshReason") == "hit"
        else "本次已重新拉取并刷新缓存"
    )
    member_line = (
        f"群成员记录: 存在，群名片/昵称 {member.get('displayName') or member.get('nickname') or member.get('card') or qq}"
        if member
        else "群成员记录: 缓存中没有该 QQ"
    )
    return "\n".join(
        [
            f"群 {cache.get('groupId')} QQ {qq} 没有可输出的排名。",
            f"缓存状态: {status}，缓存生成时间: {format_display_time(cache.get('fetchedAt'))}",
            member_line,
            f"当前可排名成员: {cache.get('successCount', 0)} / 群成员 {cache.get('memberCount', 0)}，跳过: {cache.get('skippedCount', 0)}",
            "可能原因: 水鱼按 QQ 查不到、对方隐私/未开放第三方查询，或 rating=0；这些成员按规则不输出也不缓存。",
            f"正序文件: {paths['asc']}",
            f"倒序文件: {paths['desc']}",
        ]
    )


def format_rank_at_missing_text(cache: dict[str, Any], rank_info: dict[str, Any]) -> str:
    paths = report_paths(str(cache["groupId"]))
    status = (
        "本次使用一天内缓存，未重新拉取"
        if cache.get("cacheRefreshReason") == "hit"
        else "本次已重新拉取并刷新缓存"
    )
    sort_order = rank_info.get("sortOrder")
    label = "倒序" if sort_order == "desc" else "正序"
    return "\n".join(
        [
            f"群 {cache.get('groupId')} rating {label}第 {rank_info.get('requestedRank')} 名不存在。",
            f"缓存状态: {status}，缓存生成时间: {format_display_time(cache.get('fetchedAt'))}",
            f"筛选: {format_rating_filter(rank_info.get('ratingMin'), rank_info.get('ratingMax'))}，匹配: {rank_info.get('matchedCount')} 条",
            f"当前可排名成员: {cache.get('successCount', 0)} / 群成员 {cache.get('memberCount', 0)}，跳过: {cache.get('skippedCount', 0)}",
            f"正序文件: {paths['asc']}",
            f"倒序文件: {paths['desc']}",
        ]
    )


def format_rank_at_text(
    cache: dict[str, Any],
    target: dict[str, Any],
    *,
    rank_info: dict[str, Any],
    output_mode: str,
) -> str:
    paths = report_paths(str(cache["groupId"]))
    status = (
        "本次使用一天内缓存，未重新拉取"
        if cache.get("cacheRefreshReason") == "hit"
        else "本次已重新拉取并刷新缓存"
    )
    sort_order = rank_info.get("sortOrder")
    label = "倒序" if sort_order == "desc" else "正序"
    lines = [
        f"群 {cache.get('groupId')} rating {label}第 {rank_info['requestedRank']} 名",
        f"缓存状态: {status}，缓存生成时间: {format_display_time(cache.get('fetchedAt'))}",
        f"筛选: rating {format_rating_filter(rank_info.get('ratingMin'), rank_info.get('ratingMax'))}，虚高指数 {format_fit_index_filter(rank_info.get('fitIndexMin'), rank_info.get('fitIndexMax'))}，匹配: {rank_info.get('matchedCount')} 条",
        f"成员: {format_member_identity_label(cache, target)}",
        f"QQ: {target.get('userId')}",
        f"Rating: {target.get('rating')}",
        f"虚高指数: {format_fit_index_cell(target.get('fitIndex')) or '—'}",
        f"全群倒序排名: {rank_info.get('rankDesc')} / {rank_info.get('totalRanked')}",
        f"全群正序排名: {rank_info.get('rankAsc')} / {rank_info.get('totalRanked')}",
        f"正序文件: {paths['asc']}",
        f"倒序文件: {paths['desc']}",
    ]
    if output_mode == "detail":
        b50 = target.get("b50")
        if isinstance(b50, dict):
            lines.extend(["", "完整 B50:", format_b50_summary(b50, 50, "b50")])
    return "\n".join(lines)


def format_member_rank_text(
    cache: dict[str, Any],
    target: dict[str, Any],
    *,
    rank_info: dict[str, Any],
    context_rows: list[dict[str, Any]],
    output_mode: str,
) -> str:
    paths = report_paths(str(cache["groupId"]))
    status = (
        "本次使用一天内缓存，未重新拉取"
        if cache.get("cacheRefreshReason") == "hit"
        else "本次已重新拉取并刷新缓存"
    )
    lines = [
        f"群 {cache.get('groupId')} QQ {target.get('userId')} 排名信息",
        f"缓存状态: {status}，缓存生成时间: {format_display_time(cache.get('fetchedAt'))}",
        f"成员: {format_member_identity_label(cache, target)}",
        f"Rating: {target.get('rating')}",
        f"虚高指数: {format_fit_index_cell(target.get('fitIndex')) or '—'}",
        f"倒序排名: {rank_info['rankDesc']} / {rank_info['totalRanked']}（前面 {rank_info['higherCount']} 人，后面 {rank_info['lowerCount']} 人）",
        f"正序排名: {rank_info['rankAsc']} / {rank_info['totalRanked']}",
        f"正序文件: {paths['asc']}",
        f"倒序文件: {paths['desc']}",
        "",
        "附近排名（排名正序）:",
        format_rank_context_table(context_rows, target_qq=str(target.get("userId"))),
    ]
    if output_mode == "detail":
        b50 = target.get("b50")
        if isinstance(b50, dict):
            lines.extend(["", "完整 B50:", format_b50_summary(b50, 50, "b50")])
    return "\n".join(lines)


def format_rank_context_table(rows: list[dict[str, Any]], *, target_qq: str) -> str:
    lines = [
        "| 排名 | QQ | QQ昵称 | QQ群昵称 | 水鱼昵称 | Rating | 虚高指数 | 标记 |",
        "| --- | --- | --- | --- | --- | ---: | --- | --- |",
    ]
    for index, item in enumerate(rows, start=1):
        absolute_rank = item.get("_rank")
        if not isinstance(absolute_rank, int):
            absolute_rank = index
        marker = "目标" if str(item.get("userId")) == target_qq else ""
        identity = item.get("identity") if isinstance(item.get("identity"), dict) else {}
        preferred_group = identity.get("preferredGroup") if isinstance(identity.get("preferredGroup"), dict) else {}
        player = item.get("player") if isinstance(item.get("player"), dict) else {}
        lines.append(
            "| "
            + " | ".join(
                [
                    str(absolute_rank),
                    str(item.get("userId") or ""),
                    escape_markdown_table(str(identity.get("qqNickname") or item.get("nickname") or "")),
                    escape_markdown_table(str(preferred_group.get("groupNickname") or item.get("card") or item.get("displayName") or "")),
                    escape_markdown_table(str(player.get("nickname") or identity.get("waterfishNickname") or "")),
                    str(item.get("rating") if item.get("rating") is not None else ""),
                    escape_markdown_table(format_fit_index_cell(item.get("fitIndex"))),
                    marker,
                ]
            )
            + " |"
        )
    return "\n".join(lines)


def format_member_identity_label(cache: dict[str, Any], item: dict[str, Any]) -> str:
    identity = identity_for_group_item(cache, item)
    preferred_group = identity.get("preferredGroup") if isinstance(identity.get("preferredGroup"), dict) else {}
    qq_name = identity.get("qqNickname") or item.get("nickname") or "未知QQ昵称"
    group_name = preferred_group.get("groupNickname") or item.get("card") or item.get("displayName") or "未知群昵称"
    player = item.get("player") if isinstance(item.get("player"), dict) else {}
    waterfish_name = player.get("nickname") or identity.get("waterfishNickname") or "未知水鱼昵称"
    return f"QQ昵称 {qq_name} / QQ群昵称 {group_name} / 水鱼昵称 {waterfish_name}"


def format_member_name(item: dict[str, Any], *, include_qq: bool = True) -> str:
    name = item.get("displayName") or item.get("nickname") or item.get("card") or item.get("userId") or "未知"
    if include_qq:
        return f"{name} ({item.get('userId')})"
    return str(name)


def format_error_short(error_info: Any) -> str:
    if not isinstance(error_info, dict):
        return "查询失败"
    code = error_info.get("code") or "ERROR"
    message = error_info.get("message") or "查询失败"
    return f"{code}: {message}"


def escape_markdown_table(value: str) -> str:
    return value.replace("|", "\\|").replace("\n", " ")


def format_cache_status_text(status: dict[str, Any]) -> str:
    if not status["cacheExists"]:
        return f"群 {status['groupId']} 没有缓存。\n缓存路径: {status['cachePath']}"
    freshness = "未过期" if status["fresh"] else "已过期"
    return "\n".join(
        [
            f"群 {status['groupId']} 缓存存在，{freshness}。",
            f"生成时间: {format_display_time(status.get('fetchedAt'))}",
            f"年龄: {status.get('ageSeconds'):.0f} 秒 / TTL {status.get('ttlSeconds')} 秒"
            if status.get("ageSeconds") is not None
            else f"TTL: {status.get('ttlSeconds')} 秒",
            f"群成员: {status.get('memberCount')}，成功: {status.get('successCount')}，失败: {status.get('failureCount')}",
            f"跳过: {status.get('skippedCount')}，缓存范围: 可查询且 rating>0 的完整 B50 歌曲明细（存在: {status.get('containsDetailedB50')}）",
            f"正序文件: {status['reportFiles']['asc']}（存在: {status['reportFilesExist']['asc']}）",
            f"倒序文件: {status['reportFiles']['desc']}（存在: {status['reportFilesExist']['desc']}）",
        ]
    )


def write_message(message: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(message, ensure_ascii=False, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def success(message_id: Any, result: dict[str, Any]) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": message_id, "result": result}


def error(message_id: Any, code: int, message: str, data: Any = None) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "jsonrpc": "2.0",
        "id": message_id,
        "error": {"code": code, "message": message},
    }
    if data is not None:
        payload["error"]["data"] = data
    return payload


def is_group_b50_error(exc: Exception) -> bool:
    return isinstance(exc, GroupB50Error) or exc.__class__.__name__ == "GroupB50Error"


def group_b50_error_payload(exc: Exception) -> dict[str, Any]:
    to_dict = getattr(exc, "to_dict", None)
    if callable(to_dict):
        try:
            return to_dict()
        except Exception:
            pass
    return {"code": getattr(exc, "code", "ERROR"), "message": str(exc)}


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
    tool_name = params.get("name")
    arguments = params.get("arguments") or {}
    if not isinstance(arguments, dict):
        return success(
            message_id,
            {"content": [{"type": "text", "text": "arguments 必须是对象。"}], "isError": True},
        )

    try:
        if tool_name == GROUP_B50_REPORT_TOOL["name"]:
            result = group_b50_report(arguments)
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": result["text"]}],
                    "structuredContent": result,
                    "isError": False,
                },
            )
        if tool_name == GROUP_B50_CACHE_STATUS_TOOL["name"]:
            result = cache_status(arguments)
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": format_cache_status_text(result)}],
                    "structuredContent": result,
                    "isError": False,
                },
            )
        if tool_name == GROUP_B50_JOB_STATUS_TOOL["name"]:
            result = group_b50_job_status(arguments)
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": result["text"]}],
                    "structuredContent": result,
                    "isError": False,
                },
            )
        if tool_name == GROUP_B50_MEMBER_RANK_TOOL["name"]:
            result = group_b50_member_rank(arguments)
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": result["text"]}],
                    "structuredContent": result,
                    "isError": False,
                },
            )
        if tool_name == GROUP_B50_RANK_AT_TOOL["name"]:
            result = group_b50_rank_at(arguments)
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": result["text"]}],
                    "structuredContent": result,
                    "isError": False,
                },
            )
        if tool_name == CLEAR_GROUP_B50_CACHE_TOOL["name"]:
            group_id = normalize_identifier(arguments.get("groupId"), "groupId")
            result = clear_cache(group_id)
            text = "群 B50 缓存已清除。" if result["cleared"] else "该群没有可清除的 B50 缓存。"
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": text}],
                    "structuredContent": result,
                    "isError": False,
                },
            )
        # ---- song-rank feature ----
        if tool_name == _song_rank.GROUP_SONG_SCORE_REPORT_TOOL["name"]:
            result = _song_rank.group_song_score_report(arguments)
            return success(
                message_id,
                {"content": [{"type": "text", "text": result["text"]}], "structuredContent": result, "isError": False},
            )
        if tool_name == _song_rank.GROUP_SONG_SCORE_MEMBER_RANK_TOOL["name"]:
            result = _song_rank.group_song_score_member_rank(arguments)
            return success(
                message_id,
                {"content": [{"type": "text", "text": result["text"]}], "structuredContent": result, "isError": False},
            )
        if tool_name == _song_rank.GROUP_SONG_SCORE_CACHE_STATUS_TOOL["name"]:
            result = _song_rank.song_cache_status(arguments)
            return success(
                message_id,
                {"content": [{"type": "text", "text": json.dumps(result, ensure_ascii=False, indent=2)}], "structuredContent": result, "isError": False},
            )
        if tool_name == _song_rank.GROUP_SONG_SCORE_JOB_STATUS_TOOL["name"]:
            result = _song_rank.song_job_status(arguments)
            return success(
                message_id,
                {"content": [{"type": "text", "text": result["text"]}], "structuredContent": result, "isError": False},
            )
        if tool_name == _song_rank.CLEAR_GROUP_SONG_SCORE_CACHE_TOOL["name"]:
            group_id = normalize_identifier(arguments.get("groupId"), "groupId")
            result = _song_rank.song_clear_cache(group_id)
            text = "群单曲成绩缓存已清除。" if result["cleared"] else "该群没有可清除的单曲成绩缓存。"
            return success(
                message_id,
                {"content": [{"type": "text", "text": text}], "structuredContent": result, "isError": False},
            )
    except Exception as exc:
        if not is_group_b50_error(exc):
            raise
        return success(
            message_id,
            {
                "content": [{"type": "text", "text": str(exc)}],
                "structuredContent": {"error": group_b50_error_payload(exc)},
                "isError": True,
            },
        )

    return error(message_id, -32602, f"Unknown tool: {tool_name}")


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
            message_id = None
            if "message" in locals() and isinstance(message, dict):
                message_id = message.get("id")
            write_message(error(message_id, -32603, "Internal error", str(exc)))


if __name__ == "__main__":
    main()
