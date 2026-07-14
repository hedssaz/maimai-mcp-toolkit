from __future__ import annotations

import json
import os
import socket
import sys
import threading
import time
import traceback
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone
from typing import Any
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from . import __version__
from .store import (
    DAILY_RESET_HOUR_UTC,
    build_stats,
    cache_age_seconds,
    cache_path,
    empty_cache,
    get_identity,
    is_cache_fresh,
    now_iso,
    optional_string,
    read_cache,
    resolve_identities,
    upsert_friend,
    upsert_group,
    upsert_group_member,
    write_cache,
)


SERVER_NAME = "qq-identity-mcp"
DEFAULT_NAPCAT_BASE_URL = "http://napcat:3000"
BACKGROUND_JOBS: dict[str, threading.Thread] = {}
BACKGROUND_JOBS_LOCK = threading.Lock()
AUTO_REFRESH_STARTED = False
DEFAULT_DISPLAY_TIMEZONE = "Asia/Shanghai"


class QqIdentityError(Exception):
    def __init__(
        self,
        message: str,
        *,
        code: str = "QQ_IDENTITY_ERROR",
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


REFRESH_TOOL = {
    "name": "refresh_qq_identity_cache",
    "description": (
        "从 NapCat 拉取机器人账号的好友列表、加入的群列表、各群成员 QQ/QQ昵称/群名片，"
        "刷新共享 QQ 身份缓存。不会保存好友备注。"
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "forceRefresh": {"type": "boolean", "description": "是否忽略 1 天 TTL 强制刷新。"},
            "napcatBaseUrl": {"type": "string", "description": "覆盖 NAPCAT_BASE_URL。"},
            "noCache": {"type": "boolean", "description": "传给 get_group_member_list，默认 true。"},
            "timeoutMs": {
                "type": "integer",
                "minimum": 1000,
                "maximum": 60000,
                "description": "单次 NapCat HTTP 请求超时，默认 10000ms。",
            },
            "groupDelayMs": {
                "type": "integer",
                "minimum": 0,
                "maximum": 10000,
                "description": "每个群成员列表请求之间的间隔，默认 250ms。",
            },
            "maxGroups": {
                "type": "integer",
                "minimum": 1,
                "description": "最多刷新多少个群，主要用于测试。",
            },
        },
        "additionalProperties": False,
    },
}

CACHE_STATUS_TOOL = {
    "name": "qq_identity_cache_status",
    "description": "查看 QQ 身份缓存状态、年龄、统计和后台刷新任务。",
    "inputSchema": {"type": "object", "properties": {}, "additionalProperties": False},
}

JOB_STATUS_TOOL = {
    "name": "qq_identity_job_status",
    "description": "查看 QQ 身份缓存后台刷新任务状态。",
    "inputSchema": {"type": "object", "properties": {}, "additionalProperties": False},
}

RESOLVE_TOOL = {
    "name": "resolve_qq_identity",
    "description": "通过 QQ号、QQ昵称、群昵称/群名片、水鱼昵称反查 QQ 身份；重名时返回候选让 Agent 追问。",
    "inputSchema": {
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": "QQ号、QQ昵称、群昵称/群名片或水鱼昵称。"},
            "groupId": {"type": "string", "description": "可选，优先匹配指定群内昵称。"},
            "maxResults": {
                "type": "integer",
                "minimum": 1,
                "maximum": 20,
                "description": "最多返回候选数量，默认 10。",
            },
        },
        "required": ["query"],
        "additionalProperties": False,
    },
}

GET_IDENTITY_TOOL = {
    "name": "get_qq_identity",
    "description": "通过 QQ 号读取缓存中的 QQ昵称、群昵称/群名片、水鱼昵称。",
    "inputSchema": {
        "type": "object",
        "properties": {
            "qq": {"type": "string", "description": "QQ 号。"},
            "groupId": {"type": "string", "description": "可选，返回该群的群昵称优先项。"},
        },
        "required": ["qq"],
        "additionalProperties": False,
    },
}

TOOLS = [REFRESH_TOOL, CACHE_STATUS_TOOL, JOB_STATUS_TOOL, RESOLVE_TOOL, GET_IDENTITY_TOOL]


def normalize_timeout_ms(value: Any) -> int:
    if value is None:
        return 10000
    if not isinstance(value, int) or value < 1000 or value > 60000:
        raise QqIdentityError("timeoutMs 必须是 1000 到 60000 之间的整数。", code="INVALID_INPUT")
    return value


def normalize_delay_ms(value: Any) -> int:
    if value is None:
        return 250
    if not isinstance(value, int) or value < 0 or value > 10000:
        raise QqIdentityError("groupDelayMs 必须是 0 到 10000 之间的整数。", code="INVALID_INPUT")
    return value


def normalize_max_groups(value: Any) -> int | None:
    if value is None:
        return None
    if not isinstance(value, int) or value < 1:
        raise QqIdentityError("maxGroups 必须是正整数。", code="INVALID_INPUT")
    return value


def normalize_max_results(value: Any) -> int:
    if value is None:
        return 10
    if not isinstance(value, int) or value < 1 or value > 20:
        raise QqIdentityError("maxResults 必须是 1 到 20 之间的整数。", code="INVALID_INPUT")
    return value


def normalize_napcat_base_url(value: Any) -> str:
    if isinstance(value, str) and value.strip():
        return value.strip().rstrip("/")
    return os.environ.get("NAPCAT_BASE_URL", DEFAULT_NAPCAT_BASE_URL).strip().rstrip("/")


def display_timezone() -> timezone:
    configured = (
        os.environ.get("QQ_IDENTITY_DISPLAY_TZ")
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


def cache_status() -> dict[str, Any]:
    cache = read_cache()
    age = cache_age_seconds(cache)
    return {
        "cacheExists": cache_path().exists(),
        "fresh": is_cache_fresh(cache),
        "ageSeconds": age,
        "dailyResetHourUtc": DAILY_RESET_HOUR_UTC,
        "fetchedAt": cache.get("fetchedAt"),
        "updatedAt": cache.get("updatedAt"),
        "stats": cache.get("stats") or build_stats(cache),
        "cachePath": str(cache_path()),
        "job": read_job_status(),
    }


def refresh_qq_identity_cache(arguments: dict[str, Any]) -> dict[str, Any]:
    cache = read_cache()
    force_refresh = arguments.get("forceRefresh") is True
    if not force_refresh and is_cache_fresh(cache):
        status = cache_status()
        job = read_job_status()
        prefix = "QQ 身份缓存仍在 1 天有效期内，未重新拉取。"
        if job and job.get("status") == "running":
            text = "QQ 身份缓存刷新仍在进行，未启动新任务。\n\n" + format_job_status_text(job)
        else:
            if job and job.get("status") == "completed":
                prefix = "最近一次 QQ 身份缓存刷新已完成；当前缓存仍在 1 天有效期内，未重新拉取。"
            elif job and job.get("status") == "failed":
                prefix = "最近一次 QQ 身份缓存刷新失败；当前缓存仍在 1 天有效期内，未重新拉取。"
            text = format_cache_status_text(status, prefix=prefix)
            if job:
                text += "\n\n最近刷新任务:\n" + format_job_status_text(job)
        return {
            "started": False,
            "cache": status,
            "job": job,
            "text": text,
        }

    job = start_refresh_job(
        arguments,
        refresh_reason="forceRefresh" if force_refresh else "stale_or_missing",
        timeout_ms=normalize_timeout_ms(arguments.get("timeoutMs")),
        group_delay_ms=normalize_delay_ms(arguments.get("groupDelayMs")),
        max_groups=normalize_max_groups(arguments.get("maxGroups")),
    )
    return {
        "started": True,
        "cache": cache_status(),
        "job": job,
        "text": format_job_started_text(job),
    }


def start_refresh_job(
    arguments: dict[str, Any],
    *,
    refresh_reason: str,
    timeout_ms: int,
    group_delay_ms: int,
    max_groups: int | None,
) -> dict[str, Any]:
    with BACKGROUND_JOBS_LOCK:
        existing = BACKGROUND_JOBS.get("refresh")
        if existing and existing.is_alive():
            return read_job_status() or {"status": "running"}
        job = {
            "status": "running",
            "startedAt": now_iso(),
            "finishedAt": None,
            "refreshReason": refresh_reason,
            "message": "QQ 身份缓存刷新已启动。",
            "processedGroups": 0,
            "totalGroups": None,
        }
        write_job_status(job)
        thread = threading.Thread(
            target=run_refresh_job,
            args=(arguments, refresh_reason, timeout_ms, group_delay_ms, max_groups),
            name="qq-identity-refresh",
            daemon=True,
        )
        BACKGROUND_JOBS["refresh"] = thread
        thread.start()
        return job


def run_refresh_job(
    arguments: dict[str, Any],
    refresh_reason: str,
    timeout_ms: int,
    group_delay_ms: int,
    max_groups: int | None,
) -> None:
    try:
        cache = build_identity_cache(
            napcat_base_url=normalize_napcat_base_url(arguments.get("napcatBaseUrl")),
            no_cache=arguments.get("noCache") is not False,
            timeout_ms=timeout_ms,
            group_delay_ms=group_delay_ms,
            max_groups=max_groups,
        )
        cache["fetchedAt"] = now_iso()
        cache["refreshReason"] = refresh_reason
        write_cache(cache)
        status = cache_status()
        write_job_status(
            {
                "status": "completed",
                "startedAt": (read_job_status() or {}).get("startedAt"),
                "finishedAt": now_iso(),
                "refreshReason": refresh_reason,
                "message": "QQ 身份缓存刷新完成。",
                "stats": status.get("stats"),
            }
        )
    except Exception as exc:
        write_job_status(
            {
                "status": "failed",
                "startedAt": (read_job_status() or {}).get("startedAt"),
                "finishedAt": now_iso(),
                "refreshReason": refresh_reason,
                "message": str(exc),
                "error": exc.to_dict()
                if isinstance(exc, QqIdentityError)
                else {"code": "UNKNOWN_ERROR", "message": str(exc)},
            }
        )


def build_identity_cache(
    *,
    napcat_base_url: str,
    no_cache: bool,
    timeout_ms: int,
    group_delay_ms: int,
    max_groups: int | None,
) -> dict[str, Any]:
    cache = empty_cache()
    friends = fetch_friend_list(napcat_base_url=napcat_base_url, timeout_ms=timeout_ms)
    for friend in friends:
        upsert_friend(cache, friend.get("userId"), friend.get("nickname"))

    groups = fetch_group_list(napcat_base_url=napcat_base_url, timeout_ms=timeout_ms)
    if max_groups is not None:
        groups = groups[:max_groups]
    write_job_status(
        {
            **(read_job_status() or {}),
            "message": "已拉取好友和群列表，开始拉取群成员。",
            "processedGroups": 0,
            "totalGroups": len(groups),
            "friendCount": len(friends),
        }
    )
    for index, group in enumerate(groups, start=1):
        group_id = group.get("groupId")
        group_name = group.get("groupName")
        upsert_group(cache, group_id, group_name)
        members = fetch_group_members(
            group_id,
            napcat_base_url=napcat_base_url,
            no_cache=no_cache,
            timeout_ms=timeout_ms,
        )
        for member in members:
            upsert_group_member(
                cache,
                group_id=group_id,
                group_name=group_name,
                qq=member.get("userId"),
                nickname=member.get("nickname"),
                card=member.get("card"),
            )
        write_job_status(
            {
                **(read_job_status() or {}),
                "message": f"已刷新群 {index}/{len(groups)}：{group_name or group_id}",
                "processedGroups": index,
                "totalGroups": len(groups),
                "currentGroupId": group_id,
                "currentGroupName": group_name,
                "uniqueUsers": len(cache.get("users") or {}),
            }
        )
        if group_delay_ms > 0 and index < len(groups):
            time.sleep(group_delay_ms / 1000)
    return cache


def fetch_friend_list(*, napcat_base_url: str, timeout_ms: int) -> list[dict[str, Any]]:
    response = post_json(f"{napcat_base_url}/get_friend_list", {}, timeout_ms=timeout_ms)
    raw_friends = extract_list_response(response, "好友列表")
    friends = []
    for item in raw_friends:
        if not isinstance(item, dict):
            continue
        user_id = item.get("user_id", item.get("userId"))
        if isinstance(user_id, int):
            user_id = str(user_id)
        if not isinstance(user_id, str) or not user_id.strip():
            continue
        friends.append(
            {
                "userId": user_id.strip(),
                "nickname": optional_string(item.get("nickname")),
            }
        )
    return friends


def fetch_group_list(*, napcat_base_url: str, timeout_ms: int) -> list[dict[str, Any]]:
    response = post_json(f"{napcat_base_url}/get_group_list", {}, timeout_ms=timeout_ms)
    raw_groups = extract_list_response(response, "群列表")
    groups = []
    for item in raw_groups:
        if not isinstance(item, dict):
            continue
        group_id = item.get("group_id", item.get("groupId"))
        if isinstance(group_id, int):
            group_id = str(group_id)
        if not isinstance(group_id, str) or not group_id.strip():
            continue
        groups.append(
            {
                "groupId": group_id.strip(),
                "groupName": optional_string(item.get("group_name", item.get("groupName"))),
                "memberCount": item.get("member_count", item.get("memberCount")),
            }
        )
    return groups


def fetch_group_members(
    group_id: Any,
    *,
    napcat_base_url: str,
    no_cache: bool,
    timeout_ms: int,
) -> list[dict[str, Any]]:
    payload = {"group_id": int(group_id) if str(group_id).isdigit() else group_id, "no_cache": no_cache}
    response = post_json(f"{napcat_base_url}/get_group_member_list", payload, timeout_ms=timeout_ms)
    raw_members = extract_list_response(response, "群成员列表")
    members = []
    for item in raw_members:
        if not isinstance(item, dict):
            continue
        user_id = item.get("user_id", item.get("userId"))
        if isinstance(user_id, int):
            user_id = str(user_id)
        if not isinstance(user_id, str) or not user_id.strip():
            continue
        members.append(
            {
                "userId": user_id.strip(),
                "nickname": optional_string(item.get("nickname")),
                "card": optional_string(item.get("card")),
            }
        )
    return members


def post_json(url: str, payload: dict[str, Any], *, timeout_ms: int) -> Any:
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
        body_text = exc.read().decode("utf-8", errors="replace")
        raise QqIdentityError(
            f"NapCat 请求失败：HTTP {exc.code}",
            code="HTTP_ERROR",
            status=exc.code,
            body=body_text,
        ) from exc
    except socket.timeout as exc:
        raise QqIdentityError(f"NapCat 请求超时（{timeout_ms}ms）。", code="TIMEOUT") from exc
    except urllib.error.URLError as exc:
        reason = getattr(exc, "reason", exc)
        raise QqIdentityError(f"NapCat 请求失败：{reason}", code="NETWORK_ERROR") from exc

    try:
        return json.loads(response_body)
    except json.JSONDecodeError as exc:
        raise QqIdentityError("NapCat 返回了非 JSON 内容。", code="INVALID_JSON", body=response_body) from exc


def extract_list_response(response: Any, label: str) -> list[Any]:
    if isinstance(response, list):
        return response
    if not isinstance(response, dict):
        raise QqIdentityError(f"NapCat {label}返回结构不是对象或数组。", code="INVALID_JSON")
    if response.get("retcode") not in (None, 0):
        raise QqIdentityError(
            f"NapCat {label}返回错误：{response.get('message') or response.get('wording') or response.get('retcode')}",
            code="NAPCAT_ERROR",
            body=json.dumps(response, ensure_ascii=False),
        )
    data = response.get("data")
    if isinstance(data, list):
        return data
    raise QqIdentityError(f"NapCat {label}响应中没有 data 数组。", code="INVALID_JSON")


def get_job_status() -> dict[str, Any]:
    return {
        "job": read_job_status(),
        "cache": cache_status(),
        "text": format_job_status_text(read_job_status()),
    }


def read_job_status() -> dict[str, Any] | None:
    path = cache_path().with_name("identity_job_status.json")
    if not path.exists():
        return None
    try:
        parsed = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return None
    return parsed if isinstance(parsed, dict) else None


def write_job_status(status: dict[str, Any]) -> None:
    path = cache_path().with_name("identity_job_status.json")
    path.parent.mkdir(parents=True, exist_ok=True)
    temp_path = path.with_name(f".{path.name}.{os.getpid()}.{threading.get_ident()}.tmp")
    temp_path.write_text(json.dumps(status, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    os.replace(temp_path, path)


def format_cache_status_text(status: dict[str, Any], *, prefix: str | None = None) -> str:
    stats = status.get("stats") or {}
    lines = []
    if prefix:
        lines.append(prefix)
    lines.extend(
        [
            f"缓存存在: {status.get('cacheExists')}，未过期: {status.get('fresh')}",
            f"生成时间: {format_display_time(status.get('fetchedAt'))}",
            f"好友: {stats.get('friendCount', 0)}，群: {stats.get('groupCount', 0)}，群成员记录: {stats.get('groupMemberRows', 0)}，唯一 QQ: {stats.get('uniqueUsers', 0)}",
            "好友侧仅保存 QQ 昵称，不保存备注。",
            f"缓存路径: {status.get('cachePath')}",
        ]
    )
    return "\n".join(lines)


def format_job_started_text(job: dict[str, Any]) -> str:
    return "\n".join(
        [
            "QQ 身份缓存刷新已启动。",
            f"状态: {job.get('status')}，原因: {job.get('refreshReason')}，启动时间: {format_display_time(job.get('startedAt'))}",
            "稍后调用 qq_identity_job_status 查看进度；完成后可用 resolve_qq_identity 反查昵称。",
        ]
    )


def format_job_status_text(job: dict[str, Any] | None) -> str:
    if not job:
        return "当前没有 QQ 身份缓存刷新任务。"
    lines = [
        f"QQ 身份缓存刷新任务状态: {job.get('status')}",
        f"启动时间: {format_display_time(job.get('startedAt'))}",
        f"完成时间: {format_display_time(job.get('finishedAt'), missing='未完成')}",
        f"说明: {job.get('message')}",
    ]
    if job.get("status") == "running":
        lines.append(f"进度: {job.get('processedGroups', 0)}/{job.get('totalGroups') or '?'} 个群")
        if job.get("uniqueUsers") is not None:
            lines.append(f"当前唯一 QQ: {job.get('uniqueUsers')}")
    if job.get("status") == "completed" and job.get("stats"):
        stats = job["stats"]
        lines.append(
            f"统计: 好友 {stats.get('friendCount', 0)}，群 {stats.get('groupCount', 0)}，唯一 QQ {stats.get('uniqueUsers', 0)}"
        )
    if job.get("status") == "failed" and job.get("error"):
        lines.append(f"错误: {job['error'].get('message')}")
    return "\n".join(lines)


def format_resolve_text(result: dict[str, Any]) -> str:
    matches = result.get("matches") or []
    if not matches:
        return f"没有从 QQ 身份缓存中找到：{result.get('query')}"
    lines = [
        f"找到 {len(matches)} 个候选"
        + ("（存在重名，请让用户选择 QQ）" if result.get("ambiguous") else "")
        + ":",
        "| 序号 | QQ | QQ昵称 | 群昵称 | 水鱼昵称 | 匹配字段 |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for index, item in enumerate(matches, start=1):
        preferred_group = item.get("preferredGroup") if isinstance(item.get("preferredGroup"), dict) else None
        group_name = ""
        if preferred_group:
            group_name = str(preferred_group.get("groupNickname") or "")
        elif item.get("groups"):
            first_group = item["groups"][0]
            if isinstance(first_group, dict):
                group_name = str(first_group.get("groupNickname") or "")
        lines.append(
            "| "
            + " | ".join(
                [
                    str(index),
                    str(item.get("qq") or ""),
                    escape_table(str(item.get("qqNickname") or item.get("friendNickname") or "")),
                    escape_table(group_name),
                    escape_table(str(item.get("waterfishNickname") or "")),
                    escape_table(", ".join(item.get("matchedFields") or [])),
                ]
            )
            + " |"
        )
    return "\n".join(lines)


def format_identity_text(identity: dict[str, Any] | None, qq: str) -> str:
    if not identity:
        return f"QQ {qq} 不在当前 QQ 身份缓存中。"
    lines = [
        f"QQ: {identity.get('qq')}",
        f"QQ昵称: {identity.get('qqNickname') or identity.get('friendNickname') or '未知'}",
        f"水鱼昵称: {identity.get('waterfishNickname') or '未知'}",
    ]
    preferred_group = identity.get("preferredGroup")
    if isinstance(preferred_group, dict):
        lines.append(
            f"当前群昵称: {preferred_group.get('groupNickname') or '未知'}"
            + (f"（{preferred_group.get('groupName')}）" if preferred_group.get("groupName") else "")
        )
    groups = identity.get("groups") or []
    if groups:
        lines.append(f"所在群记录: {len(groups)} 个")
    return "\n".join(lines)


def escape_table(value: str) -> str:
    return value.replace("|", "\\|").replace("\n", " ")


def ensure_auto_refresh_started() -> None:
    global AUTO_REFRESH_STARTED
    if AUTO_REFRESH_STARTED:
        return
    if os.environ.get("QQ_IDENTITY_AUTO_REFRESH", "1") in {"0", "false", "False"}:
        AUTO_REFRESH_STARTED = True
        return
    AUTO_REFRESH_STARTED = True
    thread = threading.Thread(target=auto_refresh_loop, name="qq-identity-auto-refresh", daemon=True)
    thread.start()


def auto_refresh_loop() -> None:
    interval_seconds = int(os.environ.get("QQ_IDENTITY_AUTO_REFRESH_CHECK_SECONDS", "3600"))
    while True:
        try:
            if not is_cache_fresh(read_cache()):
                start_refresh_job(
                    {},
                    refresh_reason="auto_daily",
                    timeout_ms=10000,
                    group_delay_ms=250,
                    max_groups=None,
                )
        except Exception:
            pass
        time.sleep(max(60, interval_seconds))


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
    tool_name = params.get("name")
    arguments = params.get("arguments") or {}
    if not isinstance(arguments, dict):
        return success(message_id, {"content": [{"type": "text", "text": "arguments 必须是对象。"}], "isError": True})

    try:
        if tool_name == REFRESH_TOOL["name"]:
            result = refresh_qq_identity_cache(arguments)
            return success(message_id, {"content": [{"type": "text", "text": result["text"]}], "structuredContent": result, "isError": False})
        if tool_name == CACHE_STATUS_TOOL["name"]:
            result = cache_status()
            return success(message_id, {"content": [{"type": "text", "text": format_cache_status_text(result)}], "structuredContent": result, "isError": False})
        if tool_name == JOB_STATUS_TOOL["name"]:
            result = get_job_status()
            return success(message_id, {"content": [{"type": "text", "text": result["text"]}], "structuredContent": result, "isError": False})
        if tool_name == RESOLVE_TOOL["name"]:
            result = resolve_identities(
                arguments.get("query"),
                group_id=arguments.get("groupId"),
                max_results=normalize_max_results(arguments.get("maxResults")),
            )
            return success(message_id, {"content": [{"type": "text", "text": format_resolve_text(result)}], "structuredContent": result, "isError": False})
        if tool_name == GET_IDENTITY_TOOL["name"]:
            qq = str(arguments.get("qq") or "").strip()
            if not qq:
                raise QqIdentityError("必须提供 qq。", code="INVALID_INPUT")
            result = get_identity(qq, arguments.get("groupId"))
            return success(message_id, {"content": [{"type": "text", "text": format_identity_text(result, qq)}], "structuredContent": {"identity": result}, "isError": False})
    except QqIdentityError as exc:
        return success(
            message_id,
            {
                "content": [{"type": "text", "text": str(exc)}],
                "structuredContent": {"error": exc.to_dict()},
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
    ensure_auto_refresh_started()
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
            write_message(error(None, -32603, str(exc)))


if __name__ == "__main__":
    main()
