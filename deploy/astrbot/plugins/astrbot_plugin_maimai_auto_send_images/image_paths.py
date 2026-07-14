from __future__ import annotations

import json
import os
import re
from pathlib import Path
from typing import Any, Iterable


IMAGE_EXTENSIONS = (".png", ".jpg", ".jpeg", ".webp", ".gif")
DEFAULT_RENDER_TOOL_NAMES = {
    "render_b50_image",
}
DEFAULT_RENDER_TOOL_PREFIXES = (
    "render_maimai_",
)
TEXT_PATH_RE = re.compile(
    r"(?P<path>(?:/[^\s\"'<>，。；：、)）\]}]+|[A-Za-z]:[\\/][^\s\"'<>，。；：、)）\]}]+)"
    r"\.(?:png|jpg|jpeg|webp|gif))",
    re.IGNORECASE,
)


def is_render_tool_name(tool_name: str, extra_patterns: Iterable[str] = ()) -> bool:
    if tool_name in DEFAULT_RENDER_TOOL_NAMES:
        return True
    if any(tool_name.startswith(prefix) for prefix in DEFAULT_RENDER_TOOL_PREFIXES):
        return True
    for pattern in extra_patterns:
        pattern = pattern.strip()
        if not pattern:
            continue
        try:
            if re.search(pattern, tool_name):
                return True
        except re.error:
            continue
    return False


def is_subagent_tool_name(tool_name: str) -> bool:
    lowered = tool_name.lower()
    return (
        lowered.startswith("transfer_to_")
        or lowered.startswith("handoff_to_")
        or "subagent" in lowered
        or "sub_agent" in lowered
    )


def is_maimai_subagent_tool_name(tool_name: str) -> bool:
    lowered = tool_name.lower()
    return lowered in {"transfer_to_maimai", "handoff_to_maimai"} or lowered.startswith(
        ("transfer_to_maimai_", "handoff_to_maimai_")
    )


def parse_lines(value: Any) -> list[str]:
    if not isinstance(value, str):
        return []
    return [line.strip() for line in value.splitlines() if line.strip()]


def parse_prefix_mappings(value: Any) -> list[tuple[str, str]]:
    mappings: list[tuple[str, str]] = []
    for line in parse_lines(value):
        if "=>" in line:
            old, new = line.split("=>", 1)
        elif "=" in line:
            old, new = line.split("=", 1)
        else:
            continue
        old = old.strip()
        new = new.strip()
        if old and new:
            mappings.append((old, new))
    return mappings


def mapped_path(path: str, mappings: Iterable[tuple[str, str]]) -> str:
    for old, new in mappings:
        if path == old or path.startswith(old.rstrip("/") + "/"):
            return new.rstrip("/") + path[len(old.rstrip("/")) :]
    return path


def existing_image_path(path: str, mappings: Iterable[tuple[str, str]] = ()) -> str | None:
    candidate = mapped_path(path.strip(), mappings)
    if not candidate.lower().endswith(IMAGE_EXTENSIONS):
        return None
    resolved = Path(candidate).expanduser()
    if resolved.exists() and resolved.is_file():
        return str(resolved)
    return None


def collect_paths_from_mapping(value: Any) -> list[str]:
    paths: list[str] = []
    if isinstance(value, dict):
        for key, item in value.items():
            if key in {"imagePath", "image_path", "path", "filePath", "file_path"} and isinstance(item, str):
                paths.append(item)
            else:
                paths.extend(collect_paths_from_mapping(item))
    elif isinstance(value, list):
        for item in value:
            paths.extend(collect_paths_from_mapping(item))
    return paths


def collect_paths_from_text(text: str) -> list[str]:
    paths = [match.group("path").rstrip(".,;") for match in TEXT_PATH_RE.finditer(text)]
    stripped = text.strip()
    if stripped.startswith("{") or stripped.startswith("["):
        try:
            parsed = json.loads(stripped)
        except json.JSONDecodeError:
            parsed = None
        if parsed is not None:
            paths.extend(collect_paths_from_mapping(parsed))
    return paths


def unique_existing_paths(paths: Iterable[str], mappings: Iterable[tuple[str, str]] = ()) -> list[str]:
    seen: set[str] = set()
    result: list[str] = []
    for raw_path in paths:
        path = existing_image_path(raw_path, mappings)
        if not path:
            continue
        key = os.path.realpath(path)
        if key in seen:
            continue
        seen.add(key)
        result.append(path)
    return result


def strip_paths_from_text(text: str, sent_paths: Iterable[str]) -> str:
    cleaned = text
    sent = [path for path in sent_paths if path]
    for path in sent:
        variants = {
            path,
            os.path.realpath(path),
            str(Path(path)),
            Path(path).name,
        }
        for variant in sorted(variants, key=len, reverse=True):
            if variant:
                cleaned = cleaned.replace(variant, "")
    cleaned = TEXT_PATH_RE.sub("", cleaned)
    lines = []
    for line in cleaned.splitlines():
        compact = line.strip()
        if not compact:
            continue
        if re.fullmatch(r"(图片|路径|imagePath|image_path|path|文件|file)\s*[:：]?\s*", compact, re.IGNORECASE):
            continue
        lines.append(line.rstrip())
    return "\n".join(lines).strip()
