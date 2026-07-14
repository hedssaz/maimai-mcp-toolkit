"""玩家级 B50 与完整成绩缓存的纯文件后端。"""

from __future__ import annotations

import json
import os
import threading
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any


# 每天 22:00 CST（14:00 UTC）过期，可通过环境变量覆盖 UTC 小时值。
DAILY_RESET_HOUR_UTC: int = int(os.environ.get("MAIMAI_CACHE_RESET_HOUR_UTC", "14"))


def get_cache_dir() -> Path:
    configured = os.environ.get("PLAYER_CACHE_DIR")
    if configured:
        return Path(configured).expanduser().resolve()
    return (Path.cwd() / "player-cache").resolve()


def _b50_path(qq: str) -> Path:
    return get_cache_dir() / "b50" / f"{qq}.json"


def _records_path(qq: str) -> Path:
    return get_cache_dir() / "records" / f"{qq}.json"


def _read_json(path: Path) -> dict[str, Any] | None:
    if not path.exists():
        return None
    try:
        parsed = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return None
    return parsed if isinstance(parsed, dict) else None


def _write_json_atomic(path: Path, data: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temp_path = path.with_name(
        f".{path.name}.{os.getpid()}.{threading.get_ident()}.tmp"
    )
    temp_path.write_text(
        json.dumps(data, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    os.replace(temp_path, path)


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def _daily_reset_time(now: datetime) -> datetime:
    reset = now.replace(hour=DAILY_RESET_HOUR_UTC, minute=0, second=0, microsecond=0)
    if now < reset:
        reset -= timedelta(days=1)
    return reset


def _is_fresh(entry: dict[str, Any] | None, ttl_seconds: int | None = None) -> bool:
    if not entry:
        return False
    fetched_at = entry.get("fetchedAt")
    if not isinstance(fetched_at, str):
        return False
    try:
        parsed = datetime.fromisoformat(fetched_at)
    except ValueError:
        return False
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    now = datetime.now(timezone.utc)
    if ttl_seconds is not None:
        return (now - parsed).total_seconds() <= ttl_seconds
    return parsed >= _daily_reset_time(now)


def _normalize_qq(qq: Any) -> str | None:
    if isinstance(qq, int):
        qq = str(qq)
    if not isinstance(qq, str) or not qq.strip():
        return None
    return qq.strip()


def read_player_b50(qq: str) -> dict[str, Any] | None:
    normalized = _normalize_qq(qq)
    if not normalized:
        return None
    return _read_json(_b50_path(normalized))


def write_player_b50(qq: str, b50: dict[str, Any]) -> None:
    normalized = _normalize_qq(qq)
    if not normalized or not isinstance(b50, dict):
        return
    _write_json_atomic(
        _b50_path(normalized),
        {"qq": normalized, "fetchedAt": _now_iso(), "b50": b50},
    )


def is_player_b50_fresh(
    entry: dict[str, Any] | None,
    *,
    ttl_seconds: int | None = None,
) -> bool:
    return _is_fresh(entry, ttl_seconds)


def read_player_records(qq: str) -> dict[str, Any] | None:
    normalized = _normalize_qq(qq)
    if not normalized:
        return None
    return _read_json(_records_path(normalized))


def write_player_records(qq: str, records: dict[str, Any]) -> None:
    normalized = _normalize_qq(qq)
    if not normalized or not isinstance(records, dict):
        return
    _write_json_atomic(
        _records_path(normalized),
        {"qq": normalized, "fetchedAt": _now_iso(), "records": records},
    )


def is_player_records_fresh(
    entry: dict[str, Any] | None,
    *,
    ttl_seconds: int | None = None,
) -> bool:
    return _is_fresh(entry, ttl_seconds)


def merge_player_record(qq: Any, raw_record: dict[str, Any]) -> bool:
    """把单条成绩合并进已有完整缓存；缓存不存在时不创建。"""

    normalized = _normalize_qq(qq)
    if not normalized or not isinstance(raw_record, dict):
        return False
    entry = read_player_records(normalized)
    if not isinstance(entry, dict):
        return False
    records_doc = (
        entry.get("records") if isinstance(entry.get("records"), dict) else None
    )
    if not isinstance(records_doc, dict):
        return False
    records_list = (
        records_doc.get("records")
        if isinstance(records_doc.get("records"), list)
        else []
    )

    target_song_id = raw_record.get("song_id")
    target_level = raw_record.get("level_index")
    matched_index: int | None = None
    for index, existing in enumerate(records_list):
        if (
            isinstance(existing, dict)
            and existing.get("song_id") == target_song_id
            and existing.get("level_index") == target_level
        ):
            matched_index = index
            break
    if matched_index is None:
        records_list.append(raw_record)
    else:
        records_list[matched_index] = raw_record

    records_doc["records"] = records_list
    write_player_records(normalized, records_doc)
    return True
