from __future__ import annotations

import json
import os
import sys
from pathlib import Path
from typing import Any

from lxns_oauth import LxnsOAuthError, OAuthConfig, OAuthService, OAuthStore

from . import __version__


SERVER_NAME = "lxns-oauth"
TOOL_PREFIX = "maimai_lxns_"


def _string_property(description: str) -> dict[str, Any]:
    return {"type": "string", "description": description}


CONTEXT_PROPERTIES = {
    "qq": _string_property("AstrBot 用户标识；核心内部作为 OAuth subject。"),
    "subject": _string_property("qq 的兼容别名。"),
    "adapterId": _string_property("AstrBot adapter_id。"),
    "adapter": _string_property("adapterId 的兼容别名。"),
    "groupId": _string_property("会话或群标识。"),
    "conversation": _string_property("groupId 的兼容别名。"),
    "botQq": _string_property("当前机器人标识。"),
    "bot": _string_property("botQq 的兼容别名。"),
    "state": _string_property(
        "可选，由可信回调桥生成的 opaque OAuth state。不会出现在结果字段中。"
    ),
}


OAUTH_URL_TOOL = {
    "name": f"{TOOL_PREFIX}oauth_url",
    "description": "创建落雪 OAuth 授权链接；state 为 opaque 随机值或调用方提供的可信签名值。",
    "inputSchema": {
        "type": "object",
        "properties": {
            **CONTEXT_PROPERTIES,
            "scopes": _string_property("可选 OAuth scope 覆盖值。"),
            "ttlSeconds": {
                "type": "integer",
                "minimum": 1,
                "maximum": 3600,
                "description": "OAuth state 有效期，默认 600 秒。",
            },
        },
        "required": ["qq"],
        "additionalProperties": False,
    },
}

BIND_CODE_TOOL = {
    "name": f"{TOOL_PREFIX}bind_code",
    "description": "用手工 code、code=... 或完整 callback URL 完成落雪 OAuth 绑定。",
    "inputSchema": {
        "type": "object",
        "properties": {
            **CONTEXT_PROPERTIES,
            "code": _string_property("OAuth code、code=... 或完整 callback URL。"),
        },
        "required": ["qq", "code"],
        "additionalProperties": False,
    },
}

PREPARE_POKE_TOOL = {
    "name": f"{TOOL_PREFIX}prepare_poke",
    "description": "交换 OAuth code，并将 token 暂存到独立 SQLite，等待原上下文拍一拍确认。",
    "inputSchema": {
        "type": "object",
        "properties": {
            **CONTEXT_PROPERTIES,
            "code": _string_property("OAuth code、code=... 或完整 callback URL。"),
            "ttlSeconds": {
                "type": "integer",
                "minimum": 1,
                "maximum": 1800,
                "description": "待确认有效期，默认 300 秒。",
            },
        },
        "required": ["qq", "code", "adapterId", "groupId", "botQq"],
        "additionalProperties": False,
    },
}

CONFIRM_POKE_TOOL = {
    "name": f"{TOOL_PREFIX}confirm_poke",
    "description": "仅在 subject、adapter、conversation、bot 全部匹配时确认待绑定 token。",
    "inputSchema": {
        "type": "object",
        "properties": {**CONTEXT_PROPERTIES},
        "required": ["qq", "adapterId", "groupId", "botQq"],
        "additionalProperties": False,
    },
}

STATUS_TOOL = {
    "name": f"{TOOL_PREFIX}status",
    "description": "查看落雪 OAuth 是否已绑定或正在等待确认；不会返回 token 信息。",
    "inputSchema": {
        "type": "object",
        "properties": {
            "qq": CONTEXT_PROPERTIES["qq"],
            "subject": CONTEXT_PROPERTIES["subject"],
        },
        "required": ["qq"],
        "additionalProperties": False,
    },
}

UNBIND_TOOL = {
    "name": f"{TOOL_PREFIX}unbind",
    "description": "删除该 subject 的 OAuth token、state 和待确认记录。",
    "inputSchema": {
        "type": "object",
        "properties": {
            "qq": CONTEXT_PROPERTIES["qq"],
            "subject": CONTEXT_PROPERTIES["subject"],
        },
        "required": ["qq"],
        "additionalProperties": False,
    },
}

TOOLS = [
    OAUTH_URL_TOOL,
    BIND_CODE_TOOL,
    PREPARE_POKE_TOOL,
    CONFIRM_POKE_TOOL,
    STATUS_TOOL,
    UNBIND_TOOL,
]

_DEFAULT_SERVICE: OAuthService | None = None


def oauth_db_path_from_env() -> Path:
    configured = str(
        os.environ.get("LXNS_OAUTH_DB") or os.environ.get("LXNS_OAUTH_DB_PATH") or ""
    ).strip()
    if configured:
        return Path(configured).expanduser()
    return (
        Path(__file__).resolve().parents[1] / "data" / ".lxns-oauth" / "oauth.sqlite3"
    )


def default_service() -> OAuthService:
    global _DEFAULT_SERVICE
    if _DEFAULT_SERVICE is None:
        _DEFAULT_SERVICE = OAuthService(
            OAuthConfig.from_env(), OAuthStore(oauth_db_path_from_env())
        )
    return _DEFAULT_SERVICE


def _argument(arguments: dict[str, Any], primary: str, alias: str) -> Any:
    value = arguments.get(primary)
    return arguments.get(alias) if value in (None, "") else value


def _subject(arguments: dict[str, Any]) -> Any:
    return _argument(arguments, "qq", "subject")


def _context(arguments: dict[str, Any]) -> tuple[Any, Any, Any]:
    return (
        _argument(arguments, "adapterId", "adapter"),
        _argument(arguments, "groupId", "conversation"),
        _argument(arguments, "botQq", "bot"),
    )


def _result_text(tool_name: str, result: dict[str, Any]) -> str:
    if tool_name == OAUTH_URL_TOOL["name"]:
        return (
            "打开下面的落雪授权链接完成授权：\n"
            f"{result['authorizationUrl']}\n"
            "授权完成后提交回调 code；拍一拍流程还需要原上下文确认。"
        )
    if tool_name == BIND_CODE_TOOL["name"]:
        return "落雪 OAuth 已绑定。"
    if tool_name == PREPARE_POKE_TOOL["name"]:
        return "已收到落雪授权，等待原用户在原会话拍一拍当前机器人确认。"
    if tool_name == CONFIRM_POKE_TOOL["name"]:
        status = result.get("status")
        return {
            "confirmed": "落雪 OAuth 已确认绑定。",
            "context_mismatch": "待确认绑定与当前用户、适配器、会话或机器人不匹配。",
            "expired": "待确认绑定已过期，请重新授权。",
            "not_found": "没有找到待确认的落雪 OAuth 绑定。",
        }.get(str(status), "落雪 OAuth 确认未完成。")
    if tool_name == STATUS_TOOL["name"]:
        if result.get("bound"):
            return "落雪 OAuth：已绑定。"
        if result.get("pending"):
            return "落雪 OAuth：等待拍一拍确认。"
        return "落雪 OAuth：未绑定。"
    if tool_name == UNBIND_TOOL["name"]:
        return (
            "落雪 OAuth 已解绑。"
            if result.get("changed")
            else "当前没有已绑定的落雪 OAuth。"
        )
    return "操作完成。"


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


def handle_tool_call(
    message_id: Any,
    params: dict[str, Any] | None,
    *,
    service: OAuthService | None = None,
) -> dict[str, Any]:
    params = params or {}
    raw_tool_name = str(params.get("name") or "")
    tool_name = raw_tool_name
    arguments = params.get("arguments") or {}
    if not isinstance(arguments, dict):
        return success(
            message_id,
            {
                "content": [{"type": "text", "text": "arguments 必须是对象。"}],
                "isError": True,
            },
        )
    known_tools = {tool["name"] for tool in TOOLS}
    if tool_name not in known_tools:
        return error(message_id, -32602, f"Unknown tool: {raw_tool_name}")
    try:
        active_service = service or default_service()
        if tool_name == OAUTH_URL_TOOL["name"]:
            adapter, conversation, bot = _context(arguments)
            result = active_service.oauth_url(
                _subject(arguments),
                adapter=adapter,
                conversation=conversation,
                bot=bot,
                state=arguments.get("state"),
                scopes=arguments.get("scopes"),
                ttl_seconds=600
                if arguments.get("ttlSeconds") is None
                else arguments.get("ttlSeconds"),
            )
        elif tool_name == BIND_CODE_TOOL["name"]:
            adapter, conversation, bot = _context(arguments)
            result = active_service.bind_code(
                _subject(arguments),
                arguments.get("code"),
                adapter=adapter,
                conversation=conversation,
                bot=bot,
                state=arguments.get("state"),
            )
        elif tool_name == PREPARE_POKE_TOOL["name"]:
            adapter, conversation, bot = _context(arguments)
            result = active_service.prepare_poke(
                _subject(arguments),
                arguments.get("code"),
                adapter=adapter,
                conversation=conversation,
                bot=bot,
                state=arguments.get("state"),
                ttl_seconds=300
                if arguments.get("ttlSeconds") is None
                else arguments.get("ttlSeconds"),
            )
        elif tool_name == CONFIRM_POKE_TOOL["name"]:
            adapter, conversation, bot = _context(arguments)
            result = active_service.confirm_poke(
                _subject(arguments),
                adapter=adapter,
                conversation=conversation,
                bot=bot,
            )
        elif tool_name == STATUS_TOOL["name"]:
            result = active_service.status(_subject(arguments))
        elif tool_name == UNBIND_TOOL["name"]:
            result = active_service.unbind(_subject(arguments))
    except LxnsOAuthError as exc:
        return success(
            message_id,
            {
                "content": [{"type": "text", "text": str(exc)}],
                "structuredContent": {"error": exc.to_dict()},
                "isError": True,
            },
        )
    except Exception:
        return success(
            message_id,
            {
                "content": [{"type": "text", "text": "落雪 OAuth 内部错误。"}],
                "structuredContent": {
                    "error": {
                        "code": "INTERNAL_ERROR",
                        "message": "落雪 OAuth 内部错误。",
                    }
                },
                "isError": True,
            },
        )

    return success(
        message_id,
        {
            "content": [{"type": "text", "text": _result_text(tool_name, result)}],
            "structuredContent": result,
            "isError": False,
        },
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


def write_message(message: dict[str, Any]) -> None:
    sys.stdout.write(
        json.dumps(message, ensure_ascii=False, separators=(",", ":")) + "\n"
    )
    sys.stdout.flush()


def main() -> None:
    for line in sys.stdin:
        if not line.strip():
            continue
        try:
            message = json.loads(line)
            response = handle_request(message)
        except Exception:
            response = error(None, -32603, "Internal error")
        if response is not None:
            write_message(response)


if __name__ == "__main__":
    main()
