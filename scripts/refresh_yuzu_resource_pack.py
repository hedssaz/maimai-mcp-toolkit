#!/usr/bin/env python3
"""Download Yuzu Resource.7z and overwrite maimaidx static assets."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.request
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
DEFAULT_RESOURCE_URL = "https://cloud.yuzuchan.moe/f/nXt6/Resource.7z"
DEFAULT_STATIC_DIR = ROOT / "maimaidx_render_mcp" / "static"
DEFAULT_TIMEOUT_SECONDS = 1800
DEFAULT_EXTRACT_TIMEOUT_SECONDS = 900


def env_int(name: str, default: int) -> int:
    try:
        return int(os.environ.get(name, str(default)))
    except (TypeError, ValueError):
        return default


def default_static_dir() -> Path:
    configured = os.environ.get("MAIMAIDX_STATIC_DIR")
    return Path(configured).expanduser().resolve() if configured else DEFAULT_STATIC_DIR.resolve()


def looks_like_static_dir(path: Path) -> bool:
    return (path / "mai" / "pic").is_dir() and (path / "mai" / "cover").is_dir()


def download_archive(url: str, target: Path, timeout_seconds: int) -> dict:
    request = urllib.request.Request(
        url,
        headers={"User-Agent": "maimai-resource-refresh/1.0"},
    )
    with urllib.request.urlopen(request, timeout=timeout_seconds) as response:
        target.parent.mkdir(parents=True, exist_ok=True)
        with target.open("wb") as file:
            shutil.copyfileobj(response, file)
        return {
            "contentLength": response.headers.get("content-length", ""),
        }


def find_7z_executable() -> str | None:
    configured = os.environ.get("MAIMAI_7Z")
    if configured:
        return configured
    for name in ("7z", "7zz", "7za"):
        found = shutil.which(name)
        if found:
            return found
    return None


def locate_static_source(extract_root: Path) -> Path:
    candidates = [
        extract_root / "Resource" / "static",
        extract_root / "static",
        extract_root,
    ]
    candidates.extend(path for path in extract_root.rglob("static") if path.is_dir())
    seen: set[Path] = set()
    for candidate in candidates:
        resolved = candidate.resolve()
        if resolved in seen:
            continue
        seen.add(resolved)
        if looks_like_static_dir(candidate):
            return candidate
    raise RuntimeError("extracted Resource.7z does not contain a recognizable static directory")


def extract_archive_source(archive: Path, extract_root: Path, timeout_seconds: int) -> Path:
    executable = find_7z_executable()
    if not executable:
        raise RuntimeError("7z/7zz/7za not found; cannot extract Resource.7z")
    extract_root.mkdir(parents=True, exist_ok=True)
    completed = subprocess.run(
        [executable, "x", "-y", str(archive), f"-o{extract_root}"],
        capture_output=True,
        text=True,
        timeout=timeout_seconds,
        check=False,
    )
    if completed.returncode != 0:
        stderr = (completed.stderr or completed.stdout or "").strip()[-1000:]
        raise RuntimeError(f"Resource.7z extraction failed: {stderr}")
    return locate_static_source(extract_root)


def refresh_resource_pack(
    *,
    url: str,
    static_dir: Path,
    timeout_seconds: int,
    extract_timeout_seconds: int,
) -> dict:
    with tempfile.TemporaryDirectory(prefix="maimai-yuzu-resource-download-") as temp_dir:
        temp_root = Path(temp_dir)
        archive = temp_root / "Resource.7z"
        download = download_archive(url, archive, timeout_seconds)
        source = extract_archive_source(archive, temp_root / "extracted", extract_timeout_seconds)
        static_dir.mkdir(parents=True, exist_ok=True)
        shutil.copytree(source, static_dir, dirs_exist_ok=True)
    return {
        "ok": True,
        "changed": True,
        "downloaded": True,
        "extracted": True,
        "contentLength": download.get("contentLength", ""),
        "staticDir": str(static_dir),
        "updatedAt": datetime.now(timezone.utc).isoformat(),
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Download Yuzu Resource.7z into the maimaidx static directory.")
    parser.add_argument("--url", default=os.environ.get("MAIMAI_YUZU_RESOURCE_URL", DEFAULT_RESOURCE_URL))
    parser.add_argument("--static-dir", type=Path, default=default_static_dir())
    parser.add_argument("--timeout", type=int, default=env_int("MAIMAI_YUZU_RESOURCE_TIMEOUT_SECONDS", DEFAULT_TIMEOUT_SECONDS))
    parser.add_argument(
        "--extract-timeout",
        type=int,
        default=env_int("MAIMAI_YUZU_RESOURCE_EXTRACT_TIMEOUT_SECONDS", DEFAULT_EXTRACT_TIMEOUT_SECONDS),
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    static_dir = args.static_dir.expanduser().resolve()
    try:
        result = refresh_resource_pack(
            url=args.url,
            static_dir=static_dir,
            timeout_seconds=max(1, args.timeout),
            extract_timeout_seconds=max(1, args.extract_timeout),
        )
    except Exception as exc:
        result = {
            "ok": False,
            "changed": False,
            "error": str(exc),
            "staticDir": str(static_dir),
        }
    print(json.dumps(result, ensure_ascii=False), flush=True)
    return 0 if result.get("ok") else 1


if __name__ == "__main__":
    sys.exit(main())
