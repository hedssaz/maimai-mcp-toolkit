"""每日定时刷新曲库数据源，供 cron 调用。

设计原则：
- 每个数据源独立运行，一个失败不影响其余。
- 各更新脚本内部使用 tempfile + os.replace，失败时原文件不动。
- 最后打印一行 JSON 摘要，便于 cron 日志解析。
"""

from __future__ import annotations

import json
import hashlib
import os
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parent.parent

SOURCES: list[tuple[str, str]] = [
    ("lxns",               "scripts/update_music_data.py"),
    ("divingfish",         "scripts/update_divingfish_data.py"),
    ("yuzu",               "scripts/update_yuzu_alias_data.py"),
    ("chart_stats",        "scripts/update_chart_stats.py"),
    ("plate",              "scripts/update_plate_data.py"),
]

CN_RENDER_INPUTS: list[tuple[str, Path]] = [
    ("lxns", ROOT / "data" / "lxns_song_list.json"),
    ("divingfish", ROOT / "data" / "divingfish_song_list.json"),
    ("plate", ROOT / "data" / "maimaidxplate.json"),
]


def env_int(name: str, default: int) -> int:
    try:
        return int(os.environ.get(name, str(default)))
    except (TypeError, ValueError):
        return default


RENDER_REGEN_TIMEOUT_SECONDS = env_int("MAIMAI_RENDER_BACKGROUND_REGEN_TIMEOUT_SECONDS", 1800)
RESOURCE_REFRESH_TIMEOUT_SECONDS = env_int("MAIMAI_YUZU_RESOURCE_REFRESH_TIMEOUT_SECONDS", 1800)


def env_bool(name: str, default: bool) -> bool:
    value = os.environ.get(name)
    if value in (None, ""):
        return default
    return value.strip().lower() in {"1", "true", "yes", "y", "on"}


def canonical_json_bytes(payload: object) -> bytes:
    return json.dumps(
        payload,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")


def read_json(path: Path) -> object:
    if not path.exists():
        return {"missing": True}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {"raw_sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def normalize_scalar(value: object) -> object:
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        number = round(float(value), 6)
        return int(number) if number.is_integer() else number
    return value


def normalize_lxns_render_input(payload: object) -> list[dict]:
    songs = payload.get("songs") if isinstance(payload, dict) else payload
    if not isinstance(songs, list):
        return []
    normalized = []
    for song in songs:
        if not isinstance(song, dict):
            continue
        difficulties = song.get("difficulties") if isinstance(song.get("difficulties"), dict) else {}
        charts = []
        for chart_type, chart_items in difficulties.items():
            if not isinstance(chart_items, list):
                continue
            for chart in chart_items:
                if not isinstance(chart, dict):
                    continue
                charts.append(
                    {
                        "type": chart.get("type") or chart_type,
                        "difficulty": normalize_scalar(chart.get("difficulty")),
                        "level": str(chart.get("level") or ""),
                        "ds": normalize_scalar(chart.get("level_value")),
                    }
                )
        if charts:
            normalized.append(
                {
                    "id": str(song.get("id") or ""),
                    "charts": sorted(charts, key=lambda item: (
                        str(item.get("type")),
                        str(item.get("difficulty")),
                        str(item.get("level")),
                        str(item.get("ds")),
                    )),
                }
            )
    return sorted(normalized, key=lambda item: str(item.get("id")))


def normalize_divingfish_render_input(payload: object) -> list[dict]:
    songs = payload if isinstance(payload, list) else []
    normalized = []
    for song in songs:
        if not isinstance(song, dict):
            continue
        normalized.append(
            {
                "id": str(song.get("id") or ""),
                "type": str(song.get("type") or ""),
                "level": [str(value) for value in song.get("level") or []],
                "ds": [normalize_scalar(value) for value in song.get("ds") or []],
            }
        )
    return sorted(normalized, key=lambda item: (item["id"], item["type"]))


def normalize_plate_render_input(payload: object) -> dict[str, list[str]]:
    content = payload.get("content", payload) if isinstance(payload, dict) else {}
    if not isinstance(content, dict):
        return {}
    return {
        str(name): sorted(str(song_id) for song_id in ids)
        for name, ids in content.items()
        if isinstance(ids, list)
    }


def normalized_cn_render_input(root: Path = ROOT) -> dict:
    return {
        "lxns": normalize_lxns_render_input(read_json(root / "data" / "lxns_song_list.json")),
        "divingfish": normalize_divingfish_render_input(read_json(root / "data" / "divingfish_song_list.json")),
        "plate": normalize_plate_render_input(read_json(root / "data" / "maimaidxplate.json")),
    }


def cn_render_input_fingerprint(root: Path = ROOT) -> dict:
    normalized = normalized_cn_render_input(root)
    digest = hashlib.sha256()
    files = {}
    for label, configured_path in CN_RENDER_INPUTS:
        path = root / configured_path.relative_to(ROOT)
        content = canonical_json_bytes(normalized[label])
        file_digest = hashlib.sha256(content).hexdigest()
        rel_path = str(path.relative_to(root))
        digest.update(label.encode("utf-8"))
        digest.update(b"\0")
        digest.update(rel_path.encode("utf-8"))
        digest.update(b"\0")
        digest.update(file_digest.encode("ascii"))
        digest.update(b"\0")
        files[label] = {
            "path": rel_path,
            "exists": path.exists(),
            "sha256": file_digest,
        }
    return {"sha256": digest.hexdigest(), "files": files}


def run_render_background_regeneration(timeout: int = RENDER_REGEN_TIMEOUT_SECONDS) -> dict:
    start = time.monotonic()
    try:
        result = subprocess.run(
            [sys.executable, "scripts/regenerate_render_backgrounds.py"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        elapsed = round(time.monotonic() - start, 2)
        return {
            "ok": result.returncode == 0,
            "returncode": result.returncode,
            "elapsed": elapsed,
            "stdout": result.stdout.strip()[-4000:] if result.stdout else "",
            "stderr": result.stderr.strip()[-4000:] if result.stderr else "",
        }
    except subprocess.TimeoutExpired:
        elapsed = round(time.monotonic() - start, 2)
        return {
            "ok": False,
            "returncode": None,
            "elapsed": elapsed,
            "stdout": "",
            "stderr": f"timeout after {timeout}s",
        }
    except Exception as exc:
        elapsed = round(time.monotonic() - start, 2)
        return {
            "ok": False,
            "returncode": None,
            "elapsed": elapsed,
            "stdout": "",
            "stderr": str(exc),
        }


def run_yuzu_resource_refresh(timeout: int = RESOURCE_REFRESH_TIMEOUT_SECONDS) -> dict:
    if not env_bool("MAIMAI_YUZU_RESOURCE_REFRESH", True):
        return {"ok": True, "changed": False, "skipped": True, "reason": "disabled by MAIMAI_YUZU_RESOURCE_REFRESH"}
    start = time.monotonic()
    try:
        result = subprocess.run(
            [sys.executable, "scripts/refresh_yuzu_resource_pack.py"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        elapsed = round(time.monotonic() - start, 2)
        stdout = result.stdout.strip()
        parsed: dict[str, Any] = {}
        if stdout:
            try:
                parsed = json.loads(stdout.splitlines()[-1])
            except Exception:
                parsed = {}
        parsed.update(
            {
                "ok": result.returncode == 0 and parsed.get("ok", True),
                "returncode": result.returncode,
                "elapsed": elapsed,
                "stdout": stdout[-4000:] if stdout else "",
                "stderr": result.stderr.strip()[-4000:] if result.stderr else "",
            }
        )
        return parsed
    except subprocess.TimeoutExpired:
        elapsed = round(time.monotonic() - start, 2)
        return {
            "ok": False,
            "changed": False,
            "returncode": None,
            "elapsed": elapsed,
            "stdout": "",
            "stderr": f"timeout after {timeout}s",
        }
    except Exception as exc:
        elapsed = round(time.monotonic() - start, 2)
        return {
            "ok": False,
            "changed": False,
            "returncode": None,
            "elapsed": elapsed,
            "stdout": "",
            "stderr": str(exc),
        }


def run_source(name: str, script: str, timeout: int = 60) -> dict:
    start = time.monotonic()
    try:
        result = subprocess.run(
            [sys.executable, script],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        elapsed = round(time.monotonic() - start, 2)
        ok = result.returncode == 0
        return {
            "source": name,
            "ok": ok,
            "returncode": result.returncode,
            "elapsed": elapsed,
            "stdout": result.stdout.strip()[-2000:] if result.stdout else "",
            "stderr": result.stderr.strip()[-2000:] if result.stderr else "",
        }
    except subprocess.TimeoutExpired:
        elapsed = round(time.monotonic() - start, 2)
        return {"source": name, "ok": False, "returncode": None, "elapsed": elapsed,
                "stdout": "", "stderr": f"timeout after {timeout}s"}
    except Exception as exc:
        elapsed = round(time.monotonic() - start, 2)
        return {"source": name, "ok": False, "returncode": None, "elapsed": elapsed,
                "stdout": "", "stderr": str(exc)}


def main() -> None:
    started_at = datetime.now(timezone.utc).isoformat()
    print(f"[nightly_refresh] started at {started_at}", flush=True)
    cn_inputs_before = cn_render_input_fingerprint()

    results = []
    for name, script in SOURCES:
        print(f"  → {name} ...", end=" ", flush=True)
        r = run_source(name, script)
        status = "OK" if r["ok"] else f"FAILED (rc={r['returncode']})"
        print(f"{status}  ({r['elapsed']}s)", flush=True)
        if not r["ok"] and r["stderr"]:
            for line in r["stderr"].splitlines()[-5:]:
                print(f"     {line}", flush=True)
        results.append(r)

    # 成功后重新编译派生缓存
    succeeded = [r["source"] for r in results if r["ok"]]
    failed = [r["source"] for r in results if not r["ok"]]
    if succeeded:
        compile_result = subprocess.run(
            [sys.executable, "-m", "compileall", "-q", "-x", r"(^|/)\._", "maimai_mcp"],
            cwd=ROOT, capture_output=True, text=True,
        )
        if compile_result.returncode != 0:
            print(f"  [warn] compileall failed: {compile_result.stderr.strip()[-500:]}", flush=True)

    cn_inputs_after = cn_render_input_fingerprint()
    cn_inputs_changed = cn_inputs_before["sha256"] != cn_inputs_after["sha256"]

    if cn_inputs_changed:
        print("  → yuzu resource ...", end=" ", flush=True)
        resource_refresh = run_yuzu_resource_refresh()
        if resource_refresh.get("skipped"):
            print(f"SKIPPED ({resource_refresh.get('reason', 'disabled')})", flush=True)
        else:
            status = "OK" if resource_refresh.get("ok") else f"FAILED (rc={resource_refresh.get('returncode')})"
            print(f"{status}  ({resource_refresh.get('elapsed', 0)}s)", flush=True)
            if not resource_refresh.get("ok") and resource_refresh.get("stderr"):
                for line in str(resource_refresh["stderr"]).splitlines()[-5:]:
                    print(f"     {line}", flush=True)
    else:
        resource_refresh = {
            "ok": True,
            "skipped": True,
            "reason": "cn render inputs unchanged",
        }
        print("  → yuzu resource ... SKIPPED (cn render inputs unchanged)", flush=True)

    if cn_inputs_changed and resource_refresh.get("ok"):
        print("  → render backgrounds ...", end=" ", flush=True)
        background_regeneration = run_render_background_regeneration()
        status = "OK" if background_regeneration["ok"] else (
            f"FAILED (rc={background_regeneration['returncode']})"
        )
        print(f"{status}  ({background_regeneration['elapsed']}s)", flush=True)
        if not background_regeneration["ok"] and background_regeneration["stderr"]:
            for line in background_regeneration["stderr"].splitlines()[-5:]:
                print(f"     {line}", flush=True)
    elif cn_inputs_changed:
        background_regeneration = {
            "ok": False,
            "skipped": True,
            "reason": "yuzu resource refresh failed",
        }
        print("  → render backgrounds ... SKIPPED (yuzu resource refresh failed)", flush=True)
    else:
        background_regeneration = {
            "ok": True,
            "skipped": True,
            "reason": "cn render inputs unchanged",
        }
        print("  → render backgrounds ... SKIPPED (cn render inputs unchanged)", flush=True)

    summary = {
        "startedAt": started_at,
        "finishedAt": datetime.now(timezone.utc).isoformat(),
        "succeeded": succeeded,
        "failed": failed,
        "cnRenderInputsChanged": cn_inputs_changed,
        "cnRenderInputsBefore": cn_inputs_before["sha256"],
        "cnRenderInputsAfter": cn_inputs_after["sha256"],
        "resourceRefresh": resource_refresh,
        "backgroundRegeneration": background_regeneration,
    }
    print(json.dumps(summary, ensure_ascii=False), flush=True)

    # 有失败也以 0 退出，避免 cron 发错误邮件；失败细节已在日志中
    sys.exit(0)


if __name__ == "__main__":
    main()
