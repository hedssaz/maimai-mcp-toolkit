from __future__ import annotations

import contextlib
import contextvars
import os
import time
from datetime import datetime
from pathlib import Path
from uuid import uuid4
from typing import Any

_OUTPUT_CONTEXT: contextvars.ContextVar[dict[str, Any] | None] = contextvars.ContextVar(
    "maimaidx_output_context",
    default=None,
)
_LAST_CLEANUP = 0.0


def safe_filename_part(value: str) -> str:
    text = "".join(c if c.isascii() and (c.isalnum() or c in "-_.") else "_" for c in value)
    text = "_".join(part for part in text.split("_") if part)
    return text.strip("._-") or "maimai"


def _output_base(output_dir: str | None = None) -> Path:
    base = Path(output_dir) if output_dir else Path(
        os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR", str(Path.cwd() / "maimai-images"))
    )
    base.mkdir(parents=True, exist_ok=True)
    return base


def _unlink_quietly(path: Path) -> None:
    try:
        path.unlink()
    except FileNotFoundError:
        pass
    except OSError:
        pass


def _cleanup_old_outputs(base: Path) -> None:
    global _LAST_CLEANUP
    now = time.time()
    interval = float(os.environ.get("MAIMAIDX_RENDER_OUTPUT_CLEANUP_INTERVAL_SECONDS", "300"))
    if interval > 0 and now - _LAST_CLEANUP < interval:
        return
    _LAST_CLEANUP = now

    ttl = float(os.environ.get("MAIMAIDX_RENDER_OUTPUT_TTL_SECONDS", "21600"))
    max_files = int(os.environ.get("MAIMAIDX_RENDER_OUTPUT_MAX_FILES", "500"))
    files = [path for path in base.glob("*.png") if path.is_file()]

    if ttl > 0:
        cutoff = now - ttl
        for path in files:
            try:
                if path.stat().st_mtime < cutoff:
                    _unlink_quietly(path)
            except OSError:
                pass

    if max_files > 0:
        remaining = [path for path in base.glob("*.png") if path.is_file()]
        if len(remaining) > max_files:
            remaining.sort(key=lambda path: path.stat().st_mtime if path.exists() else 0)
            for path in remaining[: len(remaining) - max_files]:
                _unlink_quietly(path)


def _next_stem_index(base: Path, stem: str) -> int:
    prefix = f"{stem}_"
    highest = -1
    for path in base.glob(f"{stem}_*.png"):
        suffix = path.stem[len(prefix):] if path.stem.startswith(prefix) else ""
        if suffix.isdigit():
            highest = max(highest, int(suffix))
    return highest + 1


@contextlib.contextmanager
def image_output_context(stem: str, output_dir: str | None = None):
    base = _output_base(output_dir)
    safe_stem = safe_filename_part(stem)
    _cleanup_old_outputs(base)
    token = _OUTPUT_CONTEXT.set({"stem": safe_stem, "counter": _next_stem_index(base, safe_stem), "base": base})
    try:
        yield
    finally:
        _OUTPUT_CONTEXT.reset(token)


def next_image_path(prefix: str, output_dir: str | None = None) -> Path:
    context = _OUTPUT_CONTEXT.get()
    if context is not None and output_dir is None:
        base = context["base"]
        index = int(context["counter"])
        context["counter"] = index + 1
        return base / f"{context['stem']}_{index:03d}.png"

    base = _output_base(output_dir)
    _cleanup_old_outputs(base)
    safe_prefix = safe_filename_part(prefix)
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S_%f")
    return base / f"{safe_prefix}_{timestamp}_{uuid4().hex[:8]}.png"


def image_path_payload(path: Path, image: Any | None = None) -> dict[str, Any]:
    if image is not None and hasattr(image, "size"):
        width, height = image.size
    else:
        from PIL import Image

        with Image.open(path) as opened:
            width, height = opened.size
    return {
        "imagePath": str(path),
        "mimeType": "image/png",
        "width": width,
        "height": height,
    }
