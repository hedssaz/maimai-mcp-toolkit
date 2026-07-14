from __future__ import annotations

import json
import sys
import traceback
from typing import Any

from . import __version__
from scripts.maimai_update_records_workflow import (
    WorkflowError,
    bind_import_token,
    update_records_workflow,
)


SERVER_NAME = "maimai-update-records-direct-mcp"

TOOLS = [
    {
        "name": "maimai_bind_import_token",
        "description": "Direct-only: bind sender QQ to a Diving-Fish Import-Token for score upload.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "qq": {"type": "string"},
                "importToken": {"type": "string"},
            },
            "required": ["qq", "importToken"],
            "additionalProperties": False,
        },
    },
    {
        "name": "maimai_update_records",
        "description": "Direct-only: QR login, dump official raw records, convert to Diving-Fish update_records and upload.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "qq": {"type": "string"},
                "qrContent": {"type": "string"},
                "keyship": {"type": "string"},
                "logoutid": {"type": "integer", "enum": [1, 2]},
                "titleVer": {"type": "string"},
                "timeout": {"type": "number"},
            },
            "required": ["qq", "qrContent"],
            "additionalProperties": False,
        },
    },
]


def text_result(text: str, structured: dict[str, Any] | None = None, *, is_error: bool = False) -> dict[str, Any]:
    result: dict[str, Any] = {
        "content": [{"type": "text", "text": text}],
        "isError": is_error,
    }
    if structured is not None:
        result["structuredContent"] = structured
    return result


def call_tool(name: str, arguments: dict[str, Any]) -> dict[str, Any]:
    if name == "maimai_bind_import_token":
        result = bind_import_token(arguments.get("qq"), arguments.get("importToken"))
        return text_result(result["text"], result)
    if name == "maimai_update_records":
        result = update_records_workflow(
            qq=arguments.get("qq"),
            qr_content=arguments.get("qrContent"),
            keyship=arguments.get("keyship") or None,
            logoutid=arguments.get("logoutid"),
            title_ver=arguments.get("titleVer") or None,
            timeout=float(arguments.get("timeout") or 240.0),
        )
        return text_result(result["text"], result)
    raise WorkflowError(f"未知工具：{name}")


def handle_request(message: dict[str, Any]) -> dict[str, Any] | None:
    method = message.get("method")
    message_id = message.get("id")
    try:
        if method == "initialize":
            return {
                "jsonrpc": "2.0",
                "id": message_id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "serverInfo": {"name": SERVER_NAME, "version": __version__},
                    "capabilities": {"tools": {}},
                },
            }
        if method == "tools/list":
            return {"jsonrpc": "2.0", "id": message_id, "result": {"tools": TOOLS}}
        if method == "tools/call":
            params = message.get("params") if isinstance(message.get("params"), dict) else {}
            name = params.get("name")
            arguments = params.get("arguments") if isinstance(params.get("arguments"), dict) else {}
            if not isinstance(name, str):
                raise WorkflowError("tools/call 缺少工具名。")
            return {"jsonrpc": "2.0", "id": message_id, "result": call_tool(name, arguments)}
        if method == "notifications/initialized":
            return None
        return {
            "jsonrpc": "2.0",
            "id": message_id,
            "error": {"code": -32601, "message": f"Method not found: {method}"},
        }
    except WorkflowError as exc:
        return {
            "jsonrpc": "2.0",
            "id": message_id,
            "result": text_result(str(exc), {"ok": False, "error": str(exc)}, is_error=True),
        }
    except Exception as exc:  # noqa: BLE001 - return MCP-visible error with stderr trace
        traceback.print_exc(file=sys.stderr)
        return {
            "jsonrpc": "2.0",
            "id": message_id,
            "result": text_result(f"成绩上传工作流失败：{exc}", {"ok": False, "error": str(exc)}, is_error=True),
        }


def main() -> None:
    for raw_line in sys.stdin:
        raw_line = raw_line.strip()
        if not raw_line:
            continue
        try:
            message = json.loads(raw_line)
        except json.JSONDecodeError:
            continue
        if not isinstance(message, dict):
            continue
        response = handle_request(message)
        if response is None:
            continue
        print(json.dumps(response, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
