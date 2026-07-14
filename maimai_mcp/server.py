from __future__ import annotations

import json
import sys
import traceback
from copy import deepcopy
from typing import Any

from . import __version__
from .scoring import (
    MaimaiError as ScoringError,
    TOOLS as SCORING_TOOL_DEFS,
    find_score_combinations,
    score_counts,
)
from .search import (
    SearchError,
    _normalize_name_alias_kind,
    clear_search_caches,
    collect_song_results,
    delete_custom_alias,
    delete_name_alias,
    flatten_note_counts,
    list_name_aliases,
    list_songs_by_id,
    list_versions,
    random_songs,
    resolve_single_chart_note_totals,
    save_custom_alias,
    save_name_alias,
    search_songs,
    to_simplified,
    unique_preserve_order,
)
from .source_refresh import RefreshError, refresh_sources, refresh_sources_bg, read_bg_job_status
from .today import format_today_maimai, song_from_search_result, today_maimai


SERVER_NAME = "maimai-local-search"
OUTPUT_ARGUMENT_KEYS = {"format", "include_raw", "debug"}
TODAY_DEFAULT_BOT_NAME = "MaiBot"


def _is_name_alias_kind(value: Any) -> bool:
    """委托给 search._normalize_name_alias_kind 作单点判定，避免别名 token 在两处维护。"""
    if value is None or value == "":
        return False
    try:
        _normalize_name_alias_kind(value)
    except SearchError:
        return False
    return True
OUTPUT_FORMAT_PROPERTIES: dict[str, dict[str, Any]] = {
    "format": {
        "type": "string",
        "enum": ["text", "compact", "json"],
        "description": (
            "Tool result format. Defaults to text (full verbose: every source × every chart). "
            "Use compact for list view (grouped by ST/DX chart type, deduplicated across sources, "
            "no fit_label noise) — good for limit>=5 searches. Use json only for debugging or full raw fields."
        ),
    },
    "include_raw": {
        "type": "boolean",
        "description": "Return the full raw JSON payload instead of the readable text summary.",
    },
    "debug": {
        "type": "boolean",
        "description": "Alias for include_raw; returns raw JSON for troubleshooting.",
    },
}

SEARCH_TOOL = {
    "name": "search_maimai_songs",
    "description": (
        "Search local maimai DX song data by song ID, fuzzy title, chart level, "
        "chart constant, difficulty, or chart type. Fuzzy title search also checks "
        "local aliases. Returns every matching song and the matching charts for each song. "
        "This branch uses CN/LXNS, Diving-Fish, Yuzu aliases, and local custom aliases only."
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Song ID like 11451/id11451, a fuzzy song title, or an alias fragment.",
            },
            "level": {
                "type": "string",
                "description": "Chart level, for example 13, 13+, 14+. Trailing ? in data is ignored.",
            },
            "genre": {
                "type": "string",
                "description": "Song genre/category filter. Fuzzy substring match, for example POPS or niconico.",
            },
            "version": {
                "type": "string",
                "description": "Song/chart version filter. Fuzzy substring match, for example PRiSM PLUS or dx-prism-plus.",
            },
            "ds": {
                "description": "Chart constant. Supports a number, or a string range such as 13.4-13.8.",
                "oneOf": [{"type": "number"}, {"type": "string"}],
            },
            "ds_min": {
                "type": "number",
                "description": "Minimum chart constant. Overrides ds range lower bound when supplied.",
            },
            "ds_max": {
                "type": "number",
                "description": "Maximum chart constant. Overrides ds range upper bound when supplied.",
            },
            "fit_diff": {
                "description": "Fitted chart constant. Single values are bucketed by one decimal, e.g. 13.1 means [13.10, 13.20). String ranges such as 13.13-13.27 are exact ranges.",
                "oneOf": [{"type": "number"}, {"type": "string"}],
            },
            "fit_diff_min": {
                "type": "number",
                "description": "Minimum fitted chart constant. Range bounds are not bucketed.",
            },
            "fit_diff_max": {
                "type": "number",
                "description": "Maximum fitted chart constant. Range bounds are not bucketed.",
            },
            "fit_delta": {
                "description": "Actual ds minus fitted constant. Supports exact value or string range.",
                "oneOf": [{"type": "number"}, {"type": "string"}],
            },
            "fit_delta_min": {"type": "number", "description": "Minimum ds - fit_diff."},
            "fit_delta_max": {"type": "number", "description": "Maximum ds - fit_diff."},
            "fit_label": {
                "type": "string",
                "description": "Filter by 虚高 or 虚低. 虚高 means ds - fit_diff > 0; 虚低 means ds - fit_diff < 0.",
            },
            "region_has": {
                "description": "Require song availability in cn/国服.",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}],
            },
            "region_missing": {
                "description": "Require song not to be available in cn/国服.",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}],
            },
            "sort": {
                "type": "string",
                "enum": ["fit_delta_desc", "fit_delta_asc", "fit_diff_asc", "fit_diff_desc", "虚高", "虚低", "最虚高", "最虚低"],
                "description": "Optional fit sorting. fit_delta_desc finds most 虚高; fit_delta_asc finds most 虚低.",
            },
            "difficulty": {
                "type": "string",
                "description": "Difficulty such as Basic/Advanced/Expert/Master/Re:MASTER or 绿/黄/红/紫/白.",
            },
            "song_type": {
                "type": "string",
                "description": "Chart type: standard/SD, dx, or utage/宴.",
                "enum": ["standard", "SD", "sd", "st", "std", "标准", "标", "dx", "DX", "utage1p", "1p", "单人宴", "单人", "utage", "宴", "宴会场", "utage2p", "2p", "双人宴", "合奏宴", "合奏"],
            },
            "is_new": {
                "type": "boolean",
                "description": "Filter by new song status. true returns only new songs, false returns only non-new songs.",
            },
            "is_new_source": {
                "type": "string",
                "description": "Which source to check for is_new. Only cn is supported in this branch.",
                "enum": ["cn"],
            },
            "id": {
                "description": "Song ID match. Supports a single integer, or a string range like '100-500'.",
                "oneOf": [{"type": "integer"}, {"type": "string"}],
            },
            "id_min": {
                "type": "integer",
                "description": "Optional minimum song ID (inclusive). Overrides id range lower bound.",
            },
            "id_max": {
                "type": "integer",
                "description": "Optional maximum song ID (inclusive). Overrides id range upper bound.",
            },
            "bpm": {
                "description": "BPM filter. Supports a single number, or a string range like '160-180'.",
                "oneOf": [{"type": "number"}, {"type": "string"}],
            },
            "bpm_min": {
                "type": "number",
                "description": "Optional minimum BPM (inclusive). Overrides bpm range lower bound.",
            },
            "bpm_max": {
                "type": "number",
                "description": "Optional maximum BPM (inclusive). Overrides bpm range upper bound.",
            },
            "is_locked": {
                "type": "boolean",
                "description": "Lock status is unavailable without dxdata; true returns no public-source songs, false treats songs as unlocked.",
            },
            "artist": {
                "type": "string",
                "description": "Song artist filter. Case-insensitive substring match across all sources, e.g. 'sasakure' or 'かめりあ'.",
            },
            "charter": {
                "type": "string",
                "description": "Chart designer (谱师) filter. Case-insensitive substring match against per-chart note designer, e.g. 'jun' or 'safari'. Narrows matched_charts.",
            },
            "tag": {
                "description": "Unsupported in this branch: dxrating tag data is not included.",
                "oneOf": [
                    {"type": "string"},
                    {"type": "integer"},
                    {"type": "array", "items": {"oneOf": [{"type": "string"}, {"type": "integer"}]}},
                ],
            },
            "tag_exclude": {
                "description": "Unsupported in this branch: dxrating tag data is not included.",
                "oneOf": [
                    {"type": "string"},
                    {"type": "integer"},
                    {"type": "array", "items": {"oneOf": [{"type": "string"}, {"type": "integer"}]}},
                ],
            },
            "released_after": {
                "type": "string",
                "description": "Unsupported in this branch: per-chart release dates require dxdata.",
            },
            "released_before": {
                "type": "string",
                "description": "Unsupported in this branch: per-chart release dates require dxdata.",
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 2000,
                "description": "Optional maximum result count. Omit to return all matches.",
            },
        },
        "additionalProperties": False,
    },
}

BATCH_SEARCH_TOOL = {
    "name": "batch_search_maimai_songs",
    "description": (
        "Batch version of search_maimai_songs. Processes multiple independent song/chart "
        "searches in one MCP call so callers can preserve local-search context and avoid "
        "one stdio round trip per chart."
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "items": {
                "type": "array",
                "minItems": 1,
                "maxItems": 200,
                "description": "Independent search requests. Each item accepts the same filters as search_maimai_songs plus an optional key.",
                "items": {
                    "type": "object",
                    "properties": {
                        **deepcopy(SEARCH_TOOL["inputSchema"]["properties"]),
                        "key": {
                            "type": "string",
                            "description": "Optional caller-provided key echoed in the response.",
                        },
                    },
                },
            },
        },
        "additionalProperties": False,
    },
}

REFRESH_JOB_STATUS_TOOL = {
    "name": "refresh_maimai_sources_job_status",
    "description": (
        "查询后台刷新任务的进度。接受 refresh_maimai_sources(force=true) 返回的 jobId，"
        "返回当前每个源的刷新状态（running/completed/failed）。"
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "jobId": {
                "type": "string",
                "description": "refresh_maimai_sources(force=true) 返回的 jobId。",
            },
        },
        "required": ["jobId"],
        "additionalProperties": False,
    },
}

_ALIAS_KIND_PROP = {
    "type": "string",
    "enum": ["song", "artist", "charter", "曲师", "谱师"],
    "description": (
        "Which alias dictionary to target. Defaults to 'song' (per-track alias in data/custom_aliases.json). "
        "Use 'artist' (曲师) for data/artist_aliases.json and 'charter' (谱师/note designer) for data/charter_aliases.json — "
        "those make search_maimai_songs's artist/charter filters treat the alias as a synonym of the canonical name."
    ),
}

ADD_ALIAS_TOOL = {
    "name": "add_maimai_alias",
    "description": (
        "Add a local custom alias. Defaults to song aliases (data/custom_aliases.json) — pass kind=artist/charter "
        "to register an artist or note-designer alias instead."
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "kind": _ALIAS_KIND_PROP,
            "song_id": {
                "description": "[kind=song] Song ID.",
                "oneOf": [{"type": "integer"}, {"type": "string"}],
            },
            "title": {
                "type": "string",
                "description": "[kind=song] Exact title to match when song_id is not available.",
            },
            "canonical": {
                "type": "string",
                "description": "[kind=artist|charter] Canonical (official) artist or charter name to anchor the alias to, e.g. 'sasakure.UK' or 'jun'.",
            },
            "alias": {
                "type": "string",
                "description": "Alias to add (e.g. '笹倉' for canonical 'sasakure.UK').",
            },
        },
        "required": ["alias"],
        "additionalProperties": False,
    },
}

DELETE_ALIAS_TOOL = {
    "name": "delete_maimai_alias",
    "description": (
        "Delete a previously-added custom alias. Defaults to song aliases — pass kind=artist/charter "
        "to remove from the corresponding name-alias dictionary."
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "kind": _ALIAS_KIND_PROP,
            "song_id": {
                "description": "[kind=song] Song ID.",
                "oneOf": [{"type": "integer"}, {"type": "string"}],
            },
            "title": {
                "type": "string",
                "description": "[kind=song] Exact title to match when song_id is not available.",
            },
            "canonical": {
                "type": "string",
                "description": "[kind=artist|charter] Canonical name the alias is registered under.",
            },
            "alias": {
                "type": "string",
                "description": "Alias to delete.",
            },
        },
        "required": ["alias"],
        "additionalProperties": False,
    },
}

LIST_ALIASES_TOOL = {
    "name": "list_maimai_aliases",
    "description": (
        "List aliases. Defaults to song aliases (merged from LXNS, Yuzu, legacy CSV, and custom). "
        "Pass kind=artist/charter to dump the corresponding name-alias dictionary; in that case "
        "song_id/title/limit are ignored and only the optional substring `query` filters entries."
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "kind": _ALIAS_KIND_PROP,
            "query": {
                "type": "string",
                "description": "[kind=song] Song ID / title / alias fragment to match. [kind=artist|charter] Optional substring against canonical or alias.",
            },
            "song_id": {
                "description": "[kind=song] Song ID. Used as query when supplied.",
                "oneOf": [{"type": "integer"}, {"type": "string"}],
            },
            "title": {
                "type": "string",
                "description": "[kind=song] Song title or fuzzy title. Used as query when query/song_id is omitted.",
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 200,
                "description": "[kind=song] Maximum number of matched songs to return. Defaults to 20.",
            },
        },
        "additionalProperties": False,
    },
}

REFRESH_SOURCES_TOOL = {
    "name": "refresh_maimai_sources",
    "description": (
        "Manually refresh local maimai data sources. By default it refreshes only sources "
        "older than the 3-day TTL; pass force=true to refresh regardless of age. "
        "Due sources are refreshed concurrently; failed sources are reported individually. "
        "Pass background=true to run refresh in a background thread (like group ranking refresh) "
        "to avoid MCP timeout when refreshing many sources."
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "sources": {
                "description": "Source or sources to refresh: all, lxns, yuzu, divingfish, chart_stats, plate. Defaults to all.",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}],
            },
            "source": {
                "type": "string",
                "description": "Alias of sources for a single source name.",
            },
            "force": {
                "type": "boolean",
                "default": False,
                "description": "Refresh even when source files are newer than ttl_days. When true, automatically uses background mode.",
            },
            "ttl_days": {
                "type": "number",
                "default": 0.0208,
                "description": "Source freshness TTL in days. Defaults to 0.0208 (30 minutes).",
            },
            "check_only": {
                "type": "boolean",
                "default": False,
                "description": "Only report source age and whether refresh is due; do not download.",
            },
            "timeout_seconds": {
                "type": "integer",
                "minimum": 1,
                "default": 30,
                "description": "Per-source command timeout. Sources refresh concurrently.",
            },
            "background": {
                "type": "boolean",
                "default": False,
                "description": "Run refresh in background thread and return immediately. Use for large or force refreshes to avoid MCP timeout.",
            },
        },
        "additionalProperties": False,
    },
}

RANDOM_TOOL = {
    "name": "random_maimai_songs",
    "description": (
        "Pick one or more random songs from the local maimai DX song data. "
        "If no chart filter is supplied, this randomizes by song ID only. "
        "Supplying level, ds, difficulty, or chart type randomizes within matching charts."
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "count": {
                "type": "integer",
                "minimum": 1,
                "maximum": 100,
                "description": "Number of random songs to return. Defaults to 1.",
            },
            "level": {
                "type": "string",
                "description": (
                    "Optional chart level filter, for example 13, 13+, 14+. "
                    "Can be combined with ds, difficulty, and song_type."
                ),
            },
            "genre": {
                "type": "string",
                "description": "Optional song genre/category filter. Can be used with or without level.",
            },
            "version": {
                "type": "string",
                "description": "Optional song/chart version filter. Can be used with or without chart filters.",
            },
            "ds": {
                "description": (
                    "Optional chart constant filter. Supports a number or range such as 13.4-13.8. "
                    "Can be used with or without level."
                ),
                "oneOf": [{"type": "number"}, {"type": "string"}],
            },
            "ds_min": {"type": "number", "description": "Optional minimum chart constant."},
            "ds_max": {"type": "number", "description": "Optional maximum chart constant."},
            "fit_diff": {
                "description": "Optional fitted chart constant. Single values are bucketed by one decimal, e.g. 13.1 means [13.10, 13.20).",
                "oneOf": [{"type": "number"}, {"type": "string"}],
            },
            "fit_diff_min": {"type": "number", "description": "Optional minimum fitted chart constant."},
            "fit_diff_max": {"type": "number", "description": "Optional maximum fitted chart constant."},
            "fit_delta": {
                "description": "Optional actual ds minus fitted constant. Supports exact value or string range.",
                "oneOf": [{"type": "number"}, {"type": "string"}],
            },
            "fit_delta_min": {"type": "number", "description": "Optional minimum ds - fit_diff."},
            "fit_delta_max": {"type": "number", "description": "Optional maximum ds - fit_diff."},
            "fit_label": {"type": "string", "description": "Optional 虚高 or 虚低 filter."},
            "region_has": {
                "description": "Require availability in cn/国服.",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}],
            },
            "region_missing": {
                "description": "Require missing availability in cn/国服.",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}],
            },
            "difficulty": {
                "type": "string",
                "description": "Optional difficulty filter such as Master, Re:MASTER, 紫, or 白.",
            },
            "song_type": {
                "type": "string",
                "description": "Optional chart type filter: standard/SD, dx, or utage/宴.",
                "enum": ["standard", "SD", "sd", "st", "std", "标准", "标", "dx", "DX", "utage1p", "1p", "单人宴", "单人", "utage", "宴", "宴会场", "utage2p", "2p", "双人宴", "合奏宴", "合奏"],
            },
            "artist": {
                "type": "string",
                "description": "Optional song artist filter. Substring match across all sources.",
            },
            "charter": {
                "type": "string",
                "description": "Optional chart designer (谱师) substring filter. Counts as a chart filter, so randomization will run within matching charts.",
            },
            "tag": {
                "description": "Unsupported in this branch: dxrating tag data is not included.",
                "oneOf": [
                    {"type": "string"},
                    {"type": "integer"},
                    {"type": "array", "items": {"oneOf": [{"type": "string"}, {"type": "integer"}]}},
                ],
            },
            "tag_exclude": {
                "description": "Unsupported in this branch: dxrating tag data is not included.",
                "oneOf": [
                    {"type": "string"},
                    {"type": "integer"},
                    {"type": "array", "items": {"oneOf": [{"type": "string"}, {"type": "integer"}]}},
                ],
            },
            "released_after": {
                "type": "string",
                "description": "Unsupported in this branch: per-chart release dates require dxdata.",
            },
            "released_before": {
                "type": "string",
                "description": "Unsupported in this branch: per-chart release dates require dxdata.",
            },
            "seed": {
                "description": "Optional seed for reproducible random picks.",
                "oneOf": [{"type": "integer"}, {"type": "string"}],
            },
        },
        "additionalProperties": False,
    },
}

LIST_BY_ID_TOOL = {
    "name": "list_maimai_songs_by_id",
    "description": (
        "List the first N songs ordered by normalized song ID, ascending or descending. "
        "Optional filters can restrict songs by chart level, chart constant, difficulty, or chart type."
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "order": {
                "type": "string",
                "description": "ID order. Supports asc/desc, 正序/倒序, or 升序/降序. Defaults to asc.",
                "enum": ["asc", "desc", "正序", "倒序", "升序", "降序"],
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 2000,
                "description": "Number of songs to return. Defaults to 20.",
            },
            "level": {
                "type": "string",
                "description": "Optional chart level filter, for example 13, 13+, 14+.",
            },
            "genre": {
                "type": "string",
                "description": "Optional song genre/category filter. Fuzzy substring match.",
            },
            "version": {
                "type": "string",
                "description": "Optional song/chart version filter. Fuzzy substring match.",
            },
            "ds": {
                "description": "Optional chart constant filter. Supports a number or range such as 13.4-13.8.",
                "oneOf": [{"type": "number"}, {"type": "string"}],
            },
            "ds_min": {"type": "number", "description": "Optional minimum chart constant."},
            "ds_max": {"type": "number", "description": "Optional maximum chart constant."},
            "fit_diff": {
                "description": "Optional fitted chart constant filter. Single values are bucketed by one decimal.",
                "oneOf": [{"type": "number"}, {"type": "string"}],
            },
            "fit_diff_min": {"type": "number", "description": "Optional minimum fitted chart constant."},
            "fit_diff_max": {"type": "number", "description": "Optional maximum fitted chart constant."},
            "fit_delta": {
                "description": "Optional actual ds minus fitted constant. Supports exact value or string range.",
                "oneOf": [{"type": "number"}, {"type": "string"}],
            },
            "fit_delta_min": {"type": "number", "description": "Optional minimum ds - fit_diff."},
            "fit_delta_max": {"type": "number", "description": "Optional maximum ds - fit_diff."},
            "fit_label": {"type": "string", "description": "Optional 虚高 or 虚低 filter."},
            "region_has": {
                "description": "Require availability in cn/国服.",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}],
            },
            "region_missing": {
                "description": "Require missing availability in cn/国服.",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}],
            },
            "sort": {
                "type": "string",
                "enum": ["fit_delta_desc", "fit_delta_asc", "fit_diff_asc", "fit_diff_desc", "虚高", "虚低", "最虚高", "最虚低"],
                "description": "Optional fit sorting for filtered ID lists.",
            },
            "difficulty": {
                "type": "string",
                "description": "Optional difficulty filter such as Master, Re:MASTER, 紫, or 白.",
            },
            "song_type": {
                "type": "string",
                "description": "Optional chart type filter: standard/SD, dx, or utage/宴.",
                "enum": ["standard", "SD", "sd", "st", "std", "标准", "标", "dx", "DX", "utage1p", "1p", "单人宴", "单人", "utage", "宴", "宴会场", "utage2p", "2p", "双人宴", "合奏宴", "合奏"],
            },
            "artist": {
                "type": "string",
                "description": "Optional song artist substring filter.",
            },
            "charter": {
                "type": "string",
                "description": "Optional chart designer (谱师) substring filter. Narrows matched_charts.",
            },
            "tag": {
                "description": "Unsupported in this branch: dxrating tag data is not included.",
                "oneOf": [
                    {"type": "string"},
                    {"type": "integer"},
                    {"type": "array", "items": {"oneOf": [{"type": "string"}, {"type": "integer"}]}},
                ],
            },
            "tag_exclude": {
                "description": "Unsupported in this branch: dxrating tag data is not included.",
                "oneOf": [
                    {"type": "string"},
                    {"type": "integer"},
                    {"type": "array", "items": {"oneOf": [{"type": "string"}, {"type": "integer"}]}},
                ],
            },
            "released_after": {
                "type": "string",
                "description": "Unsupported in this branch: per-chart release dates require dxdata.",
            },
            "released_before": {
                "type": "string",
                "description": "Unsupported in this branch: per-chart release dates require dxdata.",
            },
        },
        "additionalProperties": False,
    },
}

TODAY_TOOL = {
    "name": "today_maimai",
    "description": (
        "今日舞萌 / 今日 maimai 运势。按原项目 qqhash 逻辑计算今日人品、宜忌，"
        "并用取完 11 个宜忌值后右移过的 h 从本地完整曲库里选今日推荐歌曲。"
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "qq": {
                "oneOf": [{"type": "integer"}, {"type": "string"}],
                "description": "用于计算今日舞萌的 QQ 号。",
            },
            "botName": {
                "type": "string",
                "description": f"提醒文案里的 Bot 名称。默认 {TODAY_DEFAULT_BOT_NAME}。",
            },
            "offset": {
                "oneOf": [{"type": "integer"}, {"type": "string"}],
                "description": "可选人品偏移值。默认 0；会加到原公式 days 上，让不同 Bot 得到不同结果。",
            },
        },
        "required": ["qq"],
        "additionalProperties": False,
    },
}

LIST_VERSIONS_TOOL = {
    "name": "list_maimai_versions",
    "description": (
        "List all versions present in the local maimai song cache. "
        "Returns version names/codes with song counts, chart counts, data sources, "
        "and the latest CN/LXNS version codes plus their year prefixes."
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Optional fuzzy filter for version name/code, for example PRiSM or 25010.",
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 5000,
                "description": "Optional maximum number of versions to return.",
            },
        },
        "additionalProperties": False,
    },
}


def build_scoring_tools() -> list[dict[str, Any]]:
    tools = deepcopy(SCORING_TOOL_DEFS)
    for tool in tools:
        if tool.get("name") != "find_score_combinations":
            continue
        schema = tool["inputSchema"]
        schema.pop("required", None)
        properties = schema.setdefault("properties", {})
        properties.update(
            {
                "query": {
                    "type": "string",
                    "description": (
                        "Optional song ID, old DX ID, fuzzy title, or alias. "
                        "When note_totals is omitted, the server resolves this to one chart and uses its note totals."
                    ),
                },
                "song_id": {
                    "oneOf": [{"type": "integer"}, {"type": "string"}],
                    "description": "Optional explicit song ID, used like query when note_totals is omitted.",
                },
                "title": {
                    "type": "string",
                    "description": "Optional exact/fuzzy title, used like query when note_totals is omitted.",
                },
                "level": {
                    "type": "string",
                    "description": "Optional chart level filter for direct song lookup, for example 13+.",
                },
                "genre": {
                    "type": "string",
                    "description": "Optional genre/category filter for direct song lookup.",
                },
                "version": {
                    "type": "string",
                    "description": "Optional song/chart version filter for direct song lookup.",
                },
                "ds": {
                    "oneOf": [{"type": "number"}, {"type": "string"}],
                    "description": "Optional chart constant or range for direct song lookup.",
                },
                "ds_min": {
                    "type": "number",
                    "description": "Optional minimum chart constant for direct song lookup.",
                },
                "ds_max": {
                    "type": "number",
                    "description": "Optional maximum chart constant for direct song lookup.",
                },
                "fit_diff": {
                    "oneOf": [{"type": "number"}, {"type": "string"}],
                    "description": "Optional fitted chart constant filter for direct song lookup.",
                },
                "fit_diff_min": {
                    "type": "number",
                    "description": "Optional minimum fitted chart constant for direct song lookup.",
                },
                "fit_diff_max": {
                    "type": "number",
                    "description": "Optional maximum fitted chart constant for direct song lookup.",
                },
                "fit_delta": {
                    "oneOf": [{"type": "number"}, {"type": "string"}],
                    "description": "Optional ds - fit_diff filter for direct song lookup.",
                },
                "fit_delta_min": {
                    "type": "number",
                    "description": "Optional minimum ds - fit_diff for direct song lookup.",
                },
                "fit_delta_max": {
                    "type": "number",
                    "description": "Optional maximum ds - fit_diff for direct song lookup.",
                },
                "fit_label": {
                    "type": "string",
                    "description": "Optional 虚高 or 虚低 filter for direct song lookup.",
                },
                "region_has": {
                    "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}],
                    "description": "Optional required region availability for direct song lookup.",
                },
                "region_missing": {
                    "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}],
                    "description": "Optional required missing region availability for direct song lookup.",
                },
                "difficulty": {
                    "type": "string",
                    "description": "Optional difficulty filter for direct song lookup.",
                },
                "song_type": {
                    "type": "string",
                    "description": "Optional chart type filter for direct song lookup: standard/SD, dx, or utage/宴.",
                    "enum": ["standard", "SD", "sd", "st", "std", "标准", "标", "dx", "DX", "utage1p", "1p", "单人宴", "单人", "utage", "宴", "宴会场", "utage2p", "2p", "双人宴", "合奏宴", "合奏"],
                },
                "artist": {
                    "type": "string",
                    "description": "Optional artist substring filter for direct song lookup.",
                },
                "charter": {
                    "type": "string",
                    "description": "Optional chart designer (谱师) substring filter for direct song lookup.",
                },
            }
        )
        tool["description"] += (
            " If note_totals is omitted, query/song_id/title can be supplied; the server "
            "searches the local song cache, calculates only when exactly one song and one chart match, "
            "and otherwise returns candidate songs/charts without score calculation."
        )
    return tools


def add_output_format_options(tool: dict[str, Any]) -> dict[str, Any]:
    schema = tool.setdefault("inputSchema", {}).setdefault("properties", {})
    schema.update(deepcopy(OUTPUT_FORMAT_PROPERTIES))
    return tool


SCORING_TOOLS = build_scoring_tools()
TOOLS = [
    SEARCH_TOOL,
    BATCH_SEARCH_TOOL,
    ADD_ALIAS_TOOL,
    DELETE_ALIAS_TOOL,
    LIST_ALIASES_TOOL,
    REFRESH_SOURCES_TOOL,
    REFRESH_JOB_STATUS_TOOL,
    RANDOM_TOOL,
    TODAY_TOOL,
    LIST_BY_ID_TOOL,
    LIST_VERSIONS_TOOL,
    *SCORING_TOOLS,
]
for tool_definition in TOOLS:
    add_output_format_options(tool_definition)


def write_message(message: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(message, ensure_ascii=False, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def success(message_id: Any, result: dict[str, Any]) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": message_id, "result": result}


def error(message_id: Any, code: int, message: str, data: Any = None) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "jsonrpc": "2.0",
        "id": message_id,
        "error": {"code": code, "message": message},
    }
    if data is not None:
        payload["error"]["data"] = data
    return payload


def handle_initialize(message_id: Any, params: dict[str, Any] | None) -> dict[str, Any]:
    requested_version = (params or {}).get("protocolVersion") or "2024-11-05"
    return success(
        message_id,
        {
            "protocolVersion": requested_version,
            "capabilities": {"tools": {"listChanged": False}},
            "serverInfo": {"name": SERVER_NAME, "version": __version__},
        },
    )


SCORING_LOOKUP_KEYS = {
    "query",
    "song_id",
    "title",
    "level",
    "genre",
    "version",
    "ds",
    "ds_min",
    "ds_max",
    "fit_diff",
    "fit_diff_min",
    "fit_diff_max",
    "fit_delta",
    "fit_delta_min",
    "fit_delta_max",
    "fit_label",
    "region_has",
    "region_missing",
    "difficulty",
    "song_type",
}


def clean_arguments(arguments: dict[str, Any]) -> dict[str, Any]:
    return {
        key: value
        for key, value in arguments.items()
        if key not in OUTPUT_ARGUMENT_KEYS
    }


def wants_raw_json(arguments: dict[str, Any] | None) -> bool:
    arguments = arguments or {}
    requested_format = str(arguments.get("format", "")).strip().lower()
    return (
        requested_format == "json"
        or bool(arguments.get("include_raw"))
        or bool(arguments.get("debug"))
    )


def wants_compact(arguments: dict[str, Any] | None) -> bool:
    if not arguments:
        return False
    return str(arguments.get("format", "")).strip().lower() == "compact"


def fmt(value: Any, fallback: str = "-") -> str:
    if value in (None, "", [], {}):
        return fallback
    if isinstance(value, float):
        return f"{value:.4f}".rstrip("0").rstrip(".")
    return str(value)


def format_ds(value: Any, fallback: str = "-") -> str:
    """格式化谱面定数；整数定数也保留一位小数。"""
    if value in (None, "", [], {}):
        return fallback
    try:
        numeric = float(value)
    except (TypeError, ValueError):
        return str(value)
    text = f"{numeric:.4f}".rstrip("0").rstrip(".")
    return f"{text}.0" if numeric.is_integer() else text


def format_regions(regions: dict[str, Any] | None) -> str:
    if not isinstance(regions, dict):
        return "-"
    label_map = {"cn": "国服"}
    active = [label_map[key] for key in ("cn",) if regions.get(key)]
    return "/".join(active) if active else "-"


def format_notes(notes: Any) -> str:
    counts = flatten_note_counts(notes)
    if not counts["total"] and not any(counts[key] for key in ("tap", "hold", "slide", "touch", "break")):
        return "-"
    return (
        f"合计 {counts['total']} "
        f"(Tap {counts['tap']}, Hold {counts['hold']}, Slide {counts['slide']}, "
        f"Touch {counts['touch']}, Break {counts['break']})"
    )


def format_aliases(aliases: list[Any], limit: int = 8) -> str:
    values = unique_preserve_order([to_simplified(str(alias)) for alias in aliases or [] if str(alias).strip()])
    if not values:
        return ""
    suffix = f" (+{len(values) - limit})" if len(values) > limit else ""
    return "别名: " + " / ".join(values[:limit]) + suffix


MATCH_MODE_LABELS = {"exact": "精确", "prefix": "前缀", "contains": "包含"}
MATCH_FIELD_LABELS = {
    "song_id": "歌曲ID",
    "source_id": "源ID",
    "title": "歌名",
    "alias": "别名",
    "pinyin": "拼音",
    "keyword": "关键字",
}


def format_match(match: Any) -> str:
    if not isinstance(match, dict):
        return ""
    field = str(match.get("field") or "")
    if field in {"", "none", "unknown"}:
        return ""
    label = MATCH_FIELD_LABELS.get(field)
    if not label:
        raw_label = str(match.get("label") or "").strip()
        label = raw_label[:-2] if raw_label.endswith("命中") else raw_label
    if not label:
        return ""
    mode_label = MATCH_MODE_LABELS.get(str(match.get("mode") or ""))
    value = to_simplified(str(match.get("value") or "")).strip()
    suffix = f": {value}" if value else ""
    mode = f"({mode_label})" if mode_label else ""
    return f"命中 {label}{mode}{suffix}"


def format_source_differences(song: dict[str, Any]) -> str:
    source_fields = song.get("source_fields")
    if not isinstance(source_fields, dict) or len(source_fields) < 2:
        return ""
    # 收集每个字段在各源的值
    field_values: dict[str, dict[str, str]] = {}
    for label in ("cn", "divingfish"):
        fields = source_fields.get(label)
        if not isinstance(fields, dict):
            continue
        for key in ("version", "genre", "release_date", "is_new"):
            val = fields.get(key)
            if val in (None, ""):
                continue
            if key not in field_values:
                field_values[key] = {}
            field_values[key][label] = str(val)
        # ds 特殊处理
        ds = fields.get("ds")
        if ds:
            if "ds" not in field_values:
                field_values["ds"] = {}
            field_values["ds"][label] = "/".join(format_ds(v) for v in ds)
    # 只显示有差异的字段
    pieces: list[str] = []
    for key, values in field_values.items():
        unique = set(values.values())
        if len(unique) <= 1:
            continue  # 所有源值相同，不显示
        label_map = {"cn": "落雪国服源", "divingfish": "水鱼国服源"}
        field_label = {"version": "版本", "genre": "流派", "release_date": "上线日期", "is_new": "新曲", "ds": "定数"}.get(key, key)
        parts = [f"{label_map.get(l, l)} {v}" for l, v in values.items()]
        pieces.append(f"{field_label}: " + ", ".join(parts))
    return "源差异: " + "；".join(pieces) if pieces else ""


CHART_SOURCE_LABELS = {"cn": "落雪国服源", "divingfish": "水鱼国服源"}
CHART_TYPE_LABELS = {"standard": "ST", "st": "ST", "sd": "ST", "dx": "DX"}


def format_chart_type(value: Any) -> str:
    text = str(value or "").strip()
    if not text:
        return "-"
    return CHART_TYPE_LABELS.get(text.lower(), text.upper())


def format_chart_type_list(values: list[Any]) -> str:
    labels = unique_preserve_order([format_chart_type(v) for v in values if v not in (None, "")])
    labels.sort(key=lambda t: {"ST": 0, "DX": 1}.get(t, 9))
    return "/".join(labels)


def numeric_text(value: Any) -> str:
    try:
        return str(int(value))
    except (TypeError, ValueError):
        return ""


def chart_display_id(song: dict[str, Any], chart: dict[str, Any]) -> str:
    chart_type = format_chart_type(chart.get("chart_type"))
    for key in ("chart_id", "music_id", "musicId", "internal_id"):
        chart_id = numeric_text(chart.get(key))
        if not chart_id:
            continue
        if chart_type != "DX" or int(chart_id) > 10000:
            return chart_id

    base_id = numeric_text(song.get("id"))
    if not base_id:
        return ""
    if chart_type == "DX":
        base_number = int(base_id)
        return str(base_number if base_number > 10000 else base_number + 10000)
    return base_id


def chart_ids_by_type(song: dict[str, Any]) -> dict[str, str]:
    ids: dict[str, str] = {}
    for chart in dedupe_charts_by_difficulty(song.get("matched_charts") or []):
        chart_type = format_chart_type(chart.get("chart_type"))
        if chart_type in ids:
            continue
        chart_id = chart_display_id(song, chart)
        if chart_id:
            ids[chart_type] = chart_id
    return dict(sorted(ids.items(), key=lambda item: {"ST": 0, "DX": 1}.get(item[0], 9)))


def format_song_id(song: dict[str, Any], *, compact: bool = False) -> str:
    ids = chart_ids_by_type(song)
    if not ids:
        song_id = fmt(song.get("id"))
        return f"#{song_id}" if compact else song_id
    unique_ids = unique_preserve_order(ids.values())
    if len(unique_ids) == 1:
        return f"#{unique_ids[0]}" if compact else unique_ids[0]
    if compact:
        return "ID " + " / ".join(f"{chart_type}#{chart_id}" for chart_type, chart_id in ids.items())
    return " / ".join(f"{chart_type} {chart_id}" for chart_type, chart_id in ids.items())


def format_chart(chart: dict[str, Any], song: dict[str, Any] | None = None) -> str:
    source = CHART_SOURCE_LABELS.get(chart.get("source"), fmt(chart.get("source")).upper())
    chart_type = format_chart_type(chart.get("chart_type"))
    chart_id = chart_display_id(song or {}, chart)
    chart_type_piece = f"{chart_type}#{chart_id}" if chart_id else chart_type
    difficulty = fmt(chart.get("difficulty"))
    fit_diff = format_ds(chart.get("fit_diff"))
    fit_delta = fmt(chart.get("fit_delta"))
    fit_label = fmt(chart.get("fit_label"), "")
    fit_piece = f"拟合 {fit_diff}, 差值 {fit_delta}"
    if fit_label:
        fit_piece += f", {fit_label}"
    extras: list[str] = []
    if chart.get("version") not in (None, ""):
        extras.append(f"版本 {chart['version']}")
    if chart.get("is_buddy"):
        extras.append("双人谱")
    kanji = chart.get("kanji")
    if isinstance(kanji, str) and kanji:
        extras.append(f"字标 {kanji}")
    description = chart.get("description")
    if isinstance(description, str) and description:
        desc = description if len(description) <= 40 else description[:40] + "…"
        extras.append(f"说明 {desc}")
    extras_piece = f" | {' '.join(extras)}" if extras else ""
    return (
        f"- {source} {chart_type_piece} {difficulty} 等级 {fmt(chart.get('level'))} "
        f"定数 {format_ds(chart.get('ds'))} | {fit_piece} | {format_notes(chart.get('notes'))} | "
        f"谱师 {fmt(chart.get('charter'))}{extras_piece}"
    )


SONG_SOURCE_LABELS = {
    "lxns": "落雪国服源",
    "cndivingfish": "水鱼国服源",
}


def format_song(song: dict[str, Any], index: int | None = None) -> str:
    prefix = f"{index}. " if index is not None else ""
    raw_source = song.get("source", "")
    source_display = "+".join(SONG_SOURCE_LABELS.get(s, s) for s in raw_source.split("+"))
    lines = [
        (
            f"{prefix}{fmt(song.get('title'))} | 编号 {format_song_id(song)} | "
            f"{fmt(song.get('artist'))} | 来源 {source_display} | "
            f"地区 {format_regions(song.get('regions'))} | 版本 {fmt(song.get('version'))} | "
            f"BPM {fmt(song.get('bpm'))}"
        )
    ]
    meta_pieces: list[str] = []
    if song.get("release_date") not in (None, ""):
        meta_pieces.append(f"上线日期 {song['release_date']}")
    if song.get("is_new") is True:
        meta_pieces.append("新曲")
    if song.get("genre") not in (None, ""):
        meta_pieces.append(f"流派 {song['genre']}")
    chart_types = song.get("available_chart_types") or []
    if chart_types:
        meta_pieces.append("谱面 " + format_chart_type_list(chart_types))
    match_line = format_match(song.get("match"))
    if match_line:
        meta_pieces.append(match_line)
    if meta_pieces:
        lines.append("  " + " | ".join(meta_pieces))
    alias_line = format_aliases(song.get("aliases") or [])
    if alias_line:
        lines.append(f"  {alias_line}")
    source_line = format_source_differences(song)
    if source_line:
        lines.append(f"  {source_line}")
    charts = song.get("matched_charts") or []
    for chart in charts[:12]:
        lines.append("  " + format_chart(chart, song))
    if len(charts) > 12:
        lines.append(f"  ... 还有 {len(charts) - 12} 张谱面未展开")
    return "\n".join(lines)


def criteria_summary(criteria: dict[str, Any] | None) -> str:
    if not isinstance(criteria, dict):
        return "-"
    pieces = [
        f"{key}={value}"
        for key, value in criteria.items()
        if value not in (None, "", [], {})
    ]
    return ", ".join(pieces) if pieces else "-"


def format_song_result_set(title: str, result: dict[str, Any], *, compact: bool = False) -> str:
    header_pieces = [f"{title}: 返回 {result.get('count', 0)} / {result.get('total_matches', result.get('total_candidates', 0))}"]
    if result.get("truncated"):
        header_pieces[-1] += "，已截断（加 limit 看更多）"
    lines = [
        " ".join(header_pieces),
        f"条件: {criteria_summary(result.get('criteria'))}",
    ]
    songs = result.get("songs") or []
    if not songs:
        lines.append("没有匹配歌曲。")
        return "\n".join(lines)
    formatter = format_song_compact if compact else format_song
    for index, song in enumerate(songs, 1):
        lines.append(formatter(song, index))
    return "\n".join(lines)


# 谱面去重 + 紧凑行：按难度压缩为 4-5 行。
DIFFICULTY_SHORT = {"Basic": "Bas", "Advanced": "Adv", "Expert": "Exp", "Master": "Mst", "Re:MASTER": "ReM"}
SOURCE_PRIORITY_ORDER = ("cn", "divingfish")


def dedupe_charts_by_difficulty(charts: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """每个 (chart_type, difficulty_index) 只保留一张，按源优先级 cn > divingfish。

    返回值已经按 (chart_type, difficulty_index) 排序，方便 compact 输出。
    """
    if not charts:
        return []
    best: dict[tuple[str, int], dict[str, Any]] = {}
    for chart in charts:
        if not isinstance(chart, dict):
            continue
        key = (str(chart.get("chart_type") or ""), int(chart.get("difficulty_index") or 0))
        existing = best.get(key)
        if existing is None:
            best[key] = chart
            continue
        new_source = str(chart.get("source") or "")
        old_source = str(existing.get("source") or "")
        try:
            if SOURCE_PRIORITY_ORDER.index(new_source) < SOURCE_PRIORITY_ORDER.index(old_source):
                best[key] = chart
        except ValueError:
            continue
    return sorted(
        best.values(),
        key=lambda c: (str(c.get("chart_type") or ""), int(c.get("difficulty_index") or 0)),
    )


def format_chart_compact(chart: dict[str, Any]) -> str:
    """单张谱面的一段（用在难度行里）：`Mst 14/14.0 (拟合 14.38 虚低)`。"""
    difficulty = chart.get("difficulty") or ""
    short = DIFFICULTY_SHORT.get(str(difficulty), str(difficulty)[:3])
    level = fmt(chart.get("level"))
    ds = format_ds(chart.get("ds"))
    fit_diff = chart.get("fit_diff")
    fit_label = chart.get("fit_label")
    fit_piece = ""
    if isinstance(fit_diff, (int, float)):
        label_part = f" {fit_label}" if isinstance(fit_label, str) and fit_label else ""
        fit_piece = f" (拟合 {fit_diff:.2f}{label_part})"
    # level 是等级标签，ds 是定数；即使数值相同也要分别显示。
    head = level if ds == "-" else f"{level}/{ds}"
    return f"{short} {head}{fit_piece}"


def format_song_compact(song: dict[str, Any], index: int | None = None) -> str:
    """单首歌的紧凑视图：
    第一行：标题 + ID + 艺人 + BPM + 版本
    第二行：4-5 个难度行内并排（Bas 7 / Adv 8+/8.9 / Exp 13/13.3 (拟合 12.99 虚高) / Mst 14/14.0 (拟合 14.38 虚低)）
    第三行（可选）：别名（最多 5 个）
    """
    prefix = f"{index}. " if index is not None else ""
    title = fmt(song.get("title"))
    sid = format_song_id(song, compact=True)
    artist = fmt(song.get("artist"))
    bpm = fmt(song.get("bpm"))
    version = fmt(song.get("version"))
    chart_types = song.get("available_chart_types") or []
    chart_type_piece = ""
    if chart_types:
        chart_type_piece = " | 谱面 " + format_chart_type_list(chart_types)
    head = f"{prefix}{title} | {sid} | {artist} | {bpm} BPM | v{version}{chart_type_piece}"
    match_line = format_match(song.get("match"))
    if match_line:
        head += f" | {match_line}"

    extras: list[str] = []
    if song.get("is_new") is True:
        extras.append("新曲")
    if song.get("is_locked") is True:
        extras.append("锁定")
    image_url = song.get("image_url")
    # is_locked / 新曲 / image_url 不是必显项，紧凑模式默认隐藏；新曲单独标
    lines = [head]
    if extras:
        lines[-1] += "  [" + " ".join(extras) + "]"

    charts = dedupe_charts_by_difficulty(song.get("matched_charts") or [])
    if charts:
        grouped: dict[str, list[dict[str, Any]]] = {}
        for chart in charts:
            grouped.setdefault(format_chart_type(chart.get("chart_type")), []).append(chart)
        for chart_type in sorted(grouped, key=lambda t: {"ST": 0, "DX": 1}.get(t, 9)):
            chart_id = chart_display_id(song, grouped[chart_type][0])
            chart_type_piece = f"{chart_type} #{chart_id}" if chart_id else chart_type
            lines.append(f"  {chart_type_piece}: " + " / ".join(format_chart_compact(c) for c in grouped[chart_type]))

    aliases = song.get("aliases") or []
    if aliases:
        shown = aliases[:5]
        suffix = f" (+{len(aliases) - 5})" if len(aliases) > 5 else ""
        lines.append("  别名: " + " / ".join(shown) + suffix)

    return "\n".join(lines)


def format_batch_search_result(result: dict[str, Any]) -> str:
    counts = result.get("counts") or {}
    lines = [
        (
            "批量搜索结果: "
            f"请求 {counts.get('requested', 0)}，成功 {counts.get('success', 0)}，失败 {counts.get('failure', 0)}"
        )
    ]
    for item in (result.get("items") or [])[:50]:
        if not isinstance(item, dict):
            continue
        key = item.get("key") or item.get("index")
        if not item.get("ok"):
            error_info = item.get("error") if isinstance(item.get("error"), dict) else {}
            lines.append(f"- {key}: ERROR {error_info.get('message', '搜索失败')}")
            continue
        search_result = item.get("result") if isinstance(item.get("result"), dict) else {}
        lines.append(
            f"- {key}: 返回 {search_result.get('count', 0)} / {search_result.get('total_matches', 0)}"
        )
    if len(result.get("items") or []) > 50:
        lines.append(f"... 还有 {len(result.get('items') or []) - 50} 项未展开")
    return "\n".join(lines)


def format_versions_result(result: dict[str, Any]) -> str:
    lines = [
        f"版本列表: 返回 {result.get('count', 0)} / {result.get('total_matches', 0)}，truncated={result.get('truncated', False)}",
        f"条件: {criteria_summary(result.get('criteria'))}",
    ]
    latest_cn_versions = result.get("latest_cn_versions") or []
    latest_cn_years = result.get("latest_cn_years") or []
    if latest_cn_versions:
        year_text = " / ".join(f"{year} / 20{year}" for year in latest_cn_years)
        lines.append(
            f"国服最新版本号: {' / '.join(str(version) for version in latest_cn_versions)}"
            f"；按年份查传 version={year_text}"
        )
    for item in result.get("versions") or []:
        sources = "/".join(item.get("sources") or [])
        lines.append(
            f"- {fmt(item.get('version'))} | songs {fmt(item.get('song_count'))} | "
            f"charts {fmt(item.get('chart_count'))} | sources {sources or '-'}"
        )
    return "\n".join(lines)


_KIND_LABEL = {"artist": "曲师", "charter": "谱师"}


def _is_name_alias_result(result: dict[str, Any]) -> bool:
    return result.get("kind") in _KIND_LABEL


def format_alias_result(result: dict[str, Any]) -> str:
    status = "已存在" if result.get("existed") else "已新增"
    warning = result.get("warning")
    if _is_name_alias_result(result):
        label = _KIND_LABEL[result["kind"]]
        lines = [
            f"{label}别名{status}: {fmt(result.get('alias'))} -> {fmt(result.get('canonical'))}",
            f"该 {label} 当前别名: {', '.join(result.get('aliases') or []) or '无'}",
            f"保存位置: {fmt(result.get('document'))}",
        ]
        if warning:
            lines.append(f"⚠ {warning}")
        return "\n".join(lines)
    lines = [
        f"别名{status}: {fmt(result.get('alias'))} -> {fmt(result.get('title'))} (ID {fmt(result.get('song_id'))})",
        f"保存位置: {fmt(result.get('document'))}",
    ]
    if warning:
        lines.append(f"⚠ {warning}")
    return "\n".join(lines)


def format_delete_alias_result(result: dict[str, Any]) -> str:
    if _is_name_alias_result(result):
        label = _KIND_LABEL[result["kind"]]
        return (
            f"已删除{label}别名: {fmt(result.get('removed_alias'))} "
            f"({label}: {fmt(result.get('canonical'))})\n"
            f"剩余别名: {', '.join(result.get('remaining_aliases') or []) or '无'}\n"
            f"保存位置: {fmt(result.get('document'))}"
        )
    return (
        f"已删除别名: {fmt(result.get('removed_alias'))} "
        f"(歌曲: {fmt(result.get('title'))}, ID {fmt(result.get('song_id'))})\n"
        f"剩余别名: {', '.join(result.get('remaining_aliases') or []) or '无'}\n"
        f"保存位置: {fmt(result.get('document'))}"
    )


def format_alias_list_result(result: dict[str, Any]) -> str:
    if _is_name_alias_result(result):
        label = _KIND_LABEL[result["kind"]]
        entries = result.get("entries") or []
        lines = [f"{label}别名词典: 共 {result.get('count', 0)} 条 (保存位置: {fmt(result.get('document'))})"]
        if not entries:
            lines.append(f"暂无{label}别名。")
        for index, entry in enumerate(entries, 1):
            aliases = entry.get("aliases") or []
            lines.append(
                f"{index}. {fmt(entry.get('canonical'))} → {', '.join(aliases) or '无'}"
            )
        return "\n".join(lines)
    lines = [
        f"别名列表: 返回 {result.get('count', 0)} / {result.get('total_matches', 0)}，truncated={result.get('truncated', False)}",
        f"条件: {criteria_summary(result.get('criteria'))}",
    ]
    songs = result.get("songs") or []
    if not songs:
        lines.append("没有匹配歌曲。")
        return "\n".join(lines)
    for index, song in enumerate(songs, 1):
        aliases = unique_preserve_order([str(alias) for alias in song.get("aliases") or [] if str(alias).strip()])
        lines.append(
            f"{index}. {fmt(song.get('title'))} | ID {fmt(song.get('id'))} | "
            f"source {fmt(song.get('source'))} | 别名数 {len(aliases)}"
        )
        lines.append("  " + (" / ".join(aliases) if aliases else "无别名"))
    return "\n".join(lines)


def _format_bg_refresh(result: dict[str, Any]) -> str:
    """格式化后台刷新结果"""
    lines = [
        f"后台刷新: jobId={result.get('jobId')} status={result.get('status')}",
        f"过期 {result.get('dueSources', '?')}/{result.get('totalSources', '?')} 源，刷新中...",
    ]
    for source, state in (result.get("sourceStates") or {}).items():
        tag = "🔄" if state.get("expired") else "✅"
        lines.append(
            f"  {tag} {source} ({state.get('label', source)}): "
            f"过期={state.get('expired')} 年龄={fmt(state.get('age_days'))}天"
        )
    lines.append(f"> {result.get('message', '')}")
    return "\n".join(lines)


def format_bg_job_status(result: dict[str, Any]) -> str:
    """格式化后台刷新进度查询结果"""
    if result.get("error"):
        return f"错误: {result['error']}"

    status = result.get("status", "unknown")
    lines = [
        f"刷新进度: status={status} "
        f"completed={result.get('completedSources', '?')}/{result.get('totalSources', '?')}",
        f"成功: {', '.join(result.get('succeededSources', [])) or '-'}",
        f"失败: {', '.join(result.get('failedSources', [])) or '-'}",
        f"message: {result.get('message', '')}",
    ]
    for source, cmd in (result.get("sources") or {}).items():
        tag = "✅" if cmd.get("returncode") == 0 else "❌"
        dur = fmt(cmd.get("duration_seconds"))
        lines.append(f"  {tag} {source}: rc={cmd.get('returncode')} dur={dur}s")
    return "\n".join(lines)


def format_refresh_result(result: dict[str, Any]) -> str:
    if result.get("background"):
        return _format_bg_refresh(result)
    lines = [
        f"源刷新: force={result.get('force')} check_only={result.get('check_only')} ttl_days={fmt(result.get('ttl_days'))}",
        f"应刷新: {', '.join(result.get('due_sources') or []) or '-'}",
        f"已刷新: {', '.join(result.get('refreshed_sources') or []) or '-'}",
        f"已跳过: {', '.join(result.get('skipped_sources') or []) or '-'}",
        f"失败: {', '.join(result.get('failed_sources') or []) or '-'}",
    ]
    for source, status in (result.get("sources") or {}).items():
        lines.append(
            f"- {source}: expired={status.get('expired')} age_days={fmt(status.get('age_days'))} "
            f"mtime={fmt(status.get('mtime'))}"
        )
    for command in result.get("commands") or []:
        if command.get("error"):
            lines.append(f"! {command.get('source')}: {command.get('error')}")
            continue
        lines.append(
            f"+ {command.get('source')}: returncode={command.get('returncode')} "
            f"duration={fmt(command.get('duration_seconds'))}s"
        )
        if command.get("stdout"):
            lines.append("  stdout: " + str(command.get("stdout")).replace("\n", " | "))
        if command.get("stderr"):
            lines.append("  stderr: " + str(command.get("stderr")).replace("\n", " | "))
    return "\n".join(lines)


def format_percent_details(value: Any) -> str:
    if not isinstance(value, dict):
        return fmt(value)
    return (
        f"{fmt(value.get('display_floor'))} floor / "
        f"{fmt(value.get('display_half_up'))} half-up "
        f"(raw {fmt(value.get('raw'))})"
    )


def format_score_counts_result(result: dict[str, Any]) -> str:
    totals = result.get("totals") or {}
    lines = [
        "计分结果:",
        f"- oldscore: {fmt(totals.get('oldscore'))}",
        f"- oldacc: {format_percent_details(totals.get('oldacc'))}",
        f"- dxscore: {fmt(totals.get('dxscore'))}",
        f"- dxacc: {format_percent_details(totals.get('dxacc'))}",
        f"- base: {fmt(totals.get('base'))}",
        f"- break_bonus: {fmt(totals.get('break_bonus'))}",
    ]
    note_totals = result.get("note_totals") or {}
    if note_totals:
        lines.append(
            "物量: "
            + ", ".join(f"{key} {value}" for key, value in note_totals.items())
        )
    rows = result.get("rows") or []
    for row in rows[:20]:
        contribution = row.get("contribution") or {}
        lines.append(
            f"- {fmt(row.get('note_type'))}.{fmt(row.get('judgment'))} x{fmt(row.get('count'))}: "
            f"oldscore {fmt(contribution.get('oldscore'))}, dxscore {fmt(contribution.get('dxscore'))}, "
            f"base {fmt(contribution.get('base'))}, bonus {fmt(contribution.get('break_bonus'))}"
        )
    if len(rows) > 20:
        lines.append(f"... 还有 {len(rows) - 20} 行明细未展开")
    return "\n".join(lines)


def format_counts(counts: dict[str, Any]) -> str:
    parts: list[str] = []
    for note_type, judgments in counts.items():
        if not isinstance(judgments, dict) or not judgments:
            continue
        inner = ", ".join(f"{judgment}:{count}" for judgment, count in judgments.items())
        parts.append(f"{note_type}({inner})")
    return "; ".join(parts) if parts else "-"


def format_lookup_failure(result: dict[str, Any]) -> str:
    reason = result.get("reason")
    lines = [f"未计算: {fmt(reason)}", f"条件: {criteria_summary(result.get('criteria'))}"]
    if result.get("songs"):
        lines.append("候选歌曲:")
        for song in result.get("songs", [])[:20]:
            lines.append(
                f"- {fmt(song.get('title'))} | ID {fmt(song.get('id'))} | charts {fmt(song.get('matched_chart_count'))}"
            )
    if result.get("charts"):
        lines.append("候选谱面:")
        for chart in result.get("charts", [])[:20]:
            lines.append(format_chart(chart))
    return "\n".join(lines)


def format_find_score_result(result: dict[str, Any]) -> str:
    if result.get("resolved") is False or result.get("calculated") is False and result.get("reason"):
        return format_lookup_failure(result)
    lines = [
        f"反查结果: found={result.get('found')} mode={fmt(result.get('score_mode'))} truncated={result.get('truncated', False)}",
        f"匹配分数数: {fmt(result.get('matching_score_count'))}; 返回组合: {fmt(result.get('returned_solution_count'))}; 组合总数: {fmt(result.get('matching_combination_count'))}",
    ]
    if result.get("lookup"):
        lookup = result["lookup"]
        song = lookup.get("song") or {}
        chart = lookup.get("chart") or {}
        lines.append(
            f"谱面: {fmt(song.get('title'))} ID {fmt(song.get('id'))} | "
            f"{fmt(chart.get('chart_type')).upper()} {fmt(chart.get('difficulty'))} Lv {fmt(chart.get('level'))} ds {fmt(chart.get('ds'))}"
        )
    if result.get("target_range"):
        lines.append(f"目标: {criteria_summary(result.get('target_range'))}")
    if result.get("target_acc_range"):
        lines.append(f"目标达成率: {criteria_summary(result.get('target_acc_range'))}")
    if result.get("shortcut_constraints"):
        lines.append(f"快捷约束: {criteria_summary(result.get('shortcut_constraints'))}")
    for index, solution in enumerate(result.get("solutions") or [], 1):
        solution_score = solution.get("score")
        mode_value = solution.get(result.get("score_mode"))
        if solution_score is None and isinstance(mode_value, dict):
            solution_score = mode_value.get("display_floor") or mode_value.get("raw")
        lines.append(
            f"{index}. score={fmt(solution_score)} "
            f"counts: {format_counts(solution.get('counts') or {})}"
        )
    if not result.get("solutions"):
        lines.append("没有找到满足条件的判定组合。")
    return "\n".join(lines)


def format_tool_result(tool_name: str, result: dict[str, Any], *, compact: bool = False) -> str:
    if tool_name == "today_maimai":
        return str(result.get("text") or "")
    if tool_name == "search_maimai_songs":
        return format_song_result_set("搜索结果", result, compact=compact)
    if tool_name == "batch_search_maimai_songs":
        return format_batch_search_result(result)
    if tool_name == "random_maimai_songs":
        return format_song_result_set("随机结果", result, compact=compact)
    if tool_name == "list_maimai_songs_by_id":
        return format_song_result_set("ID 列表", result, compact=compact)
    if tool_name == "list_maimai_versions":
        return format_versions_result(result)
    if tool_name == "add_maimai_alias":
        return format_alias_result(result)
    if tool_name == "delete_maimai_alias":
        return format_delete_alias_result(result)
    if tool_name == "list_maimai_aliases":
        return format_alias_list_result(result)
    if tool_name == "refresh_maimai_sources":
        return format_refresh_result(result)
    if tool_name == "refresh_maimai_sources_job_status":
        return format_bg_job_status(result)
    if tool_name == "score_counts":
        return format_score_counts_result(result)
    if tool_name == "find_score_combinations":
        return format_find_score_result(result)
    return json.dumps(result, ensure_ascii=False, indent=2)


def result_content(
    message_id: Any,
    result: dict[str, Any],
    *,
    tool_name: str,
    arguments: dict[str, Any] | None = None,
    is_error: bool = False,
) -> dict[str, Any]:
    text = (
        json.dumps(result, ensure_ascii=False, indent=2)
        if wants_raw_json(arguments)
        else format_tool_result(tool_name, result, compact=wants_compact(arguments))
    )
    return success(
        message_id,
        {"content": [{"type": "text", "text": text}], "isError": is_error},
    )


def text_error_content(message_id: Any, exc: Exception) -> dict[str, Any]:
    return success(
        message_id,
        {"content": [{"type": "text", "text": str(exc)}], "isError": True},
    )


def direct_lookup_query(arguments: dict[str, Any]) -> str | None:
    for key in ("query", "song_id", "title"):
        value = arguments.get(key)
        if value not in (None, ""):
            return str(value)
    return None


def find_score_combinations_with_optional_lookup(arguments: dict[str, Any]) -> dict[str, Any]:
    if arguments.get("note_totals") is not None:
        scoring_arguments = {
            key: value
            for key, value in arguments.items()
            if key not in SCORING_LOOKUP_KEYS
        }
        return find_score_combinations(scoring_arguments)

    query = direct_lookup_query(arguments)
    if query is None:
        raise ScoringError("note_totals or query/song_id/title is required")

    resolution = resolve_single_chart_note_totals(
        query=query,
        level=arguments.get("level"),
        genre=arguments.get("genre"),
        version=arguments.get("version"),
        ds=arguments.get("ds"),
        ds_min=arguments.get("ds_min"),
        ds_max=arguments.get("ds_max"),
        fit_diff=arguments.get("fit_diff"),
        fit_diff_min=arguments.get("fit_diff_min"),
        fit_diff_max=arguments.get("fit_diff_max"),
        fit_delta=arguments.get("fit_delta"),
        fit_delta_min=arguments.get("fit_delta_min"),
        fit_delta_max=arguments.get("fit_delta_max"),
        fit_label=arguments.get("fit_label"),
        region_has=arguments.get("region_has"),
        region_missing=arguments.get("region_missing"),
        difficulty=arguments.get("difficulty"),
        song_type=arguments.get("song_type"),
        artist=arguments.get("artist"),
        charter=arguments.get("charter"),
    )
    if not resolution.get("resolved"):
        return resolution

    scoring_arguments = {
        key: value
        for key, value in arguments.items()
        if key not in SCORING_LOOKUP_KEYS
    }
    scoring_arguments["note_totals"] = resolution["note_totals"]
    result = find_score_combinations(scoring_arguments)
    result["calculated"] = True
    result["lookup"] = resolution
    return result


def list_song_aliases(arguments: dict[str, Any]) -> dict[str, Any]:
    query_value = arguments.get("query")
    if query_value in (None, ""):
        query_value = arguments.get("song_id")
    if query_value in (None, ""):
        query_value = arguments.get("title")
    if query_value in (None, ""):
        raise SearchError("query, song_id, or title is required")

    limit = arguments.get("limit", 20)
    result = search_songs(query=str(query_value), limit=limit)
    songs = []
    for song in result.get("songs") or []:
        aliases = unique_preserve_order([str(alias) for alias in song.get("aliases") or [] if str(alias).strip()])
        songs.append(
            {
                "id": song.get("id"),
                "source_id": song.get("source_id"),
                "source_ids": song.get("source_ids"),
                "title": song.get("title"),
                "artist": song.get("artist"),
                "source": song.get("source"),
                "source_labels": song.get("source_labels"),
                "alias_count": len(aliases),
                "aliases": aliases,
            }
        )
    criteria = dict(result.get("criteria") or {})
    criteria["query"] = str(query_value)
    criteria["limit"] = limit
    return {
        "count": len(songs),
        "total_matches": result.get("total_matches", 0),
        "truncated": result.get("truncated", False),
        "criteria": criteria,
        "songs": songs,
    }


def query_today_maimai(arguments: dict[str, Any]) -> dict[str, Any]:
    qq = str(arguments.get("qq") or "").strip()
    if not qq:
        raise SearchError("qq is required")
    int(qq)
    offset = arguments.get("offset", 0)
    bot_name = str(arguments.get("botName") or TODAY_DEFAULT_BOT_NAME).strip() or TODAY_DEFAULT_BOT_NAME
    results, _ = collect_song_results()
    songs = [song for item in results if (song := song_from_search_result(item)) is not None]
    if not songs:
        raise SearchError("本地曲库为空，无法生成今日舞萌。")

    result = today_maimai(qq, songs, offset=offset)
    selected = result["song"]
    text = format_today_maimai(bot_name, qq, songs, offset=offset)
    return {
        "qq": qq,
        "rp": result["rp"],
        "good": result["good"],
        "bad": result["bad"],
        "offset": result["offset"],
        "total_candidates": len(songs),
        "song": {
            "id": selected.id,
            "title": selected.title,
            "ds": list(selected.ds),
        },
        "text": text,
    }


def batch_search_songs(arguments: dict[str, Any]) -> dict[str, Any]:
    items = arguments.get("items")
    if not isinstance(items, list) or not items:
        raise SearchError("items must be a non-empty array")
    if len(items) > 200:
        raise SearchError("items can contain at most 200 searches")

    results: list[dict[str, Any]] = []
    success_count = 0
    for index, item in enumerate(items):
        if not isinstance(item, dict):
            results.append(
                {
                    "index": index,
                    "key": None,
                    "ok": False,
                    "error": {"message": "item must be an object"},
                    "result": None,
                }
            )
            continue
        try:
            result = search_songs(
                query=item.get("query"),
                level=item.get("level"),
                genre=item.get("genre"),
                version=item.get("version"),
                ds=item.get("ds"),
                ds_min=item.get("ds_min"),
                ds_max=item.get("ds_max"),
                fit_diff=item.get("fit_diff"),
                fit_diff_min=item.get("fit_diff_min"),
                fit_diff_max=item.get("fit_diff_max"),
                fit_delta=item.get("fit_delta"),
                fit_delta_min=item.get("fit_delta_min"),
                fit_delta_max=item.get("fit_delta_max"),
                fit_label=item.get("fit_label"),
                region_has=item.get("region_has"),
                region_missing=item.get("region_missing"),
                difficulty=item.get("difficulty"),
                song_type=item.get("song_type"),
                artist=item.get("artist"),
                charter=item.get("charter"),
                tag=item.get("tag"),
                tag_exclude=item.get("tag_exclude"),
                released_after=item.get("released_after"),
                released_before=item.get("released_before"),
                sort=item.get("sort"),
                limit=item.get("limit"),
            )
            success_count += 1
            results.append(
                {
                    "index": index,
                    "key": item.get("key"),
                    "ok": True,
                    "result": result,
                    "error": None,
                }
            )
        except (SearchError, ValueError) as exc:
            results.append(
                {
                    "index": index,
                    "key": item.get("key"),
                    "ok": False,
                    "result": None,
                    "error": {"message": str(exc)},
                }
            )

    return {
        "count": len(results),
        "counts": {
            "requested": len(items),
            "success": success_count,
            "failure": len(items) - success_count,
        },
        "items": results,
    }


def handle_tool_call(message_id: Any, params: dict[str, Any] | None) -> dict[str, Any]:
    params = params or {}
    tool_name = params.get("name")
    if tool_name not in {tool["name"] for tool in TOOLS}:
        return error(message_id, -32602, f"Unknown tool: {tool_name}")

    raw_arguments = params.get("arguments") or {}
    arguments = clean_arguments(raw_arguments)
    if tool_name == "score_counts":
        try:
            result = score_counts(arguments)
        except (ScoringError, ValueError) as exc:
            return text_error_content(message_id, exc)
        return result_content(
            message_id,
            result,
            tool_name=tool_name,
            arguments=raw_arguments,
        )

    if tool_name == "find_score_combinations":
        try:
            result = find_score_combinations_with_optional_lookup(arguments)
        except (ScoringError, SearchError, ValueError) as exc:
            return text_error_content(message_id, exc)
        return result_content(
            message_id,
            result,
            tool_name=tool_name,
            arguments=raw_arguments,
        )

    if tool_name == ADD_ALIAS_TOOL["name"]:
        try:
            if _is_name_alias_kind(arguments.get("kind")):
                result = save_name_alias(
                    kind=arguments.get("kind", ""),
                    canonical=arguments.get("canonical", ""),
                    alias=arguments.get("alias", ""),
                )
            else:
                result = save_custom_alias(
                    song_id=arguments.get("song_id"),
                    title=arguments.get("title"),
                    alias=arguments.get("alias", ""),
                )
        except (SearchError, ValueError) as exc:
            return text_error_content(message_id, exc)
        return result_content(
            message_id,
            result,
            tool_name=tool_name,
            arguments=raw_arguments,
        )

    if tool_name == DELETE_ALIAS_TOOL["name"]:
        try:
            if _is_name_alias_kind(arguments.get("kind")):
                result = delete_name_alias(
                    kind=arguments.get("kind", ""),
                    canonical=arguments.get("canonical", ""),
                    alias=arguments.get("alias", ""),
                )
            else:
                result = delete_custom_alias(
                    song_id=arguments.get("song_id"),
                    title=arguments.get("title"),
                    alias=arguments.get("alias", ""),
                )
        except (SearchError, ValueError) as exc:
            return text_error_content(message_id, exc)
        return result_content(
            message_id,
            result,
            tool_name=tool_name,
            arguments=raw_arguments,
        )

    if tool_name == LIST_ALIASES_TOOL["name"]:
        try:
            if _is_name_alias_kind(arguments.get("kind")):
                result = list_name_aliases(
                    kind=arguments.get("kind", ""),
                    query=arguments.get("query"),
                )
            else:
                result = list_song_aliases(arguments)
        except (SearchError, ValueError) as exc:
            return text_error_content(message_id, exc)
        return result_content(
            message_id,
            result,
            tool_name=tool_name,
            arguments=raw_arguments,
        )

    if tool_name == REFRESH_SOURCES_TOOL["name"]:
        try:
            force = arguments.get("force") is True
            bg = arguments.get("background") is True or arguments.get("bg") is True or force
            if bg:
                result = refresh_sources_bg(arguments)
            else:
                result = refresh_sources(arguments)
                clear_search_caches()
        except (RefreshError, ValueError) as exc:
            return text_error_content(message_id, exc)
        return result_content(
            message_id,
            result,
            tool_name=tool_name,
            arguments=raw_arguments,
        )

    if tool_name == REFRESH_JOB_STATUS_TOOL["name"]:
        job_id = str(arguments.get("jobId", ""))
        if not job_id:
            return text_error_content(message_id, RefreshError("需要 jobId 参数"))
        result = read_bg_job_status(job_id)
        return result_content(
            message_id,
            result,
            tool_name=tool_name,
            arguments=raw_arguments,
        )

    if tool_name == RANDOM_TOOL["name"]:
        try:
            result = random_songs(
                count=arguments.get("count"),
                level=arguments.get("level"),
                genre=arguments.get("genre"),
                version=arguments.get("version"),
                ds=arguments.get("ds"),
                ds_min=arguments.get("ds_min"),
                ds_max=arguments.get("ds_max"),
                fit_diff=arguments.get("fit_diff"),
                fit_diff_min=arguments.get("fit_diff_min"),
                fit_diff_max=arguments.get("fit_diff_max"),
                fit_delta=arguments.get("fit_delta"),
                fit_delta_min=arguments.get("fit_delta_min"),
                fit_delta_max=arguments.get("fit_delta_max"),
                fit_label=arguments.get("fit_label"),
                region_has=arguments.get("region_has"),
                region_missing=arguments.get("region_missing"),
                difficulty=arguments.get("difficulty"),
                song_type=arguments.get("song_type"),
                artist=arguments.get("artist"),
                charter=arguments.get("charter"),
                tag=arguments.get("tag"),
                tag_exclude=arguments.get("tag_exclude"),
                released_after=arguments.get("released_after"),
                released_before=arguments.get("released_before"),
                sort=arguments.get("sort"),
                seed=arguments.get("seed"),
            )
        except (SearchError, ValueError) as exc:
            return text_error_content(message_id, exc)
        return result_content(
            message_id,
            result,
            tool_name=tool_name,
            arguments=raw_arguments,
        )

    if tool_name == TODAY_TOOL["name"]:
        try:
            result = query_today_maimai(arguments)
        except (SearchError, ValueError) as exc:
            return text_error_content(message_id, exc)
        return result_content(
            message_id,
            result,
            tool_name=tool_name,
            arguments=raw_arguments,
        )

    if tool_name == LIST_BY_ID_TOOL["name"]:
        try:
            result = list_songs_by_id(
                order=arguments.get("order", "asc"),
                limit=arguments.get("limit"),
                level=arguments.get("level"),
                genre=arguments.get("genre"),
                version=arguments.get("version"),
                ds=arguments.get("ds"),
                ds_min=arguments.get("ds_min"),
                ds_max=arguments.get("ds_max"),
                fit_diff=arguments.get("fit_diff"),
                fit_diff_min=arguments.get("fit_diff_min"),
                fit_diff_max=arguments.get("fit_diff_max"),
                fit_delta=arguments.get("fit_delta"),
                fit_delta_min=arguments.get("fit_delta_min"),
                fit_delta_max=arguments.get("fit_delta_max"),
                fit_label=arguments.get("fit_label"),
                region_has=arguments.get("region_has"),
                region_missing=arguments.get("region_missing"),
                difficulty=arguments.get("difficulty"),
                song_type=arguments.get("song_type"),
                artist=arguments.get("artist"),
                charter=arguments.get("charter"),
                tag=arguments.get("tag"),
                tag_exclude=arguments.get("tag_exclude"),
                released_after=arguments.get("released_after"),
                released_before=arguments.get("released_before"),
                sort=arguments.get("sort"),
            )
        except (SearchError, ValueError) as exc:
            return text_error_content(message_id, exc)
        return result_content(
            message_id,
            result,
            tool_name=tool_name,
            arguments=raw_arguments,
        )

    if tool_name == LIST_VERSIONS_TOOL["name"]:
        try:
            result = list_versions(
                query=arguments.get("query"),
                limit=arguments.get("limit"),
            )
        except (SearchError, ValueError) as exc:
            return text_error_content(message_id, exc)
        return result_content(
            message_id,
            result,
            tool_name=tool_name,
            arguments=raw_arguments,
        )

    if tool_name == BATCH_SEARCH_TOOL["name"]:
        try:
            result = batch_search_songs(arguments)
        except (SearchError, ValueError) as exc:
            return text_error_content(message_id, exc)
        return result_content(
            message_id,
            result,
            tool_name=tool_name,
            arguments=raw_arguments,
        )

    try:
        result = search_songs(
            query=arguments.get("query"),
            level=arguments.get("level"),
            genre=arguments.get("genre"),
            version=arguments.get("version"),
            ds=arguments.get("ds"),
            ds_min=arguments.get("ds_min"),
            ds_max=arguments.get("ds_max"),
            fit_diff=arguments.get("fit_diff"),
            fit_diff_min=arguments.get("fit_diff_min"),
            fit_diff_max=arguments.get("fit_diff_max"),
            fit_delta=arguments.get("fit_delta"),
            fit_delta_min=arguments.get("fit_delta_min"),
            fit_delta_max=arguments.get("fit_delta_max"),
            fit_label=arguments.get("fit_label"),
            region_has=arguments.get("region_has"),
            region_missing=arguments.get("region_missing"),
            difficulty=arguments.get("difficulty"),
            song_type=arguments.get("song_type"),
            is_new=arguments.get("is_new"),
            is_new_source=arguments.get("is_new_source"),
            id=arguments.get("id"),
            id_min=arguments.get("id_min"),
            id_max=arguments.get("id_max"),
            bpm=arguments.get("bpm"),
            bpm_min=arguments.get("bpm_min"),
            bpm_max=arguments.get("bpm_max"),
            is_locked=arguments.get("is_locked"),
            artist=arguments.get("artist"),
            charter=arguments.get("charter"),
            tag=arguments.get("tag"),
            tag_exclude=arguments.get("tag_exclude"),
            released_after=arguments.get("released_after"),
            released_before=arguments.get("released_before"),
            sort=arguments.get("sort"),
            limit=arguments.get("limit"),
        )
    except (SearchError, ValueError) as exc:
        return text_error_content(message_id, exc)

    return result_content(
        message_id,
        result,
        tool_name=tool_name,
        arguments=raw_arguments,
    )


def handle_request(message: dict[str, Any]) -> dict[str, Any] | None:
    message_id = message.get("id")
    method = message.get("method")
    params = message.get("params")

    if method == "initialize":
        return handle_initialize(message_id, params)
    if method == "tools/list":
        return success(message_id, {"tools": TOOLS})
    if method == "tools/call":
        return handle_tool_call(message_id, params)
    if method == "prompts/list":
        return success(message_id, {"prompts": []})
    if method == "resources/list":
        return success(message_id, {"resources": []})
    if method == "ping":
        return success(message_id, {})

    if message_id is None:
        return None
    return error(message_id, -32601, f"Method not found: {method}")


def main() -> None:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
            response = handle_request(message)
            if response is not None:
                write_message(response)
        except Exception as exc:  # Defensive: never let the MCP process die on one bad message.
            traceback.print_exc(file=sys.stderr)
            message_id = None
            if "message" in locals() and isinstance(message, dict):
                message_id = message.get("id")
            write_message(error(message_id, -32603, "Internal error", str(exc)))


if __name__ == "__main__":
    main()
