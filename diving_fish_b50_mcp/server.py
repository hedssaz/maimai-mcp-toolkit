from __future__ import annotations

import json
import hashlib
import os
import socket
import sys
import time
import traceback
import urllib.error
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
from contextlib import contextmanager
from http.cookies import SimpleCookie
from datetime import datetime, timezone
from functools import lru_cache
from pathlib import Path
from typing import Any
import threading

from . import __version__

# 让 maimai_mcp 作为兄弟包被 import：
# - 本地合并 workspace 下，maimai_mcp 已经在 sys.path（同级目录）
# - 服务器部署时 diving-fish 和 maimai-local-search 是两个独立 mount，
#   需要手动把后者加到 sys.path 才能 import maimai_mcp.server
for _candidate in (
    Path(__file__).resolve().parent.parent,
    Path(os.environ.get("PYTHONPATH", "")),
):
    if _candidate and (_candidate / "maimai_mcp" / "__init__.py").exists():
        candidate_str = str(_candidate)
        if candidate_str not in sys.path:
            sys.path.insert(0, candidate_str)
        break

from .api_catalog import (
    API_CATALOG,
    OPERATION_NAMES,
    build_api_url,
    build_cover_url,
    public_api_catalog,
    public_endpoint_metadata,
)
from player_cache import (
    is_player_b50_fresh as _is_player_b50_fresh,
    merge_player_record as _merge_player_record_cache,
    read_player_b50 as _read_player_b50,
    write_player_b50 as _write_player_b50_cache,
    write_player_records as _write_player_records_cache,
)
from qq_identity_mcp.store import get_identity, resolve_identities, upsert_waterfish_profile


SERVER_NAME = "diving-fish-b50-mcp"
QUERY_PLAYER_URL = "https://www.diving-fish.com/api/maimaidxprober/query/player"
NETWORK_RETRY_DELAYS_SECONDS = (0.4, 1.2)
B50_DIFFICULTIES = ("Basic", "Advanced", "Expert", "Master", "Re:MASTER")
B50_CHART_STATS_KEYS = ("cnt", "diff", "avg", "avg_dx", "std_dev", "dist", "fc_dist")
FAST_FITTED_B50_INDEX_CACHE_VERSION = 1
CURRENT_VERSION_ENV_NAMES = ("MAIMAI_LOCAL_CURRENT_VERSIONS", "MAIMAI_CURRENT_VERSIONS")
DIVINGFISH_VERSION_ORDER = (
    "maimai",
    "maimai PLUS",
    "maimai GreeN",
    "maimai GreeN PLUS",
    "maimai ORANGE",
    "maimai ORANGE PLUS",
    "maimai PiNK",
    "maimai PiNK PLUS",
    "maimai MURASAKi",
    "maimai MURASAKi PLUS",
    "maimai MiLK",
    "maimai MiLK PLUS",
    "maimai FiNALE",
    "maimai でらっくす",
    "maimai でらっくす Splash",
    "maimai でらっくす Splash PLUS",
    "maimai でらっくす UNiVERSE",
    "maimai でらっくす UNiVERSE PLUS",
    "maimai でらっくす FESTiVAL",
    "maimai でらっくす FESTiVAL PLUS",
    "maimai でらっくす BUDDiES",
    "maimai でらっくす BUDDiES PLUS",
    "maimai でらっくす PRiSM",
    "maimai でらっくす PRiSM PLUS",
    "maimai でらっくす CiRCLE",
)
DIVINGFISH_VERSION_RANK = {
    " ".join(version.strip().casefold().split()): index
    for index, version in enumerate(DIVINGFISH_VERSION_ORDER)
}
_FAST_FITTED_B50_INDEX_CACHE_LOCK = threading.Lock()
_FAST_FITTED_B50_INDEX_CACHE: dict[str, Any] = {"fingerprint": None, "index": None}


class DivingFishError(Exception):
    def __init__(
        self,
        message: str,
        *,
        code: str = "DIVING_FISH_ERROR",
        status: int | None = None,
        body: str | None = None,
    ) -> None:
        super().__init__(message)
        self.code = code
        self.status = status
        self.body = body

    def to_dict(self) -> dict[str, Any]:
        return {
            "code": self.code,
            "message": str(self),
            "status": self.status,
            "body": self.body,
        }


B50_DISPLAY_INPUT_PROPERTIES = {
    "sortBy": {
        "type": "string",
        "enum": ["default", "ra", "achievement", "ds", "fitDiff", "fitDelta", "title"],
        "description": (
            "文本 B50 歌曲排序字段。default/ra 按单曲 ra，achievement 按达成率，"
            "ds 按原始定数，fitDiff 按拟合定数，fitDelta 按原定数-拟合定数，title 按曲名。"
        ),
    },
    "sortOrder": {
        "type": "string",
        "enum": ["desc", "asc"],
        "description": "排序方向，默认 desc。",
    },
    "level": {
        "type": "string",
        "description": "可选，只展示指定等级，例如 13、13+、14+。",
    },
    "difficulty": {
        "type": "string",
        "description": "可选，只展示指定难度，例如 Basic/Advanced/Expert/Master/Re:MASTER 或 绿/黄/红/紫/白。",
    },
    "dsMin": {"type": "number", "description": "可选，只展示原始定数不低于该值的歌曲。"},
    "dsMax": {"type": "number", "description": "可选，只展示原始定数不高于该值的歌曲。"},
    "achievementMin": {"type": "number", "description": "可选，只展示达成率不低于该值的歌曲。"},
    "achievementMax": {"type": "number", "description": "可选，只展示达成率不高于该值的歌曲。"},
    "raMin": {"type": "number", "description": "可选，只展示单曲 ra 不低于该值的歌曲。"},
    "raMax": {"type": "number", "description": "可选，只展示单曲 ra 不高于该值的歌曲。"},
    "fitDiffMin": {"type": "number", "description": "可选，只展示拟合定数不低于该值的歌曲。"},
    "fitDiffMax": {"type": "number", "description": "可选，只展示拟合定数不高于该值的歌曲。"},
    "fitDeltaMin": {"type": "number", "description": "可选，只展示原定数-拟合定数不低于该值的歌曲。"},
    "fitDeltaMax": {"type": "number", "description": "可选，只展示原定数-拟合定数不高于该值的歌曲。"},
    "fitLabel": {
        "type": "string",
        "enum": ["虚高", "虚低"],
        "description": "可选，只展示拟合差值标签。虚高表示 ds - fitDiff > 0，虚低表示 ds - fitDiff < 0。",
    },
}


QUERY_B50_TOOL = {
    "name": "query_b50",
    "description": (
        "通过水鱼查分器 /query/player 接口查询 maimai DX B50。"
        "必须提供 qq 或 username 其中一个，目标账号需允许第三方查询。"
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "qq": {
                "type": "string",
                "description": "QQ 号。和 username 二选一。",
            },
            "username": {
                "type": "string",
                "description": "水鱼用户名。和 qq 二选一。",
            },
            "target": {
                "type": "string",
                "description": "QQ号、QQ昵称、群昵称/群名片、水鱼昵称或水鱼 username。未提供 qq/username 时会先用 QQ 身份缓存反查 QQ，查不到则按水鱼 username 查询。",
            },
            "topN": {
                "type": "integer",
                "minimum": 0,
                "maximum": 50,
                "description": "文本摘要中展示的最高 ra 成绩数量，默认 50，最大 50。",
            },
            "section": {
                "type": "string",
                "enum": [
                    "b50",
                    "all",
                    "b35",
                    "sd",
                    "old",
                    "b15",
                    "dx",
                    "new",
                    "split",
                    "both",
                ],
                "description": (
                    "文本摘要展示范围：b50/all 合并排行，b35/sd/old 旧曲，"
                    "b15/dx/new 新曲，split/both 分组展示。"
                ),
            },
            "includeRaw": {
                "type": "boolean",
                "description": "是否在返回 JSON 中附带水鱼原始响应。",
            },
            "timeoutMs": {
                "type": "integer",
                "minimum": 1000,
                "maximum": 30000,
                "description": "请求超时时间，单位毫秒，默认 10000。",
            },
            "includeChartMetadata": {
                "type": "boolean",
                "description": "是否调用 maimai-local-search MCP 补充拟合定数、差值和谱面元数据。单人 query_b50 默认 true。",
            },
            "groupId": {
                "type": "string",
                "description": "可选，当前群号；用于从 QQ 身份缓存中优先显示该群群昵称。",
            },
            **B50_DISPLAY_INPUT_PROPERTIES,
        },
        "additionalProperties": False,
    },
}

QUERY_B50_BATCH_TOOL = {
    "name": "query_b50_batch",
    "description": (
        "批量输入 QQ 号，逐个调用水鱼查分器查询 maimai DX B50。"
        "用于群榜等批量场景；每个 QQ 独立返回成功或错误。"
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "qqs": {
                "type": "array",
                "items": {"type": "string"},
                "minItems": 1,
                "maxItems": 500,
                "description": "要查询的 QQ 号列表。",
            },
            "topN": {
                "type": "integer",
                "minimum": 0,
                "maximum": 50,
                "description": "每个文本摘要中展示的最高 ra 成绩数量，默认 50。",
            },
            "section": QUERY_B50_TOOL["inputSchema"]["properties"]["section"],
            "includeRaw": QUERY_B50_TOOL["inputSchema"]["properties"]["includeRaw"],
            "includeSummaries": {
                "type": "boolean",
                "description": "是否为每个成功结果附带文本 B50 摘要，默认 false。",
            },
            "timeoutMs": QUERY_B50_TOOL["inputSchema"]["properties"]["timeoutMs"],
            "includeChartMetadata": {
                "type": "boolean",
                "description": "是否为批量结果调用 maimai-local-search MCP 补充谱面元数据。批量默认仅在 includeSummaries 或筛选/排序需要时启用。",
            },
            "queryDelayMs": {
                "type": "integer",
                "minimum": 0,
                "maximum": 10000,
                "description": "每个 QQ 查询之间的等待时间，默认 250ms。",
            },
            "maxConcurrency": {
                "type": "integer",
                "minimum": 1,
                "maximum": 20,
                "description": "批量查询并发数，默认 1。群榜建议 3-5。",
            },
            "groupId": QUERY_B50_TOOL["inputSchema"]["properties"]["groupId"],
            **B50_DISPLAY_INPUT_PROPERTIES,
        },
        "required": ["qqs"],
        "additionalProperties": False,
    },
}

QUERY_COMPUTED_B50_TOOL = {
    "name": "query_computed_b50",
    "description": (
        "使用 Developer-Token 拉取玩家完整 maimai 成绩后，按拟合定数重新计算每张谱面的单曲 rating，"
        "再按水鱼曲库中最新的 basic_info.from 大版本重排 B35/B15。"
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "qq": QUERY_B50_TOOL["inputSchema"]["properties"]["qq"],
            "username": QUERY_B50_TOOL["inputSchema"]["properties"]["username"],
            "target": QUERY_B50_TOOL["inputSchema"]["properties"]["target"],
            "topN": QUERY_B50_TOOL["inputSchema"]["properties"]["topN"],
            "section": QUERY_B50_TOOL["inputSchema"]["properties"]["section"],
            "includeRaw": QUERY_B50_TOOL["inputSchema"]["properties"]["includeRaw"],
            "timeoutMs": QUERY_B50_TOOL["inputSchema"]["properties"]["timeoutMs"],
            "includeChartMetadata": {
                "type": "boolean",
                "description": "兼容参数。拟合 B50 必须匹配 maimai-local-search 拟合定数，因此不会跳过谱面元数据。",
            },
            "groupId": QUERY_B50_TOOL["inputSchema"]["properties"]["groupId"],
            **B50_DISPLAY_INPUT_PROPERTIES,
        },
        "additionalProperties": False,
    },
}

QUERY_MAIMAI_SONG_SCORE_TOOL = {
    "name": "query_maimai_song_score",
    "description": (
        "按 maimai 曲名/别名/曲目 ID 查询指定玩家的单曲成绩。"
        "传 musicId 直接用 Diving-Fish ID 查；传 songQuery 会先调 maimai-local-search 解析曲名。"
        "需要 Diving-Fish Developer-Token，可使用已绑定 token。"
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "qq": {
                "type": "string",
                "description": "QQ 号。和 username/target 三选一。",
            },
            "username": {
                "type": "string",
                "description": "水鱼用户名。和 qq/target 三选一。",
            },
            "target": {
                "type": "string",
                "description": "QQ号、QQ昵称、群昵称/群名片、水鱼昵称或水鱼 username。",
            },
            "musicId": {
                "oneOf": [{"type": "integer"}, {"type": "string"}],
                "description": "Diving-Fish maimai 曲目 ID。和 songQuery 二选一。",
            },
            "songQuery": {
                "type": "string",
                "description": "曲名、别名或曲目 ID。和 musicId 二选一。内部先调 maimai-local-search 解析再查分。",
            },
            "difficulty": {"type": "string", "description": "可选，用于本地曲库筛选，如 Master、Expert、紫、红。"},
            "songType": {"type": "string", "description": "可选，筛选 SD/DX/宴 谱面。"},
            "searchLimit": {"type": "integer", "minimum": 1, "maximum": 20, "description": "本地曲库候选上限，默认 5。"},
            "developerToken": {"type": "string", "description": "可选，覆盖本地已绑定 Developer-Token。"},
            "includeRaw": {"type": "boolean", "description": "是否返回水鱼原始响应。"},
            "timeoutMs": QUERY_B50_TOOL["inputSchema"]["properties"]["timeoutMs"],
            "groupId": QUERY_B50_TOOL["inputSchema"]["properties"]["groupId"],
        },
        "additionalProperties": False,
    },
}

QUERY_MAIMAI_PLAYER_RECORDS_TOOL = {
    "name": "query_maimai_player_records",
    "description": (
        "查询玩家的 maimai 完整成绩（需 Developer-Token），支持按等级、版本、牌子、曲目 ID 过滤。"
        "查询成功会自动写入 player_cache.records，群单曲榜后续查询可直接复用。"
        "按牌子查询时使用本地 maimaidxplate.json 的国服 ID 白名单。"
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "qq": {
                "type": "string",
                "description": "QQ 号。和 username/target 三选一。",
            },
            "username": {
                "type": "string",
                "description": "水鱼用户名。和 qq/target 三选一。",
            },
            "target": {
                "type": "string",
                "description": "QQ号、QQ昵称、群昵称/群名片、水鱼昵称或水鱼 username。",
            },
            "level": {
                "type": "string",
                "description": "按等级过滤，如 14、14+、15",
            },
            "version": {
                "type": "array",
                "items": {"type": "string"},
                "description": "按版本过滤，如 [\"maimai でらっくす\"]。不传返回全量。",
            },
            "plate": {
                "type": "string",
                "description": "按牌子过滤，如 熊/華/爽/真/超。需配合 server 参数。",
            },
            "server": {
                "type": "string",
                "enum": ["cn"],
                "description": "牌子所属服务器，默认 cn。当前分支仅支持国服。",
            },
            "musicId": {
                "oneOf": [{"type": "integer"}, {"type": "string"}],
                "description": "按 Diving-Fish 曲目 ID 过滤。",
            },
            "includeRaw": {"type": "boolean", "description": "是否返回水鱼原始响应。"},
            "timeoutMs": QUERY_B50_TOOL["inputSchema"]["properties"]["timeoutMs"],
            "groupId": QUERY_B50_TOOL["inputSchema"]["properties"]["groupId"],
        },
        "additionalProperties": False,
    },
}

LIST_APIS_TOOL = {
    "name": "list_diving_fish_apis",
    "description": (
        "列出本 MCP 支持的 Diving-Fish 官方 API operation，并标注请求方法、路径、"
        "鉴权方式、是否需要 Developer-Token/Import-Token、是否会修改数据。"
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "game": {
                "type": "string",
                "enum": ["maimaidxprober", "chunithmprober"],
                "description": "可选，按游戏数据类别过滤。",
            },
            "auth": {
                "type": "string",
                "enum": [
                    "none",
                    "login",
                    "login_or_import_token",
                    "developer_token",
                    "login_credentials",
                ],
                "description": "可选，按鉴权方式过滤。",
            },
            "includeMutating": {
                "type": "boolean",
                "description": "是否包含会修改/删除数据的接口，默认 false。",
            },
        },
        "additionalProperties": False,
    },
}

DIVING_FISH_API_TOOL = {
    "name": "diving_fish_api",
    "description": (
        "按 operation 调用 Diving-Fish 官方 API。需要 Developer-Token、Import-Token、"
        "登录 jwtToken 的接口会在 list_diving_fish_apis 中标注。会修改或删除数据的接口"
        "必须传 confirm 等于 operation。"
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "operation": {
                "type": "string",
                "enum": OPERATION_NAMES,
                "description": "要调用的 API operation。",
            },
            "query": {"type": "object", "description": "URL 查询参数。"},
            "body": {"description": "JSON 请求体。"},
            "rawBody": {
                "type": "string",
                "description": "HTML 等原始请求体，仅用于 *_html 接口。",
            },
            "developerToken": {"type": "string", "description": "Developer-Token。"},
            "importToken": {"type": "string", "description": "Import-Token。"},
            "jwtToken": {"type": "string", "description": "登录后得到的 jwt_token。"},
            "ifNoneMatch": {
                "type": "string",
                "description": "If-None-Match 缓存校验值，需保留引号。",
            },
            "includeHeaders": {
                "type": "boolean",
                "description": "是否在 structuredContent 返回响应头。",
            },
            "headers": {"type": "object", "description": "额外请求头；一般不需要。"},
            "confirm": {
                "type": "string",
                "description": "会修改或删除数据的接口必须传 operation 本身作为确认。",
            },
            "timeoutMs": {
                "type": "integer",
                "minimum": 1000,
                "maximum": 30000,
                "description": "请求超时时间，单位毫秒，默认 10000。",
            },
        },
        "required": ["operation"],
        "additionalProperties": False,
    },
}

BIND_DEVELOPER_TOKEN_TOOL = {
    "name": "bind_developer_token",
    "description": (
        "把 Developer-Token 绑定到本地 MCP 持久存储。之后调用需要 Developer-Token "
        "的 API 时可省略 developerToken。仅应在私聊或安全上下文中使用。"
    ),
    "inputSchema": {
        "type": "object",
        "properties": {
            "developerToken": {
                "type": "string",
                "description": "Diving-Fish Developer-Token。",
            }
        },
        "required": ["developerToken"],
        "additionalProperties": False,
    },
}

DEVELOPER_TOKEN_STATUS_TOOL = {
    "name": "developer_token_status",
    "description": "查看本地 MCP 是否已绑定 Developer-Token。不会返回明文 token。",
    "inputSchema": {"type": "object", "properties": {}, "additionalProperties": False},
}

CLEAR_DEVELOPER_TOKEN_TOOL = {
    "name": "clear_developer_token",
    "description": "清除本地 MCP 持久存储中的 Developer-Token。",
    "inputSchema": {"type": "object", "properties": {}, "additionalProperties": False},
}

TOOLS = [
    QUERY_B50_TOOL,
    QUERY_B50_BATCH_TOOL,
    QUERY_COMPUTED_B50_TOOL,
    QUERY_MAIMAI_SONG_SCORE_TOOL,
    QUERY_MAIMAI_PLAYER_RECORDS_TOOL,
    LIST_APIS_TOOL,
    DIVING_FISH_API_TOOL,
    BIND_DEVELOPER_TOKEN_TOOL,
    DEVELOPER_TOKEN_STATUS_TOOL,
    CLEAR_DEVELOPER_TOKEN_TOOL,
]


def normalize_identifier(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    value = value.strip()
    return value or None


def validate_lookup(arguments: dict[str, Any]) -> dict[str, str]:
    qq = normalize_identifier(arguments.get("qq"))
    username = normalize_identifier(arguments.get("username"))
    target = normalize_identifier(arguments.get("target"))

    if not qq and not username and target:
        try:
            qq = resolve_target_to_qq(target, group_id=normalize_identifier(arguments.get("groupId")))
        except DivingFishError as exc:
            if exc.code != "IDENTITY_NOT_FOUND":
                raise
            username = target

    if not qq and not username:
        raise DivingFishError("必须提供 qq 或 username 其中一个。", code="INVALID_INPUT")
    if qq and username:
        raise DivingFishError("qq 和 username 只能提供其中一个。", code="INVALID_INPUT")
    return {"qq": qq} if qq else {"username": username or ""}


def resolve_target_to_qq(target: str, *, group_id: str | None) -> str:
    result = resolve_identities(target, group_id=group_id, max_results=50)
    matches = result.get("matches") if isinstance(result.get("matches"), list) else []
    if not matches:
        raise DivingFishError(
            f"没有从 QQ 身份缓存中找到：{target}",
            code="IDENTITY_NOT_FOUND",
        )
    top_score = max(int(item.get("matchScore") or 0) for item in matches)
    top_matches = [item for item in matches if int(item.get("matchScore") or 0) == top_score]
    unique_qqs = {
        item.get("qq")
        for item in top_matches
        if isinstance(item.get("qq"), str) and item.get("qq")
    }
    if len(unique_qqs) == 1:
        return str(next(iter(unique_qqs)))

    if len(unique_qqs) > 1 or result.get("ambiguous"):
        candidates = [
            {
                "qq": item.get("qq"),
                "qqNickname": item.get("qqNickname"),
                "waterfishNickname": item.get("waterfishNickname"),
                "preferredGroup": item.get("preferredGroup"),
                "groups": item.get("groups"),
                "matchedFields": item.get("matchedFields"),
            }
            for item in top_matches[:10]
        ]
        raise DivingFishError(
            f"昵称“{target}”匹配到多个 QQ，请让用户选择具体 QQ。",
            code="AMBIGUOUS_IDENTITY",
            body=json.dumps(candidates, ensure_ascii=False),
        )
    qq = matches[0].get("qq")
    if not isinstance(qq, str) or not qq:
        raise DivingFishError(f"昵称“{target}”没有可用 QQ。", code="IDENTITY_NOT_FOUND")
    return qq


def normalize_timeout_ms(value: Any) -> int:
    if value is None:
        return 10000
    if not isinstance(value, int) or value < 1000 or value > 30000:
        raise DivingFishError(
            "timeoutMs 必须是 1000 到 30000 之间的整数。",
            code="INVALID_INPUT",
        )
    return value


def normalize_query_delay_ms(value: Any) -> int:
    if value is None:
        return 250
    if not isinstance(value, int) or value < 0 or value > 10000:
        raise DivingFishError(
            "queryDelayMs 必须是 0 到 10000 之间的整数。",
            code="INVALID_INPUT",
        )
    return value


def normalize_max_concurrency(value: Any) -> int:
    if value is None:
        return 1
    if not isinstance(value, int) or value < 1 or value > 20:
        raise DivingFishError(
            "maxConcurrency 必须是 1 到 20 之间的整数。",
            code="INVALID_INPUT",
        )
    return value


def normalize_music_id(value: Any) -> int:
    if isinstance(value, bool):
        raise DivingFishError("musicId 必须是整数。", code="INVALID_INPUT")
    if isinstance(value, int):
        music_id = value
    elif isinstance(value, str) and value.strip():
        text = value.strip()
        if text.lower().startswith("id"):
            text = text[2:].strip()
        try:
            music_id = int(text)
        except ValueError as exc:
            raise DivingFishError("musicId 必须是整数。", code="INVALID_INPUT") from exc
    else:
        raise DivingFishError("必须提供 musicId。", code="INVALID_INPUT")
    if music_id <= 0:
        raise DivingFishError("musicId 必须是正整数。", code="INVALID_INPUT")
    return music_id


def normalize_top_n(value: Any) -> int:
    if value is None:
        return 50
    if not isinstance(value, int) or value < 0 or value > 50:
        raise DivingFishError("topN 必须是 0 到 50 之间的整数。", code="INVALID_INPUT")
    return value


def normalize_optional_number(value: Any, field_name: str) -> float | None:
    if value in (None, ""):
        return None
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise DivingFishError(f"{field_name} 必须是数字。", code="INVALID_INPUT")
    return float(value)


def normalize_b50_sort_by(value: Any) -> str:
    if value in (None, ""):
        return "default"
    if not isinstance(value, str):
        raise DivingFishError("sortBy 必须是字符串。", code="INVALID_INPUT")
    aliases = {
        "default": "default",
        "b50": "default",
        "ra": "ra",
        "songra": "ra",
        "achievement": "achievement",
        "achievements": "achievement",
        "ach": "achievement",
        "ds": "ds",
        "constant": "ds",
        "fitdiff": "fitDiff",
        "fit_diff": "fitDiff",
        "fit": "fitDiff",
        "fitted": "fitDiff",
        "fitdelta": "fitDelta",
        "fit_delta": "fitDelta",
        "delta": "fitDelta",
        "title": "title",
    }
    normalized = aliases.get(value.replace(" ", "").replace("-", "_").lower())
    if normalized is None:
        raise DivingFishError(
            "sortBy 必须是 default、ra、achievement、ds、fitDiff、fitDelta 或 title。",
            code="INVALID_INPUT",
        )
    return normalized


def normalize_b50_sort_order(value: Any) -> str:
    if value in (None, ""):
        return "desc"
    if not isinstance(value, str) or value not in {"desc", "asc"}:
        raise DivingFishError("sortOrder 必须是 desc 或 asc。", code="INVALID_INPUT")
    return value


def normalize_b50_display_options(arguments: dict[str, Any]) -> dict[str, Any]:
    return {
        "sortBy": normalize_b50_sort_by(arguments.get("sortBy")),
        "sortOrder": normalize_b50_sort_order(arguments.get("sortOrder")),
        "level": normalize_identifier(arguments.get("level")),
        "difficulty": normalize_identifier(arguments.get("difficulty")),
        "dsMin": normalize_optional_number(arguments.get("dsMin"), "dsMin"),
        "dsMax": normalize_optional_number(arguments.get("dsMax"), "dsMax"),
        "achievementMin": normalize_optional_number(arguments.get("achievementMin"), "achievementMin"),
        "achievementMax": normalize_optional_number(arguments.get("achievementMax"), "achievementMax"),
        "raMin": normalize_optional_number(arguments.get("raMin"), "raMin"),
        "raMax": normalize_optional_number(arguments.get("raMax"), "raMax"),
        "fitDiffMin": normalize_optional_number(arguments.get("fitDiffMin"), "fitDiffMin"),
        "fitDiffMax": normalize_optional_number(arguments.get("fitDiffMax"), "fitDiffMax"),
        "fitDeltaMin": normalize_optional_number(arguments.get("fitDeltaMin"), "fitDeltaMin"),
        "fitDeltaMax": normalize_optional_number(arguments.get("fitDeltaMax"), "fitDeltaMax"),
        "fitLabel": normalize_fit_label(arguments.get("fitLabel")),
    }


def b50_display_controls_active(options: dict[str, Any]) -> bool:
    if options.get("sortBy") not in (None, "default"):
        return True
    if options.get("sortOrder") not in (None, "desc"):
        return True
    return any(
        options.get(key) not in (None, "")
        for key in (
            "level",
            "difficulty",
            "dsMin",
            "dsMax",
            "achievementMin",
            "achievementMax",
            "raMin",
            "raMax",
            "fitDiffMin",
            "fitDiffMax",
            "fitDeltaMin",
            "fitDeltaMax",
            "fitLabel",
        )
    )


def normalize_fit_label(value: Any) -> str | None:
    label = normalize_identifier(value)
    if label is None:
        return None
    aliases = {
        "虚高": "虚高",
        "高": "虚高",
        "over": "虚高",
        "overrated": "虚高",
        "虚低": "虚低",
        "低": "虚低",
        "under": "虚低",
        "underrated": "虚低",
    }
    normalized = aliases.get(label.casefold())
    if normalized is None:
        raise DivingFishError("fitLabel 必须是 虚高 或 虚低。", code="INVALID_INPUT")
    return normalized


def query_b50_should_include_chart_metadata(arguments: dict[str, Any]) -> bool:
    return arguments.get("includeChartMetadata") is not False


def b50_cache_metadata_quality(b50: Any) -> int:
    """Small quality score for player_cache B50 metadata.

    0 = no metadata / explicitly skipped, 1 = metadata attempted, 2 = usable
    fitIndex. Fast B50 render queries should not downgrade richer cache entries.
    """
    if not isinstance(b50, dict):
        return 0
    fit_index = b50.get("fitIndex") if isinstance(b50.get("fitIndex"), dict) else None
    if fit_index and fit_index.get("available"):
        return 2
    metadata = b50.get("chartMetadata") if isinstance(b50.get("chartMetadata"), dict) else None
    if not metadata or metadata.get("skipped") is True:
        return 0
    return 1


def player_b50_cache_should_write(existing_entry: Any, result: dict[str, Any]) -> bool:
    if not _is_player_b50_fresh(existing_entry):
        return True
    existing_b50 = existing_entry.get("b50") if isinstance(existing_entry, dict) else None
    existing_quality = b50_cache_metadata_quality(existing_b50)
    result_quality = b50_cache_metadata_quality(result)
    if result_quality > existing_quality:
        return True
    if result_quality < existing_quality:
        return False
    existing_rating = None
    if isinstance(existing_b50, dict):
        existing_player = existing_b50.get("player") if isinstance(existing_b50.get("player"), dict) else {}
        existing_rating = existing_player.get("rating")
    player = result.get("player") if isinstance(result.get("player"), dict) else {}
    return player.get("rating") != existing_rating


def maimai_local_search_timeout_ms(timeout_ms: int) -> int:
    configured = os.environ.get("MAIMAI_LOCAL_SEARCH_TIMEOUT_MS")
    if configured not in (None, ""):
        return normalize_timeout_ms(configured)
    return max(timeout_ms, 30000)


def query_b50_batch_should_include_chart_metadata(
    arguments: dict[str, Any],
    *,
    include_summaries: bool,
    display_options: dict[str, Any],
) -> bool:
    if arguments.get("includeChartMetadata") is True:
        return True
    if arguments.get("includeChartMetadata") is False:
        return False
    return include_summaries or b50_display_controls_active(display_options)


# 历史遗留：parse_mcp_args_env / default_maimai_local_search_cwd 是 subprocess 时代
# 用来定位 maimai-local-search 启动命令和工作目录的；现在 maimai_mcp 直接 import，
# 这两个函数已经没有调用方，所以删掉。


class MaimaiLocalSearchClient:
    """In-process wrapper around maimai_mcp tools.

    Previously this spawned the maimai-local-search MCP as a stdio subprocess
    and talked JSON-RPC over its pipes. After both workspaces were merged into
    one Python project, maimai_mcp is a sibling package — we just call
    handle_tool_call directly. Same call_tool(name, arguments) interface so
    every existing caller and `with` block keeps working unchanged.

    Bonus: maimai_mcp's lru_cache on the data loaders is shared across all
    calls in this process, so after the first lookup every subsequent search
    is a pure in-memory hit (<1ms vs. ~150ms subprocess startup).
    """

    def __init__(self, *, timeout_ms: int) -> None:
        # timeout_ms was meaningful for subprocess wait; in-process calls
        # complete in microseconds-to-low-milliseconds, so it's only kept for
        # API compatibility with the old client.
        self.timeout_ms = timeout_ms
        self._next_id = 1

    def __enter__(self) -> "MaimaiLocalSearchClient":
        return self

    def __exit__(self, *_args: object) -> None:
        return None

    def call_tool(self, tool_name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        # Lazy import so this module can still load if maimai_mcp is somehow
        # absent from PYTHONPATH (e.g. the package is being unit-tested in
        # isolation). Top-level import would couple them more tightly than
        # needed and pull in maimai_mcp's data loaders on every diving-fish
        # MCP startup.
        from maimai_mcp.server import handle_tool_call

        envelope = handle_tool_call(self._next_id, {"name": tool_name, "arguments": arguments})
        self._next_id += 1
        if isinstance(envelope, dict) and envelope.get("error"):
            err = envelope.get("error") or {}
            raise DivingFishError(
                f"maimai_mcp 工具错误：{err.get('message') or err}",
                code="MAIMAI_MCP_ERROR",
            )
        result = envelope.get("result") if isinstance(envelope, dict) else None
        if not isinstance(result, dict):
            raise DivingFishError("maimai_mcp 返回了无效 result。", code="MAIMAI_MCP_ERROR")
        return result


@contextmanager
def _maimai_search_session(
    existing: "MaimaiLocalSearchClient | None",
    *,
    timeout_ms: int,
):
    """提供 MaimaiLocalSearchClient：传入已 enter 的就复用，None 就新建一次性。

    用于 query_b50_batch 批量复用一个 maimai-local-search 子进程，避免每首 B50 都重启。
    """
    if existing is not None:
        yield existing
        return
    with MaimaiLocalSearchClient(timeout_ms=timeout_ms) as own_client:
        yield own_client


def extract_mcp_text(response: dict[str, Any]) -> str:
    content = response.get("content")
    if isinstance(content, list) and content:
        first = content[0]
        if isinstance(first, dict) and isinstance(first.get("text"), str):
            return first["text"]
    return ""


def call_maimai_search_json(
    client: MaimaiLocalSearchClient,
    arguments: dict[str, Any],
) -> dict[str, Any] | None:
    response = client.call_tool("search_maimai_songs", arguments)
    if response.get("isError"):
        return None
    text = extract_mcp_text(response)
    try:
        parsed = json.loads(text)
    except json.JSONDecodeError:
        return None
    return parsed if isinstance(parsed, dict) else None


def call_maimai_batch_search_json(
    client: MaimaiLocalSearchClient,
    items: list[dict[str, Any]],
) -> dict[str, Any] | None:
    response = client.call_tool("batch_search_maimai_songs", {"items": items, "format": "json"})
    if response.get("isError"):
        return None
    text = extract_mcp_text(response)
    try:
        parsed = json.loads(text)
    except json.JSONDecodeError:
        return None
    return parsed if isinstance(parsed, dict) else None


def call_maimai_batch_search_json_chunks(
    client: MaimaiLocalSearchClient,
    items: list[dict[str, Any]],
    results_by_key: dict[str, dict[str, Any]],
    *,
    chunk_size: int = 200,
) -> int:
    searched = 0
    for offset in range(0, len(items), chunk_size):
        chunk = items[offset : offset + chunk_size]
        batch = call_maimai_batch_search_json(client, chunk)
        searched += len(chunk)
        if not merge_b50_batch_results(results_by_key, batch):
            raise DivingFishError(
                "maimai-local-search 批量搜索没有返回 JSON。",
                code="MAIMAI_MCP_ERROR",
            )
    return searched


def merge_b50_batch_results(
    results_by_key: dict[str, dict[str, Any]],
    batch: dict[str, Any] | None,
) -> bool:
    if not isinstance(batch, dict):
        return False
    for item in batch.get("items") or []:
        if isinstance(item, dict) and item.get("key") is not None:
            results_by_key[str(item.get("key"))] = item
    return True


def unmatched_b50_songs(
    results_by_key: dict[str, dict[str, Any]],
    songs: list[tuple[str, int, dict[str, Any]]],
) -> list[tuple[str, int, dict[str, Any]]]:
    return [
        (section, index, song)
        for section, index, song in songs
        if find_b50_search_chart(results_by_key, section, index, song) is None
    ]


_CACHE_NOT_FOUND = object()


def b50_chart_metadata_cache_key(song: dict[str, Any]) -> str | None:
    song_id = str(song.get("songId") or "").strip()
    chart_type = local_search_song_type(song) or ""
    difficulty = b50_difficulty_index(song)
    difficulty_key = str(difficulty) if difficulty is not None else str(song.get("levelLabel") or "").strip().casefold()
    if song_id:
        return f"id:{song_id}|type:{chart_type}|diff:{difficulty_key}"
    title = str(song.get("title") or "").strip().casefold()
    if title:
        level = str(song.get("level") or "").strip()
        return f"title:{title}|type:{chart_type}|diff:{difficulty_key}|level:{level}"
    return None


def get_b50_chart_metadata_cache_entry(
    cache: dict[str, dict[str, Any] | None] | None,
    lock: threading.Lock | None,
    key: str | None,
) -> object:
    if cache is None or key is None:
        return _CACHE_NOT_FOUND
    if lock is None:
        return cache.get(key, _CACHE_NOT_FOUND)
    with lock:
        return cache.get(key, _CACHE_NOT_FOUND)


def set_b50_chart_metadata_cache_entry(
    cache: dict[str, dict[str, Any] | None] | None,
    lock: threading.Lock | None,
    key: str | None,
    chart: dict[str, Any] | None,
) -> None:
    if cache is None or key is None:
        return
    value = dict(chart) if isinstance(chart, dict) else None
    if lock is None:
        cache[key] = value
        return
    with lock:
        cache[key] = value


def b50_chart_metadata_cache_size(cache: dict[str, dict[str, Any] | None] | None, lock: threading.Lock | None) -> int:
    if cache is None:
        return 0
    if lock is None:
        return len(cache)
    with lock:
        return len(cache)


def enrich_b50_with_maimai_local_search(
    result: dict[str, Any],
    *,
    timeout_ms: int,
    client: "MaimaiLocalSearchClient | None" = None,
    metadata_cache: dict[str, dict[str, Any] | None] | None = None,
    metadata_cache_lock: threading.Lock | None = None,
) -> None:
    """补拟合定数到 B50 result。

    传入 client（已 enter 的 MaimaiLocalSearchClient）时复用它，不再起新子进程；
    用于 query_b50_batch 这种连续多个查询的场景。
    None 时和原行为一致：内部 with 一个新 client。
    """
    charts = result.get("charts") if isinstance(result.get("charts"), dict) else {}
    songs: list[tuple[str, int, dict[str, Any]]] = []
    for section in ("sd", "dx"):
        for index, song in enumerate(charts.get(section) or []):
            if isinstance(song, dict):
                songs.append((section, index, song))

    if not songs:
        result["chartMetadata"] = {
            "source": "maimai-local-search",
            "available": True,
            "matched": 0,
            "missing": 0,
        }
        compute_b50_fit_index(result)
        return

    search_songs: list[tuple[str, int, dict[str, Any]]] = []
    metadata_cache_hits = 0
    metadata_cache_missing_hits = 0
    for section, index, song in songs:
        key = b50_chart_metadata_cache_key(song)
        cached = get_b50_chart_metadata_cache_entry(metadata_cache, metadata_cache_lock, key)
        if cached is _CACHE_NOT_FOUND:
            search_songs.append((section, index, song))
            continue
        if isinstance(cached, dict):
            attach_b50_chart_metadata(song, cached)
            metadata_cache_hits += 1
        else:
            metadata_cache_missing_hits += 1

    if not search_songs:
        result["chartMetadata"] = {
            "source": "maimai-local-search",
            "available": True,
            "requested": len(songs),
            "searchItems": 0,
            "idSearchItems": 0,
            "fallbackSearchItems": 0,
            "matched": metadata_cache_hits,
            "missing": metadata_cache_missing_hits,
            "metadataCacheHits": metadata_cache_hits,
            "metadataCacheMissingHits": metadata_cache_missing_hits,
            "metadataCacheSize": b50_chart_metadata_cache_size(metadata_cache, metadata_cache_lock),
        }
        compute_b50_fit_index(result)
        return

    id_items: list[dict[str, Any]] = []
    for section, index, song in search_songs:
        id_items.extend(build_b50_batch_search_items(section, index, song, include_title=False))

    initial_items = id_items
    initial_fallback_items_count = 0
    if not initial_items:
        initial_items = []
        for section, index, song in search_songs:
            initial_items.extend(build_b50_batch_search_items(section, index, song, include_id=False))
        initial_fallback_items_count = len(initial_items)

    if not initial_items:
        result["chartMetadata"] = {
            "source": "maimai-local-search",
            "available": True,
            "requested": len(songs),
            "searchItems": 0,
            "matched": metadata_cache_hits,
            "missing": metadata_cache_missing_hits + len(search_songs),
            "metadataCacheHits": metadata_cache_hits,
            "metadataCacheMissingHits": metadata_cache_missing_hits,
            "metadataCacheSize": b50_chart_metadata_cache_size(metadata_cache, metadata_cache_lock),
        }
        compute_b50_fit_index(result)
        return

    results_by_key: dict[str, dict[str, Any]] = {}
    search_items = 0
    fallback_items_count = initial_fallback_items_count
    local_timeout_ms = maimai_local_search_timeout_ms(timeout_ms)
    try:
        with _maimai_search_session(client, timeout_ms=local_timeout_ms) as active_client:
            batch = call_maimai_batch_search_json(active_client, initial_items)
            search_items += len(initial_items)
            if not merge_b50_batch_results(results_by_key, batch):
                raise DivingFishError(
                    "maimai-local-search 批量搜索没有返回 JSON。",
                    code="MAIMAI_MCP_ERROR",
                )

            if id_items:
                fallback_items: list[dict[str, Any]] = []
                for section, index, song in unmatched_b50_songs(results_by_key, search_songs):
                    fallback_items.extend(
                        build_b50_batch_search_items(section, index, song, include_id=False)
                    )
                if fallback_items:
                    fallback_batch = call_maimai_batch_search_json(active_client, fallback_items)
                    search_items += len(fallback_items)
                    fallback_items_count += len(fallback_items)
                    if not merge_b50_batch_results(results_by_key, fallback_batch):
                        raise DivingFishError(
                            "maimai-local-search 标题兜底搜索没有返回 JSON。",
                            code="MAIMAI_MCP_ERROR",
                        )
    except DivingFishError as exc:
        result["chartMetadata"] = {
            "source": "maimai-local-search",
            "available": False,
            "error": exc.to_dict(),
            "matched": metadata_cache_hits,
            "missing": len(songs) - metadata_cache_hits,
            "metadataCacheHits": metadata_cache_hits,
            "metadataCacheMissingHits": metadata_cache_missing_hits,
            "metadataCacheSize": b50_chart_metadata_cache_size(metadata_cache, metadata_cache_lock),
        }
        compute_b50_fit_index(result)
        return
    matched = metadata_cache_hits
    missing = metadata_cache_missing_hits
    for section, index, song in search_songs:
        chart = find_b50_search_chart(results_by_key, section, index, song)
        key = b50_chart_metadata_cache_key(song)
        if chart is None:
            set_b50_chart_metadata_cache_entry(metadata_cache, metadata_cache_lock, key, None)
            missing += 1
            continue
        attach_b50_chart_metadata(song, chart)
        set_b50_chart_metadata_cache_entry(metadata_cache, metadata_cache_lock, key, chart)
        matched += 1

    result["chartMetadata"] = {
        "source": "maimai-local-search",
        "available": True,
        "requested": len(songs),
        "searchItems": search_items,
        "idSearchItems": len(id_items),
        "fallbackSearchItems": fallback_items_count,
        "timeoutMs": local_timeout_ms,
        "matched": matched,
        "missing": missing,
        "metadataCacheHits": metadata_cache_hits,
        "metadataCacheMissingHits": metadata_cache_missing_hits,
        "metadataCacheSize": b50_chart_metadata_cache_size(metadata_cache, metadata_cache_lock),
    }
    compute_b50_fit_index(result)


def build_b50_batch_search_items(
    section: str,
    index: int,
    song: dict[str, Any],
    *,
    include_id: bool = True,
    include_title: bool = True,
) -> list[dict[str, Any]]:
    base = {
        "level": song.get("level"),
        "difficulty": song.get("levelLabel"),
        "song_type": local_search_song_type(song),
        "limit": 5,
    }
    base = {key: value for key, value in base.items() if value not in (None, "")}
    items = []
    song_id = song.get("songId")
    if include_id and song_id not in (None, ""):
        items.append({**base, "key": f"{section}:{index}:id", "query": f"id{song_id}"})
    title = song.get("title")
    if include_title and title:
        items.append({**base, "key": f"{section}:{index}:title", "query": title})
    return items


def local_search_song_type(song: dict[str, Any]) -> str | None:
    raw_type = str(song.get("type") or "").casefold()
    if raw_type == "dx":
        return "dx"
    if raw_type in {"sd", "std", "standard"}:
        return "standard"
    return None


def find_b50_search_chart(
    results_by_key: dict[str, dict[str, Any]],
    section: str,
    index: int,
    song: dict[str, Any],
) -> dict[str, Any] | None:
    for suffix in ("id", "title"):
        item = results_by_key.get(f"{section}:{index}:{suffix}")
        if not item or item.get("ok") is not True:
            continue
        result = item.get("result") if isinstance(item.get("result"), dict) else {}
        chart = select_b50_chart_from_search_result(result, song)
        if chart is not None:
            return chart
    return None


def select_b50_chart_from_search_result(result: dict[str, Any], song: dict[str, Any]) -> dict[str, Any] | None:
    expected_type = local_search_song_type(song)
    expected_difficulty_index = b50_difficulty_index(song)
    candidates: list[dict[str, Any]] = []
    for found_song in result.get("songs") or []:
        if not isinstance(found_song, dict):
            continue
        for chart in found_song.get("matched_charts") or []:
            if not isinstance(chart, dict):
                continue
            if expected_type and chart.get("chart_type") != expected_type:
                continue
            if expected_difficulty_index is not None and chart.get("difficulty_index") != expected_difficulty_index:
                continue
            candidates.append(chart)
    return candidates[0] if candidates else None


def b50_difficulty_index(song: dict[str, Any]) -> int | None:
    value = song.get("levelIndex")
    if isinstance(value, int):
        return value
    if isinstance(value, float) and value.is_integer():
        return int(value)
    label = str(song.get("levelLabel") or "").replace(":", "").replace(" ", "").casefold()
    return {
        "basic": 0,
        "advanced": 1,
        "expert": 2,
        "master": 3,
        "remaster": 4,
    }.get(label)


def compute_b50_fit_index(result: dict[str, Any]) -> None:
    charts = result.get("charts") if isinstance(result.get("charts"), dict) else {}
    sections = {
        "b35": list(charts.get("sd") or []),
        "b15": list(charts.get("dx") or []),
    }
    sections["b50"] = sections["b35"] + sections["b15"]

    sub_indexes: dict[str, dict[str, Any]] = {}
    for key, songs in sections.items():
        sub_indexes[key] = _compute_fit_index_for_section(songs)

    b50 = sub_indexes["b50"]
    available = bool(b50.get("counted"))
    result["fitIndex"] = {
        "available": available,
        "label": fit_index_label(b50.get("virtualRatio")) if available else None,
        "b50": b50,
        "b35": sub_indexes["b35"],
        "b15": sub_indexes["b15"],
    }


def maimai_dx_ra(ds: float, achievements: float) -> int:
    """单曲 DX Rating = floor(定数 × min(达成率%, 100.5) ÷ 100 × 评级系数)，向下取整。"""
    capped = min(achievements, 100.5)
    coefficient = _maimai_dx_coefficient(capped)
    return int(ds * (capped / 100.0) * coefficient)


def _maimai_dx_coefficient(achievements: float) -> float:
    # 达成率从高到低分段查评级系数
    if achievements >= 100.5:
        return 22.4
    if achievements >= 100.0:
        return 21.6
    if achievements >= 99.5:
        return 21.1
    if achievements >= 99.0:
        return 20.8
    if achievements >= 98.0:
        return 20.3
    if achievements >= 97.0:
        return 20.0
    if achievements >= 94.0:
        return 16.8
    if achievements >= 90.0:
        return 15.2
    if achievements >= 80.0:
        return 13.6
    if achievements >= 75.0:
        return 12.0
    if achievements >= 70.0:
        return 11.2
    if achievements >= 60.0:
        return 9.6
    if achievements >= 50.0:
        return 8.0
    if achievements >= 40.0:
        return 6.4
    if achievements >= 30.0:
        return 4.8
    if achievements >= 20.0:
        return 3.2
    if achievements >= 10.0:
        return 1.6
    return 0.0


def _compute_fit_index_for_section(songs: list[dict[str, Any]]) -> dict[str, Any]:
    total_virtual_ra = 0.0
    total_ra = 0.0
    weighted_delta_sum = 0.0
    counted = 0
    missing = 0
    for song in songs:
        if not isinstance(song, dict):
            continue
        ra = song.get("originalRa", song.get("ra"))
        ds = song.get("ds")
        fit_diff = song.get("fitDiff")
        achievements = song.get("achievements")
        if (
            isinstance(ra, (int, float))
            and not isinstance(ra, bool)
            and isinstance(ds, (int, float))
            and not isinstance(ds, bool)
            and ds > 0
            and isinstance(fit_diff, (int, float))
            and not isinstance(fit_diff, bool)
            and isinstance(achievements, (int, float))
            and not isinstance(achievements, bool)
        ):
            ra_f = float(ra)
            ds_f = float(ds)
            fit_diff_f = float(fit_diff)
            ach_f = float(achievements)
            actual_ra = int(ra_f)
            fitted_ra = maimai_dx_ra(fit_diff_f, ach_f)
            virtual_ra = actual_ra - fitted_ra
            total_virtual_ra += float(virtual_ra)
            weighted_delta_sum += ra_f * (ds_f - fit_diff_f)
            total_ra += ra_f
            counted += 1
        else:
            missing += 1
    virtual_ratio = (total_virtual_ra / total_ra * 100.0) if total_ra > 0 else None
    weighted_avg_delta = (weighted_delta_sum / total_ra) if total_ra > 0 else None
    return {
        "virtualRating": total_virtual_ra if counted else None,
        "virtualRatio": virtual_ratio,
        "weightedAvgFitDelta": weighted_avg_delta,
        "counted": counted,
        "missing": missing,
        "totalRa": total_ra if counted else None,
    }


def summarize_fit_index_for_batch(fit_index: Any) -> dict[str, Any] | None:
    if not isinstance(fit_index, dict):
        return None
    b50 = fit_index.get("b50") if isinstance(fit_index.get("b50"), dict) else {}
    if not b50.get("counted"):
        return {
            "available": False,
            "label": "数据不足",
            "virtualRating": None,
            "virtualRatio": None,
            "counted": 0,
            "missing": b50.get("missing") or 0,
        }
    return {
        "available": True,
        "label": fit_index.get("label"),
        "virtualRating": b50.get("virtualRating"),
        "virtualRatio": b50.get("virtualRatio"),
        "counted": b50.get("counted"),
        "missing": b50.get("missing") or 0,
    }


def fit_index_label(virtual_ratio_percent: float | None) -> str:
    if virtual_ratio_percent is None:
        return "数据不足"
    score = float(virtual_ratio_percent)
    if score > 1.0:
        return "明显虚高（水）"
    if score > 0.2:
        return "略微虚高"
    if score < -1.0:
        return "明显虚低（硬实力）"
    if score < -0.2:
        return "略微虚低"
    return "基本持平"


def attach_b50_chart_metadata(song: dict[str, Any], chart: dict[str, Any]) -> None:
    if chart.get("fit_diff") is not None:
        song["fitDiff"] = chart.get("fit_diff")
    if chart.get("fit_delta") is not None:
        song["fitDelta"] = chart.get("fit_delta")
    if chart.get("fit_label") is not None:
        song["fitLabel"] = chart.get("fit_label")
    if chart.get("fit_stats") is not None:
        song["fitStats"] = chart.get("fit_stats")
    if chart.get("fit_source_id") is not None:
        song["fitSourceId"] = chart.get("fit_source_id")
    song["localSearchChart"] = {
        key: chart.get(key)
        for key in (
            "source",
            "source_name",
            "chart_type",
            "difficulty",
            "difficulty_index",
            "level",
            "ds",
            "version",
        )
        if chart.get(key) is not None
    }


def query_b50(
    arguments: dict[str, Any],
    *,
    _shared_maimai_client: "MaimaiLocalSearchClient | None" = None,
    _shared_chart_metadata_cache: dict[str, dict[str, Any] | None] | None = None,
    _shared_chart_metadata_cache_lock: threading.Lock | None = None,
) -> dict[str, Any]:
    """单人 B50 查询。

    _shared_maimai_client：批量场景下由 query_b50_batch 传入复用，避免每个成员都
    重新 spawn 一个 maimai-local-search 子进程。普通单人调用保持原行为（None）。
    """
    lookup = validate_lookup(arguments)
    timeout_ms = normalize_timeout_ms(arguments.get("timeoutMs"))
    group_id = normalize_identifier(arguments.get("groupId"))
    raw = post_diving_fish({**lookup, "b50": "1"}, timeout_ms=timeout_ms)
    result = normalize_b50_response(raw, lookup, group_id=group_id)
    if query_b50_should_include_chart_metadata(arguments):
        enrich_b50_with_maimai_local_search(
            result,
            timeout_ms=timeout_ms,
            client=_shared_maimai_client,
            metadata_cache=_shared_chart_metadata_cache,
            metadata_cache_lock=_shared_chart_metadata_cache_lock,
        )
    else:
        result["chartMetadata"] = {
            "source": "maimai-local-search",
            "skipped": True,
            "available": False,
            "matched": 0,
            "missing": result.get("counts", {}).get("total", 0),
        }
    # 顺手把按 qq 查到的 B50 写入跨 MCP 共享的 player_cache，让群榜刷新可短路水鱼调用。
    # 按 username 查的不写（没 qq 无法定位）。旧缓存还在 TTL 内时，只有 rating 改变
    # 或这次结果包含更完整的拟合定数/fitIndex 时才覆盖，避免快速渲染的轻量结果冲掉富缓存。
    if lookup.get("qq"):
        try:
            existing = _read_player_b50(lookup["qq"])
            if player_b50_cache_should_write(existing, result):
                _write_player_b50_cache(lookup["qq"], result)
        except Exception:
            pass
    return result


def query_computed_b50(arguments: dict[str, Any]) -> dict[str, Any]:
    """Compute fitted B35/B15 from full player records instead of /query/player B50.

    The B15/new split uses only the local Diving-Fish song list:
    the latest ranked `basic_info.from` version provides the current version set.
    No LXNS/CN version code is consulted here.

    Candidate charts are sorted by fitted single-song rating:
    `maimai_dx_ra(fitDiff, achievements)`.
    """
    lookup = validate_lookup(arguments)
    timeout_ms = normalize_timeout_ms(arguments.get("timeoutMs"))
    group_id = normalize_identifier(arguments.get("groupId"))
    records_result = query_maimai_player_records(
        {
            **lookup,
            "timeoutMs": timeout_ms,
            "groupId": group_id,
        }
    )
    return compute_b50_from_records(records_result, lookup, group_id=group_id, timeout_ms=timeout_ms)


def compute_b50_from_records(
    records_result: dict[str, Any],
    lookup: dict[str, str],
    *,
    group_id: str | None = None,
    timeout_ms: int = 10000,
) -> dict[str, Any]:
    records = records_result.get("records") if isinstance(records_result.get("records"), list) else []
    current_versions = _divingfish_current_versions()
    candidates: list[dict[str, Any]] = []
    deduped: dict[tuple[Any, str, Any], dict[str, Any]] = {}
    skipped = {
        "nonScoreType": 0,
        "missingRating": 0,
        "missingAchievement": 0,
        "missingVersion": 0,
        "missingFitDiff": 0,
        "missingFittedRating": 0,
        "duplicateLowerRa": 0,
    }

    for record in records:
        if not isinstance(record, dict):
            continue
        song_type = str(record.get("type") or "").upper()
        if song_type not in {"SD", "DX"}:
            skipped["nonScoreType"] += 1
            continue
        achievements = record.get("achievements")
        if not isinstance(achievements, (int, float)) or isinstance(achievements, bool):
            skipped["missingAchievement"] += 1
            continue
        version = _record_waterfish_version(record, allow_record_fallback=False)
        if not version:
            skipped["missingVersion"] += 1
            continue
        item = dict(record)
        item["version"] = version
        item["b50Section"] = "dx" if version in current_versions else "sd"
        candidates.append(item)

    chart_metadata = attach_fitted_b50_record_metadata(candidates, timeout_ms=timeout_ms)

    for item in candidates:
        fit_diff = item.get("fitDiff")
        achievements = item.get("achievements")
        if (
            not isinstance(fit_diff, (int, float))
            or isinstance(fit_diff, bool)
            or not isinstance(achievements, (int, float))
            or isinstance(achievements, bool)
        ):
            skipped["missingFitDiff"] += 1
            continue
        fitted_ra = maimai_dx_ra(float(fit_diff), float(achievements))
        if fitted_ra <= 0:
            skipped["missingFittedRating"] += 1
            continue
        original_ra = item.get("ra")
        if isinstance(original_ra, (int, float)) and not isinstance(original_ra, bool):
            item["originalRa"] = int(original_ra)
        item["fittedRa"] = fitted_ra
        item["ra"] = fitted_ra
        item["ratingBase"] = "fitDiff"
        song_type = str(item.get("type") or "").upper()
        key = (
            item.get("songId"),
            song_type,
            item.get("levelIndex"),
        )
        existing = deduped.get(key)
        if existing is not None and _computed_b50_sort_key(existing) >= _computed_b50_sort_key(item):
            skipped["duplicateLowerRa"] += 1
            continue
        if existing is not None:
            skipped["duplicateLowerRa"] += 1
        deduped[key] = item

    sd_candidates = [item for item in deduped.values() if item.get("b50Section") == "sd"]
    dx_candidates = [item for item in deduped.values() if item.get("b50Section") == "dx"]
    sd = sorted(sd_candidates, key=_computed_b50_sort_key, reverse=True)[:35]
    dx = sorted(dx_candidates, key=_computed_b50_sort_key, reverse=True)[:15]
    sd_rating = sum(int(song.get("fittedRa") or song.get("ra") or 0) for song in sd)
    dx_rating = sum(int(song.get("fittedRa") or song.get("ra") or 0) for song in dx)

    raw_player = records_result.get("player") if isinstance(records_result.get("player"), dict) else {}
    actual_rating = optional_number(raw_player.get("rating"))
    player_for_identity = {
        "nickname": optional_string(raw_player.get("nickname")),
        "username": optional_string(raw_player.get("username") or lookup.get("username")),
        "rating": actual_rating,
        "additionalRating": optional_number(raw_player.get("additionalRating")),
        "plate": optional_string(raw_player.get("plate")),
    }
    identity = load_and_update_identity(lookup, player_for_identity, group_id=group_id)
    computed_rating = sd_rating + dx_rating
    player = {
        **player_for_identity,
        "rating": computed_rating,
        "actualRating": actual_rating,
    }

    result = {
        "source": "diving-fish-records",
        "endpoint": records_result.get("endpoint", "/dev/player/records"),
        "lookup": lookup,
        "requestedAt": datetime.now(timezone.utc).isoformat(),
        "player": player,
        "identity": identity,
        "counts": {
            "sd": len(sd),
            "dx": len(dx),
            "total": len(sd) + len(dx),
        },
        "ratingBreakdown": {
            "sd": sd_rating,
            "dx": dx_rating,
            "total": computed_rating,
        },
        "charts": {
            "sd": sd,
            "dx": dx,
        },
        "chartMetadata": chart_metadata,
        "computedB50": {
            "source": "playerRecords",
            "ratingSource": "maimai-local-search fitDiff",
            "versionSource": "latest diving-fish basic_info.from",
            "currentVersions": sorted(current_versions),
            "recordCount": len(records),
            "versionEligibleCount": len(candidates),
            "eligibleCount": len(deduped),
            "sdCandidateCount": len(sd_candidates),
            "dxCandidateCount": len(dx_candidates),
            "skipped": skipped,
            "actualRating": actual_rating,
            "computedRating": computed_rating,
        },
    }
    compute_b50_fit_index(result)
    return result


def attach_fitted_b50_record_metadata(candidates: list[dict[str, Any]], *, timeout_ms: int) -> dict[str, Any]:
    search_songs: list[tuple[str, int, dict[str, Any]]] = []
    for index, item in enumerate(candidates):
        section = str(item.get("b50Section") or "sd")
        search_songs.append((section, index, item))

    if not search_songs:
        return {
            "source": "maimai-local-search",
            "available": True,
            "requested": 0,
            "matched": 0,
            "missing": 0,
        }

    requested = len(search_songs)
    search_songs, fast_matched = attach_fitted_b50_record_metadata_fast(search_songs)
    if not search_songs:
        return {
            "source": "maimai-local-index",
            "available": True,
            "requested": requested,
            "searchItems": 0,
            "idSearchItems": 0,
            "fallbackSearchItems": 0,
            "matched": fast_matched,
            "chartMatched": fast_matched,
            "missing": 0,
            "fastMatched": fast_matched,
        }

    id_items: list[dict[str, Any]] = []
    for section, index, song in search_songs:
        id_items.extend(build_b50_batch_search_items(section, index, song, include_title=False))

    initial_items = id_items
    initial_fallback_items_count = 0
    if not initial_items:
        initial_items = []
        for section, index, song in search_songs:
            initial_items.extend(build_b50_batch_search_items(section, index, song, include_id=False))
        initial_fallback_items_count = len(initial_items)

    if not initial_items:
        return {
            "source": "maimai-local-index+search",
            "available": True,
            "requested": requested,
            "searchItems": 0,
            "matched": fast_matched,
            "chartMatched": fast_matched,
            "missing": requested - fast_matched,
            "fastMatched": fast_matched,
        }

    results_by_key: dict[str, dict[str, Any]] = {}
    search_items = 0
    fallback_items_count = initial_fallback_items_count
    local_timeout_ms = maimai_local_search_timeout_ms(timeout_ms)
    with MaimaiLocalSearchClient(timeout_ms=local_timeout_ms) as active_client:
        search_items += call_maimai_batch_search_json_chunks(active_client, initial_items, results_by_key)

        if id_items:
            fallback_items: list[dict[str, Any]] = []
            for section, index, song in unmatched_b50_songs(results_by_key, search_songs):
                fallback_items.extend(build_b50_batch_search_items(section, index, song, include_id=False))
            if fallback_items:
                search_items += call_maimai_batch_search_json_chunks(active_client, fallback_items, results_by_key)
                fallback_items_count += len(fallback_items)

    chart_matched = 0
    usable = 0
    for section, index, song in search_songs:
        chart = find_b50_search_chart(results_by_key, section, index, song)
        if chart is None:
            continue
        attach_b50_chart_metadata(song, chart)
        chart_matched += 1
        fit_diff = song.get("fitDiff")
        if isinstance(fit_diff, (int, float)) and not isinstance(fit_diff, bool):
            usable += 1

    return {
        "source": "maimai-local-index+search",
        "available": True,
        "requested": requested,
        "searchItems": search_items,
        "idSearchItems": len(id_items),
        "fallbackSearchItems": fallback_items_count,
        "timeoutMs": local_timeout_ms,
        "matched": fast_matched + usable,
        "chartMatched": fast_matched + chart_matched,
        "missing": requested - fast_matched - usable,
        "fastMatched": fast_matched,
    }


def _computed_b50_sort_key(song: dict[str, Any]) -> tuple[float, float, float, str]:
    ra = song.get("fittedRa", song.get("ra"))
    achievements = song.get("achievements")
    ds = song.get("fitDiff", song.get("ds"))
    return (
        float(ra) if isinstance(ra, (int, float)) and not isinstance(ra, bool) else 0.0,
        float(achievements) if isinstance(achievements, (int, float)) and not isinstance(achievements, bool) else 0.0,
        float(ds) if isinstance(ds, (int, float)) and not isinstance(ds, bool) else 0.0,
        str(song.get("title") or ""),
    )


def query_b50_batch(arguments: dict[str, Any]) -> dict[str, Any]:
    qqs = validate_qq_batch(arguments)
    timeout_ms = normalize_timeout_ms(arguments.get("timeoutMs"))
    query_delay_ms = normalize_query_delay_ms(arguments.get("queryDelayMs"))
    max_concurrency = normalize_max_concurrency(arguments.get("maxConcurrency"))
    include_raw = arguments.get("includeRaw") is True
    include_summaries = arguments.get("includeSummaries") is True
    top_n = normalize_top_n(arguments.get("topN"))
    section = arguments.get("section", "b50")
    group_id = normalize_identifier(arguments.get("groupId"))
    display_options = normalize_b50_display_options(arguments)
    progress_callback = arguments.get("_progressCallback")
    include_chart_metadata = query_b50_batch_should_include_chart_metadata(
        arguments,
        include_summaries=include_summaries,
        display_options=display_options,
    )
    if not isinstance(section, str):
        raise DivingFishError("section 必须是字符串。", code="INVALID_INPUT")

    chart_metadata_cache: dict[str, dict[str, Any] | None] | None = {} if include_chart_metadata else None
    chart_metadata_cache_lock = threading.Lock() if include_chart_metadata else None

    def query_one(qq: str, shared_client: "MaimaiLocalSearchClient | None" = None) -> dict[str, Any]:
        try:
            b50 = query_b50(
                {
                    "qq": qq,
                    "timeoutMs": timeout_ms,
                    "groupId": group_id,
                    "includeChartMetadata": include_chart_metadata,
                },
                _shared_maimai_client=shared_client,
                _shared_chart_metadata_cache=chart_metadata_cache,
                _shared_chart_metadata_cache_lock=chart_metadata_cache_lock,
            )
            visible_b50 = public_result(b50, include_raw=include_raw)
            player = visible_b50.get("player") or {}
            rating_breakdown = visible_b50.get("ratingBreakdown") or {}
            return {
                "qq": qq,
                "ok": True,
                "player": player,
                "rating": player.get("rating"),
                "b50Rating": rating_breakdown.get("total"),
                "fitIndex": summarize_fit_index_for_batch(visible_b50.get("fitIndex")),
                "result": visible_b50,
                "summary": format_b50_summary(b50, top_n, section, display_options)
                if include_summaries
                else None,
                "error": None,
            }
        except DivingFishError as exc:
            return {
                "qq": qq,
                "ok": False,
                "player": None,
                "rating": None,
                "b50Rating": None,
                "fitIndex": None,
                "result": None,
                "summary": None,
                "error": exc.to_dict(),
            }

    def emit_progress(completed: int, total: int, item: dict[str, Any]) -> None:
        if not callable(progress_callback):
            return
        try:
            progress_callback(
                {
                    "completed": completed,
                    "total": total,
                    "qq": item.get("qq"),
                    "ok": item.get("ok"),
                    "metadataCacheSize": b50_chart_metadata_cache_size(
                        chart_metadata_cache,
                        chart_metadata_cache_lock,
                    ),
                }
            )
        except Exception:
            pass

    # 批量场景下共享 maimai-local-search 子进程，避免每个成员都重新 spawn 一次。
    # 顺序模式：一个客户端贯穿整批。
    # 并发模式：每个 worker 线程持有自己的客户端（thread-local），整批最多 N 个子进程，
    #          而不是 47 个。clients_registry 用于 finally 阶段统一关闭。
    local_maimai_timeout = (
        maimai_local_search_timeout_ms(timeout_ms) if include_chart_metadata else timeout_ms
    )
    thread_local_clients = threading.local()
    clients_registry: list[MaimaiLocalSearchClient] = []
    registry_lock = threading.Lock()

    def get_thread_client() -> "MaimaiLocalSearchClient | None":
        if not include_chart_metadata:
            return None
        existing = getattr(thread_local_clients, "client", None)
        if existing is not None:
            return existing
        client = MaimaiLocalSearchClient(timeout_ms=local_maimai_timeout)
        client.__enter__()
        thread_local_clients.client = client
        with registry_lock:
            clients_registry.append(client)
        return client

    def close_all_clients() -> None:
        with registry_lock:
            to_close = list(clients_registry)
            clients_registry.clear()
        for client in to_close:
            try:
                client.__exit__(None, None, None)
            except Exception:
                pass

    try:
        if max_concurrency == 1:
            results = []
            shared_client = get_thread_client()
            for index, qq in enumerate(qqs):
                item = query_one(qq, shared_client)
                results.append(item)
                emit_progress(index + 1, len(qqs), item)
                if query_delay_ms > 0 and index < len(qqs) - 1:
                    time.sleep(query_delay_ms / 1000)
        else:
            results: list[dict[str, Any] | None] = [None] * len(qqs)

            def worker(qq: str) -> dict[str, Any]:
                return query_one(qq, get_thread_client())

            with ThreadPoolExecutor(max_workers=max_concurrency) as executor:
                future_to_index = {}
                for index, qq in enumerate(qqs):
                    future_to_index[executor.submit(worker, qq)] = index
                    if query_delay_ms > 0 and index < len(qqs) - 1:
                        time.sleep(query_delay_ms / 1000)
                completed = 0
                for future in as_completed(future_to_index):
                    item = future.result()
                    results[future_to_index[future]] = item
                    completed += 1
                    emit_progress(completed, len(qqs), item)
            results = [item for item in results if item is not None]
    finally:
        close_all_clients()

    success_count = sum(1 for item in results if item["ok"])
    return {
        "source": "diving-fish",
        "endpoint": QUERY_PLAYER_URL,
        "requestedAt": datetime.now(timezone.utc).isoformat(),
        "counts": {
            "requested": len(qqs),
            "success": success_count,
            "failure": len(qqs) - success_count,
        },
        "results": results,
    }


def query_maimai_song_score(arguments: dict[str, Any]) -> dict[str, Any]:
    lookup = validate_lookup(arguments)
    music_id = normalize_music_id(arguments.get("musicId"))
    song_query = normalize_identifier(arguments.get("songQuery"))
    timeout_ms = normalize_timeout_ms(arguments.get("timeoutMs"))

    # songQuery 解析为 musicId
    if song_query and not music_id:
        try:
            search_args: dict[str, Any] = {"query": song_query, "limit": 5, "format": "json"}
            if normalize_identifier(arguments.get("difficulty")):
                search_args["difficulty"] = normalize_identifier(arguments["difficulty"])
            if normalize_identifier(arguments.get("songType")):
                search_args["song_type"] = normalize_identifier(arguments["songType"])
            with MaimaiLocalSearchClient(timeout_ms=30000) as client:
                response = client.call_tool("search_maimai_songs", search_args)
            if not response.get("isError"):
                text = extract_mcp_text(response)
                parsed = json.loads(text) if isinstance(text, str) else {}
                songs = parsed.get("songs", []) if isinstance(parsed, dict) else []
                if songs and isinstance(songs, list):
                    for song in songs:
                        if not isinstance(song, dict):
                            continue
                        df_ids = []
                        for fid in [
                            song.get("source_ids", {}).get("divingfish"),
                            song.get("id"),
                        ]:
                            if fid:
                                try:
                                    mid = normalize_music_id(fid)
                                    if mid:
                                        df_ids.append(mid)
                                except Exception:
                                    pass
                        if df_ids:
                            music_id = df_ids[0]
                            break
        except Exception:
            pass

    if not music_id:
        from . import DivingFishError as _DFE
        raise _DFE("需要提供 musicId 或 songQuery（且能解析到有效曲目）。", code="INVALID_INPUT")
    api_arguments: dict[str, Any] = {
        "operation": "maimai_dev_player_record_post",
        "body": {**lookup, "music_id": [music_id]},
        "timeoutMs": timeout_ms,
    }
    developer_token = normalize_identifier(arguments.get("developerToken"))
    if developer_token:
        api_arguments["developerToken"] = developer_token
    api_result = call_diving_fish_api(api_arguments)

    # 单曲查询完顺手把这条 record 合并进已有的 player_cache.records。
    # 缓存不存在时不新建（单首歌不足以代表"完整 records 最新"），避免群单曲榜误判。
    if lookup.get("qq") and api_result.get("status") == 200:
        for raw_record in _iter_raw_song_records(api_result.get("data")):
            try:
                _merge_player_record_cache(lookup["qq"], raw_record)
            except Exception:
                pass

    return normalize_maimai_song_score_response(
        api_result,
        lookup,
        music_id=music_id,
        group_id=normalize_identifier(arguments.get("groupId")),
    )


def _iter_raw_song_records(data: Any) -> list[dict[str, Any]]:
    """从 /dev/player/record 返回中抽出 raw song record 列表。

    实测两种形态：
    - {"<music_id>": [{...}, {...}]}   # 按 music_id 分组
    - {"records": [{...}, ...]}        # 平铺
    - [{...}, ...]                     # 直接是列表
    """
    if isinstance(data, list):
        return [item for item in data if isinstance(item, dict)]
    if not isinstance(data, dict):
        return []
    if isinstance(data.get("records"), list):
        return [item for item in data["records"] if isinstance(item, dict)]
    out: list[dict[str, Any]] = []
    for value in data.values():
        if isinstance(value, list):
            out.extend(item for item in value if isinstance(item, dict))
    return out


def _version_filter_values(version_filter: Any) -> set[str]:
    if version_filter in (None, ""):
        return set()
    if isinstance(version_filter, list):
        return {str(value) for value in version_filter if value not in (None, "")}
    return {str(version_filter)}


def _title_type_key(title: Any, chart_type: Any) -> tuple[str, str] | None:
    title_text = str(title or "").strip().lower()
    type_text = str(chart_type or "").strip().upper()
    if not title_text or not type_text:
        return None
    return title_text, type_text


def _divingfish_song_list_paths() -> list[Path]:
    return [
        Path(os.environ.get("DIVING_FISH_SONG_LIST_PATH", "")),
        Path(__file__).resolve().parent.parent / "data" / "divingfish_song_list.json",
        Path.cwd() / "data" / "divingfish_song_list.json",
    ]


def _divingfish_chart_stats_paths() -> list[Path]:
    return [
        Path(os.environ.get("DIVING_FISH_CHART_STATS_PATH", "")),
        Path(__file__).resolve().parent.parent / "data" / "divingfish_chart_stats.json",
        Path.cwd() / "data" / "divingfish_chart_stats.json",
    ]


def _read_first_json_payload(paths: list[Path]) -> Any:
    for path in paths:
        if not path or str(path) == "." or not path.exists():
            continue
        try:
            return json.loads(path.read_text(encoding="utf-8"))
        except Exception:
            continue
    return None


def _first_existing_path(paths: list[Path]) -> Path | None:
    for path in paths:
        if not path or str(path) == ".":
            continue
        try:
            if path.exists() and path.is_file():
                return path
        except OSError:
            continue
    return None


def _iter_divingfish_song_list() -> list[dict[str, Any]]:
    payload = _read_first_json_payload(_divingfish_song_list_paths())
    songs = payload.get("songs", payload) if isinstance(payload, dict) else payload
    return [song for song in songs if isinstance(song, dict)] if isinstance(songs, list) else []


def _load_divingfish_chart_stats() -> dict[str, list[dict[str, Any]]]:
    payload = _read_first_json_payload(_divingfish_chart_stats_paths())
    charts = payload.get("charts") if isinstance(payload, dict) else None
    if not isinstance(charts, dict):
        return {}
    return {
        str(song_id): [item if isinstance(item, dict) else {} for item in stats]
        for song_id, stats in charts.items()
        if isinstance(stats, list)
    }


def _file_md5(path: Path) -> str:
    digest = hashlib.md5()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _file_fingerprint(path: Path) -> dict[str, Any] | None:
    try:
        stat = path.stat()
        return {
            "path": str(path.resolve()),
            "size": stat.st_size,
            "mtimeNs": stat.st_mtime_ns,
            "md5": _file_md5(path),
        }
    except OSError:
        return None


def _fast_fitted_b50_metadata_fingerprint() -> dict[str, Any] | None:
    song_list_path = _first_existing_path(_divingfish_song_list_paths())
    chart_stats_path = _first_existing_path(_divingfish_chart_stats_paths())
    if song_list_path is None or chart_stats_path is None:
        return None
    song_list_fingerprint = _file_fingerprint(song_list_path)
    chart_stats_fingerprint = _file_fingerprint(chart_stats_path)
    if song_list_fingerprint is None or chart_stats_fingerprint is None:
        return None
    return {
        "version": FAST_FITTED_B50_INDEX_CACHE_VERSION,
        "songList": song_list_fingerprint,
        "chartStats": chart_stats_fingerprint,
    }


def _fast_fitted_b50_index_cache_dir() -> Path:
    configured = os.environ.get("DIVING_FISH_FIT_INDEX_CACHE_DIR")
    if configured:
        return Path(configured).expanduser().resolve()
    return (Path(__file__).resolve().parent.parent / ".cache" / "diving-fish-b50").resolve()


def _fast_fitted_b50_index_cache_path() -> Path:
    return _fast_fitted_b50_index_cache_dir() / "fit-metadata-index.json"


def _read_fast_fitted_b50_index_disk_cache(
    fingerprint: dict[str, Any],
) -> dict[str, dict[str, Any]] | None:
    path = _fast_fitted_b50_index_cache_path()
    if not path.exists():
        return None
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return None
    if not isinstance(payload, dict) or payload.get("fingerprint") != fingerprint:
        return None
    index = payload.get("index")
    if not isinstance(index, dict):
        return None
    return {
        str(key): value
        for key, value in index.items()
        if isinstance(value, dict)
    }


def _write_fast_fitted_b50_index_disk_cache(
    fingerprint: dict[str, Any],
    index: dict[str, dict[str, Any]],
) -> None:
    path = _fast_fitted_b50_index_cache_path()
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        payload = {
            "createdAt": datetime.now(timezone.utc).isoformat(),
            "fingerprint": fingerprint,
            "index": index,
        }
        temp_path = path.with_name(f".{path.name}.{os.getpid()}.{threading.get_ident()}.tmp")
        temp_path.write_text(json.dumps(payload, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
        os.replace(temp_path, path)
    except Exception:
        pass


@lru_cache(maxsize=1)
def _divingfish_song_version_index() -> tuple[dict[int, str], dict[tuple[str, str], str]]:
    """Build local Diving-Fish song id/title -> version index.

    /dev/player/records often omits `version`; the local waterfish song list is
    the stable source for mapping record song ids back to `basic_info.from`.
    """
    by_id: dict[int, str] = {}
    by_title_type: dict[tuple[str, str], str] = {}
    for song in _iter_divingfish_song_list():
        basic_info = song.get("basic_info") if isinstance(song.get("basic_info"), dict) else {}
        version = basic_info.get("from")
        if version in (None, ""):
            continue
        try:
            song_id = int(song.get("id", song.get("songId")))
            by_id[song_id] = str(version)
        except (TypeError, ValueError):
            pass
        key = _title_type_key(song.get("title") or basic_info.get("title"), song.get("type"))
        if key:
            by_title_type[key] = str(version)
    return by_id, by_title_type


@lru_cache(maxsize=1)
def _divingfish_current_versions() -> set[str]:
    """Return current/new versions according to the local Diving-Fish song list."""
    return _divingfish_current_versions_from_songs(_iter_divingfish_song_list())


def _configured_current_versions() -> set[str]:
    for env_name in CURRENT_VERSION_ENV_NAMES:
        raw = os.environ.get(env_name)
        if not raw:
            continue
        normalized = raw.replace("；", ",").replace(";", ",").replace("\n", ",")
        versions = {part.strip() for part in normalized.split(",") if part.strip()}
        if versions:
            return versions
    return set()


def _divingfish_version_rank(version: Any) -> int | None:
    if version in (None, ""):
        return None
    text = " ".join(str(version).strip().casefold().split())
    if not text:
        return None
    return DIVINGFISH_VERSION_RANK.get(text)


def _divingfish_current_versions_from_songs(songs: list[dict[str, Any]]) -> set[str]:
    """Return the latest Diving-Fish version names used for computed B15.

    `is_new` is metadata supplied by a snapshot and can lag behind new releases.
    The fitted B50 split should instead follow the newest known
    `basic_info.from` major version. If an unknown future version appears before
    this code knows its order, fall back to the snapshot's `is_new` marker for
    those unknown names.
    """
    override = _configured_current_versions()
    if override:
        return override

    ranked_versions: dict[str, int] = {}
    is_new_versions: set[str] = set()
    unknown_ranked_new_versions: set[str] = set()
    for song in songs:
        basic_info = song.get("basic_info") if isinstance(song.get("basic_info"), dict) else {}
        version = basic_info.get("from")
        if version in (None, ""):
            continue
        version_text = str(version)
        rank = _divingfish_version_rank(version_text)
        if rank is not None:
            ranked_versions[version_text] = rank
        if basic_info.get("is_new") is True:
            is_new_versions.add(version_text)
            if rank is None:
                unknown_ranked_new_versions.add(version_text)

    if unknown_ranked_new_versions:
        return unknown_ranked_new_versions
    if ranked_versions:
        latest_rank = max(ranked_versions.values())
        return {version for version, rank in ranked_versions.items() if rank == latest_rank}
    return is_new_versions


def _record_waterfish_version(record: dict[str, Any], *, allow_record_fallback: bool = True) -> str | None:
    by_id, by_title_type = _divingfish_song_version_index()
    rec_sid = normalize_record_song_id(record)
    if isinstance(rec_sid, int):
        mapped = by_id.get(rec_sid)
        if mapped is not None:
            return mapped

    key = _title_type_key(record.get("title"), record.get("type"))
    if key:
        mapped = by_title_type.get(key)
        if mapped is not None:
            return mapped

    if allow_record_fallback:
        rec_version = record.get("version")
        return str(rec_version) if rec_version not in (None, "") else None
    return None


def _record_matches_version_filter(record: dict[str, Any], version_filter: Any) -> bool:
    expected = _version_filter_values(version_filter)
    if not expected:
        return True

    # If the API already supplies a version, preserve the old behavior exactly.
    rec_version = record.get("version")
    if rec_version not in (None, ""):
        return str(rec_version) in expected

    by_id, by_title_type = _divingfish_song_version_index()
    rec_sid = normalize_record_song_id(record)
    if isinstance(rec_sid, int):
        mapped_version = by_id.get(rec_sid)
        if mapped_version is not None:
            return mapped_version in expected

    key = _title_type_key(record.get("title"), record.get("type"))
    if key:
        mapped_version = by_title_type.get(key)
        if mapped_version is not None:
            return mapped_version in expected

    return False


def query_maimai_player_records(arguments: dict[str, Any]) -> dict[str, Any]:
    """查询玩家完整成绩，支持按等级/版本/曲目ID过滤。

    和 maimaidx_render_mcp 绘图 shim 的 query_user_plate 逻辑一致：
    底层调 /dev/player/records 拿全量，本地按条件过滤。
    查询成功后 call_diving_fish_api 内部自动写 player_cache.records。
    """
    lookup = validate_lookup(arguments)
    timeout_ms = normalize_timeout_ms(arguments.get("timeoutMs"))
    level_filter = normalize_identifier(arguments.get("level"))
    version_filter = arguments.get("version")
    music_id_filter = arguments.get("musicId")
    plate_filter = normalize_identifier(arguments.get("plate"))
    server = normalize_identifier(arguments.get("server")) or "cn"

    api_arguments: dict[str, Any] = {
        "operation": "maimai_dev_player_records_get",
        "query": {**lookup},
        "timeoutMs": timeout_ms,
    }
    api_result = call_diving_fish_api(api_arguments)

    if api_result.get("status") != 200:
        raise DivingFishError(
            f"获取玩家成绩失败 (HTTP {api_result.get('status')})",
            code="API_ERROR",
            body=api_result.get("text"),
        )

    data = api_result.get("data") or {}
    raw_records = data.get("records", [])
    if not isinstance(raw_records, list):
        raw_records = []

    # 牌子过滤：构建 Diving-Fish song_id 的匹配集合
    plate_cn_ids: set | None = None
    plate_song_count: int = 0

    if plate_filter:
        if server != "cn":
            raise DivingFishError("当前分支仅支持国服牌子过滤", code="UNSUPPORTED_SERVER")
        try:
            import json
            from pathlib import Path
            # CN: 从 maimaidxplate.json 拿 Diving-Fish ID 白名单
            for base in [Path(__file__).parent.parent.parent / "data",
                        Path(__file__).parent.parent.parent / "maimaidx_render_mcp" / "data"]:
                pp = base / "maimaidxplate.json"
                if pp.exists():
                    plate_data = json.loads(pp.read_text(encoding="utf-8")).get("content", {})
                    from maimaidx_render_mcp.maimaidx import version_map, platecn
                    _v = plate_filter
                    if _v in platecn:
                        _v = platecn[_v]
                    _ver = version_map.get(_v, (None, _v))[1]
                    plate_cn_ids = set(plate_data.get(_ver, []))
                    plate_song_count = len(plate_cn_ids)
                    break
        except Exception:
            pass

    # 过滤
    filtered = []
    for r in raw_records:
        if not isinstance(r, dict):
            continue
        if level_filter:
            rec_level = str(r.get("level", ""))
            if rec_level != level_filter:
                continue
        if not _record_matches_version_filter(r, version_filter):
            continue
        if music_id_filter is not None:
            rec_sid = r.get("song_id", r.get("id", 0))
            try:
                if int(rec_sid) != int(music_id_filter):
                    continue
            except (ValueError, TypeError):
                continue
        if plate_cn_ids is not None:
            rec_sid = r.get("song_id", r.get("id", 0))
            try:
                rec_sid_int = int(rec_sid)
            except (TypeError, ValueError):
                continue
            if rec_sid_int not in plate_cn_ids:
                continue
        filtered.append(normalize_song(r))

    result = {
        "source": "diving-fish",
        "endpoint": api_result.get("endpoint", {}).get("path", "/dev/player/records"),
        "lookup": lookup,
        "player": {
            "nickname": data.get("nickname"),
            "username": lookup.get("username"),
            "rating": data.get("rating"),
            "additionalRating": optional_number(data.get("additional_rating")),
            "plate": optional_string(data.get("plate")),
        },
        "counts": {
            "total": len(raw_records),
            "filtered": len(filtered),
        },
        "records": filtered,
    }
    if plate_filter:
        result["plate"] = {
            "name": plate_filter,
            "server": server,
            "songCount": plate_song_count,
        }
    if arguments.get("includeRaw") is True:
        result["raw"] = data

    return result


def validate_qq_batch(arguments: dict[str, Any]) -> list[str]:
    qqs = arguments.get("qqs")
    if not isinstance(qqs, list) or not qqs:
        raise DivingFishError("必须提供 qqs，且至少包含一个 QQ。", code="INVALID_INPUT")
    if len(qqs) > 500:
        raise DivingFishError("qqs 最多一次查询 500 个。", code="INVALID_INPUT")
    normalized = []
    for index, qq in enumerate(qqs):
        if isinstance(qq, int):
            qq = str(qq)
        if not isinstance(qq, str) or not qq.strip():
            raise DivingFishError(f"qqs[{index}] 不能为空。", code="INVALID_INPUT")
        normalized.append(qq.strip())
    return normalized


def post_diving_fish(payload: dict[str, Any], *, timeout_ms: int) -> dict[str, Any]:
    body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
    request = urllib.request.Request(
        QUERY_PLAYER_URL,
        data=body,
        method="POST",
        headers={
            "Accept": "application/json",
            "Content-Type": "application/json",
            "User-Agent": f"{SERVER_NAME}/{__version__}",
        },
    )

    response_body = open_query_player_with_retries(request, timeout_ms=timeout_ms)

    try:
        parsed = json.loads(response_body)
    except json.JSONDecodeError as exc:
        raise DivingFishError(
            "水鱼查分器返回了非 JSON 内容。",
            code="INVALID_JSON",
            body=response_body,
        ) from exc
    if not isinstance(parsed, dict):
        raise DivingFishError("水鱼查分器返回的 JSON 结构不是对象。", code="INVALID_JSON")
    return parsed


def open_query_player_with_retries(request: urllib.request.Request, *, timeout_ms: int) -> str:
    last_exc: urllib.error.URLError | None = None
    attempts = len(NETWORK_RETRY_DELAYS_SECONDS) + 1
    for attempt in range(attempts):
        try:
            with urllib.request.urlopen(request, timeout=timeout_ms / 1000) as response:
                return response.read().decode("utf-8")
        except urllib.error.HTTPError as exc:
            error_body = exc.read().decode("utf-8", errors="replace")
            raise map_diving_fish_status(exc.code, error_body) from exc
        except socket.timeout as exc:
            raise DivingFishError(
                f"请求水鱼查分器超时（{timeout_ms}ms）。",
                code="TIMEOUT",
            ) from exc
        except urllib.error.URLError as exc:
            last_exc = exc
            if attempt < attempts - 1:
                time.sleep(NETWORK_RETRY_DELAYS_SECONDS[attempt])
                continue

    reason = getattr(last_exc, "reason", last_exc)
    raise DivingFishError(
        f"请求水鱼查分器失败（已重试 {attempts - 1} 次）：{reason}",
        code="NETWORK_ERROR",
    ) from last_exc


def map_diving_fish_status(status: int, body: str) -> DivingFishError:
    if status == 400:
        return DivingFishError(
            "用户不存在，或查询条件无法匹配到水鱼账号。",
            code="USER_NOT_FOUND",
            status=status,
            body=body,
        )
    if status == 403:
        return DivingFishError(
            "对方设置了隐私，或未同意用户协议，无法通过第三方查询 B50。",
            code="FORBIDDEN",
            status=status,
            body=body,
        )
    if status == 429:
        return DivingFishError(
            "请求过于频繁，请稍后再试。",
            code="RATE_LIMITED",
            status=status,
            body=body,
        )
    return DivingFishError(
        f"水鱼查分器请求失败：HTTP {status}",
        code="HTTP_ERROR",
        status=status,
        body=body,
    )


def list_diving_fish_apis(arguments: dict[str, Any]) -> dict[str, Any]:
    return {
        "apis": public_api_catalog(
            game=normalize_identifier(arguments.get("game")),
            auth=normalize_identifier(arguments.get("auth")),
            include_mutating=arguments.get("includeMutating") is True,
        )
    }


def _b50_fit_label_for_delta(delta: float | None) -> str | None:
    if delta is None:
        return None
    if delta > 0:
        return "虚高"
    if delta < 0:
        return "虚低"
    return None


def _numeric_song_id_variants(song_id: Any, chart_type: str | None) -> list[str]:
    try:
        numeric = int(song_id)
    except (TypeError, ValueError):
        return [str(song_id)] if song_id not in (None, "") else []
    variants = [str(numeric)]
    if chart_type == "dx":
        if 0 < numeric < 10000:
            variants.append(str(numeric + 10000))
        elif 10000 < numeric < 100000:
            variants.append(str(numeric - 10000))
    elif chart_type == "standard" and 10000 < numeric < 100000:
        variants.append(str(numeric - 10000))
    return list(dict.fromkeys(variants))


def _fast_fitted_b50_metadata_keys(song: dict[str, Any]) -> list[str]:
    chart_type = local_search_song_type(song)
    difficulty = b50_difficulty_index(song)
    if chart_type is None or difficulty is None:
        return []
    keys: list[str] = []
    for song_id in _numeric_song_id_variants(song.get("songId"), chart_type):
        keys.append(f"id:{song_id}|{chart_type}|{difficulty}")
    title = str(song.get("title") or "").strip().casefold()
    if title:
        keys.append(f"title:{title}|{chart_type}|{difficulty}")
    return keys


def _build_fast_fitted_b50_metadata_index() -> dict[str, dict[str, Any]]:
    chart_stats = _load_divingfish_chart_stats()
    if not chart_stats:
        return {}
    index: dict[str, dict[str, Any]] = {}
    for song in _iter_divingfish_song_list():
        chart_type = local_search_song_type({"type": song.get("type")})
        if chart_type is None:
            continue
        song_id = song.get("id", song.get("songId"))
        stats_for_song = chart_stats.get(str(song_id))
        if not stats_for_song:
            continue
        levels = song.get("level") if isinstance(song.get("level"), list) else []
        ds_values = song.get("ds") if isinstance(song.get("ds"), list) else []
        basic_info = song.get("basic_info") if isinstance(song.get("basic_info"), dict) else {}
        title = str(song.get("title") or basic_info.get("title") or "")
        for difficulty, stat in enumerate(stats_for_song):
            if not isinstance(stat, dict):
                continue
            fit_diff = stat.get("fit_diff")
            if not isinstance(fit_diff, (int, float)) or isinstance(fit_diff, bool):
                continue
            ds = ds_values[difficulty] if difficulty < len(ds_values) else None
            try:
                ds_float = float(ds)
            except (TypeError, ValueError):
                ds_float = None
            fit_delta = ds_float - float(fit_diff) if ds_float is not None else None
            chart = {
                "source": "divingfish",
                "source_name": "cndivingfish",
                "chart_type": chart_type,
                "difficulty": B50_DIFFICULTIES[difficulty] if difficulty < len(B50_DIFFICULTIES) else str(difficulty),
                "difficulty_index": difficulty,
                "level": levels[difficulty] if difficulty < len(levels) else None,
                "ds": ds,
                "version": basic_info.get("from"),
                "fit_diff": float(fit_diff),
                "fit_delta": fit_delta,
                "fit_label": _b50_fit_label_for_delta(fit_delta),
                "fit_stats": {
                    key: stat.get(key)
                    for key in B50_CHART_STATS_KEYS
                    if stat.get(key) is not None
                },
                "fit_source_id": str(song_id),
            }
            for variant in _numeric_song_id_variants(song_id, chart_type):
                index[f"id:{variant}|{chart_type}|{difficulty}"] = chart
            if title:
                index[f"title:{title.casefold()}|{chart_type}|{difficulty}"] = chart
    return index


def _fast_fitted_b50_metadata_index() -> dict[str, dict[str, Any]]:
    fingerprint = _fast_fitted_b50_metadata_fingerprint()
    if fingerprint is None:
        return {}

    with _FAST_FITTED_B50_INDEX_CACHE_LOCK:
        if (
            _FAST_FITTED_B50_INDEX_CACHE.get("fingerprint") == fingerprint
            and isinstance(_FAST_FITTED_B50_INDEX_CACHE.get("index"), dict)
        ):
            return _FAST_FITTED_B50_INDEX_CACHE["index"]

    disk_index = _read_fast_fitted_b50_index_disk_cache(fingerprint)
    if disk_index is not None:
        with _FAST_FITTED_B50_INDEX_CACHE_LOCK:
            _FAST_FITTED_B50_INDEX_CACHE["fingerprint"] = fingerprint
            _FAST_FITTED_B50_INDEX_CACHE["index"] = disk_index
        return disk_index

    index = _build_fast_fitted_b50_metadata_index()
    with _FAST_FITTED_B50_INDEX_CACHE_LOCK:
        if (
            _FAST_FITTED_B50_INDEX_CACHE.get("fingerprint") == fingerprint
            and isinstance(_FAST_FITTED_B50_INDEX_CACHE.get("index"), dict)
        ):
            return _FAST_FITTED_B50_INDEX_CACHE["index"]
        _FAST_FITTED_B50_INDEX_CACHE["fingerprint"] = fingerprint
        _FAST_FITTED_B50_INDEX_CACHE["index"] = index
    _write_fast_fitted_b50_index_disk_cache(fingerprint, index)
    return index


def _clear_fast_fitted_b50_metadata_index_cache() -> None:
    with _FAST_FITTED_B50_INDEX_CACHE_LOCK:
        _FAST_FITTED_B50_INDEX_CACHE["fingerprint"] = None
        _FAST_FITTED_B50_INDEX_CACHE["index"] = None


_fast_fitted_b50_metadata_index.cache_clear = _clear_fast_fitted_b50_metadata_index_cache  # type: ignore[attr-defined]


def attach_fitted_b50_record_metadata_fast(
    songs: list[tuple[str, int, dict[str, Any]]],
) -> tuple[list[tuple[str, int, dict[str, Any]]], int]:
    metadata_index = _fast_fitted_b50_metadata_index()
    if not metadata_index:
        return songs, 0
    unmatched: list[tuple[str, int, dict[str, Any]]] = []
    matched = 0
    for section, index, song in songs:
        chart = None
        for key in _fast_fitted_b50_metadata_keys(song):
            chart = metadata_index.get(key)
            if chart is not None:
                break
        if chart is None:
            unmatched.append((section, index, song))
            continue
        attach_b50_chart_metadata(song, chart)
        matched += 1
    return unmatched, matched


def call_diving_fish_api(arguments: dict[str, Any]) -> dict[str, Any]:
    operation = normalize_identifier(arguments.get("operation"))
    if not operation:
        raise DivingFishError("必须提供 operation。", code="INVALID_INPUT")
    endpoint = API_CATALOG.get(operation)
    if endpoint is None:
        raise DivingFishError(f"未知 Diving-Fish API operation：{operation}", code="INVALID_INPUT")

    if endpoint.get("mutating") and arguments.get("confirm") != operation:
        raise DivingFishError(
            f"该接口会修改或删除数据。若确认调用，请传 confirm: \"{operation}\"。",
            code="CONFIRMATION_REQUIRED",
        )

    if endpoint.get("noHttp"):
        query = arguments.get("query") if isinstance(arguments.get("query"), dict) else {}
        cover_url = build_cover_url(query.get("song_id", query.get("id")))
        return {
            "operation": operation,
            "endpoint": public_endpoint_metadata(operation, endpoint),
            "status": 200,
            "url": cover_url,
            "data": {"url": cover_url},
        }

    normalized_arguments = normalize_api_arguments(operation, arguments)
    effective_arguments = with_bound_developer_token(endpoint, normalized_arguments)
    validate_api_auth(endpoint, effective_arguments)
    timeout_ms = normalize_timeout_ms(normalized_arguments.get("timeoutMs"))
    response = request_diving_fish_endpoint(endpoint, effective_arguments, timeout_ms=timeout_ms)

    # 成功拉取完整成绩时顺手写 player_cache.records，让群单曲榜下次刷新短路。
    if (
        operation == "maimai_dev_player_records_get"
        and response.get("status") == 200
        and isinstance(response.get("data"), dict)
    ):
        query = normalized_arguments.get("query") if isinstance(normalized_arguments.get("query"), dict) else {}
        qq = query.get("qq")
        if qq is not None:
            try:
                _write_player_records_cache(qq, response["data"])
            except Exception:
                pass

    return {
        "operation": operation,
        "endpoint": public_endpoint_metadata(operation, endpoint),
        "status": response["status"],
        "url": response["url"],
        "headers": response["headers"] if normalized_arguments.get("includeHeaders") is True else None,
        "data": response.get("data"),
        "text": response.get("text"),
        "jwtToken": response.get("jwtToken"),
    }


def normalize_api_arguments(operation: str, arguments: dict[str, Any]) -> dict[str, Any]:
    if operation != "maimai_dev_player_record_post":
        return arguments
    body = arguments.get("body")
    if not isinstance(body, dict) or "music_id" not in body or isinstance(body.get("music_id"), list):
        return arguments
    return {**arguments, "body": {**body, "music_id": [body.get("music_id")]}}


def bind_developer_token(arguments: dict[str, Any]) -> dict[str, Any]:
    developer_token = normalize_identifier(arguments.get("developerToken"))
    if not developer_token:
        raise DivingFishError("必须提供 developerToken。", code="INVALID_INPUT")
    store = load_secret_store()
    store["developerToken"] = developer_token
    store["developerTokenUpdatedAt"] = datetime.now(timezone.utc).isoformat()
    save_secret_store(store)
    return developer_token_status()


def clear_developer_token() -> dict[str, Any]:
    store = load_secret_store()
    had_developer_token = bool(normalize_identifier(store.get("developerToken")))
    store.pop("developerToken", None)
    store.pop("developerTokenUpdatedAt", None)
    save_secret_store(store)
    return {
        "bound": False,
        "cleared": had_developer_token,
        "storePath": str(get_secret_store_path()),
    }


def developer_token_status() -> dict[str, Any]:
    store = load_secret_store()
    token = normalize_identifier(store.get("developerToken"))
    return {
        "bound": bool(token),
        "tokenPreview": mask_secret(token) if token else None,
        "updatedAt": store.get("developerTokenUpdatedAt"),
        "storePath": str(get_secret_store_path()),
    }


def validate_api_auth(endpoint: dict[str, Any], arguments: dict[str, Any]) -> None:
    has_developer_token = bool(normalize_identifier(arguments.get("developerToken")))
    has_import_token = bool(normalize_identifier(arguments.get("importToken")))
    has_jwt_token = bool(normalize_identifier(arguments.get("jwtToken")))

    if endpoint["auth"] == "developer_token" and not has_developer_token:
        raise DivingFishError("该接口需要 Developer-Token。", code="AUTH_REQUIRED")
    if endpoint["auth"] == "login" and not has_jwt_token:
        raise DivingFishError("该接口需要登录 jwtToken。", code="AUTH_REQUIRED")
    if endpoint["auth"] == "login_or_import_token" and not has_import_token and not has_jwt_token:
        raise DivingFishError("该接口需要 Import-Token 或登录 jwtToken。", code="AUTH_REQUIRED")


def with_bound_developer_token(
    endpoint: dict[str, Any],
    arguments: dict[str, Any],
) -> dict[str, Any]:
    if endpoint["auth"] != "developer_token" or normalize_identifier(arguments.get("developerToken")):
        return arguments
    bound_token = normalize_identifier(load_secret_store().get("developerToken"))
    if not bound_token:
        return arguments
    return {**arguments, "developerToken": bound_token}


def request_diving_fish_endpoint(
    endpoint: dict[str, Any],
    arguments: dict[str, Any],
    *,
    timeout_ms: int,
) -> dict[str, Any]:
    query = arguments.get("query") if isinstance(arguments.get("query"), dict) else {}
    url = build_api_url(endpoint, query)
    headers = build_request_headers(endpoint, arguments)
    data = None

    if endpoint["method"] not in {"GET", "HEAD"}:
        if arguments.get("rawBody") is not None:
            data = str(arguments["rawBody"]).encode("utf-8")
        elif "body" in arguments:
            headers["Content-Type"] = "application/json"
            data = json.dumps(arguments["body"], ensure_ascii=False).encode("utf-8")

    request = urllib.request.Request(url, data=data, method=endpoint["method"], headers=headers)

    try:
        with urllib.request.urlopen(request, timeout=timeout_ms / 1000) as response:
            response_body = response.read().decode("utf-8", errors="replace")
            response_headers = dict(response.headers.items())
            status = response.status
    except urllib.error.HTTPError as exc:
        response_body = exc.read().decode("utf-8", errors="replace")
        if exc.code == 304:
            return {
                "status": 304,
                "url": url,
                "headers": dict(exc.headers.items()),
                "data": None,
                "text": response_body,
                "jwtToken": None,
            }
        raise DivingFishError(
            f"水鱼查分器请求失败：HTTP {exc.code}",
            code="HTTP_ERROR",
            status=exc.code,
            body=response_body,
        ) from exc
    except socket.timeout as exc:
        raise DivingFishError(
            f"请求水鱼查分器超时（{timeout_ms}ms）。",
            code="TIMEOUT",
        ) from exc
    except urllib.error.URLError as exc:
        reason = getattr(exc, "reason", exc)
        raise DivingFishError(
            f"请求水鱼查分器失败：{reason}",
            code="NETWORK_ERROR",
        ) from exc

    parsed: Any = None
    text: str | None = None
    if response_body:
        try:
            parsed = json.loads(response_body)
        except json.JSONDecodeError:
            text = response_body

    return {
        "status": status,
        "url": url,
        "headers": response_headers,
        "data": parsed,
        "text": text,
        "jwtToken": extract_jwt_token(response_headers),
    }


def build_request_headers(endpoint: dict[str, Any], arguments: dict[str, Any]) -> dict[str, str]:
    headers = {
        "Accept": "application/json",
        "User-Agent": f"{SERVER_NAME}/{__version__}",
    }
    developer_token = normalize_identifier(arguments.get("developerToken"))
    import_token = normalize_identifier(arguments.get("importToken"))
    jwt_token = normalize_identifier(arguments.get("jwtToken"))
    if_none_match = normalize_identifier(arguments.get("ifNoneMatch"))

    if developer_token:
        headers["Developer-Token"] = developer_token
    if import_token:
        headers["Import-Token"] = import_token
    if jwt_token:
        headers["Cookie"] = f"jwt_token={jwt_token}"
    if if_none_match:
        headers["If-None-Match"] = if_none_match

    extra_headers = arguments.get("headers")
    if isinstance(extra_headers, dict):
        for key, value in extra_headers.items():
            if isinstance(key, str) and isinstance(value, str):
                headers[key] = value

    if endpoint.get("rawBodyAllowed") and arguments.get("rawBody") is not None:
        headers.setdefault("Content-Type", "text/html; charset=utf-8")

    return headers


def extract_jwt_token(headers: dict[str, str]) -> str | None:
    set_cookie = headers.get("Set-Cookie") or headers.get("set-cookie")
    if not set_cookie:
        return None
    cookie = SimpleCookie()
    cookie.load(set_cookie)
    morsel = cookie.get("jwt_token")
    return morsel.value if morsel else None


def get_secret_store_path() -> Path:
    configured = normalize_identifier(os.environ.get("DIVING_FISH_MCP_TOKEN_FILE"))
    if configured:
        return Path(configured).expanduser().resolve()

    filename = ".diving-fish-mcp-secrets.json"
    cwd = Path.cwd()
    standard_paths = [
        cwd.parent / "maimai-config" / filename,
        Path("/AstrBot/data/maimai-config") / filename,
        Path("/opt/qqbot/data/maimai-config") / filename,
    ]
    for path in standard_paths:
        if path.exists():
            return path.resolve()
    for path in standard_paths:
        if path.parent.exists():
            return path.resolve()
    return cwd / filename


def load_secret_store() -> dict[str, Any]:
    store_path = get_secret_store_path()
    if not store_path.exists():
        return {}
    try:
        parsed = json.loads(store_path.read_text(encoding="utf-8"))
    except Exception as exc:
        raise DivingFishError(
            f"读取 token 存储失败：{exc}",
            code="TOKEN_STORE_ERROR",
        ) from exc
    return parsed if isinstance(parsed, dict) else {}


def save_secret_store(store: dict[str, Any]) -> None:
    store_path = get_secret_store_path()
    try:
        store_path.parent.mkdir(parents=True, exist_ok=True)
        store_path.write_text(json.dumps(store, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        os.chmod(store_path, 0o600)
    except Exception as exc:
        raise DivingFishError(
            f"写入 token 存储失败：{exc}",
            code="TOKEN_STORE_ERROR",
        ) from exc


def mask_secret(secret: str) -> str:
    if len(secret) <= 4:
        return "*" * len(secret)
    return ("*" * max(4, len(secret) - 4)) + secret[-4:]


def format_api_result(result: dict[str, Any]) -> str:
    endpoint = result.get("endpoint") or {}
    lines = [
        f"{result.get('operation')} -> HTTP {result.get('status')}",
        f"{endpoint.get('method', '')} {result.get('url') or endpoint.get('url') or endpoint.get('path')}".strip(),
        f"鉴权: {endpoint.get('auth', 'unknown')}"
        + (f"（需要 {endpoint.get('keyRequired')}）" if endpoint.get("keyRequired") else ""),
    ]

    if result.get("jwtToken"):
        lines.append("登录成功：响应中包含 jwtToken。请只在私聊或安全环境中使用。")

    data = result.get("data") if result.get("data") is not None else result.get("text")
    if isinstance(data, list):
        lines.append(f"返回数组：{len(data)} 项")
        lines.append(json.dumps(data[:5], ensure_ascii=False, indent=2))
    elif isinstance(data, dict):
        lines.append(f"返回对象字段：{', '.join(list(data.keys())[:20]) or '(空)'}")
        lines.append(json.dumps(data, ensure_ascii=False, indent=2)[:4000])
    elif data is not None:
        lines.append(str(data)[:4000])

    return "\n".join(lines)


def normalize_b50_response(
    raw: dict[str, Any],
    lookup: dict[str, str],
    *,
    group_id: str | None = None,
) -> dict[str, Any]:
    charts = raw.get("charts") if isinstance(raw.get("charts"), dict) else {}
    sd = normalize_song_list(charts.get("sd"))
    dx = normalize_song_list(charts.get("dx"))
    sd_rating = sum(song.get("ra") or 0 for song in sd)
    dx_rating = sum(song.get("ra") or 0 for song in dx)
    player = {
        "nickname": optional_string(raw.get("nickname")),
        "username": optional_string(raw.get("username")),
        "rating": optional_number(raw.get("rating")),
        "additionalRating": optional_number(raw.get("additional_rating")),
        "plate": optional_string(raw.get("plate")),
        "userGeneralData": raw.get("user_general_data"),
    }
    identity = load_and_update_identity(lookup, player, group_id=group_id)

    return {
        "source": "diving-fish",
        "endpoint": QUERY_PLAYER_URL,
        "lookup": lookup,
        "requestedAt": datetime.now(timezone.utc).isoformat(),
        "player": player,
        "identity": identity,
        "counts": {
            "sd": len(sd),
            "dx": len(dx),
            "total": len(sd) + len(dx),
        },
        "ratingBreakdown": {
            "sd": sd_rating,
            "dx": dx_rating,
            "total": sd_rating + dx_rating,
        },
        "charts": {
            "sd": sd,
            "dx": dx,
        },
        "raw": raw,
    }


def normalize_maimai_song_score_response(
    api_result: dict[str, Any],
    lookup: dict[str, str],
    *,
    music_id: int,
    group_id: str | None = None,
) -> dict[str, Any]:
    raw = api_result.get("data")
    player: dict[str, Any] = {}
    raw_records: list[dict[str, Any]] = []
    if isinstance(raw, dict) and isinstance(raw.get("records"), list):
        player = {
            "nickname": optional_string(raw.get("nickname")),
            "rating": optional_number(raw.get("rating")),
            "additionalRating": optional_number(raw.get("additional_rating")),
            "plate": optional_string(raw.get("plate")),
        }
        raw_records = [item for item in raw["records"] if isinstance(item, dict)]
    elif isinstance(raw, dict):
        grouped_records: list[dict[str, Any]] = []
        for value in raw.values():
            if isinstance(value, list):
                grouped_records.extend(item for item in value if isinstance(item, dict))
        raw_records = grouped_records or [raw]
    elif isinstance(raw, list):
        raw_records = [item for item in raw if isinstance(item, dict)]

    records = []
    for raw_record in raw_records:
        record = normalize_song(raw_record)
        record["musicId"] = music_id
        if record.get("songId") is None:
            record["songId"] = raw_record.get("music_id", raw_record.get("song_id", music_id))
        record["raw"] = raw_record
        records.append(record)

    identity = None
    if lookup.get("qq"):
        try:
            identity = get_identity(lookup["qq"], group_id)
        except Exception:
            identity = None

    return {
        "source": "diving-fish",
        "operation": api_result.get("operation"),
        "endpoint": api_result.get("endpoint"),
        "status": api_result.get("status"),
        "url": api_result.get("url"),
        "lookup": lookup,
        "identity": identity,
        "player": player,
        "musicId": music_id,
        "requestedAt": datetime.now(timezone.utc).isoformat(),
        "records": records,
        "record": records[0] if records else None,
        "raw": raw,
    }


def load_and_update_identity(
    lookup: dict[str, str],
    player: dict[str, Any],
    *,
    group_id: str | None,
) -> dict[str, Any] | None:
    qq = lookup.get("qq")
    if not qq:
        return None
    try:
        upsert_waterfish_profile(
            qq,
            nickname=player.get("nickname"),
            username=player.get("username"),
            rating=player.get("rating"),
        )
    except Exception:
        pass
    try:
        return get_identity(qq, group_id)
    except Exception:
        return None


def normalize_song_list(value: Any) -> list[dict[str, Any]]:
    if not isinstance(value, list):
        return []
    return [normalize_song(song) for song in value if isinstance(song, dict)]


def normalize_song(song: dict[str, Any]) -> dict[str, Any]:
    return {
        "title": optional_string(song.get("title")),
        "type": optional_string(song.get("type")),
        "level": optional_string(song.get("level")),
        "levelLabel": optional_string(song.get("level_label")),
        "levelIndex": optional_number(song.get("level_index")),
        "ds": optional_number(song.get("ds")),
        "achievements": optional_number(song.get("achievements")),
        "dxScore": optional_number(song.get("dxScore")),
        "fc": optional_string(song.get("fc")),
        "fs": optional_string(song.get("fs")),
        "ra": optional_number(song.get("ra")),
        "rate": optional_string(song.get("rate")),
        "songId": normalize_record_song_id(song),
    }


def normalize_record_song_id(song: dict[str, Any]) -> int | str | None:
    for key in ("song_id", "id", "music_id"):
        value = song.get(key)
        if value in (None, ""):
            continue
        try:
            return int(value)
        except (TypeError, ValueError):
            return value if isinstance(value, str) else None
    return None


def optional_string(value: Any) -> str | None:
    return value if isinstance(value, str) and value else None


def optional_number(value: Any) -> int | float | None:
    return value if isinstance(value, (int, float)) and not isinstance(value, bool) else None


def public_result(result: dict[str, Any], *, include_raw: bool = False) -> dict[str, Any]:
    if include_raw:
        return result
    return {key: value for key, value in result.items() if key != "raw"}


def format_b50_summary(
    result: dict[str, Any],
    top_n: int = 50,
    section: str = "b50",
    display_options: dict[str, Any] | None = None,
) -> str:
    player = result.get("player") or {}
    counts = result.get("counts") or {}
    rating = result.get("ratingBreakdown") or {}
    charts = result.get("charts") or {}
    normalized_section = normalize_section(section)
    display_options = display_options or normalize_b50_display_options({})

    lines = [
        f"昵称: {player.get('nickname') or '未知'}",
    ]
    append_identity_lines(lines, result)
    lines.extend(
        [
            f"Rating: {player.get('rating') if player.get('rating') is not None else '未知'}",
            f"牌子: {player.get('plate') or '未知'}",
            f"旧曲 Best: {counts.get('sd', 0)} 首，合计 ra {rating.get('sd', 0)}",
            f"新曲 Best: {counts.get('dx', 0)} 首，合计 ra {rating.get('dx', 0)}",
            f"B50 合计: {counts.get('total', 0)} 首，合计 ra {rating.get('total', 0)}",
        ]
    )
    append_computed_b50_line(lines, result)
    append_chart_metadata_line(lines, result)
    append_fit_index_line(lines, result)
    display_summary = format_b50_display_options(display_options)
    if display_summary:
        lines.append(f"筛选/排序: {display_summary}")

    if normalized_section == "split":
        append_song_section(lines, "旧曲 B35", charts.get("sd") or [], top_n, 35, display_options)
        append_song_section(lines, "新曲 B15", charts.get("dx") or [], top_n, 15, display_options)
    elif normalized_section == "b35":
        append_song_section(lines, "旧曲 B35", charts.get("sd") or [], top_n, 35, display_options)
    elif normalized_section == "b15":
        append_song_section(lines, "新曲 B15", charts.get("dx") or [], top_n, 15, display_options)
    else:
        append_song_section(
            lines,
            "B50",
            [*(charts.get("sd") or []), *(charts.get("dx") or [])],
            top_n,
            50,
            display_options,
        )
    return "\n".join(lines)


def append_computed_b50_line(lines: list[str], result: dict[str, Any]) -> None:
    computed = result.get("computedB50") if isinstance(result.get("computedB50"), dict) else None
    if not computed:
        return
    versions = computed.get("currentVersions")
    version_text = "、".join(str(value) for value in versions) if isinstance(versions, list) else "未知"
    actual_rating = computed.get("actualRating")
    suffix = f"；水鱼原始 Rating {actual_rating}" if actual_rating is not None else ""
    lines.append(f"拟合B50: 使用拟合定数重算单曲ra并排序；新曲版本 {version_text}{suffix}")


def append_chart_metadata_line(lines: list[str], result: dict[str, Any]) -> None:
    metadata = result.get("chartMetadata") if isinstance(result.get("chartMetadata"), dict) else None
    if not metadata or metadata.get("skipped"):
        return
    if metadata.get("available"):
        lines.append(f"谱面拟合: maimai-local-search 匹配 {metadata.get('matched', 0)}/{metadata.get('requested', 0)} 首")
        return
    error_info = metadata.get("error") if isinstance(metadata.get("error"), dict) else {}
    message = error_info.get("message") or "maimai-local-search 不可用"
    lines.append(f"谱面拟合: 未可用（{message}）")


def append_fit_index_line(lines: list[str], result: dict[str, Any]) -> None:
    fit_index = result.get("fitIndex") if isinstance(result.get("fitIndex"), dict) else None
    if not fit_index:
        return
    b50 = fit_index.get("b50") if isinstance(fit_index.get("b50"), dict) else {}
    counted = b50.get("counted") or 0
    missing = b50.get("missing") or 0
    if not counted:
        lines.append(f"虚高指数: 数据不足（匹配 0/{counted + missing}）")
        return
    label = fit_index.get("label") or "—"
    suffix = f" 匹配 {counted}/{counted + missing}" if missing else ""
    lines.append("虚高指数: " + _format_fit_index_metrics(b50, label) + suffix)
    b35 = fit_index.get("b35") if isinstance(fit_index.get("b35"), dict) else {}
    b15 = fit_index.get("b15") if isinstance(fit_index.get("b15"), dict) else {}
    sub_parts = []
    if b35.get("counted"):
        sub_parts.append("B35 " + _format_fit_index_metrics(b35))
    if b15.get("counted"):
        sub_parts.append("B15 " + _format_fit_index_metrics(b15))
    if sub_parts:
        lines.append("  └ " + "；".join(sub_parts))


def _format_fit_index_metrics(section: dict[str, Any], label: str | None = None) -> str:
    parts = []
    virtual_rating = section.get("virtualRating")
    virtual_ratio = section.get("virtualRatio")
    if isinstance(virtual_rating, (int, float)):
        parts.append(f"{virtual_rating:+.1f} ra")
    if isinstance(virtual_ratio, (int, float)):
        parts.append(f"{virtual_ratio:+.2f}%")
    if label:
        parts.append(label)
    return " / ".join(parts)


def append_identity_lines(lines: list[str], result: dict[str, Any]) -> None:
    lookup = result.get("lookup") if isinstance(result.get("lookup"), dict) else {}
    identity = result.get("identity") if isinstance(result.get("identity"), dict) else None
    qq = lookup.get("qq") if isinstance(lookup, dict) else None
    if qq:
        lines.append(f"QQ: {qq}")
    if not identity:
        return
    qq_nickname = identity.get("qqNickname") or identity.get("friendNickname")
    if qq_nickname:
        lines.append(f"QQ昵称: {qq_nickname}")
    preferred_group = identity.get("preferredGroup")
    if isinstance(preferred_group, dict):
        group_label = preferred_group.get("groupNickname") or preferred_group.get("card") or preferred_group.get("nickname")
        if group_label:
            suffix = f"（{preferred_group.get('groupName')}）" if preferred_group.get("groupName") else ""
            lines.append(f"QQ群昵称: {group_label}{suffix}")
    else:
        groups = identity.get("groups") if isinstance(identity.get("groups"), list) else []
        if groups:
            first_group = groups[0]
            if isinstance(first_group, dict):
                group_label = first_group.get("groupNickname") or first_group.get("card") or first_group.get("nickname")
                suffix = f"（{first_group.get('groupName')}）" if first_group.get("groupName") else ""
                more = f" 等 {len(groups)} 个群" if len(groups) > 1 else ""
                if group_label:
                    lines.append(f"QQ群昵称: {group_label}{suffix}{more}")


def format_b50_batch_summary(result: dict[str, Any]) -> str:
    counts = result.get("counts") or {}
    lines = [
        f"批量 B50 查询完成：请求 {counts.get('requested', 0)} 个，成功 {counts.get('success', 0)} 个，失败 {counts.get('failure', 0)} 个。",
        "",
        "| 序号 | QQ | QQ昵称 | QQ群昵称 | 水鱼昵称 | Rating | B50 ra | 虚高指数 | 状态 |",
        "| --- | --- | --- | --- | --- | ---: | ---: | --- | --- |",
    ]
    for index, item in enumerate(result.get("results") or [], start=1):
        player = item.get("player") if isinstance(item.get("player"), dict) else {}
        identity = (item.get("result") or {}).get("identity") if isinstance(item.get("result"), dict) else None
        identity = identity if isinstance(identity, dict) else {}
        preferred_group = identity.get("preferredGroup") if isinstance(identity.get("preferredGroup"), dict) else {}
        error_info = item.get("error") if isinstance(item.get("error"), dict) else {}
        status = "OK" if item.get("ok") else f"{error_info.get('code', 'ERROR')}: {error_info.get('message', '查询失败')}"
        lines.append(
            "| "
            + " | ".join(
                [
                    str(index),
                    str(item.get("qq") or ""),
                    escape_markdown_table(str(identity.get("qqNickname") or identity.get("friendNickname") or "")),
                    escape_markdown_table(str(preferred_group.get("groupNickname") or "")),
                    escape_markdown_table(str(player.get("nickname") or identity.get("waterfishNickname") or "")),
                    str(item.get("rating") if item.get("rating") is not None else ""),
                    str(item.get("b50Rating") if item.get("b50Rating") is not None else ""),
                    escape_markdown_table(format_fit_index_cell(item.get("fitIndex"))),
                    escape_markdown_table(status),
                ]
            )
            + " |"
        )
    return "\n".join(lines)


def format_fit_index_cell(fit_index: Any) -> str:
    if not isinstance(fit_index, dict):
        return ""
    if not fit_index.get("available"):
        return "数据不足"
    ratio = fit_index.get("virtualRatio")
    rating = fit_index.get("virtualRating")
    parts = []
    if isinstance(ratio, (int, float)):
        parts.append(f"{ratio:+.2f}%")
    if isinstance(rating, (int, float)):
        parts.append(f"{rating:+.1f} ra")
    label = fit_index.get("label")
    if label:
        parts.append(str(label))
    return " ".join(parts)


def append_song_section(
    lines: list[str],
    label: str,
    songs: list[dict[str, Any]],
    top_n: int,
    maximum: int,
    display_options: dict[str, Any] | None = None,
) -> None:
    display_options = display_options or normalize_b50_display_options({})
    filtered_songs = [song for song in songs if b50_song_matches_display_options(song, display_options)]
    visible_songs = sort_b50_songs(filtered_songs, display_options)[: max(0, min(top_n, maximum))]

    if not visible_songs:
        if b50_display_controls_active(display_options):
            lines.extend(["", f"{label}: 筛选后没有匹配歌曲。"])
        return

    suffix = f"（匹配 {len(filtered_songs)} 首）" if b50_display_controls_active(display_options) else ""
    lines.extend(["", f"{label} Top {len(visible_songs)}{suffix}:"])
    for index, song in enumerate(visible_songs, start=1):
        lines.append(format_song_line(song, index))


def b50_song_matches_display_options(song: dict[str, Any], options: dict[str, Any]) -> bool:
    level = options.get("level")
    if level and normalize_level_text(song.get("level")) != normalize_level_text(level):
        return False
    difficulty = options.get("difficulty")
    if difficulty and normalize_difficulty_text(song.get("levelLabel")) != normalize_difficulty_text(difficulty):
        return False
    if not number_in_range(song.get("ds"), options.get("dsMin"), options.get("dsMax")):
        return False
    if not number_in_range(song.get("achievements"), options.get("achievementMin"), options.get("achievementMax")):
        return False
    if not number_in_range(song.get("ra"), options.get("raMin"), options.get("raMax")):
        return False
    if not number_in_range(song.get("fitDiff"), options.get("fitDiffMin"), options.get("fitDiffMax")):
        return False
    if not number_in_range(song.get("fitDelta"), options.get("fitDeltaMin"), options.get("fitDeltaMax")):
        return False
    fit_label = options.get("fitLabel")
    if fit_label and song.get("fitLabel") != fit_label:
        return False
    return True


def number_in_range(value: Any, low: float | None, high: float | None) -> bool:
    if low is None and high is None:
        return True
    if not isinstance(value, (int, float)) or isinstance(value, bool):
        return False
    numeric = float(value)
    if low is not None and numeric < low:
        return False
    if high is not None and numeric > high:
        return False
    return True


def normalize_level_text(value: Any) -> str:
    return str(value or "").casefold().replace(" ", "").replace("级", "").replace("?", "")


def normalize_difficulty_text(value: Any) -> str:
    text = str(value or "").casefold().replace(" ", "").replace(":", "")
    return {
        "绿": "basic",
        "黄": "advanced",
        "红": "expert",
        "紫": "master",
        "白": "remaster",
        "bas": "basic",
        "adv": "advanced",
        "exp": "expert",
        "mst": "master",
        "master": "master",
        "remaster": "remaster",
    }.get(text, text)


def sort_b50_songs(songs: list[dict[str, Any]], options: dict[str, Any]) -> list[dict[str, Any]]:
    sort_by = options.get("sortBy") or "default"
    sort_order = options.get("sortOrder") or "desc"
    if sort_by == "default":
        return sorted(songs, key=default_b50_song_sort_key)
    if sort_by == "title":
        return sorted(songs, key=lambda song: str(song.get("title") or ""), reverse=sort_order == "desc")

    field = {
        "ra": "ra",
        "achievement": "achievements",
        "ds": "ds",
        "fitDiff": "fitDiff",
        "fitDelta": "fitDelta",
    }[sort_by]
    descending = sort_order == "desc"
    return sorted(
        songs,
        key=lambda song: (
            song.get(field) is None,
            -float(song.get(field) or 0) if descending else float(song.get(field) or 0),
            default_b50_song_sort_key(song),
        ),
    )


def default_b50_song_sort_key(song: dict[str, Any]) -> tuple[Any, ...]:
    return (
        -(song.get("ra") if song.get("ra") is not None else -1),
        -(song.get("achievements") if song.get("achievements") is not None else -1),
        str(song.get("title") or ""),
    )


def format_b50_display_options(options: dict[str, Any]) -> str:
    if not b50_display_controls_active(options):
        return ""
    pieces = []
    if options.get("level"):
        pieces.append(f"等级 {options['level']}")
    if options.get("difficulty"):
        pieces.append(f"难度 {options['difficulty']}")
    pieces.extend(format_range_piece(label, options.get(low), options.get(high)) for label, low, high in (
        ("定数", "dsMin", "dsMax"),
        ("达成率", "achievementMin", "achievementMax"),
        ("ra", "raMin", "raMax"),
        ("拟合定数", "fitDiffMin", "fitDiffMax"),
        ("差值", "fitDeltaMin", "fitDeltaMax"),
    ))
    if options.get("fitLabel"):
        pieces.append(str(options["fitLabel"]))
    if options.get("sortBy") not in (None, "default") or options.get("sortOrder") not in (None, "desc"):
        pieces.append(f"排序 {options.get('sortBy', 'default')} {options.get('sortOrder', 'desc')}")
    return "，".join(piece for piece in pieces if piece)


def format_range_piece(label: str, low: float | None, high: float | None) -> str:
    if low is None and high is None:
        return ""
    if low is not None and high is not None:
        return f"{label} {format_number(low)}-{format_number(high)}"
    if low is not None:
        return f"{label}>={format_number(low)}"
    return f"{label}<={format_number(high)}"


def normalize_section(section: str) -> str:
    if section in {"b35", "sd", "old"}:
        return "b35"
    if section in {"b15", "dx", "new"}:
        return "b15"
    if section in {"split", "both"}:
        return "split"
    return "b50"


def format_song_line(song: dict[str, Any], rank: int) -> str:
    if song.get("fittedRa") is not None:
        rating_details = [
            f"拟合ra {song['fittedRa']}",
            f"原ra {song['originalRa']}" if song.get("originalRa") is not None else None,
        ]
    else:
        rating_details = [f"ra {song['ra']}" if song.get("ra") is not None else None]
    details = [
        song.get("levelLabel"),
        song.get("level"),
        f"定数 {song['ds']}" if song.get("ds") is not None else None,
        f"拟合 {format_number(song['fitDiff'])}" if song.get("fitDiff") is not None else None,
        f"差值 {format_signed_number(song['fitDelta'])}" if song.get("fitDelta") is not None else None,
        song.get("fitLabel"),
        f"{song['achievements']}%" if song.get("achievements") is not None else None,
        *rating_details,
        str(song["rate"]).upper() if song.get("rate") else None,
        str(song["fc"]).upper() if song.get("fc") else None,
        str(song["fs"]).upper() if song.get("fs") else None,
    ]
    details = [item for item in details if item]
    suffix = f" - {' / '.join(details)}" if details else ""
    return f"{rank}. [{song.get('type') or '?'}] {song.get('title') or '未知歌曲'}{suffix}"


def format_number(value: Any) -> str:
    if isinstance(value, float):
        return f"{value:.4f}".rstrip("0").rstrip(".")
    return str(value)


def format_signed_number(value: Any) -> str:
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return f"{value:+.4f}".rstrip("0").rstrip(".")
    return str(value)


def format_maimai_song_score(result: dict[str, Any]) -> str:
    lookup = result.get("lookup") if isinstance(result.get("lookup"), dict) else {}
    identity = result.get("identity") if isinstance(result.get("identity"), dict) else {}
    player = result.get("player") if isinstance(result.get("player"), dict) else {}
    records = result.get("records") if isinstance(result.get("records"), list) else []
    target = lookup.get("qq") or lookup.get("username") or "未知"
    lines = [
        "maimai 单曲成绩",
        f"目标: {target}",
        f"music_id: {result.get('musicId')}",
    ]
    if player:
        player_bits = [
            f"昵称: {player.get('nickname')}" if player.get("nickname") else None,
            f"Rating: {player.get('rating')}" if player.get("rating") is not None else None,
            f"牌子: {player.get('plate')}" if player.get("plate") else None,
        ]
        player_line = " / ".join(item for item in player_bits if item)
        if player_line:
            lines.append(player_line)
    if identity:
        qq_name = identity.get("qqNickname") or identity.get("friendNickname")
        waterfish_name = identity.get("waterfishNickname")
        if qq_name:
            lines.append(f"QQ昵称: {qq_name}")
        if waterfish_name:
            lines.append(f"水鱼昵称: {waterfish_name}")
    if not records:
        lines.append("未返回该曲成绩数据。")
        return "\n".join(lines)

    lines.append(f"返回成绩: {len(records)} 条")
    for index, record in enumerate(records, start=1):
        if not isinstance(record, dict):
            continue
        lines.append(format_song_line(record, index))
        details = []
        if record.get("dxScore") is not None:
            details.append(f"DX Score {record.get('dxScore')}")
        if record.get("levelIndex") is not None:
            details.append(f"level_index {record.get('levelIndex')}")
        if record.get("songId") is not None:
            details.append(f"song_id {record.get('songId')}")
        if details:
            lines.append("   " + " / ".join(details))
    return "\n".join(lines)


def public_song_score_result(result: dict[str, Any], *, include_raw: bool) -> dict[str, Any]:
    if include_raw:
        return result
    payload = {key: value for key, value in result.items() if key != "raw"}
    record = payload.get("record")
    if isinstance(record, dict):
        payload["record"] = {key: value for key, value in record.items() if key != "raw"}
    records = payload.get("records")
    if isinstance(records, list):
        payload["records"] = [
            {key: value for key, value in item.items() if key != "raw"}
            if isinstance(item, dict)
            else item
            for item in records
        ]
    return payload


def escape_markdown_table(value: str) -> str:
    return value.replace("|", "\\|").replace("\n", " ")


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


def handle_tool_call(message_id: Any, params: dict[str, Any] | None) -> dict[str, Any]:
    params = params or {}
    tool_name = params.get("name")
    arguments = params.get("arguments") or {}
    if not isinstance(arguments, dict):
        return success(
            message_id,
            {"content": [{"type": "text", "text": "arguments 必须是对象。"}], "isError": True},
        )
    if tool_name == LIST_APIS_TOOL["name"]:
        result = list_diving_fish_apis(arguments)
        return success(
            message_id,
            {
                "content": [
                    {"type": "text", "text": json.dumps(result["apis"], ensure_ascii=False, indent=2)}
                ],
                "structuredContent": result,
                "isError": False,
            },
        )

    if tool_name == DIVING_FISH_API_TOOL["name"]:
        try:
            result = call_diving_fish_api(arguments)
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": format_api_result(result)}],
                    "structuredContent": result,
                    "isError": False,
                },
            )
        except (DivingFishError, ValueError) as exc:
            error_data = exc.to_dict() if isinstance(exc, DivingFishError) else {
                "code": "INVALID_INPUT",
                "message": str(exc),
                "status": None,
                "body": None,
            }
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": str(exc)}],
                    "structuredContent": {"error": error_data},
                    "isError": True,
                },
            )

    if tool_name == BIND_DEVELOPER_TOKEN_TOOL["name"]:
        try:
            result = bind_developer_token(arguments)
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": "Developer-Token 已绑定。"}],
                    "structuredContent": result,
                    "isError": False,
                },
            )
        except DivingFishError as exc:
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": str(exc)}],
                    "structuredContent": {"error": exc.to_dict()},
                    "isError": True,
                },
            )

    if tool_name == DEVELOPER_TOKEN_STATUS_TOOL["name"]:
        result = developer_token_status()
        text = (
            f"Developer-Token 已绑定（{result['tokenPreview']}）。"
            if result["bound"]
            else "Developer-Token 未绑定。"
        )
        return success(
            message_id,
            {
                "content": [{"type": "text", "text": text}],
                "structuredContent": result,
                "isError": False,
            },
        )

    if tool_name == CLEAR_DEVELOPER_TOKEN_TOOL["name"]:
        result = clear_developer_token()
        return success(
            message_id,
            {
                "content": [
                    {
                        "type": "text",
                        "text": "Developer-Token 已清除。"
                        if result["cleared"]
                        else "没有已绑定的 Developer-Token。",
                    }
                ],
                "structuredContent": result,
                "isError": False,
            },
        )

    if tool_name == QUERY_MAIMAI_SONG_SCORE_TOOL["name"]:
        try:
            result = query_maimai_song_score(arguments)
            payload = public_song_score_result(result, include_raw=arguments.get("includeRaw") is True)
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": json.dumps(payload, ensure_ascii=False, indent=2)}],
                    "structuredContent": payload,
                    "isError": False,
                },
            )
        except DivingFishError as exc:
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": str(exc)}],
                    "structuredContent": {"error": exc.to_dict()},
                    "isError": True,
                },
            )

    if tool_name == QUERY_MAIMAI_PLAYER_RECORDS_TOOL["name"]:
        try:
            result = query_maimai_player_records(arguments)
            payload = public_result(result, include_raw=arguments.get("includeRaw") is True)
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": json.dumps(payload, ensure_ascii=False, indent=2)}],
                    "structuredContent": payload,
                    "isError": False,
                },
            )
        except DivingFishError as exc:
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": str(exc)}],
                    "structuredContent": {"error": exc.to_dict()},
                    "isError": True,
                },
            )

    if tool_name == QUERY_B50_BATCH_TOOL["name"]:
        try:
            result = query_b50_batch(arguments)
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": format_b50_batch_summary(result)}],
                    "structuredContent": result,
                    "isError": False,
                },
            )
        except DivingFishError as exc:
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": str(exc)}],
                    "structuredContent": {"error": exc.to_dict()},
                    "isError": True,
                },
            )

    if tool_name == QUERY_COMPUTED_B50_TOOL["name"]:
        try:
            top_n = arguments.get("topN", 50)
            if not isinstance(top_n, int) or top_n < 0 or top_n > 50:
                raise DivingFishError("topN 必须是 0 到 50 之间的整数。", code="INVALID_INPUT")
            section = arguments.get("section", "b50")
            if not isinstance(section, str):
                raise DivingFishError("section 必须是字符串。", code="INVALID_INPUT")
            display_options = normalize_b50_display_options(arguments)
            result = query_computed_b50(arguments)
            payload = public_result(result, include_raw=arguments.get("includeRaw") is True)
            return success(
                message_id,
                {
                    "content": [
                        {"type": "text", "text": format_b50_summary(result, top_n, section, display_options)}
                    ],
                    "structuredContent": payload,
                    "isError": False,
                },
            )
        except DivingFishError as exc:
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": str(exc)}],
                    "structuredContent": {"error": exc.to_dict()},
                    "isError": True,
                },
            )

    if tool_name == QUERY_B50_TOOL["name"]:
        try:
            top_n = arguments.get("topN", 50)
            if not isinstance(top_n, int) or top_n < 0 or top_n > 50:
                raise DivingFishError("topN 必须是 0 到 50 之间的整数。", code="INVALID_INPUT")
            section = arguments.get("section", "b50")
            if not isinstance(section, str):
                raise DivingFishError("section 必须是字符串。", code="INVALID_INPUT")
            display_options = normalize_b50_display_options(arguments)
            result = query_b50(arguments)
            payload = public_result(result, include_raw=arguments.get("includeRaw") is True)
            return success(
                message_id,
                {
                    "content": [
                        {"type": "text", "text": format_b50_summary(result, top_n, section, display_options)}
                    ],
                    "structuredContent": payload,
                    "isError": False,
                },
            )
        except DivingFishError as exc:
            return success(
                message_id,
                {
                    "content": [{"type": "text", "text": str(exc)}],
                    "structuredContent": {"error": exc.to_dict()},
                    "isError": True,
                },
            )

    return error(message_id, -32602, f"Unknown tool: {tool_name}")


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
        except Exception as exc:
            traceback.print_exc(file=sys.stderr)
            message_id = None
            if "message" in locals() and isinstance(message, dict):
                message_id = message.get("id")
            write_message(error(message_id, -32603, "Internal error", str(exc)))


if __name__ == "__main__":
    main()
