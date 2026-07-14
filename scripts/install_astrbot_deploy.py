#!/usr/bin/env python3
"""Install this repository into the standard AstrBot data layout.

The script keeps all per-instance data under one AstrBot data directory and
updates only the known maimai MCP entries in mcp_server.json.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import os
import shutil
from datetime import datetime
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]

STANDARD_DIRS = (
    "maimai-mcp",
    "maimai-config",
    "plugins",
    "maimai-yuzu-static/Resource/static",
    "maimai-images",
    "maimai-covers",
    "player-cache",
    "group-b50-cache",
    "group-song-cache",
    "qq-identity-cache",
    "b50-images",
)

EXCLUDED_NAMES = {
    ".DS_Store",
    ".claude",
    ".git",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".venv",
    "__pycache__",
    "b50-images",
    "cover_cache",
    "group-b50-cache",
    "group-song-cache",
    "logs",
    "maimai-covers",
    "maimai-images",
    "node_modules",
    "player-cache",
    "qq-identity-cache",
}

EXCLUDED_PATTERNS = (
    "._*",
)

EXCLUDED_FILES = {
    ".diving-fish-mcp-secrets.json",
    ".env",
    ".env.local",
    "b50-image-style.json",
}

OLD_CODE_DIRS = (
    "diving-fish-b50-mcp",
    "maimai-local-search",
)

MAIMAI_SERVER_NAMES = (
    "maimai-local-search",
    "diving-fish-b50",
    "group-b50",
    "qq-identity",
    "maimaidx-render",
    "maimai-score-query",
    "b50-image",
)

AUTO_SEND_IMAGES_PLUGIN_NAME = "astrbot_plugin_maimai_auto_send_images"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Install this repository into an AstrBot data directory."
    )
    parser.add_argument(
        "--data-dir",
        default="/opt/qqbot/data",
        help="AstrBot host data directory. Default: /opt/qqbot/data",
    )
    parser.add_argument(
        "--container-data-dir",
        default="/AstrBot/data",
        help="The same data directory as seen inside the AstrBot container. Default: /AstrBot/data",
    )
    parser.add_argument(
        "--project-dir",
        default=str(REPO_ROOT),
        help="Project directory to copy. Default: repository root.",
    )
    parser.add_argument(
        "--napcat-base-url",
        default="http://napcat:3000",
        help="NapCat OneBot HTTP URL visible from AstrBot. Default: http://napcat:3000",
    )
    parser.add_argument(
        "--skip-static",
        action="store_true",
        help="Do not copy maimaidx_render_mcp/static into maimai-yuzu-static.",
    )
    parser.add_argument(
        "--no-write-config",
        action="store_true",
        help="Create directories and copy files but do not update mcp_server.json.",
    )
    parser.add_argument(
        "--enable-b50-image",
        action="store_true",
        help="Deprecated: b50-image is enabled by default. Kept for old deployment commands.",
    )
    parser.add_argument(
        "--disable-b50-image",
        action="store_true",
        help="Disable the legacy b50-image MCP entry.",
    )
    parser.add_argument(
        "--archive-old",
        action="store_true",
        help="Move old separate code directories into _archive after syncing maimai-mcp.",
    )
    parser.add_argument(
        "--disable-auto-send-images-plugin",
        action="store_true",
        help="Do not copy the AstrBot plugin that auto-sends maimai MCP image results.",
    )
    return parser.parse_args()


def container_path(base: str, *parts: str) -> str:
    normalized = base.rstrip("/")
    suffix = "/".join(part.strip("/") for part in parts if part)
    return f"{normalized}/{suffix}" if suffix else normalized


def now_stamp() -> str:
    return datetime.now().strftime("%Y%m%d-%H%M%S")


def ensure_dirs(data_dir: Path) -> None:
    for rel in STANDARD_DIRS:
        (data_dir / rel).mkdir(parents=True, exist_ok=True)


def copy_project(project_dir: Path, target_dir: Path) -> None:
    project_dir = project_dir.resolve()
    target_dir.mkdir(parents=True, exist_ok=True)

    def ignore(directory: str, names: list[str]) -> set[str]:
        ignored: set[str] = set()
        dir_path = Path(directory)
        for name in names:
            path = dir_path / name
            if (
                name in EXCLUDED_NAMES
                or name in EXCLUDED_FILES
                or name.endswith((".pyc", ".pyo"))
                or any(fnmatch.fnmatch(name, pattern) for pattern in EXCLUDED_PATTERNS)
            ):
                ignored.add(name)
                continue
            try:
                rel = path.resolve().relative_to(project_dir).as_posix()
            except ValueError:
                rel = ""
            if rel == "maimaidx_render_mcp/static":
                ignored.add(name)
            if rel == "data/custom_aliases.json":
                ignored.add(name)
        return ignored

    shutil.copytree(project_dir, target_dir, dirs_exist_ok=True, ignore=ignore)


def copy_static(project_dir: Path, data_dir: Path, skip_static: bool) -> bool:
    if skip_static:
        return False
    source = project_dir / "maimaidx_render_mcp" / "static"
    target = data_dir / "maimai-yuzu-static" / "Resource" / "static"
    if not source.exists():
        return False
    shutil.copytree(source, target, dirs_exist_ok=True, ignore=shutil.ignore_patterns("__pycache__", "*.pyc", ".DS_Store", "._*"))
    return True


def copy_auto_send_images_plugin(project_dir: Path, data_dir: Path, enabled: bool) -> bool:
    if not enabled:
        return False
    source = project_dir / "deploy" / "astrbot" / "plugins" / AUTO_SEND_IMAGES_PLUGIN_NAME
    target = data_dir / "plugins" / AUTO_SEND_IMAGES_PLUGIN_NAME
    if not source.exists():
        return False
    shutil.copytree(
        source,
        target,
        dirs_exist_ok=True,
        ignore=shutil.ignore_patterns("__pycache__", "*.pyc", ".DS_Store", "._*"),
    )
    return True


def copy_first_existing(candidates: list[Path], target: Path, *, mode: int | None = None) -> bool:
    if target.exists():
        return False
    for candidate in candidates:
        if candidate.exists() and candidate.is_file():
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(candidate, target)
            if mode is not None:
                os.chmod(target, mode)
            return True
    return False


def preserve_config(project_dir: Path, data_dir: Path) -> dict[str, bool]:
    config_dir = data_dir / "maimai-config"
    code_dir = data_dir / "maimai-mcp"

    token_copied = copy_first_existing(
        [
            data_dir / "maimai-config" / ".diving-fish-mcp-secrets.json",
            data_dir / "maimai-mcp" / ".diving-fish-mcp-secrets.json",
            data_dir / "diving-fish-b50-mcp" / ".diving-fish-mcp-secrets.json",
            project_dir / ".diving-fish-mcp-secrets.json",
        ],
        config_dir / ".diving-fish-mcp-secrets.json",
        mode=0o600,
    )
    aliases_copied = copy_first_existing(
        [
            data_dir / "maimai-config" / "custom_aliases.json",
            data_dir / "maimai-mcp" / "data" / "custom_aliases.json",
            data_dir / "maimai-local-search" / "data" / "custom_aliases.json",
            project_dir / "data" / "custom_aliases.json",
        ],
        config_dir / "custom_aliases.json",
    )
    style_copied = copy_first_existing(
        [
            data_dir / "b50-image-style.json",
            data_dir / "maimai-mcp" / "b50-image-style.json",
            data_dir / "diving-fish-b50-mcp" / "b50-image-style.json",
            project_dir / "b50-image-style.json",
        ],
        data_dir / "b50-image-style.json",
    )

    alias_target = config_dir / "custom_aliases.json"
    alias_link = code_dir / "data" / "custom_aliases.json"
    if alias_target.exists():
        alias_link.parent.mkdir(parents=True, exist_ok=True)
        if alias_link.is_symlink() or alias_link.exists():
            alias_link.unlink()
        try:
            alias_link.symlink_to(Path("../../maimai-config/custom_aliases.json"))
        except OSError:
            shutil.copy2(alias_target, alias_link)

    return {
        "token_copied": token_copied,
        "aliases_copied": aliases_copied,
        "style_copied": style_copied,
        "alias_linked": alias_link.exists() or alias_link.is_symlink(),
    }


def common_env(cdata: str) -> dict[str, str]:
    return {
        "DIVING_FISH_MCP_TOKEN_FILE": container_path(cdata, "maimai-config/.diving-fish-mcp-secrets.json"),
        "PLAYER_CACHE_DIR": container_path(cdata, "player-cache"),
        "QQ_IDENTITY_CACHE_DIR": container_path(cdata, "qq-identity-cache"),
    }


def local_search_env(cdata: str) -> dict[str, str]:
    return {
        "MAIMAI_LOCAL_SEARCH_MCP_ARGS": "[\"-m\", \"maimai_mcp.server\"]",
        "MAIMAI_LOCAL_SEARCH_MCP_CWD": container_path(cdata, "maimai-mcp"),
    }


def b50_child_env(cdata: str) -> dict[str, str]:
    return {
        "DIVING_FISH_B50_MCP_ARGS": "[\"-m\", \"diving_fish_b50_mcp.server\"]",
        "DIVING_FISH_B50_MCP_CWD": container_path(cdata, "maimai-mcp"),
    }


def server_entry(module: str, cwd: str, *, active: bool = True, env: dict[str, str] | None = None) -> dict[str, Any]:
    entry: dict[str, Any] = {
        "active": active,
        "command": "python",
        "args": ["-m", module],
        "cwd": cwd,
    }
    if env:
        entry["env"] = dict(sorted(env.items()))
    return entry


def build_mcp_servers(cdata: str, napcat_base_url: str, *, enable_b50_image: bool) -> dict[str, Any]:
    code_dir = container_path(cdata, "maimai-mcp")
    base = common_env(cdata)
    local = local_search_env(cdata)
    child_b50 = b50_child_env(cdata)
    napcat_env = {"NAPCAT_BASE_URL": napcat_base_url}

    return {
        "maimai-local-search": server_entry("maimai_mcp.server", code_dir),
        "diving-fish-b50": server_entry(
            "diving_fish_b50_mcp.server",
            code_dir,
            env={**base, **local},
        ),
        "group-b50": server_entry(
            "group_b50_mcp.server",
            code_dir,
            env={
                **base,
                **local,
                **child_b50,
                **napcat_env,
                "GROUP_B50_CACHE_DIR": container_path(cdata, "group-b50-cache"),
                "GROUP_SONG_CACHE_DIR": container_path(cdata, "group-song-cache"),
            },
        ),
        "qq-identity": server_entry(
            "qq_identity_mcp.server",
            code_dir,
            env={
                **napcat_env,
                "QQ_IDENTITY_CACHE_DIR": container_path(cdata, "qq-identity-cache"),
            },
        ),
        "maimaidx-render": server_entry(
            "maimaidx_render_mcp.server",
            code_dir,
            env={
                **base,
                **local,
                "MAIMAIDX_STATIC_DIR": container_path(cdata, "maimai-yuzu-static/Resource/static"),
                "MAIMAIDX_RENDER_OUTPUT_DIR": container_path(cdata, "maimai-images"),
                "MAIMAIDX_COVER_CACHE_DIR": container_path(cdata, "maimai-covers"),
            },
        ),
        "maimai-score-query": server_entry(
            "maimai_score_mcp.server",
            code_dir,
            env={**base, **local, **child_b50},
        ),
        "b50-image": server_entry(
            "b50_image_mcp.server",
            code_dir,
            active=enable_b50_image,
            env={
                **base,
                "B50_IMAGE_YUZU_STATIC_DIR": container_path(cdata, "maimai-yuzu-static/Resource/static"),
                "B50_IMAGE_MAIBOT_STATIC_DIR": container_path(cdata, "maimai-static"),
                "B50_IMAGE_LEGACY_STATIC_DIR": container_path(cdata, "maimai-static"),
                "B50_IMAGE_OUTPUT_DIR": container_path(cdata, "b50-images"),
                "B50_IMAGE_COVER_CACHE_DIR": container_path(cdata, "maimai-covers"),
                "B50_IMAGE_STYLE_CONFIG": container_path(cdata, "b50-image-style.json"),
            },
        ),
    }


def read_json(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    with path.open("r", encoding="utf-8") as fh:
        parsed = json.load(fh)
    return parsed if isinstance(parsed, dict) else {}


def backup_file(path: Path, archive_dir: Path) -> Path | None:
    if not path.exists():
        return None
    archive_dir.mkdir(parents=True, exist_ok=True)
    target = archive_dir / path.name
    shutil.copy2(path, target)
    return target


def write_mcp_config(data_dir: Path, servers: dict[str, Any]) -> Path:
    config_path = data_dir / "mcp_server.json"
    archive_dir = data_dir / "_archive" / f"astrbot-deploy-{now_stamp()}" / "configs"
    backup_file(config_path, archive_dir)

    config = read_json(config_path)
    mcp_servers = config.get("mcpServers")
    if not isinstance(mcp_servers, dict):
        mcp_servers = {}
    for name in MAIMAI_SERVER_NAMES:
        if name in servers:
            mcp_servers[name] = servers[name]
    config["mcpServers"] = mcp_servers
    config_path.write_text(json.dumps(config, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return config_path


def archive_old_dirs(data_dir: Path) -> list[Path]:
    archive_dir = data_dir / "_archive" / f"astrbot-deploy-{now_stamp()}" / "workspaces"
    moved: list[Path] = []
    for rel in OLD_CODE_DIRS:
        source = data_dir / rel
        if not source.exists():
            continue
        archive_dir.mkdir(parents=True, exist_ok=True)
        target = archive_dir / rel
        if target.exists():
            target = archive_dir / f"{rel}-{now_stamp()}"
        shutil.move(str(source), str(target))
        moved.append(target)
    return moved


def main() -> int:
    args = parse_args()
    data_dir = Path(args.data_dir).expanduser().resolve()
    project_dir = Path(args.project_dir).expanduser().resolve()
    code_dir = data_dir / "maimai-mcp"

    if not project_dir.exists():
        raise SystemExit(f"Project directory does not exist: {project_dir}")

    ensure_dirs(data_dir)
    copy_project(project_dir, code_dir)
    static_copied = copy_static(project_dir, data_dir, args.skip_static)
    plugin_copied = copy_auto_send_images_plugin(
        project_dir,
        data_dir,
        not args.disable_auto_send_images_plugin,
    )
    preserved = preserve_config(project_dir, data_dir)

    config_path: Path | None = None
    if not args.no_write_config:
        servers = build_mcp_servers(
            args.container_data_dir,
            args.napcat_base_url,
            enable_b50_image=not args.disable_b50_image,
        )
        config_path = write_mcp_config(data_dir, servers)

    archived = archive_old_dirs(data_dir) if args.archive_old else []

    print(f"data_dir: {data_dir}")
    print(f"code_dir: {code_dir}")
    print(f"static_copied: {static_copied}")
    print(f"auto_send_images_plugin_copied: {plugin_copied}")
    print(f"config_updated: {config_path if config_path else False}")
    print(f"token_preserved: {preserved['token_copied'] or (data_dir / 'maimai-config' / '.diving-fish-mcp-secrets.json').exists()}")
    print(f"aliases_linked: {preserved['alias_linked']}")
    if archived:
        print("archived_old_dirs:")
        for path in archived:
            print(f"  {path}")
    print("restart AstrBot after install: docker restart astrbot")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
