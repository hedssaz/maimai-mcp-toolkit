from __future__ import annotations

import json
import os
import threading
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any


# 每天 22:00 CST (14:00 UTC) 过期。可通过环境变量覆盖（UTC 小时值）。
DAILY_RESET_HOUR_UTC: int = int(os.environ.get("MAIMAI_CACHE_RESET_HOUR_UTC", "14"))


def _daily_reset_time(now: datetime) -> datetime:
    reset = now.replace(hour=DAILY_RESET_HOUR_UTC, minute=0, second=0, microsecond=0)
    if now < reset:
        reset -= timedelta(days=1)
    return reset


def now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def get_cache_dir() -> Path:
    configured = os.environ.get("QQ_IDENTITY_CACHE_DIR")
    if configured:
        return Path(configured).expanduser().resolve()
    return (Path.cwd() / "qq-identity-cache").resolve()


def cache_path() -> Path:
    return get_cache_dir() / "identity_cache.json"


def empty_cache() -> dict[str, Any]:
    return {
        "version": 1,
        "fetchedAt": None,
        "updatedAt": now_iso(),
        "source": "napcat",
        "groups": {},
        "users": {},
        "stats": {
            "friendCount": 0,
            "groupCount": 0,
            "groupMemberRows": 0,
            "uniqueUsers": 0,
        },
    }


def read_cache() -> dict[str, Any]:
    path = cache_path()
    if not path.exists():
        return empty_cache()
    try:
        parsed = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return empty_cache()
    if not isinstance(parsed, dict):
        return empty_cache()
    parsed.setdefault("groups", {})
    parsed.setdefault("users", {})
    parsed.setdefault("stats", {})
    return parsed


def write_cache(cache: dict[str, Any]) -> None:
    cache["updatedAt"] = now_iso()
    cache["stats"] = build_stats(cache)
    path = cache_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    temp_path = path.with_name(f".{path.name}.{os.getpid()}.{threading.get_ident()}.tmp")
    temp_path.write_text(json.dumps(cache, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    os.replace(temp_path, path)


def build_stats(cache: dict[str, Any]) -> dict[str, int]:
    groups = cache.get("groups") if isinstance(cache.get("groups"), dict) else {}
    users = cache.get("users") if isinstance(cache.get("users"), dict) else {}
    friend_count = sum(1 for user in users.values() if isinstance(user, dict) and user.get("isFriend"))
    group_member_rows = 0
    for user in users.values():
        groups_for_user = user.get("groups") if isinstance(user, dict) else None
        if isinstance(groups_for_user, dict):
            group_member_rows += len(groups_for_user)
    return {
        "friendCount": friend_count,
        "groupCount": len(groups),
        "groupMemberRows": group_member_rows,
        "uniqueUsers": len(users),
    }


def cache_age_seconds(cache: dict[str, Any] | None = None) -> float | None:
    cache = cache or read_cache()
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


def is_cache_fresh(cache: dict[str, Any] | None = None) -> bool:
    cache = cache or read_cache()
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


def normalize_qq(value: Any) -> str | None:
    if isinstance(value, int):
        value = str(value)
    if not isinstance(value, str):
        return None
    value = value.strip()
    return value if value else None


def optional_string(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    value = value.strip()
    return value or None


def ensure_user(cache: dict[str, Any], qq: str) -> dict[str, Any]:
    users = cache.setdefault("users", {})
    user = users.get(qq)
    if not isinstance(user, dict):
        user = {"qq": qq, "groups": {}}
        users[qq] = user
    user.setdefault("qq", qq)
    user.setdefault("groups", {})
    return user


def upsert_group(cache: dict[str, Any], group_id: Any, group_name: Any = None) -> dict[str, Any] | None:
    normalized_group_id = normalize_qq(group_id)
    if not normalized_group_id:
        return None
    groups = cache.setdefault("groups", {})
    group = groups.get(normalized_group_id)
    if not isinstance(group, dict):
        group = {"groupId": normalized_group_id}
        groups[normalized_group_id] = group
    group["groupId"] = normalized_group_id
    if optional_string(group_name):
        group["groupName"] = optional_string(group_name)
    group["updatedAt"] = now_iso()
    return group


def upsert_friend(cache: dict[str, Any], qq: Any, nickname: Any = None) -> dict[str, Any] | None:
    normalized_qq = normalize_qq(qq)
    if not normalized_qq:
        return None
    user = ensure_user(cache, normalized_qq)
    user["isFriend"] = True
    if optional_string(nickname):
        user["friendNickname"] = optional_string(nickname)
        user.setdefault("qqNickname", optional_string(nickname))
    user["friendUpdatedAt"] = now_iso()
    return user


def upsert_group_member(
    cache: dict[str, Any],
    *,
    group_id: Any,
    qq: Any,
    group_name: Any = None,
    nickname: Any = None,
    card: Any = None,
) -> dict[str, Any] | None:
    normalized_group_id = normalize_qq(group_id)
    normalized_qq = normalize_qq(qq)
    if not normalized_group_id or not normalized_qq:
        return None
    group = upsert_group(cache, normalized_group_id, group_name)
    user = ensure_user(cache, normalized_qq)
    if optional_string(nickname):
        user["qqNickname"] = optional_string(nickname)
    display_name = optional_string(card) or optional_string(nickname) or normalized_qq
    user.setdefault("groups", {})[normalized_group_id] = {
        "groupId": normalized_group_id,
        "groupName": group.get("groupName") if isinstance(group, dict) else optional_string(group_name),
        "groupNickname": display_name,
        "card": optional_string(card),
        "nickname": optional_string(nickname),
        "updatedAt": now_iso(),
    }
    user["groupUpdatedAt"] = now_iso()
    return user


def upsert_waterfish_profile(
    qq: Any,
    *,
    nickname: Any = None,
    username: Any = None,
    rating: Any = None,
) -> dict[str, Any] | None:
    normalized_qq = normalize_qq(qq)
    if not normalized_qq:
        return None
    cache = read_cache()
    user = ensure_user(cache, normalized_qq)
    if optional_string(nickname):
        user["waterfishNickname"] = optional_string(nickname)
    if optional_string(username):
        user["waterfishUsername"] = optional_string(username)
    if isinstance(rating, (int, float)) and not isinstance(rating, bool):
        user["waterfishRating"] = rating
    user["waterfishUpdatedAt"] = now_iso()
    write_cache(cache)
    return identity_snapshot(cache, normalized_qq)


def identity_snapshot(cache: dict[str, Any], qq: Any, group_id: Any = None) -> dict[str, Any] | None:
    normalized_qq = normalize_qq(qq)
    if not normalized_qq:
        return None
    user = cache.get("users", {}).get(normalized_qq)
    if not isinstance(user, dict):
        return None
    preferred_group = None
    normalized_group_id = normalize_qq(group_id)
    groups = user.get("groups") if isinstance(user.get("groups"), dict) else {}
    if normalized_group_id and isinstance(groups.get(normalized_group_id), dict):
        preferred_group = groups[normalized_group_id]
    group_entries = [value for value in groups.values() if isinstance(value, dict)]
    return {
        "qq": normalized_qq,
        "qqNickname": user.get("qqNickname"),
        "friendNickname": user.get("friendNickname"),
        "preferredGroup": preferred_group,
        "groups": sorted(
            group_entries,
            key=lambda item: (str(item.get("groupName") or ""), str(item.get("groupId") or "")),
        ),
        "waterfishNickname": user.get("waterfishNickname"),
        "waterfishUsername": user.get("waterfishUsername"),
        "waterfishRating": user.get("waterfishRating"),
        "isFriend": user.get("isFriend") is True,
    }


def get_identity(qq: Any, group_id: Any = None) -> dict[str, Any] | None:
    return identity_snapshot(read_cache(), qq, group_id)


def resolve_identities(query: Any, *, group_id: Any = None, max_results: int = 10) -> dict[str, Any]:
    raw_query = str(query).strip() if query is not None else ""
    if not raw_query:
        return {"query": raw_query, "matches": [], "ambiguous": False}
    normalized_query = raw_query.casefold()
    cache = read_cache()
    users = cache.get("users") if isinstance(cache.get("users"), dict) else {}
    matches = []
    for qq, user in users.items():
        if not isinstance(user, dict):
            continue
        candidate = identity_snapshot(cache, qq, group_id)
        if not candidate:
            continue
        score, fields = match_identity(candidate, raw_query, normalized_query)
        if score <= 0:
            continue
        candidate["matchScore"] = score
        candidate["matchedFields"] = fields
        matches.append(candidate)
    matches.sort(
        key=lambda item: (
            -int(item.get("matchScore") or 0),
            str(item.get("qq") or ""),
        )
    )
    limited = matches[:max(1, max_results)]
    exact_count = sum(1 for item in limited if int(item.get("matchScore") or 0) >= 100)
    return {
        "query": raw_query,
        "groupId": normalize_qq(group_id),
        "matches": limited,
        "ambiguous": exact_count > 1 or len(limited) > 1 and int(limited[0].get("matchScore") or 0) == int(limited[1].get("matchScore") or 0),
        "cache": {
            "fetchedAt": cache.get("fetchedAt"),
            "stats": cache.get("stats"),
            "path": str(cache_path()),
        },
    }


def match_identity(candidate: dict[str, Any], raw_query: str, normalized_query: str) -> tuple[int, list[str]]:
    fields: list[str] = []
    score = 0
    qq = str(candidate.get("qq") or "")
    if raw_query == qq:
        return 200, ["qq"]

    direct_fields = {
        "qqNickname": candidate.get("qqNickname"),
        "friendNickname": candidate.get("friendNickname"),
        "waterfishNickname": candidate.get("waterfishNickname"),
        "waterfishUsername": candidate.get("waterfishUsername"),
    }
    for field_name, value in direct_fields.items():
        field_score = score_name(value, normalized_query)
        if field_score:
            score = max(score, field_score)
            fields.append(field_name)

    preferred_group = candidate.get("preferredGroup")
    if isinstance(preferred_group, dict):
        for key in ("groupNickname", "card", "nickname"):
            field_score = score_name(preferred_group.get(key), normalized_query)
            if field_score:
                score = max(score, field_score + 10)
                fields.append(f"preferredGroup.{key}")

    for group in candidate.get("groups") or []:
        if not isinstance(group, dict):
            continue
        for key in ("groupNickname", "card", "nickname"):
            field_score = score_name(group.get(key), normalized_query)
            if field_score:
                score = max(score, field_score)
                fields.append(f"group.{key}")

    return score, sorted(set(fields))


def score_name(value: Any, normalized_query: str) -> int:
    if not isinstance(value, str) or not value.strip():
        return 0
    normalized_value = value.strip().casefold()
    if normalized_value == normalized_query:
        return 100
    if normalized_query in normalized_value:
        return 50
    return 0
