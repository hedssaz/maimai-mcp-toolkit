from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor, as_completed
import json
import os
import subprocess
import sys
import threading
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
DEFAULT_TTL_DAYS = 30 / 1440  # 30 分钟
DEFAULT_TIMEOUT_SECONDS = 30
SOURCE_ALIASES = {
    "all": "all",
    "lxns": "lxns",
    "落雪": "lxns",
    "cn": "lxns",
    "yuzu": "yuzu",
    "柚子": "yuzu",
    "yuzu_alias": "yuzu",
    "yuzu_aliases": "yuzu",
    "music_alias": "yuzu",
    "别名补充": "yuzu",
    "divingfish": "divingfish",
    "水鱼": "divingfish",
    "df": "divingfish",
    "chart_stats": "chart_stats",
    "chartstats": "chart_stats",
    "fit": "chart_stats",
    "拟合定数": "chart_stats",
    "plate": "plate",
    "牌子": "plate",
    "plate_data": "plate",
}


class RefreshError(ValueError):
    """User-facing source refresh error."""


def source_definitions() -> dict[str, dict[str, Any]]:
    return {
        "lxns": {
            "label": "LXNS 曲库和别名",
            "targets": [
                ROOT / "data" / "lxns_song_list.json",
                ROOT / "data" / "lxns_alias_list.json",
            ],
            "command": [sys.executable, "scripts/update_music_data.py"],
        },
        "divingfish": {
            "label": "DivingFish 国服曲库",
            "targets": [ROOT / "data" / "divingfish_song_list.json"],
            "command": [sys.executable, "scripts/update_divingfish_data.py"],
        },
        "yuzu": {
            "label": "Yuzu 别名补充",
            "targets": [ROOT / "data" / "music_alias.json"],
            "command": [sys.executable, "scripts/update_yuzu_alias_data.py"],
        },
        "chart_stats": {
            "label": "Diving-Fish 拟合定数",
            "targets": [ROOT / "data" / "divingfish_chart_stats.json"],
            "command": [sys.executable, "scripts/update_chart_stats.py"],
        },
        "plate": {
            "label": "CN 牌子曲目白名单",
            "targets": [ROOT / "data" / "maimaidxplate.json"],
            "command": [sys.executable, "scripts/update_plate_data.py"],
        },
    }


def parse_ttl_days(value: Any) -> float:
    if value in (None, ""):
        return DEFAULT_TTL_DAYS
    try:
        ttl_days = float(value)
    except (TypeError, ValueError) as exc:
        raise RefreshError("ttl_days must be a number") from exc
    if ttl_days < 0:
        raise RefreshError("ttl_days must be >= 0")
    return ttl_days


def parse_bool(value: Any, default: bool = False) -> bool:
    if value in (None, ""):
        return default
    if isinstance(value, bool):
        return value
    text = str(value).strip().lower()
    if text in {"1", "true", "yes", "y", "on"}:
        return True
    if text in {"0", "false", "no", "n", "off"}:
        return False
    raise RefreshError("boolean value must be true or false")


def normalize_sources(value: Any) -> list[str]:
    if value in (None, "", "all"):
        return list(source_definitions())
    if isinstance(value, str):
        raw_values = [part.strip() for part in value.split(",") if part.strip()]
    elif isinstance(value, list):
        raw_values = [str(item).strip() for item in value if str(item).strip()]
    else:
        raise RefreshError("sources must be a string or string array")

    normalized: list[str] = []
    for raw in raw_values:
        key = raw.lower().replace("-", "_").replace(" ", "_")
        source = SOURCE_ALIASES.get(key)
        if source is None:
            raise RefreshError("unknown source: " + raw)
        if source == "all":
            return list(source_definitions())
        if source not in normalized:
            normalized.append(source)
    if not normalized:
        raise RefreshError("at least one source is required")
    return normalized


def target_mtimes(targets: list[Path]) -> dict[Path, float]:
    return {target: target.stat().st_mtime for target in targets if target.exists()}


def source_status(source: str, ttl_days: float, now: float | None = None) -> dict[str, Any]:
    definitions = source_definitions()
    if source not in definitions:
        raise RefreshError("unknown source: " + source)
    now = time.time() if now is None else now
    definition = definitions[source]
    targets = definition["targets"]
    target_mtime_map = target_mtimes(targets)
    missing_targets = [target for target in targets if target not in target_mtime_map]
    ttl_seconds = ttl_days * 86400
    oldest_mtime = min(target_mtime_map.values()) if target_mtime_map else None
    age_seconds = None if oldest_mtime is None else max(0.0, now - oldest_mtime)
    target_states = []
    expired_targets = []
    for target in targets:
        target_mtime = target_mtime_map.get(target)
        target_age_seconds = None if target_mtime is None else max(0.0, now - target_mtime)
        target_expired = target_mtime is None or target_age_seconds is None or target_age_seconds >= ttl_seconds
        if target_expired:
            expired_targets.append(target)
        target_states.append(
            {
                "target": str(target.relative_to(ROOT)),
                "exists": target_mtime is not None,
                "mtime": datetime.fromtimestamp(target_mtime, timezone.utc).isoformat() if target_mtime is not None else None,
                "age_seconds": round(target_age_seconds, 3) if target_age_seconds is not None else None,
                "age_days": round(target_age_seconds / 86400, 6) if target_age_seconds is not None else None,
                "expired": target_expired,
            }
        )
    expired = bool(expired_targets)
    return {
        "source": source,
        "label": definition["label"],
        "targets": [str(path.relative_to(ROOT)) for path in targets],
        "exists": not missing_targets and oldest_mtime is not None,
        "missing_targets": [str(path.relative_to(ROOT)) for path in missing_targets],
        "expired_targets": [str(path.relative_to(ROOT)) for path in expired_targets],
        "target_statuses": target_states,
        "mtime": datetime.fromtimestamp(oldest_mtime, timezone.utc).isoformat() if oldest_mtime is not None else None,
        "age_seconds": round(age_seconds, 3) if age_seconds is not None else None,
        "age_days": round(age_seconds / 86400, 6) if age_seconds is not None else None,
        "ttl_days": ttl_days,
        "expired": expired,
    }


def command_for_source(source: str) -> list[str]:
    definitions = source_definitions()
    command = definitions[source].get("command")
    if not command:
        raise RefreshError("source has no refresh command: " + source)
    return list(command)


def run_command(command: list[str], timeout_seconds: int) -> dict[str, Any]:
    started_at = time.time()
    try:
        completed = subprocess.run(
            command,
            cwd=ROOT,
            text=True,
            capture_output=True,
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        stdout = exc.stdout.decode("utf-8", errors="replace") if isinstance(exc.stdout, bytes) else exc.stdout
        stderr = exc.stderr.decode("utf-8", errors="replace") if isinstance(exc.stderr, bytes) else exc.stderr
        return {
            "command": command,
            "returncode": None,
            "timeout": True,
            "duration_seconds": round(time.time() - started_at, 3),
            "stdout": (stdout or "").strip()[-4000:],
            "stderr": (stderr or "").strip()[-4000:],
        }
    return {
        "command": command,
        "returncode": completed.returncode,
        "timeout": False,
        "duration_seconds": round(time.time() - started_at, 3),
        "stdout": completed.stdout.strip()[-4000:],
        "stderr": completed.stderr.strip()[-4000:],
    }


def refresh_one_source(source: str, timeout_seconds: int) -> dict[str, Any]:
    try:
        command = command_for_source(source)
        command_result = run_command(command, timeout_seconds)
    except Exception as exc:
        return {"source": source, "error": str(exc)}
    command_result["source"] = source
    return command_result


def refresh_sources(arguments: dict[str, Any] | None = None) -> dict[str, Any]:
    arguments = arguments or {}
    ttl_days = parse_ttl_days(arguments.get("ttl_days", arguments.get("source_ttl_days")))
    sources = normalize_sources(arguments.get("sources", arguments.get("source")))
    force = parse_bool(arguments.get("force"), False)
    check_only = parse_bool(arguments.get("check_only"), False)
    timeout_seconds = int(arguments.get("timeout_seconds", DEFAULT_TIMEOUT_SECONDS))
    if timeout_seconds < 1:
        raise RefreshError("timeout_seconds must be >= 1")

    now = time.time()
    statuses_before = {
        source: source_status(source, ttl_days, now)
        for source in sources
    }
    due_sources = [
        source
        for source in sources
        if force or statuses_before[source]["expired"]
    ]

    result: dict[str, Any] = {
        "ttl_days": ttl_days,
        "force": force,
        "check_only": check_only,
        "requested_sources": sources,
        "due_sources": due_sources,
        "refreshed_sources": [],
        "skipped_sources": [source for source in sources if source not in due_sources],
        "failed_sources": [],
        "sources": statuses_before,
        "commands": [],
        "derived_updated": False,
    }
    if check_only or not due_sources:
        return result

    command_results: dict[str, dict[str, Any]] = {}
    with ThreadPoolExecutor(max_workers=min(len(due_sources), 4)) as executor:
        futures = {
            executor.submit(refresh_one_source, source, timeout_seconds): source
            for source in due_sources
        }
        for future in as_completed(futures):
            source = futures[future]
            try:
                command_results[source] = future.result()
            except Exception as exc:
                command_results[source] = {"source": source, "error": str(exc)}

    for source in due_sources:
        command_result = command_results[source]
        command_result["source"] = source
        result["commands"].append(command_result)
        if command_result.get("returncode") == 0:
            result["refreshed_sources"].append(source)
        else:
            result["failed_sources"].append(source)

    if result["refreshed_sources"]:
        compile_result = run_command([sys.executable, "-m", "compileall", "-x", r"(^|/)\._", "maimai_mcp"], timeout_seconds)
        compile_result["source"] = "derived"
        result["commands"].append(compile_result)
        result["derived_updated"] = compile_result["returncode"] == 0
        if compile_result["returncode"] != 0:
            result["failed_sources"].append("derived")

    result["sources"] = {
        source: source_status(source, ttl_days)
        for source in sources
    }
    return result


# ============================================================
# 后台刷新（群榜模式：spawn 线程，立即返回，不阻塞 MCP 调用）
# ============================================================

_BG_JOB_STATUS_DIR = ROOT / "logs" / "source_refresh"
_BG_JOB_LOCK = threading.Lock()


def _bg_write_status(status_path: Path, data: dict[str, Any]) -> None:
    status_path.parent.mkdir(parents=True, exist_ok=True)
    temp = status_path.with_name(f".{status_path.name}.{os.getpid()}.{threading.get_ident()}.tmp")
    temp.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    os.replace(temp, status_path)


def _bg_run_refresh(job_id: str, arguments: dict[str, Any]) -> None:
    """后台线程：并发刷新所有源，每完成一个就更新进度文件。"""
    status_path = _BG_JOB_STATUS_DIR / f"{job_id}.json"
    sources = normalize_sources(arguments.get("sources", arguments.get("source")))
    force = parse_bool(arguments.get("force"), False)
    timeout_seconds = int(arguments.get("timeout_seconds", DEFAULT_TIMEOUT_SECONDS))

    status = {
        "jobId": job_id,
        "status": "running",
        "startedAt": datetime.now(timezone.utc).isoformat(),
        "totalSources": len(sources),
        "completedSources": 0,
        "succeededSources": [],
        "failedSources": [],
        "sources": {},
        "message": "并发刷新中...",
    }
    _bg_write_status(status_path, status)

    # 并发执行
    with ThreadPoolExecutor(max_workers=min(len(sources), 4)) as executor:
        futures = {
            executor.submit(refresh_one_source, source, timeout_seconds): source
            for source in sources
        }
        for future in as_completed(futures):
            source = futures[future]
            try:
                command_result = future.result()
            except Exception as exc:
                command_result = {"source": source, "error": str(exc)}
            ok = command_result.get("returncode") == 0
            if ok:
                status["succeededSources"].append(source)
            else:
                status["failedSources"].append(source)
            status["completedSources"] += 1
            status["sources"][source] = command_result
            status["message"] = f"刷新进度: {status['completedSources']}/{len(sources)} (成功 {len(status['succeededSources'])}, 失败 {len(status['failedSources'])})"
            _bg_write_status(status_path, status)

    if status["succeededSources"]:
        try:
            from .search import clear_search_caches

            clear_search_caches()
        except Exception as exc:
            status["cacheInvalidationError"] = str(exc)

    status["status"] = "finished"
    status["finishedAt"] = datetime.now(timezone.utc).isoformat()
    status["message"] = f"刷新完成: 成功 {len(status['succeededSources'])}/{len(sources)}"
    _bg_write_status(status_path, status)


def read_bg_job_status(job_id: str) -> dict[str, Any]:
    """读取后台刷新进度"""
    status_path = _BG_JOB_STATUS_DIR / f"{job_id}.json"
    if not status_path.exists():
        return {"error": f"未找到刷新任务 {job_id}"}
    try:
        return json.loads(status_path.read_text(encoding="utf-8"))
    except Exception:
        return {"error": "读取刷新状态失败"}


def refresh_sources_bg(arguments: dict[str, Any]) -> dict[str, Any]:
    """后台刷新：spawn 线程跑 refresh_one_source 逐源刷新，立即返回 jobId。"""
    job_id = uuid.uuid4().hex[:12]
    ttl_days = parse_ttl_days(arguments.get("ttl_days", arguments.get("source_ttl_days")))
    sources = normalize_sources(arguments.get("sources", arguments.get("source")))
    force = parse_bool(arguments.get("force"), False)
    source_states = {s: source_status(s, ttl_days) for s in sources}
    due = [s for s in sources if force or source_states[s]["expired"]]

    thread = threading.Thread(
        target=_bg_run_refresh,
        args=(job_id, arguments),
        name=f"source-refresh-{job_id}",
        daemon=True,
    )
    thread.start()

    return {
        "background": True,
        "jobId": job_id,
        "status": "running",
        "totalSources": len(sources),
        "dueSources": len(due),
        "sourceStates": {
            s: {"expired": source_states[s]["expired"], "age_days": source_states[s]["age_days"],
                "label": source_states[s]["label"]}
            for s in sources
        },
        "message": f"后台刷新已启动（{len(due)}/{len(sources)} 源过期）。"
                   f"稍后调用 refresh_maimai_sources_job_status(jobId=\"{job_id}\") 查看进度。",
    }
