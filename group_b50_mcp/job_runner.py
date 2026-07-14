"""按 (feature, groupId) 分组的后台刷新任务调度。

被 group-rank-mcp 里的多个 feature（B50 / 单曲成绩榜 / 未来的其他榜单）共享。
每个 feature 自己管理 cache_dir 和 job_status.json 路径；本模块只持有 in-memory
的线程字典和锁，并提供路径无关的读写/进度更新原语。
"""

from __future__ import annotations

import json
import os
import threading
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable


# (feature, groupId) -> Thread。feature 让 B50 刷新和单曲成绩刷新可以同时
# 跑在同一个群上而不互相覆盖 in-memory 句柄。
BACKGROUND_JOBS: dict[tuple[str, str], threading.Thread] = {}
JOBS_LOCK = threading.Lock()
# 全局写状态锁。所有 feature 共用一把，避免不同 feature 之间产生交叉死锁；
# job 之间本来就只在自己 feature 的状态文件上写，互不阻塞热点。
STATE_LOCK = threading.RLock()


class StaleRefreshJob(Exception):
    """job 已被新 job 取代时由 ensure_current_refresh_job 抛出。"""


def now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def write_json_atomic(path: Path, data: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temp_path = path.with_name(f".{path.name}.{os.getpid()}.{threading.get_ident()}.tmp")
    temp_path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    os.replace(temp_path, path)


def read_job_status(status_path: Path) -> dict[str, Any] | None:
    if not status_path.exists():
        return None
    try:
        parsed = json.loads(status_path.read_text(encoding="utf-8"))
    except Exception:
        return None
    return parsed if isinstance(parsed, dict) else None


def write_job_status(status_path: Path, status: dict[str, Any]) -> None:
    write_json_atomic(status_path, status)


def ensure_current_refresh_job(status_path: Path, job_id: str) -> dict[str, Any]:
    current = read_job_status(status_path)
    if not current or current.get("jobId") != job_id:
        raise StaleRefreshJob()
    return current


def write_refresh_progress(
    *,
    status_path: Path,
    job_id: str,
    processed: int,
    total: int,
    cached_count: int,
    skipped_count: int,
    transient_failure_count: int,
    message: str,
) -> None:
    with STATE_LOCK:
        current = ensure_current_refresh_job(status_path, job_id)
        current.update(
            {
                "status": "running",
                "message": message,
                "processedCount": processed,
                "totalCount": total,
                "cachedCount": cached_count,
                "skippedCount": skipped_count,
                "transientFailureCount": transient_failure_count,
                "updatedAt": now_iso(),
            }
        )
        write_job_status(status_path, current)


def start_refresh_job(
    *,
    feature: str,
    group_id: str,
    status_path: Path,
    refresh_reason: str,
    runner: Callable[[str], None],
    start_message: str,
    thread_name: str | None = None,
    extra_fields: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """启动一个 feature/group 的后台刷新任务。

    - runner 必须接受 job_id 作为唯一参数；本模块负责生成 job_id 并写初始
      job_status，再把 job_id 传给 runner。
    - 同一 (feature, group_id) 已经在跑时直接返回当前 status，不重复启动。
    """
    key = (feature, group_id)
    with JOBS_LOCK:
        existing = BACKGROUND_JOBS.get(key)
        if existing and existing.is_alive():
            status = read_job_status(status_path)
            if status:
                return status
        job_id = uuid.uuid4().hex
        job: dict[str, Any] = {
            "jobId": job_id,
            "feature": feature,
            "groupId": group_id,
            "status": "running",
            "startedAt": now_iso(),
            "finishedAt": None,
            "refreshReason": refresh_reason,
            "message": start_message,
        }
        if extra_fields:
            job.update(extra_fields)
        write_job_status(status_path, job)
        thread = threading.Thread(
            target=runner,
            args=(job_id,),
            name=thread_name or f"group-rank-refresh-{feature}-{group_id}",
            daemon=True,
        )
        BACKGROUND_JOBS[key] = thread
        thread.start()
        return job
