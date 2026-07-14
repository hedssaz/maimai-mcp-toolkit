from __future__ import annotations

from typing import Any


API_BASE_URL = "https://www.diving-fish.com/api"
COVER_BASE_URL = "https://www.diving-fish.com/covers"


API_CATALOG: dict[str, dict[str, Any]] = {
    "maimai_login": {
        "game": "maimaidxprober",
        "method": "POST",
        "path": "/login",
        "auth": "login_credentials",
        "keyRequired": False,
        "description": "使用 Diving-Fish 用户名和密码登录，返回 jwt_token cookie。",
        "bodyHint": {"username": "your_username", "password": "your_password"},
    },
    "maimai_player_agreement_get": {
        "game": "maimaidxprober",
        "method": "GET",
        "path": "/player/agreement",
        "auth": "login",
        "keyRequired": False,
        "description": "获取当前登录用户是否同意用户协议。",
    },
    "maimai_player_agreement_post": {
        "game": "maimaidxprober",
        "method": "POST",
        "path": "/player/agreement",
        "auth": "login",
        "keyRequired": False,
        "mutating": True,
        "description": "更新当前登录用户是否同意用户协议。",
        "bodyHint": {"accept_agreement": True},
    },
    "maimai_player_profile_get": {
        "game": "maimaidxprober",
        "method": "GET",
        "path": "/player/profile",
        "auth": "login",
        "keyRequired": False,
        "description": "获取当前登录用户资料。",
    },
    "maimai_player_profile_post": {
        "game": "maimaidxprober",
        "method": "POST",
        "path": "/player/profile",
        "auth": "login",
        "keyRequired": False,
        "mutating": True,
        "description": "更新当前登录用户资料。",
        "bodyHint": {"nickname": "new_nickname", "privacy": False},
    },
    "maimai_player_import_token_put": {
        "game": "maimaidxprober",
        "method": "PUT",
        "path": "/player/import_token",
        "auth": "login",
        "keyRequired": False,
        "mutating": True,
        "description": "生成新的 Import-Token 并覆盖旧 token。",
    },
    "maimai_music_data_get": {
        "game": "maimaidxprober",
        "method": "GET",
        "path": "/music_data",
        "auth": "none",
        "keyRequired": False,
        "description": "获取 maimai DX 歌曲数据。支持 If-None-Match 缓存校验。",
    },
    "maimai_player_records_get": {
        "game": "maimaidxprober",
        "method": "GET",
        "path": "/player/records",
        "auth": "login_or_import_token",
        "keyRequired": "Import-Token 或登录 jwt_token",
        "description": "获取当前用户 maimai 完整成绩。",
    },
    "maimai_player_test_data_get": {
        "game": "maimaidxprober",
        "method": "GET",
        "path": "/player/test_data",
        "auth": "none",
        "keyRequired": False,
        "description": "获取 maimai 完整成绩测试数据。",
    },
    "maimai_dev_player_records_get": {
        "game": "maimaidxprober",
        "method": "GET",
        "path": "/dev/player/records",
        "auth": "developer_token",
        "keyRequired": "Developer-Token",
        "description": "通过 Developer-Token 获取指定用户 maimai 完整成绩。",
        "queryHint": {"qq": "123456789"},
    },
    "maimai_dev_player_record_post": {
        "game": "maimaidxprober",
        "method": "POST",
        "path": "/dev/player/record",
        "auth": "developer_token",
        "keyRequired": "Developer-Token",
        "description": "通过 Developer-Token 获取指定用户指定歌曲的 maimai 单曲成绩。",
        "bodyHint": {"qq": "123456789", "music_id": [11466]},
    },
    "maimai_query_player_post": {
        "game": "maimaidxprober",
        "method": "POST",
        "path": "/query/player",
        "auth": "none",
        "keyRequired": False,
        "description": "无需验证查询用户 maimai 简略成绩。B50 需要 body 带 b50。",
        "bodyHint": {"qq": "123456789", "b50": "1"},
    },
    "maimai_query_plate_post": {
        "game": "maimaidxprober",
        "method": "POST",
        "path": "/query/plate",
        "auth": "none",
        "keyRequired": False,
        "description": "按版本获取用户 maimai 成绩，取决于用户隐私设置。",
        "bodyHint": {"qq": "123456789", "version": ["maimai でらっくす FESTiVAL PLUS"]},
    },
    "maimai_cover_url": {
        "game": "maimaidxprober",
        "method": "GET",
        "path": "*/covers",
        "auth": "none",
        "keyRequired": False,
        "noHttp": True,
        "description": "按歌曲 ID 生成封面 URL。10001 到 11000 会按文档映射到 ID-10000。",
        "queryHint": {"song_id": 38},
    },
    "maimai_rating_ranking_get": {
        "game": "maimaidxprober",
        "method": "GET",
        "path": "/rating_ranking",
        "auth": "none",
        "keyRequired": False,
        "description": "获取公开用户 username-rating 数据。",
    },
    "maimai_player_update_records_post": {
        "game": "maimaidxprober",
        "method": "POST",
        "path": "/player/update_records",
        "auth": "login_or_import_token",
        "keyRequired": "Import-Token 或登录 jwt_token",
        "mutating": True,
        "description": "批量更新当前用户 maimai 成绩。",
    },
    "maimai_player_update_records_html_post": {
        "game": "maimaidxprober",
        "method": "POST",
        "path": "/player/update_records_html",
        "auth": "login",
        "keyRequired": False,
        "mutating": True,
        "rawBodyAllowed": True,
        "description": "通过 HTML 源码导入 maimai 成绩。",
    },
    "maimai_player_update_record_post": {
        "game": "maimaidxprober",
        "method": "POST",
        "path": "/player/update_record",
        "auth": "login_or_import_token",
        "keyRequired": "Import-Token 或登录 jwt_token",
        "mutating": True,
        "description": "更新当前用户 maimai 单曲成绩。",
    },
    "maimai_player_delete_records_delete": {
        "game": "maimaidxprober",
        "method": "DELETE",
        "path": "/player/delete_records",
        "auth": "login_or_import_token",
        "keyRequired": "Import-Token 或登录 jwt_token",
        "mutating": True,
        "destructive": True,
        "description": "删除当前用户全部 maimai 成绩。",
    },
    "maimai_chart_stats_get": {
        "game": "maimaidxprober",
        "method": "GET",
        "path": "/chart_stats",
        "auth": "none",
        "keyRequired": False,
        "description": "获取 maimai 谱面拟合难度和分布统计。",
    },
    "chunithm_music_data_get": {
        "game": "chunithmprober",
        "method": "GET",
        "path": "/music_data",
        "auth": "none",
        "keyRequired": False,
        "description": "获取 CHUNITHM 歌曲数据。支持 If-None-Match 缓存校验。",
    },
    "chunithm_latest_version_get": {
        "game": "chunithmprober",
        "method": "GET",
        "path": "/latest_version",
        "auth": "none",
        "keyRequired": False,
        "description": "获取 CHUNITHM 当前新曲版本标识。",
    },
    "chunithm_player_records_get": {
        "game": "chunithmprober",
        "method": "GET",
        "path": "/player/records",
        "auth": "login_or_import_token",
        "keyRequired": "Import-Token 或登录 jwt_token",
        "description": "获取当前用户 CHUNITHM 完整成绩。",
    },
    "chunithm_player_test_data_get": {
        "game": "chunithmprober",
        "method": "GET",
        "path": "/player/test_data",
        "auth": "none",
        "keyRequired": False,
        "description": "获取 CHUNITHM 测试成绩数据。",
    },
    "chunithm_dev_player_records_get": {
        "game": "chunithmprober",
        "method": "GET",
        "path": "/dev/player/records",
        "auth": "developer_token",
        "keyRequired": "Developer-Token",
        "description": "通过 Developer-Token 获取指定用户 CHUNITHM 完整成绩。",
        "queryHint": {"qq": "123456789"},
    },
    "chunithm_update_records_html_post": {
        "game": "chunithmprober",
        "method": "POST",
        "path": "/player/update_records_html",
        "auth": "login_or_import_token",
        "keyRequired": "Import-Token 或登录 jwt_token",
        "mutating": True,
        "rawBodyAllowed": True,
        "description": "通过 HTML 源码导入 CHUNITHM 成绩。",
        "queryHint": {"recent": 0},
    },
    "chunithm_delete_records_delete": {
        "game": "chunithmprober",
        "method": "DELETE",
        "path": "/player/delete_records",
        "auth": "login_or_import_token",
        "keyRequired": "Import-Token 或登录 jwt_token",
        "mutating": True,
        "destructive": True,
        "description": "删除当前用户全部 CHUNITHM 成绩。",
    },
    "chunithm_query_player_post": {
        "game": "chunithmprober",
        "method": "POST",
        "path": "/query/player",
        "auth": "none",
        "keyRequired": False,
        "description": "无需验证查询用户 CHUNITHM 简略成绩（b30+n20）。",
        "bodyHint": {"qq": "123456789"},
    },
    "public_count_view_get": {
        "game": "maimaidxprober",
        "method": "GET",
        "path": "/count_view",
        "auth": "none",
        "keyRequired": False,
        "description": "获取查分器主页 views 次数。",
    },
    "public_alive_check_get": {
        "game": "maimaidxprober",
        "method": "GET",
        "path": "/alive_check",
        "auth": "none",
        "keyRequired": False,
        "description": "验证服务器状态。",
    },
    "public_message_get": {
        "game": "maimaidxprober",
        "method": "GET",
        "path": "/message",
        "auth": "none",
        "keyRequired": False,
        "description": "获取查分器主页今日留言。",
    },
    "public_message_post": {
        "game": "maimaidxprober",
        "method": "POST",
        "path": "/message",
        "auth": "login",
        "keyRequired": False,
        "mutating": True,
        "description": "提交查分器主页今日留言。",
        "bodyHint": {"text": "早", "nickname": ""},
    },
    "public_advertisements_get": {
        "game": "maimaidxprober",
        "method": "GET",
        "path": "/advertisements",
        "auth": "none",
        "keyRequired": False,
        "description": "获取查分器主页广告。",
    },
}

OPERATION_NAMES = list(API_CATALOG.keys())


def public_endpoint_metadata(operation: str, endpoint: dict[str, Any]) -> dict[str, Any]:
    return {
        "operation": operation,
        "game": endpoint["game"],
        "method": endpoint["method"],
        "path": endpoint["path"],
        "url": None if endpoint.get("noHttp") else build_api_url(endpoint, {}),
        "auth": endpoint["auth"],
        "keyRequired": endpoint["keyRequired"],
        "mutating": endpoint.get("mutating") is True,
        "destructive": endpoint.get("destructive") is True,
        "requiresConfirmation": endpoint.get("mutating") is True,
        "description": endpoint["description"],
        "queryHint": endpoint.get("queryHint"),
        "bodyHint": endpoint.get("bodyHint"),
    }


def public_api_catalog(
    *,
    game: str | None = None,
    auth: str | None = None,
    include_mutating: bool = False,
) -> dict[str, dict[str, Any]]:
    result: dict[str, dict[str, Any]] = {}
    for operation, endpoint in API_CATALOG.items():
        if game and endpoint["game"] != game:
            continue
        if auth and endpoint["auth"] != auth:
            continue
        if not include_mutating and endpoint.get("mutating"):
            continue
        result[operation] = public_endpoint_metadata(operation, endpoint)
    return result


def build_api_url(endpoint: dict[str, Any], query: dict[str, Any] | None = None) -> str:
    from urllib.parse import urlencode

    url = f"{API_BASE_URL}/{endpoint['game']}{endpoint['path']}"
    query = query or {}
    pairs: list[tuple[str, str]] = []
    for key, value in query.items():
        if value is None:
            continue
        if isinstance(value, list):
            pairs.extend((key, str(item)) for item in value)
        else:
            pairs.append((key, str(value)))
    if pairs:
        url = f"{url}?{urlencode(pairs)}"
    return url


def build_cover_url(song_id: Any) -> str:
    try:
        raw_id = int(str(song_id))
    except (TypeError, ValueError) as exc:
        raise ValueError("maimai_cover_url 需要 query.song_id 或 query.id 为整数。") from exc
    cover_id = raw_id - 10000 if 10000 < raw_id <= 11000 else raw_id
    return f"{COVER_BASE_URL}/{cover_id:05d}.png"
