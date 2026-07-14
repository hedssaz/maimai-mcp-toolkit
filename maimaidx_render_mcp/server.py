"""maimaidx_render_mcp — maimaiDX 绘图渲染 MCP

基于 maimaiDX (Yuri-YuzuChaN/maimaiDX) 的绘图代码 + 本地 MCP 数据源。
yuzu 风格走 maimaiDX 原版绘图，maibot/legacy 走 b50_extra。
"""

from __future__ import annotations

import asyncio
import copy
import json
import os
import re
import sys
import threading
import traceback
from pathlib import Path
from typing import Any

from PIL import Image

from maimaidx_render_mcp.maimaidx.maimai_best_50 import (
    DrawBest,
    ScoreBaseImage,
    generate as b50_generate,
    image_to_base64 as mdx_image_to_base64,
)
from maimaidx_render_mcp.maimaidx.maimaidx_model import (
    BasicInfo,
    ChartInfo,
    Music,
    PlayInfoDefault,
    PlayInfoDev,
    RaMusic,
    UserInfo,
    UserInfoDev,
    Data,
)
from maimaidx_render_mcp.maimaidx.maimaidx_music_info import (
    draw_music_info as _draw_music_info,
    draw_music_play_data as _draw_music_play_data,
    draw_plate_table as _draw_plate_table,
    draw_rating_table as _draw_rating_table,
    draw_rating as _draw_rating,
)
from maimaidx_render_mcp.maimaidx.image import music_picture
from maimaidx_render_mcp.maimaidx.maimaidx_player_score import (
    DrawScore,
    RISE_SCORE_ALGORITHM_DEFAULT,
    level_process_data as _level_process_data,
    player_plate_data as _player_plate_data,
    level_achievement_list_data as _level_achievement_list_data,
    music_global_data as _music_global_data,
    plate_message,
    rating_ranking_data as _rating_ranking_data,
    rise_score_data as _rise_score_data,
)
from maimaidx_render_mcp.maimaidx import (
    mai,
    maiApi,
    version_map,
    plate_to_dx_version,
    platecn,
    levelList,
    maimaidir,
    coverdir,
    TBFONT,
    SIYUAN,
    normalize_level_value,
)
from maimaidx_render_mcp.maimaidx.maimaidx_music import MusicList
from maimaidx_render_mcp.shim.mai_music import (
    custom_plate_exists,
    music_from_search_song,
    render_id_from_search_song,
)
from maimaidx_render_mcp.output import image_output_context, image_path_payload, next_image_path

# ============================================================
# MCP Server
# ============================================================

SERVER_NAME = "maimaidx-render-mcp"
SERVER_VERSION = "0.1.0"


def _call_tool(tool_name: str, arguments: dict[str, Any]) -> dict[str, Any]:
    with image_output_context(_output_stem(tool_name, arguments)):
        return _dispatch_tool(tool_name, arguments)


def _output_stem(tool_name: str, arguments: dict[str, Any]) -> str:
    for key in ("qq", "targetQQ", "target_qq", "userId", "user_id", "username", "groupId", "group_id"):
        value = arguments.get(key)
        if value not in (None, ""):
            return str(value)
    return tool_name.removeprefix("render_maimai_")


def _dispatch_tool(tool_name: str, arguments: dict[str, Any]) -> dict[str, Any]:
    """MCP 工具分发"""
    if tool_name == "render_maimai_b50":
        return _render_b50(arguments)
    if tool_name == "render_maimai_plate":
        return _render_plate(arguments)
    if tool_name == "render_maimai_plate_batch":
        return _render_plate_batch(arguments)
    if tool_name == "render_maimai_rating":
        return _render_rating(arguments)
    if tool_name == "render_maimai_progress":
        return _render_progress(arguments)
    if tool_name == "render_maimai_music_info":
        return _render_music_info(arguments)
    if tool_name == "render_maimai_music_info_batch":
        return _render_music_info_batch(arguments)
    if tool_name == "render_maimai_music_score":
        return _render_music_score(arguments)
    if tool_name == "render_maimai_music_global_stats":
        return _render_music_global_stats(arguments)
    if tool_name == "render_maimai_rise_score":
        return _render_rise_score(arguments)
    if tool_name == "render_maimai_score_list":
        return _render_score_list(arguments)
    if tool_name == "render_maimai_rating_ranking":
        return _render_rating_ranking(arguments)
    if tool_name == "render_maimai_plate_progress":
        return _render_plate_progress(arguments)
    if tool_name == "render_maimai_plate_progress_batch":
        return _render_plate_progress_batch(arguments)
    raise ValueError(f"Unknown tool: {tool_name}")


def _err(message: str, code: str = "ERROR") -> dict[str, Any]:
    return {"content": [{"type": "text", "text": message}], "isError": True}


def _jp_unsupported_message() -> str:
    return "当前分支不支持日服/dxdata 曲目数据。"


def _normalize_server(value: Any) -> str:
    text = str(value or "cn").strip().lower()
    if text in {"jp", "japan", "日本", "日服"}:
        return "jp"
    if text in {"custom", "自定义", "自定義"}:
        return "custom"
    return "cn"


def _cn_plate_data_key(version: str) -> str | None:
    if version in version_map:
        return version_map[version][1]
    if version in plate_to_dx_version:
        return version
    return None


def _cn_plate_supported(version: str) -> bool:
    data_key = _cn_plate_data_key(version)
    if not data_key:
        return False
    try:
        mai._ensure_loaded()
    except Exception:
        return False
    return data_key in getattr(mai, "total_plate_id_list", {})


def _normalize_plate_version_and_server(version: Any, server: str) -> tuple[str, str]:
    text = str(version or "真").strip()
    if text in platecn:
        text = platecn[text]
    if server == "jp":
        return text, "jp"
    if server == "custom":
        return text, "custom"
    if _cn_plate_supported(text):
        return text, "cn"
    if custom_plate_exists(text):
        return text, "custom"
    return text, server


def _save_and_return(image: Any, prefix: str = "maimai", output_dir: str | None = None) -> dict[str, Any]:
    """保存 PIL Image 到文件，返回路径"""
    path = next_image_path(prefix, output_dir)
    compress_level = int(os.environ.get("MAIMAIDX_RENDER_PNG_COMPRESS_LEVEL", "1") or "1")
    optimize = os.environ.get("MAIMAIDX_RENDER_PNG_OPTIMIZE", "").strip().lower() in {"1", "true", "yes", "on"}
    image.save(str(path), format="PNG", optimize=optimize, compress_level=max(0, min(9, compress_level)))
    return _image_path_result(path, image=image)


def _image_path_result(path: Path, image: Any | None = None) -> dict[str, Any]:
    payload = image_path_payload(path, image=image)
    return {"content": [{"type": "text", "text": json.dumps(payload, ensure_ascii=False)}]}


# ============================================================
# Tool: render_maimai_b50
# ============================================================

def _b50_fit_index_available(b50_data: Any) -> bool:
    fit_index = b50_data.get("fitIndex") if isinstance(b50_data, dict) else None
    return isinstance(fit_index, dict) and bool(fit_index.get("available"))


def _enrich_b50_cache_from_rendered_b50(qq: str, b50_data: dict[str, Any], *, timeout_ms: int) -> bool:
    """Enrich an already-fetched B50 with local chart metadata and write player_cache.

    This intentionally reuses the B50 payload fetched for rendering. It does not
    call Diving-Fish again; only local maimai metadata is attached.
    """
    if not qq or not isinstance(b50_data, dict):
        return False
    enriched = copy.deepcopy(b50_data)
    try:
        from diving_fish_b50_mcp.server import enrich_b50_with_maimai_local_search
        from player_cache import write_player_b50

        enrich_b50_with_maimai_local_search(enriched, timeout_ms=timeout_ms)
        if not _b50_fit_index_available(enriched):
            return False
        write_player_b50(str(qq), enriched)
        return True
    except Exception:
        traceback.print_exc(file=sys.stderr)
        return False


def _schedule_b50_cache_enrichment(qq: Any, b50_data: dict[str, Any] | None, *, timeout_ms: int) -> None:
    if not qq or not isinstance(b50_data, dict):
        return
    if _b50_fit_index_available(b50_data):
        return
    enabled = os.environ.get("MAIMAIDX_RENDER_B50_POST_CACHE_ENRICH", "1").strip().lower()
    if enabled in {"0", "false", "no", "off"}:
        return

    thread = threading.Thread(
        target=_enrich_b50_cache_from_rendered_b50,
        kwargs={"qq": str(qq), "b50_data": b50_data, "timeout_ms": timeout_ms},
        name=f"maimai-b50-cache-enrich-{qq}",
        daemon=True,
    )
    thread.start()


def _render_b50(args: dict[str, Any]) -> dict[str, Any]:
    qq = args.get("qq")
    username = args.get("username")
    style = args.get("style", "yuzu")
    compute_from_records = args.get("computeFromRecords") is True or str(args.get("source") or "").casefold() in {
        "records",
        "computed",
        "computed_b50",
        "fit",
        "fitted",
    }

    if not qq and not username:
        return _err("需要提供 qq 或 username")

    b50_data: dict[str, Any] | None = None
    timeout_ms = int(args.get("timeoutMs", 30000))

    try:
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)

        from diving_fish_b50_mcp.server import query_b50, query_computed_b50

        b50_args = {"includeChartMetadata": False, "timeoutMs": timeout_ms}
        if qq:
            b50_args["qq"] = str(qq)
        if username:
            b50_args["username"] = username

        b50_data = query_computed_b50(b50_args) if compute_from_records else query_b50(b50_args)
        if compute_from_records:
            b50_data = _b50_with_computed_display_rating(b50_data)
            rating_breakdown = b50_data.get("ratingBreakdown") if isinstance(b50_data.get("ratingBreakdown"), dict) else {}
            computed_rating = rating_breakdown.get("total")
            rating_override = int(computed_rating) if isinstance(computed_rating, (int, float)) else None
            userinfo: UserInfo = maiApi._user_info_from_b50_result(  # noqa: SLF001 - shim helper for render data reuse
                b50_data,
                rating_override=rating_override,
            )
        else:
            userinfo = maiApi._user_info_from_b50_result(b50_data)  # noqa: SLF001 - keep raw B50 for post-render cache enrich

        if not userinfo.nickname:
            return _err("未找到玩家数据")

        if style == "yuzu":
            draw_best = DrawBest(userinfo, qqid=int(qq) if qq else None)
            image = loop.run_until_complete(draw_best.draw())
        elif style in ("maibot", "legacy"):
            from b50_image_mcp.server import (
                render_b50_png,
            )

            static_root = Path(args.get("staticDir") or os.environ.get("MAIMAIDX_STATIC_DIR", ""))
            if not static_root or str(static_root) == ".":
                static_root = Path.cwd()
            cover_cache = Path(args.get("coverCacheDir") or os.environ.get("MAIMAIDX_COVER_CACHE_DIR", str(Path.cwd() / "cover_cache")))
            title = args.get("title")

            image, _ = render_b50_png(
                b50_data,
                static_root=static_root,
                cover_cache_dir=cover_cache,
                timeout_ms=timeout_ms,
                title=title,
                style=style,
            )
        else:
            return _err(f"未知风格: {style}，可选 yuzu/maibot/legacy")

        loop.close()
        response = _save_and_return(image, f"b50_{style}")
        if not compute_from_records and qq:
            _schedule_b50_cache_enrichment(qq, b50_data, timeout_ms=timeout_ms)
        return response
    except Exception as e:
        traceback.print_exc()
        return _err(f"渲染 B50 失败: {e}")


def _b50_with_computed_display_rating(b50_data: dict[str, Any]) -> dict[str, Any]:
    rating = (b50_data.get("ratingBreakdown") or {}).get("total") if isinstance(b50_data.get("ratingBreakdown"), dict) else None
    if not isinstance(rating, (int, float)):
        return b50_data
    player = b50_data.get("player") if isinstance(b50_data.get("player"), dict) else {}
    return {
        **b50_data,
        "player": {
            **player,
            "rating": int(rating),
        },
    }


# ============================================================
# Tool: render_maimai_plate — 牌子完成表
# ============================================================

def _render_plate(args: dict[str, Any]) -> dict[str, Any]:
    qq = args.get("qq")
    username = args.get("username")
    if not qq and not username:
        return _err("需要提供 qq 或 username")

    version = args.get("version", "真")
    plan = args.get("plan", "极")
    server = _normalize_server(args.get("server", "cn"))
    version, server = _normalize_plate_version_and_server(version, server)
    if server == "jp":
        return _err(_jp_unsupported_message())

    try:
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        qqid = int(qq) if qq else None
        result = loop.run_until_complete(
            _draw_plate_table(qqid, version, plan, server=server, username=username)
        )
        loop.close()

        if isinstance(result, str) and not result.startswith("base64"):
            return _err(result)
        return _ok_image(result, prefix=f"plate_{server}_{version}_{plan}")
    except Exception as e:
        traceback.print_exc()
        return _err(f"渲染牌子表失败: {e}")


async def _render_plate_batch_async(args: dict[str, Any]) -> dict[str, Any]:
    qq = args.get("qq")
    username = args.get("username")
    if not qq and not username:
        return _err("需要提供 qq 或 username")

    items = _plate_batch_items(args)
    if not items:
        return _err("需要提供 items 或 versions")

    qqid = int(qq) if qq else None
    records = await maiApi.query_user_plate(qqid=qqid, username=username)
    results: list[dict[str, Any]] = []
    errors: list[dict[str, str]] = []
    for index, item in enumerate(items, 1):
        raw_version = item.get("version", item.get("plate", "真"))
        plan = item.get("plan", args.get("plan", "极"))
        server = _normalize_server(item.get("server", args.get("server", "cn")))
        version, server = _normalize_plate_version_and_server(raw_version, server)
        label = f"{version}{plan}"
        if server == "jp":
            errors.append({"index": str(index), "label": label, "message": _jp_unsupported_message()})
            continue
        try:
            image = await _draw_plate_table(
                qqid or 0,
                version,
                plan,
                server=server,
                records=records,
            )
            payload = _ok_image_payload(image, prefix=f"plate_{server}_{version}_{plan}")
            payload.update({"index": index, "version": version, "plan": plan, "server": server, "label": label})
            results.append(payload)
        except Exception as exc:
            errors.append({"index": str(index), "label": label, "message": str(exc)})
    return _batch_result(results, errors)


def _render_plate_batch(args: dict[str, Any]) -> dict[str, Any]:
    try:
        return _run_async(_render_plate_batch_async(args))
    except Exception as e:
        traceback.print_exc()
        return _err(f"批量渲染牌子表失败: {e}")


def _ok_image(image_or_b64, prefix="maimai") -> dict[str, Any]:
    """处理 PIL Image 或 base64 字符串，保存为文件"""
    if isinstance(image_or_b64, str):
        # 上游 maimaiDX 绘图函数用 str 同时表示 base64 图片和错误文本。
        # 只有可解码的 base64 才按图片保存，其它字符串作为工具错误返回。
        import base64
        b64_data = image_or_b64
        if b64_data.startswith("base64://"):
            b64_data = b64_data[9:]
        if not b64_data:
            return _err("渲染未生成图片，可能缺少绘图依赖")
        try:
            payload = base64.b64decode(b64_data, validate=True)
        except Exception:
            return _err(image_or_b64 or "渲染未生成图片")
        path = next_image_path(prefix)
        path.write_bytes(payload)
        return _image_path_result(path)
    return _save_and_return(image_or_b64, prefix)


def _ok_image_payload(image_or_b64, prefix: str) -> dict[str, Any]:
    result = _ok_image(image_or_b64, prefix=prefix)
    if result.get("isError"):
        raise ValueError(result["content"][0]["text"])
    return json.loads(result["content"][0]["text"])


def _batch_result(results: list[dict[str, Any]], errors: list[dict[str, str]]) -> dict[str, Any]:
    payload: dict[str, Any] = {"results": results}
    images = [item for item in results if item.get("imagePath")]
    if images:
        payload["images"] = images
    if errors:
        payload["errors"] = errors
    return {
        "content": [{"type": "text", "text": json.dumps(payload, ensure_ascii=False)}],
        "isError": bool(errors and not results),
    }


def _value_list(value: Any) -> list[Any]:
    if value in (None, ""):
        return []
    if isinstance(value, list):
        return [item for item in value if item not in (None, "")]
    return [value]


def _plate_batch_items(args: dict[str, Any]) -> list[dict[str, Any]]:
    raw_items = args.get("items")
    if isinstance(raw_items, list) and raw_items:
        items: list[dict[str, Any]] = []
        for item in raw_items:
            if isinstance(item, dict):
                items.append(dict(item))
            elif item not in (None, ""):
                items.append({"version": item})
        return items

    versions = _value_list(args.get("versions", args.get("version")))
    plans = _value_list(args.get("plans", args.get("plan")))
    if not plans:
        plans = ["极"]
    items = []
    for index, version in enumerate(versions):
        plan = plans[index] if len(plans) == len(versions) else plans[0]
        items.append({"version": version, "plan": plan, "server": args.get("server", "cn")})
    return items


def _music_info_batch_items(args: dict[str, Any]) -> list[dict[str, Any]]:
    raw_items = args.get("items")
    if isinstance(raw_items, list) and raw_items:
        items: list[dict[str, Any]] = []
        for item in raw_items:
            if isinstance(item, dict):
                items.append(dict(item))
            elif item not in (None, ""):
                items.append({"query": item})
        return items

    queries = _value_list(args.get("queries", args.get("query")))
    return [{"query": query} for query in queries]


def _run_async(coro):
    loop = asyncio.new_event_loop()
    try:
        asyncio.set_event_loop(loop)
        return loop.run_until_complete(coro)
    finally:
        loop.close()
        asyncio.set_event_loop(None)


# ============================================================
# Tool: render_maimai_rating — 定数完成表
# ============================================================

def _render_rating(args: dict[str, Any]) -> dict[str, Any]:
    qq = args.get("qq")
    username = args.get("username")
    if not qq and not username:
        return _err("需要提供 qq 或 username")

    rating = normalize_level_value(args.get("rating", "14"))
    isfc = args.get("isfc", False)

    try:
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        qqid = int(qq) if qq else None
        result = loop.run_until_complete(_draw_rating_table(qqid, rating, isfc, username=username))
        loop.close()
        return _ok_image(result)
    except Exception as e:
        traceback.print_exc()
        return _err(f"渲染定数表失败: {e}")


# ============================================================
# Tool: render_maimai_progress — 等级进度图
# ============================================================

def _render_progress(args: dict[str, Any]) -> dict[str, Any]:
    qq = args.get("qq")
    username = args.get("username")
    level = normalize_level_value(args.get("level", "14"))
    plan = args.get("plan", "sss")
    category = args.get("category", "default")
    page = args.get("page", 1)
    server = _normalize_server(args.get("server", "cn"))

    if not qq and not username:
        return _err("需要提供 qq 或 username")
    if server == "jp":
        return _err(_jp_unsupported_message())

    try:
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        qqid = int(qq) if qq else None
        result = loop.run_until_complete(
            _level_process_data(qqid, username, level, plan, category, page, server)
        )
        loop.close()
        return _ok_image(result, prefix=f"progress_{server}_{level}_{plan}")
    except Exception as e:
        traceback.print_exc()
        return _err(f"渲染进度图失败: {e}")


# ============================================================
# Tool: render_maimai_music_info — 曲目信息图
# ============================================================

def _music_info_query(args: dict[str, Any]) -> str | None:
    for key in ("query", "songQuery", "song_query", "title"):
        value = args.get(key)
        if value not in (None, ""):
            return str(value)
    return None


def _normalize_song_type_arg(value: Any) -> str | None:
    if value in (None, ""):
        return None
    text = str(value).strip().lower().replace(" ", "")
    if text in {"dx", "でらっくす"}:
        return "dx"
    if text in {"sd", "st", "std", "standard", "标准", "标", "标准谱面"}:
        return "standard"
    return str(value)


def _song_type_arg(args: dict[str, Any]) -> str | None:
    for key in ("songType", "song_type", "chartType", "chart_type", "type"):
        normalized = _normalize_song_type_arg(args.get(key))
        if normalized:
            return normalized
    return None


def _infer_song_type_from_query(query: str | None) -> str | None:
    if not query:
        return None
    try:
        from maimai_mcp.search import normalize_text

        text = normalize_text(query).replace(" ", "")
    except Exception:
        text = str(query).strip().lower().replace(" ", "")
    if text.startswith("dx") or text.endswith("dx"):
        return "dx"
    for marker in ("st", "sd", "std", "standard", "标准"):
        if text.startswith(marker) or text.endswith(marker):
            return "standard"
    return None


def _infer_song_type_from_music_id(music_id: Any) -> str | None:
    if music_id in (None, ""):
        return None
    try:
        numeric_id = int(str(music_id))
    except (TypeError, ValueError):
        return None
    # Diving-Fish uses +10000 ids for DX charts. Treat an explicit offset id as a
    # chart-type selection rather than a request for both ST and DX variants.
    if numeric_id > 10000:
        return "dx"
    return None


def _is_exact_song_query(song: dict[str, Any], query: str) -> bool:
    try:
        from maimai_mcp.search import normalize_text
    except Exception:
        normalize_text = lambda value: str(value or "").strip().casefold()

    needle = normalize_text(query)
    values: list[Any] = [
        song.get("id"),
        song.get("source_id"),
        song.get("title"),
    ]
    values.extend(song.get("aliases") or [])
    source_ids = song.get("source_ids") if isinstance(song.get("source_ids"), dict) else {}
    values.extend(source_ids.values())
    values.extend(_search_song_id_values(song))
    return any(normalize_text(value) == needle for value in values if value not in (None, ""))


def _is_exact_song_identity_query(song: dict[str, Any], query: str) -> bool:
    try:
        from maimai_mcp.search import normalize_text
    except Exception:
        normalize_text = lambda value: str(value or "").strip().casefold()

    needle = normalize_text(query)
    values: list[Any] = [
        song.get("id"),
        song.get("source_id"),
        song.get("title"),
    ]
    source_ids = song.get("source_ids") if isinstance(song.get("source_ids"), dict) else {}
    values.extend(source_ids.values())
    values.extend(_search_song_id_values(song))
    return any(normalize_text(value) == needle for value in values if value not in (None, ""))


def _search_exact_music_info_song(query: str) -> dict[str, Any] | None:
    from maimai_mcp.search import search_songs

    result = search_songs(query=query, limit=None)
    exact = [
        song
        for song in (result.get("songs") or [])
        if _is_exact_song_query(song, query)
    ]
    return exact[0] if len(exact) == 1 else None


def _search_song_chart_types(song: dict[str, Any]) -> set[str]:
    raw_types = list(song.get("available_chart_types") or [])
    raw_types.extend(
        chart.get("chart_type")
        for chart in (song.get("matched_charts") or [])
        if isinstance(chart, dict)
    )
    types: set[str] = set()
    for value in raw_types:
        normalized = _normalize_song_type_arg(value)
        if normalized in {"standard", "dx"}:
            types.add(normalized)
    return types


def _chart_type_label(song_type: str | None) -> str:
    return "DX" if song_type == "dx" else "ST" if song_type == "standard" else str(song_type or "").upper()


def _numeric_display_id(value: Any) -> str:
    try:
        numeric_id = int(str(value))
    except (TypeError, ValueError):
        return ""
    return str(numeric_id) if numeric_id > 0 else ""


def _search_song_base_id(song: dict[str, Any]) -> str:
    for value in (song.get("id"), song.get("source_id")):
        numeric_id = _numeric_display_id(value)
        if numeric_id:
            return numeric_id
    source_ids = song.get("source_ids") if isinstance(song.get("source_ids"), dict) else {}
    for value in source_ids.values():
        numeric_id = _numeric_display_id(value)
        if numeric_id:
            return numeric_id
    return ""


def _search_song_chart_ids(song: dict[str, Any]) -> dict[str, str]:
    base_id = _search_song_base_id(song)
    ids: dict[str, str] = {}
    charts = song.get("matched_charts") if isinstance(song.get("matched_charts"), list) else []
    for chart in charts:
        if not isinstance(chart, dict):
            continue
        chart_type = _normalize_song_type_arg(chart.get("chart_type"))
        if chart_type not in {"standard", "dx"} or chart_type in ids:
            continue
        chart_id = ""
        for key in ("chart_id", "music_id", "musicId", "internal_id"):
            chart_id = _numeric_display_id(chart.get(key))
            if chart_id:
                break
        if not chart_id and base_id:
            base_number = int(base_id)
            chart_id = str(base_number + 10000) if chart_type == "dx" and base_number < 10000 else base_id
        if chart_id and chart_type == "dx" and int(chart_id) < 10000:
            chart_id = str(int(chart_id) + 10000)
        if chart_id:
            ids[chart_type] = chart_id

    raw_types = song.get("available_chart_types") if isinstance(song.get("available_chart_types"), list) else []
    for raw_type in raw_types:
        chart_type = _normalize_song_type_arg(raw_type)
        if chart_type not in {"standard", "dx"} or chart_type in ids or not base_id:
            continue
        base_number = int(base_id)
        ids[chart_type] = str(base_number + 10000) if chart_type == "dx" and base_number < 10000 else base_id
    return dict(sorted(ids.items(), key=lambda item: {"standard": 0, "dx": 1}.get(item[0], 9)))


def _search_song_id_values(song: dict[str, Any]) -> list[str]:
    values = [_search_song_base_id(song)]
    values.extend(_search_song_chart_ids(song).values())
    return [value for value in dict.fromkeys(values) if value]


def _format_search_song_id(song: dict[str, Any]) -> str:
    chart_ids = _search_song_chart_ids(song)
    if chart_ids:
        unique_ids = list(dict.fromkeys(chart_ids.values()))
        if len(unique_ids) == 1:
            return unique_ids[0]
        labels = {"standard": "ST", "dx": "DX"}
        return " / ".join(f"{labels.get(chart_type, chart_type.upper())}#{chart_id}" for chart_type, chart_id in chart_ids.items())
    return _search_song_base_id(song) or str(song.get("id") or song.get("source_id") or "-")


def _query_numeric_id(query: str | None) -> str:
    if not query:
        return ""
    match = re.fullmatch(r"\s*(?:id\s*)?(\d+)\s*", str(query), flags=re.IGNORECASE)
    return match.group(1) if match else ""


def _cover_only_lookup_id(args: dict[str, Any]) -> str:
    explicit_id = args.get("music_id") or args.get("musicId") or args.get("id")
    if explicit_id not in (None, ""):
        return str(explicit_id).strip()
    return _query_numeric_id(_music_info_query(args))


def _local_cover_exists(song_id: Any) -> bool:
    if song_id in (None, ""):
        return False
    try:
        cover_path = music_picture(str(song_id))
    except Exception:
        return False
    return cover_path.exists() and cover_path.name != "11000.png"


def _load_cover_only_image(song_id: Any) -> Image.Image | None:
    try:
        path = music_picture(str(song_id))
    except Exception:
        return None
    if not path.exists() or path.name == "11000.png":
        return None
    try:
        return Image.open(path).convert("RGBA")
    except Exception:
        return None


def _cover_only_title(args: dict[str, Any]) -> str:
    for key in ("knownTitle", "known_title", "name"):
        value = args.get(key)
        if value not in (None, ""):
            return str(value).strip()
    # Only treat explicit title as known metadata. A generic query may be just an
    # ID or fuzzy text that failed lookup, so do not promote it into a title.
    title = args.get("title")
    return str(title).strip() if title not in (None, "") and not _query_numeric_id(str(title)) else ""


def _cover_only_type(args: dict[str, Any]) -> str:
    song_type = _song_type_arg(args)
    if song_type == "dx":
        return "DX"
    if song_type == "standard":
        return "SD"
    return ""


def _cover_only_music(args: dict[str, Any]) -> tuple[Music, dict[str, Any] | None] | None:
    lookup_id = _cover_only_lookup_id(args)
    if not lookup_id:
        return None
    if not _local_cover_exists(lookup_id):
        return None

    title = _cover_only_title(args)
    try:
        bpm = int(float(args.get("bpm")))
    except (TypeError, ValueError):
        bpm = 0
    music = Music(
        id=str(lookup_id),
        title=title,
        type=_cover_only_type(args),
        ds=[],
        level=[],
        cids=[],
        charts=[],
        basic_info=BasicInfo.model_validate({
            "title": title,
            "artist": str(args.get("artist") or ""),
            "genre": str(args.get("genre") or args.get("category") or ""),
            "bpm": bpm,
            "from": str(args.get("version") or args.get("from") or ""),
            "is_new": bool(args.get("is_new") or args.get("isNew") or False),
        }),
        stats=[],
    )
    search_song = {
        "_partial_metadata": True,
        "id": str(lookup_id),
        "source_id": str(lookup_id),
        "title": title,
    }
    return music, search_song


def _not_found_error(exc: ValueError) -> bool:
    return str(exc).startswith("未找到曲目")


def _music_display_id(args: dict[str, Any], music: Music, search_song: dict[str, Any] | None) -> str:
    return _numeric_display_id(_music_score_query_id(args, music, search_song))


def _resolve_music(args: dict[str, Any]) -> tuple[Music, dict[str, Any] | None]:
    music_id = args.get("music_id") or args.get("musicId") or args.get("id")
    query = _music_info_query(args)
    song_type = _song_type_arg(args) or _infer_song_type_from_music_id(music_id)
    query_song_type = None if song_type else _infer_song_type_from_query(query)
    if not music_id and not query:
        raise ValueError("需要提供 music_id/id 或 query/title/songQuery")

    try:
        search_song: dict[str, Any] | None = None
        if query:
            if song_type is None:
                search_song = _search_exact_music_info_song(query)
                if (
                    search_song is not None
                    and query_song_type in _search_song_chart_types(search_song)
                    and not _is_exact_song_identity_query(search_song, query)
                ):
                    search_song = _search_one_music_info_song(query, song_type=query_song_type)
            if search_song is None:
                song_type = song_type or query_song_type
                search_song = _search_one_music_info_song(query, song_type=song_type)
            music = music_from_search_song(search_song)
            if music is None:
                lookup_id = search_song.get("id") or search_song.get("source_id")
                music = mai.by_id(str(lookup_id)) if lookup_id not in (None, "") else None
        elif song_type:
            search_song = _search_one_music_info_song(str(music_id), song_type=song_type, music_id=music_id)
            music = music_from_search_song(search_song)
        else:
            music = mai.by_id(str(music_id))
            if not music:
                search_song = _search_one_music_info_song(str(music_id))
                music = music_from_search_song(search_song)
    except ValueError as exc:
        partial = _cover_only_music(args) if _not_found_error(exc) else None
        if partial is not None:
            return partial
        raise

    if not music:
        partial = _cover_only_music(args)
        if partial is not None:
            return partial
        raise ValueError(f"未找到曲目: {query or music_id}")
    return music, search_song


def _resolve_music_variants(args: dict[str, Any]) -> list[tuple[str | None, Music, dict[str, Any] | None]]:
    music_id = args.get("music_id") or args.get("musicId") or args.get("id")
    query = _music_info_query(args)
    requested_song_type = _song_type_arg(args) or _infer_song_type_from_music_id(music_id)
    if requested_song_type:
        music, search_song = _resolve_music(args)
        return [(requested_song_type, music, search_song)]

    if not music_id and not query:
        raise ValueError("需要提供 music_id/id 或 query/title/songQuery")

    lookup_query = query or str(music_id)
    try:
        search_song = _search_exact_music_info_song(query) if query else None
        if search_song is None:
            requested_song_type = _infer_song_type_from_query(query)
            if requested_song_type:
                music, search_song = _resolve_music(args)
                return [(requested_song_type, music, search_song)]
            search_song = _search_one_music_info_song(
                str(lookup_query),
                music_id=music_id if music_id not in (None, "") else None,
            )
        else:
            requested_song_type = _infer_song_type_from_query(query)
            if (
                requested_song_type in _search_song_chart_types(search_song)
                and not _is_exact_song_identity_query(search_song, query)
            ):
                music, variant_song = _resolve_music(args)
                return [(requested_song_type, music, variant_song)]
    except ValueError as exc:
        partial = _cover_only_music(args) if _not_found_error(exc) else None
        if partial is not None:
            music, search_song = partial
            return [(None, music, search_song)]
        raise
    chart_types = _search_song_chart_types(search_song)
    if {"standard", "dx"}.issubset(chart_types):
        variants: list[tuple[str | None, Music, dict[str, Any] | None]] = []
        for song_type in ("standard", "dx"):
            variant_args = {**args, "songType": song_type}
            music, variant_song = _resolve_music(variant_args)
            variants.append((song_type, music, variant_song))
        return variants

    music = music_from_search_song(search_song)
    if music is None:
        lookup_id = search_song.get("id") or search_song.get("source_id") or music_id
        music = mai.by_id(str(lookup_id)) if lookup_id not in (None, "") else None
    if not music:
        partial = _cover_only_music(args)
        if partial is not None:
            music, search_song = partial
            return [(None, music, search_song)]
        raise ValueError(f"未找到曲目: {query or music_id}")
    return [(None, music, search_song)]


def _difficulty_index(args: dict[str, Any], music: Music) -> int:
    raw_index = args.get("level_index", args.get("levelIndex", args.get("difficulty_index")))
    if raw_index not in (None, ""):
        try:
            index = int(raw_index)
        except (TypeError, ValueError):
            raise ValueError("level_index/difficulty_index 必须是 0-4")
    else:
        difficulty = str(args.get("difficulty", args.get("diff", "master")) or "master").strip().lower()
        aliases = {
            "basic": 0,
            "bas": 0,
            "green": 0,
            "绿": 0,
            "advanced": 1,
            "adv": 1,
            "yellow": 1,
            "黄": 1,
            "expert": 2,
            "exp": 2,
            "red": 2,
            "红": 2,
            "master": 3,
            "mas": 3,
            "purple": 3,
            "紫": 3,
            "remaster": 4,
            "re:master": 4,
            "remas": 4,
            "white": 4,
            "白": 4,
        }
        if difficulty not in aliases:
            raise ValueError("difficulty 必须是 Basic/Advanced/Expert/Master/Re:MASTER 或绿/黄/红/紫/白")
        index = aliases[difficulty]

    if index < 0 or index >= len(music.ds):
        raise ValueError(f"该曲没有 level_index={index} 的难度")
    return index


def _search_one_music_info_song(
    query: str,
    *,
    song_type: str | None = None,
    music_id: Any = None,
) -> dict[str, Any]:
    from maimai_mcp.search import search_songs

    search_args: dict[str, Any] = {"limit": 5}
    if song_type:
        search_args["song_type"] = song_type
    if music_id not in (None, "") and str(music_id).isdigit():
        search_args["id"] = str(music_id)
    else:
        search_args["query"] = query
    result = search_songs(**search_args)
    songs = result.get("songs") or []
    if not songs and music_id not in (None, "") and str(music_id).isdigit():
        numeric_id = int(str(music_id))
        if numeric_id > 10000:
            retry_args = {**search_args, "id": str(numeric_id - 10000)}
            if song_type is None:
                retry_args["song_type"] = "dx"
            result = search_songs(**retry_args)
            songs = result.get("songs") or []
    if not songs:
        raise ValueError(f"未找到曲目: {query}")
    total_matches = int(result.get("total_matches", len(songs)) or 0)
    if total_matches > 1:
        exact = [song for song in songs if _is_exact_song_query(song, query)]
        if len(exact) == 1:
            return exact[0]
        lines = []
        for index, song in enumerate(songs[:5], 1):
            sid = _format_search_song_id(song)
            title = song.get("title") or "-"
            artist = song.get("artist") or "-"
            lines.append(f"{index}. {title} | ID {sid} | {artist}")
        raise ValueError("匹配到多个曲目，请指定更精确的曲名或 ID:\n" + "\n".join(lines))
    return songs[0]


def _render_music_info(args: dict[str, Any]) -> dict[str, Any]:
    qq = args.get("qq")
    username = args.get("username")

    try:
        variants = _resolve_music_variants(args)
    except ValueError as e:
        return _err(str(e), code="MUSIC_NOT_FOUND")

    if len(variants) > 1:
        try:
            return _run_async(_render_music_info_variants_async(args, variants))
        except Exception as e:
            traceback.print_exc()
            return _err(f"渲染曲目信息失败: {e}")

    song_type, music, search_song = variants[0]
    display_id = _music_display_id(args, music, search_song)
    cover_lookup_id = _music_score_query_id(args, music, search_song)
    image_name = args.get("image_name") or args.get("imageName")
    if not image_name and search_song:
        image_name = search_song.get("image_name") or search_song.get("imageName")
    cover_image = _music_info_cover_image(str(cover_lookup_id), image_name, search_song)

    try:
        qqid = int(qq) if qq else None
        result = _run_async(
            _draw_music_info(
                music,
                qqid=qqid,
                username=username,
                user=None,
                cover_image=cover_image,
                display_id=display_id,
            )
        )
        return _ok_image(result, prefix=f"music_info_{cover_lookup_id}_{_chart_type_label(song_type)}")
    except Exception as e:
        traceback.print_exc()
        return _err(f"渲染曲目信息失败: {e}")


async def _render_music_info_variants_async(
    args: dict[str, Any],
    variants: list[tuple[str | None, Music, dict[str, Any] | None]],
) -> dict[str, Any]:
    qq = args.get("qq")
    username = args.get("username")
    qqid = int(qq) if qq else None
    user = None
    if qqid or username:
        try:
            user = await maiApi.query_user_b50(
                qqid=qqid,
                username=username,
                include_chart_metadata=False,
            )
        except Exception:
            qqid = None
            username = None

    query = _music_info_query(args) or str(args.get("music_id") or args.get("id") or "")
    results: list[dict[str, Any]] = []
    errors: list[dict[str, str]] = []
    for index, (song_type, music, search_song) in enumerate(variants, 1):
        label = _chart_type_label(song_type)
        try:
            variant_args = {**args, "songType": song_type} if song_type else args
            display_id = _music_display_id(variant_args, music, search_song)
            cover_lookup_id = _music_score_query_id(variant_args, music, search_song)
            image_name = args.get("image_name") or args.get("imageName")
            if not image_name and search_song:
                image_name = search_song.get("image_name") or search_song.get("imageName")
            cover_image = _music_info_cover_image(str(cover_lookup_id), image_name, search_song)
            image = await _draw_music_info(
                music,
                qqid=qqid,
                username=username,
                user=user,
                cover_image=cover_image,
                display_id=display_id,
            )
            payload = _ok_image_payload(image, prefix=f"music_info_{cover_lookup_id}_{label}")
            payload.update({
                "index": index,
                "query": query,
                "musicId": str(cover_lookup_id),
                "title": music.title,
                "chartType": label,
            })
            results.append(payload)
        except Exception as exc:
            errors.append({"index": str(index), "query": query, "chartType": label, "message": str(exc)})
    return _batch_result(results, errors)


async def _render_music_info_batch_async(args: dict[str, Any]) -> dict[str, Any]:
    items = _music_info_batch_items(args)
    if not items:
        return _err("需要提供 items 或 queries")

    qq = args.get("qq")
    username = args.get("username")
    qqid = int(qq) if qq else None
    user = None
    if qqid or username:
        try:
            user = await maiApi.query_user_b50(
                qqid=qqid,
                username=username,
                include_chart_metadata=False,
            )
        except Exception:
            qqid = None
            username = None

    results: list[dict[str, Any]] = []
    errors: list[dict[str, str]] = []
    for index, item in enumerate(items, 1):
        merged = {**args, **item}
        query = _music_info_query(merged) or str(merged.get("music_id") or merged.get("id") or "")
        try:
            variants = _resolve_music_variants(merged)
            for sub_index, (song_type, music, search_song) in enumerate(variants, 1):
                label = _chart_type_label(song_type)
                variant_args = {**merged, "songType": song_type} if song_type else merged
                display_id = _music_display_id(variant_args, music, search_song)
                cover_lookup_id = _music_score_query_id(variant_args, music, search_song)
                image_name = merged.get("image_name") or merged.get("imageName")
                if not image_name and search_song:
                    image_name = search_song.get("image_name") or search_song.get("imageName")
                cover_image = _music_info_cover_image(str(cover_lookup_id), image_name, search_song)
                image = await _draw_music_info(
                    music,
                    qqid=qqid,
                    username=username,
                    user=user,
                    cover_image=cover_image,
                    display_id=display_id,
                )
                payload = _ok_image_payload(image, prefix=f"music_info_{cover_lookup_id}_{label}")
                payload.update({
                    "index": index,
                    "subIndex": sub_index,
                    "query": query,
                    "musicId": str(cover_lookup_id),
                    "title": music.title,
                    "chartType": label,
                })
                results.append(payload)
        except Exception as exc:
            errors.append({"index": str(index), "query": query, "message": str(exc)})
    return _batch_result(results, errors)


def _render_music_info_batch(args: dict[str, Any]) -> dict[str, Any]:
    try:
        return _run_async(_render_music_info_batch_async(args))
    except Exception as e:
        traceback.print_exc()
        return _err(f"批量渲染曲目信息失败: {e}")


def _render_music_score(args: dict[str, Any]) -> dict[str, Any]:
    qq = args.get("qq")
    username = args.get("username")
    if not qq and not username:
        return _err("需要提供 qq 或 username")

    try:
        variants = _resolve_music_variants(args)
    except ValueError as e:
        return _err(str(e), code="MUSIC_NOT_FOUND")

    if len(variants) > 1:
        try:
            return _run_async(_render_music_score_variants_async(args, variants))
        except Exception as e:
            traceback.print_exc()
            return _err(f"渲染单曲成绩图失败: {e}")

    song_type, music, search_song = variants[0]
    score_lookup_id = _music_score_query_id(args, music, search_song)
    image_name = args.get("image_name") or args.get("imageName")
    if not image_name and search_song:
        image_name = search_song.get("image_name") or search_song.get("imageName")
    cover_image = _load_cover_for_interactive_render(str(score_lookup_id), image_name)

    try:
        qqid = int(qq) if qq else None
        result = _run_async(
            _draw_music_play_data(
                qqid,
                str(score_lookup_id),
                username=username,
                music=music,
                cover_image=cover_image,
            )
        )
        return _ok_image(result, prefix=f"music_score_{score_lookup_id}_{_chart_type_label(song_type)}")
    except Exception as e:
        traceback.print_exc()
        return _err(f"渲染单曲成绩图失败: {e}")


async def _render_music_score_variants_async(
    args: dict[str, Any],
    variants: list[tuple[str | None, Music, dict[str, Any] | None]],
) -> dict[str, Any]:
    qq = args.get("qq")
    username = args.get("username")
    qqid = int(qq) if qq else None
    records = None
    if not maiApi.token:
        records = await maiApi.query_user_plate(qqid=qqid, username=username)

    query = _music_info_query(args) or str(args.get("music_id") or args.get("id") or "")
    results: list[dict[str, Any]] = []
    errors: list[dict[str, str]] = []
    for index, (song_type, music, search_song) in enumerate(variants, 1):
        label = _chart_type_label(song_type)
        score_lookup_id = _music_score_query_id({**args, "songType": song_type}, music, search_song)
        try:
            image_name = args.get("image_name") or args.get("imageName")
            if not image_name and search_song:
                image_name = search_song.get("image_name") or search_song.get("imageName")
            cover_image = _load_cover_for_interactive_render(str(score_lookup_id), image_name)
            image = await _draw_music_play_data(
                qqid,
                str(score_lookup_id),
                username=username,
                music=music,
                cover_image=cover_image,
                records=records,
            )
            payload = _ok_image_payload(image, prefix=f"music_score_{score_lookup_id}_{label}")
            payload.update({
                "index": index,
                "query": query,
                "musicId": str(score_lookup_id),
                "title": music.title,
                "chartType": label,
            })
            results.append(payload)
        except Exception as exc:
            errors.append({"index": str(index), "query": query, "chartType": label, "message": str(exc)})
    return _batch_result(results, errors)


def _load_cover_for_interactive_render(song_id: str, image_name: Any):
    _ = song_id, image_name
    return None


def _music_info_cover_image(song_id: str, image_name: Any, search_song: dict[str, Any] | None):
    _ = image_name, search_song
    return _load_cover_only_image(song_id)


def _music_score_query_id(args: dict[str, Any], music: Music, search_song: dict[str, Any] | None) -> str:
    song_type = _song_type_arg(args) or _normalize_song_type_arg(getattr(music, "type", None))
    explicit_id = args.get("music_id") or args.get("musicId") or args.get("id")
    if isinstance(search_song, dict) and search_song.get("_partial_metadata"):
        return str(explicit_id or search_song.get("id") or music.id)
    if explicit_id not in (None, ""):
        text = str(explicit_id)
        if text.isdigit() and int(text) > 10000 and song_type != "standard":
            return text

    if isinstance(search_song, dict):
        render_id = render_id_from_search_song(search_song, song_type)
        if render_id:
            return render_id

    # Local DX ids are stored without the waterfish +10000 offset.
    if str(music.type).upper() == "DX":
        try:
            numeric_id = int(str(music.id))
            if 0 < numeric_id < 10000:
                return str(numeric_id + 10000)
        except (TypeError, ValueError):
            pass
    return str(music.id)


def _render_music_global_stats(args: dict[str, Any]) -> dict[str, Any]:
    try:
        variants = _resolve_music_variants(args)
    except ValueError as e:
        return _err(str(e), code="MUSIC_NOT_FOUND")

    if len(variants) > 1:
        try:
            return _run_async(_render_music_global_stats_variants_async(args, variants))
        except Exception as e:
            traceback.print_exc()
            return _err(f"渲染全服统计图失败: {e}")

    _song_type, music, _search_song = variants[0]

    if not any(bool(stats) for stats in (music.stats or [])):
        try:
            search_song = _search_one_music_info_song(str(music.id))
            search_music = music_from_search_song(search_song)
            if search_music is not None:
                music = search_music
        except ValueError:
            pass

    try:
        level_index = _difficulty_index(args, music)
    except ValueError as e:
        return _err(str(e))

    if not music.stats or level_index >= len(music.stats) or not music.stats[level_index]:
        return _err("该曲目/难度没有可用的全服统计数据。")

    try:
        result = _run_async(_music_global_data(music, level_index))
        return _ok_image(result, prefix=f"music_stats_{music.id}_{level_index}")
    except Exception as e:
        traceback.print_exc()
        return _err(f"渲染全服统计图失败: {e}")


async def _render_music_global_stats_variants_async(
    args: dict[str, Any],
    variants: list[tuple[str | None, Music, dict[str, Any] | None]],
) -> dict[str, Any]:
    query = _music_info_query(args) or str(args.get("music_id") or args.get("id") or "")
    results: list[dict[str, Any]] = []
    errors: list[dict[str, str]] = []
    for index, (song_type, music, _search_song) in enumerate(variants, 1):
        label = _chart_type_label(song_type)
        try:
            try:
                level_index = _difficulty_index(args, music)
            except ValueError as e:
                raise ValueError(str(e))

            if not music.stats or level_index >= len(music.stats) or not music.stats[level_index]:
                raise ValueError("该曲目/难度没有可用的全服统计数据。")
            image = await _music_global_data(music, level_index)
            payload = _ok_image_payload(image, prefix=f"music_stats_{music.id}_{level_index}_{label}")
            payload.update({
                "index": index,
                "query": query,
                "musicId": str(music.id),
                "title": music.title,
                "chartType": label,
                "levelIndex": level_index,
            })
            results.append(payload)
        except Exception as exc:
            errors.append({"index": str(index), "query": query, "chartType": label, "message": str(exc)})
    return _batch_result(results, errors)


def _render_rise_score(args: dict[str, Any]) -> dict[str, Any]:
    qq = args.get("qq")
    username = args.get("username")
    if not qq and not username:
        return _err("需要提供 qq 或 username")

    level_arg = args.get("level")
    level = normalize_level_value(level_arg) if level_arg not in (None, "") else None
    score = args.get("score")
    algorithm = str(args.get("algorithm") or RISE_SCORE_ALGORITHM_DEFAULT).strip().lower()
    try:
        qqid = int(qq) if qq else None
        score_value = int(score) if score not in (None, "") else None
        result = _run_async(_rise_score_data(
            qqid,
            username=username,
            level=level,
            score=score_value,
            algorithm=algorithm,
        ))
        return _ok_image(result, prefix="rise_score")
    except Exception as e:
        traceback.print_exc()
        return _err(f"渲染上分推荐图失败: {e}")


def _render_score_list(args: dict[str, Any]) -> dict[str, Any]:
    qq = args.get("qq")
    username = args.get("username")
    if not qq and not username:
        return _err("需要提供 qq 或 username")

    rating = args.get("rating", args.get("level"))
    ds = args.get("ds")
    if rating in (None, "") and ds in (None, ""):
        return _err("需要提供 rating/level/ds")
    try:
        rating_value: str | float
        if ds not in (None, ""):
            rating_value = float(ds)
        elif isinstance(rating, int):
            rating_value = str(rating)
        elif isinstance(rating, float):
            rating_value = str(int(rating)) if rating.is_integer() else float(rating)
        else:
            text = str(rating).strip()
            rating_value = float(text) if "." in text and text.replace(".", "", 1).isdigit() else text
        page = int(args.get("page", 1) or 1)
        qqid = int(qq) if qq else None
        result = _run_async(_level_achievement_list_data(qqid, username, rating_value, page=page))
        return _ok_image(result, prefix=f"score_list_{rating_value}")
    except Exception as e:
        traceback.print_exc()
        return _err(f"渲染成绩列表图失败: {e}")


def _render_rating_ranking(args: dict[str, Any]) -> dict[str, Any]:
    name = str(args.get("name") or args.get("username") or "").strip().lower()
    qq = str(args.get("qq") or "").strip()
    try:
        if qq and not name:
            userinfo = _run_async(maiApi.query_user_b50(qqid=int(qq), include_chart_metadata=False))
            name = str(userinfo.username or "").strip().lower()
            if not name:
                return _err(f"QQ {qq} 查询结果没有水鱼 username，无法定位 Diving-Fish 公开排名。")
        if not name and (args.get("startRank") is not None or args.get("endRank") is not None):
            start_rank = int(args.get("startRank", 1) or 1)
            end_rank = int(args.get("endRank", start_rank) or start_rank)
            if start_rank < 1 or end_rank < start_rank:
                return _err("startRank/endRank 参数不合法。")
            if end_rank - start_rank + 1 > 30:
                return _err("Diving-Fish 公开排名一次最多输出 30 人。")
            result = _run_async(_rating_ranking_range_text(start_rank, end_rank))
            from maimaidx_render_mcp.maimaidx.image import text_to_image

            return _save_and_return(text_to_image(result.strip()), prefix="rating_ranking")
        page = int(args.get("page", 1) or 1)
        result = _run_async(_rating_ranking_data(name, page))
        if isinstance(result, str) and not result.startswith("base64://"):
            from maimaidx_render_mcp.maimaidx.image import text_to_image

            if result.startswith("未知错误"):
                return _err(result)
            return _save_and_return(text_to_image(result.strip()), prefix="rating_ranking")
        return _ok_image(result, prefix="rating_ranking")
    except Exception as e:
        traceback.print_exc()
        return _err(f"渲染 rating 排行榜失败: {e}")


async def _rating_ranking_range_text(start_rank: int, end_rank: int) -> str:
    import time

    rank_data = await maiApi.rating_ranking()
    total = len(rank_data)
    if not rank_data or start_rank > total:
        return f"截止至 {time.strftime('%Y-%m-%d %H:%M:%S', time.localtime())}\n未找到第 {start_rank}-{end_rank} 名的公开 ranking 数据。"
    end_rank = min(end_rank, total)
    lines = [
        f"截止至 {time.strftime('%Y-%m-%d %H:%M:%S', time.localtime())}",
        f"Diving-Fish 已注册用户 ra 排行第 {start_rank}-{end_rank} 名",
        "",
        "排名 | username | rating",
        "--- | --- | ---",
    ]
    for rank, ranker in enumerate(rank_data[start_rank - 1 : end_rank], start=start_rank):
        lines.append(f"{rank} | {ranker.username} | {ranker.ra}")
    lines.append("")
    lines.append(f"共 {total} 人")
    return "\n".join(lines)


# ============================================================
# Tool: render_maimai_plate_progress — 牌子进度（文本）
# ============================================================

def _render_plate_progress(args: dict[str, Any]) -> dict[str, Any]:
    qq = args.get("qq")
    username = args.get("username")
    version = args.get("version", "真")
    plan = args.get("plan", "极")
    server = _normalize_server(args.get("server", "cn"))
    version, server = _normalize_plate_version_and_server(version, server)
    if server == "jp":
        return _err(_jp_unsupported_message())

    if not qq and not username:
        return _err("需要提供 qq 或 username")

    try:
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        qqid = int(qq) if qq else None
        result = loop.run_until_complete(
            _player_plate_data(qqid, username, version, plan, server=server)
        )
        loop.close()

        if isinstance(result, str) and result.startswith("base64://"):
            return _ok_image(result, prefix=f"plate_progress_{server}_{version}_{plan}")
        if isinstance(result, str):
            return {"content": [{"type": "text", "text": result}]}
        return _ok_image(result, prefix=f"plate_progress_{server}_{version}_{plan}")
    except Exception as e:
        traceback.print_exc()
        return _err(f"查询牌子进度失败: {e}")


async def _render_plate_progress_batch_async(args: dict[str, Any]) -> dict[str, Any]:
    qq = args.get("qq")
    username = args.get("username")
    if not qq and not username:
        return _err("需要提供 qq 或 username")

    items = _plate_batch_items(args)
    if not items:
        return _err("需要提供 items 或 versions")

    qqid = int(qq) if qq else None
    records = await maiApi.query_user_plate(qqid=qqid, username=username)
    results: list[dict[str, Any]] = []
    errors: list[dict[str, str]] = []
    for index, item in enumerate(items, 1):
        raw_version = item.get("version", item.get("plate", "真"))
        plan = item.get("plan", args.get("plan", "极"))
        server = _normalize_server(item.get("server", args.get("server", "cn")))
        version, server = _normalize_plate_version_and_server(raw_version, server)
        label = f"{version}{plan}"
        if server == "jp":
            errors.append({"index": str(index), "label": label, "message": _jp_unsupported_message()})
            continue
        try:
            result = await _player_plate_data(
                qqid,
                username,
                version,
                plan,
                server=server,
                records=records,
            )
            item_result: dict[str, Any] = {
                "index": index,
                "version": version,
                "plan": plan,
                "server": server,
                "label": label,
            }
            if isinstance(result, str) and result.startswith("base64://"):
                item_result.update(_ok_image_payload(result, prefix=f"plate_progress_{server}_{version}_{plan}"))
            elif isinstance(result, str):
                item_result["text"] = result
            else:
                item_result.update(_ok_image_payload(result, prefix=f"plate_progress_{server}_{version}_{plan}"))
            results.append(item_result)
        except Exception as exc:
            errors.append({"index": str(index), "label": label, "message": str(exc)})
    return _batch_result(results, errors)


def _render_plate_progress_batch(args: dict[str, Any]) -> dict[str, Any]:
    try:
        return _run_async(_render_plate_progress_batch_async(args))
    except Exception as e:
        traceback.print_exc()
        return _err(f"批量查询牌子进度失败: {e}")


# ============================================================
# MCP 协议处理
# ============================================================

TOOLS = [
    {
        "name": "render_maimai_b50",
        "description": "渲染 maimai B50 成绩图。支持 yuzu（默认）、maibot、legacy 三种风格。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "qq": {"type": "string", "description": "玩家 QQ 号"},
                "username": {"type": "string", "description": "查分器用户名（与 qq 二选一）"},
                "style": {"type": "string", "description": "渲染风格: yuzu/maibot/legacy，默认 yuzu"},
                "computeFromRecords": {
                    "type": "boolean",
                    "description": "为 true 时拉取完整成绩，用拟合定数重算单曲 rating 并按拟合 rating 绘制 B50。",
                },
                "source": {
                    "type": "string",
                    "description": "可选，传 records/computed/fitted 等同于 computeFromRecords=true。",
                },
                "staticDir": {"type": "string", "description": "静态资源目录（maibot/legacy 需要）"},
                "coverCacheDir": {"type": "string", "description": "曲绘缓存目录"},
                "timeoutMs": {"type": "integer", "description": "超时毫秒"},
                "title": {"type": "string", "description": "自定义标题"},
            },
            "required": []
        },
    },
    {
        "name": "render_maimai_plate",
        "description": "渲染牌子完成表（如「真极」）。展示某版本全曲目的各难度达成状态。支持 cn（国服）和 custom（自定义）。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "qq": {"type": "string", "description": "玩家 QQ 号。和 username 二选一。"},
                "username": {"type": "string", "description": "查分器用户名。和 qq 二选一。"},
                "version": {"type": "string", "description": "版本: 真/超/檄/橙/暁/桃/櫻/紫/菫/白/雪/輝，或自定义牌子名。"},
                "plan": {"type": "string", "description": "目标: 极(FC)/将(100%)/神(AP)/舞舞(FSD/FDX)"},
                "server": {"type": "string", "enum": ["cn", "custom"], "description": "服务器，默认 cn。custom=读取 data/custom_plates.json 自定义牌子。"},
            },
            "required": ["version", "plan"]
        },
    },
    {
        "name": "render_maimai_plate_batch",
        "description": "批量渲染多个牌子完成表。只查询一次玩家完整成绩，然后生成多张牌子完成表，避免多个牌子消耗多次 Developer-Token 查询额度。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "qq": {"type": "string", "description": "玩家 QQ 号。和 username 二选一。"},
                "username": {"type": "string", "description": "查分器用户名。和 qq 二选一。"},
                "items": {
                    "type": "array",
                    "description": "牌子列表，每项如 {\"version\":\"真\",\"plan\":\"将\",\"server\":\"cn\"}；server 可省略。",
                    "items": {"type": "object"},
                },
                "versions": {"type": "array", "items": {"type": "string"}, "description": "多个版本名；未传 items 时使用。"},
                "plan": {"type": "string", "description": "通用目标: 极/将/神/舞舞。items 内的 plan 优先。"},
                "server": {"type": "string", "enum": ["cn", "custom"], "description": "通用服务器，默认 cn；items 内的 server 优先。custom=读取 data/custom_plates.json 自定义牌子。"},
            },
            "required": []
        },
    },
    {
        "name": "render_maimai_rating",
        "description": "渲染定数完成表。展示某等级的达成情况。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "qq": {"type": "string", "description": "玩家 QQ 号。和 username 二选一。"},
                "username": {"type": "string", "description": "查分器用户名。和 qq 二选一。"},
                "rating": {"type": "string", "description": "等级，如 14+、15"},
                "isfc": {"type": "boolean", "description": "是否按FC筛选"},
            },
            "required": ["rating"]
        },
    },
    {
        "name": "render_maimai_progress",
        "description": "渲染等级进度图。展示某等级达成某目标的已完成/未完成/未游玩。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "qq": {"type": "string", "description": "玩家 QQ 号"},
                "username": {"type": "string", "description": "查分器用户名"},
                "level": {"type": "string", "description": "等级，如 14、14+、15"},
                "plan": {"type": "string", "description": "目标，必须按用户目标显式传入：全S=s、全S+=s+、全SS=ss、全SS+=ss+、全SSS=sss、全SSS+=sss+、AP=ap、FC=fc、FSD=fsd；未指定目标时默认 sss。"},
                "server": {"type": "string", "enum": ["cn"], "description": "服务器，默认 cn。当前分支仅支持国服子集。"},
                "category": {"type": "string", "description": "分类: default/completed/unfinished/notstarted"},
                "page": {"type": "integer", "description": "页码"},
            },
            "required": []
        },
    },
    {
        "name": "render_maimai_music_info",
        "description": "渲染曲目信息图。展示曲绘、定数、谱师、物量等。可传 music_id/id，也可传 query/title/songQuery 让本地查歌先按曲名或别名解析。若曲库没有记录但本地存在该 ID 曲绘，会降级生成只含已知信息的曲绘图。同曲同时有 ST/DX 且未传 songType 时会自动生成两张图并返回 images 数组。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "music_id": {"type": "string", "description": "曲目 ID。找不到时会按 query 逻辑回查。"},
                "id": {"type": "string", "description": "同 music_id。"},
                "query": {"type": "string", "description": "曲名、别名或 ID。"},
                "title": {"type": "string", "description": "同 query。"},
                "songQuery": {"type": "string", "description": "同 query。"},
                "songType": {"type": "string", "enum": ["standard", "dx"], "description": "可选谱面类型。standard=标准谱面/ST/SD，dx=DX谱面；同曲有 ST/DX 且不传时会自动生成两张。"},
                "qq": {"type": "string", "description": "玩家 QQ。和 username 二选一，用于按 B50 估算各目标达成率的潜在 rating 增量。"},
                "username": {"type": "string", "description": "查分器用户名。和 qq 二选一，用于按 B50 估算各目标达成率的潜在 rating 增量。"},
            },
            "required": []
        },
    },
    {
        "name": "render_maimai_music_info_batch",
        "description": "批量渲染多首歌的曲目信息图。可一次传多个 query；如果带 qq，只查询一次 B50 并复用到所有曲目信息图；单个条目同曲有 ST/DX 且未传 songType 时会自动展开成两张。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "qq": {"type": "string", "description": "玩家 QQ。和 username 二选一；批量内只查一次 B50。"},
                "username": {"type": "string", "description": "查分器用户名。和 qq 二选一；批量内只查一次 B50。"},
                "items": {
                    "type": "array",
                    "description": "曲目列表，每项可为字符串或 {\"query\":\"曲名/别名/ID\"} / {\"music_id\":\"ID\"}。",
                    "items": {},
                },
                "queries": {"type": "array", "items": {"type": "string"}, "description": "多个曲名、别名或 ID；未传 items 时使用。"},
                "songType": {"type": "string", "enum": ["standard", "dx"], "description": "通用谱面类型；items 内的 songType 优先。"},
            },
            "required": []
        },
    },
    {
        "name": "render_maimai_music_score",
        "description": "渲染玩家单曲成绩图。展示某玩家在某首歌各难度的达成率、评级、FC/FS、DX Score 与 rating。同曲同时有 ST/DX 且未传 songType 时会自动生成两张图并返回 images 数组。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "qq": {"type": "string", "description": "玩家 QQ 号"},
                "username": {"type": "string", "description": "查分器用户名（与 qq 二选一）"},
                "music_id": {"type": "string", "description": "曲目 ID。优先传 Diving-Fish 数字 ID。"},
                "id": {"type": "string", "description": "同 music_id"},
                "query": {"type": "string", "description": "曲名、别名或 ID。会先用本地查歌解析到曲目。"},
                "title": {"type": "string", "description": "同 query"},
                "songQuery": {"type": "string", "description": "同 query"},
                "songType": {"type": "string", "enum": ["standard", "dx"], "description": "可选谱面类型。standard=标准谱面/ST/SD，dx=DX谱面；同曲有 ST/DX 且不传时会自动生成两张。"},
            },
            "required": []
        },
    },
    {
        "name": "render_maimai_music_global_stats",
        "description": "渲染曲目全服统计图。展示指定曲目和难度的 FC 分布与达成率等级分布；有 pyecharts/playwright 时使用 ECharts，否则使用内置 PIL 兜底绘制。同曲同时有 ST/DX 且未传 songType 时会自动生成两张图并返回 images 数组。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "music_id": {"type": "string", "description": "曲目 ID"},
                "id": {"type": "string", "description": "同 music_id"},
                "query": {"type": "string", "description": "曲名、别名或 ID"},
                "title": {"type": "string", "description": "同 query"},
                "songQuery": {"type": "string", "description": "同 query"},
                "songType": {"type": "string", "enum": ["standard", "dx"], "description": "可选谱面类型。standard=标准谱面/ST/SD，dx=DX谱面；同曲有 ST/DX 且不传时会自动生成两张。"},
                "difficulty": {"type": "string", "description": "难度: Basic/Advanced/Expert/Master/Re:MASTER 或绿/黄/红/紫/白，默认 Master"},
                "level_index": {"type": "integer", "description": "难度下标 0-4；提供时优先于 difficulty"},
            },
            "required": []
        },
    },
    {
        "name": "render_maimai_rise_score",
        "description": "渲染上分推荐图。根据玩家 B50 与历史成绩推荐可能提升 rating 的谱面。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "qq": {"type": "string", "description": "玩家 QQ 号"},
                "username": {"type": "string", "description": "查分器用户名（与 qq 二选一）"},
                "level": {"type": "string", "description": "可选等级过滤，如 14、14+"},
                "score": {"type": "integer", "description": "可选目标增量 rating，用于收窄推荐"},
                "algorithm": {"type": "string", "enum": ["legacy", "expected"], "description": "推荐算法。legacy=旧版拟合定数分桶随机（默认）；expected=期望收益加权随机。"},
            },
            "required": []
        },
    },
    {
        "name": "render_maimai_score_list",
        "description": "渲染玩家等级/定数成绩列表图。按等级如 14+ 或精确定数如 14.6 列出玩家成绩。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "qq": {"type": "string", "description": "玩家 QQ 号"},
                "username": {"type": "string", "description": "查分器用户名（与 qq 二选一）"},
                "rating": {"oneOf": [{"type": "string"}, {"type": "number"}], "description": "等级或精确定数，如 14+、14.6"},
                "level": {"type": "string", "description": "同 rating，用于等级"},
                "ds": {"type": "number", "description": "同 rating，用于精确定数"},
                "page": {"type": "integer", "description": "页码，默认 1"},
            },
            "required": []
        },
    },
    {
        "name": "render_maimai_rating_ranking",
        "description": "渲染 Diving-Fish 公开 rating 排行榜；可按 username 查询某玩家排名，或按页输出排行榜图。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "qq": {"type": "string", "description": "可选，玩家 QQ 号；会先查询该 QQ 对应的水鱼 username 后定位公开排名"},
                "username": {"type": "string", "description": "可选，查询指定查分器用户名的排名"},
                "name": {"type": "string", "description": "同 username"},
                "startRank": {"type": "integer", "description": "可选，公开排行榜起始名次。和 endRank 一起用于输出排名段，最多 30 人。"},
                "endRank": {"type": "integer", "description": "可选，公开排行榜结束名次。和 startRank 一起用于输出排名段，最多 30 人。"},
                "page": {"type": "integer", "description": "页码，默认 1"},
            },
            "required": []
        },
    },
    {
        "name": "render_maimai_plate_progress",
        "description": "查询牌子完成剩余进度（文本）。展示各难度未完成曲目数量。默认 cn；自定义牌子使用 custom。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "qq": {"type": "string", "description": "玩家 QQ 号"},
                "username": {"type": "string", "description": "查分器用户名"},
                "version": {"type": "string", "description": "版本，如 真/熊/爽/丸/CiRCLE"},
                "plan": {"type": "string", "description": "目标: 极/将/神/舞舞"},
                "server": {"type": "string", "enum": ["cn", "custom"], "description": "服务器，默认 cn。custom=读取 data/custom_plates.json 自定义牌子。"},
            },
            "required": []
        },
    },
    {
        "name": "render_maimai_plate_progress_batch",
        "description": "批量查询多个牌子的剩余进度。只查询一次玩家完整成绩，然后生成多个牌子进度结果，避免多个牌子消耗多次 Developer-Token 查询额度。",
        "inputSchema": {
            "type": "object",
            "properties": {
                "qq": {"type": "string", "description": "玩家 QQ 号。和 username 二选一。"},
                "username": {"type": "string", "description": "查分器用户名。和 qq 二选一。"},
                "items": {
                    "type": "array",
                    "description": "牌子列表，每项如 {\"version\":\"真\",\"plan\":\"将\",\"server\":\"cn\"}；server 可省略。",
                    "items": {"type": "object"},
                },
                "versions": {"type": "array", "items": {"type": "string"}, "description": "多个版本名；未传 items 时使用。"},
                "plan": {"type": "string", "description": "通用目标: 极/将/神/舞舞。items 内的 plan 优先。"},
                "server": {"type": "string", "enum": ["cn", "custom"], "description": "通用服务器，默认 cn；items 内的 server 优先。"},
            },
            "required": []
        },
    },
]


def handle_message(message: dict[str, Any]) -> dict[str, Any] | None:
    """处理 MCP JSON-RPC 消息"""
    msg_id = message.get("id")
    method = message.get("method")

    if method == "initialize":
        return {
            "jsonrpc": "2.0",
            "id": msg_id,
            "result": {
                "protocolVersion": "2024-11-05",
                "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION},
                "capabilities": {"tools": {}},
            },
        }
    if method == "notifications/initialized":
        return None
    if method == "tools/list":
        return {
            "jsonrpc": "2.0",
            "id": msg_id,
            "result": {"tools": TOOLS},
        }
    if method == "tools/call":
        params = message.get("params", {})
        tool_name = params.get("name", "")
        arguments = params.get("arguments", {})
        try:
            content = _call_tool(tool_name, arguments)
            return {
                "jsonrpc": "2.0",
                "id": msg_id,
                "result": content,
            }
        except Exception as e:
            return {
                "jsonrpc": "2.0",
                "id": msg_id,
                "result": _err(str(e)),
            }
    if method == "ping":
        return {"jsonrpc": "2.0", "id": msg_id, "result": {}}

    return {"jsonrpc": "2.0", "id": msg_id, "result": _err(f"Unknown method: {method}")}


def main():
    """MCP stdio 入口"""
    import sys
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        response = handle_message(message)
        if response is not None:
            sys.stdout.write(json.dumps(response, ensure_ascii=False) + "\n")
            sys.stdout.flush()


if __name__ == "__main__":
    main()
