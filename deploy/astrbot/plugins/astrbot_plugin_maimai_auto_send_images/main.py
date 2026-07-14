from __future__ import annotations

import json
import os
import re
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable

from mcp.types import CallToolResult, TextContent

try:
    from astrbot.api import AstrBotConfig, logger
except Exception:  # pragma: no cover - local test fallback
    import logging

    from astrbot.api import AstrBotConfig

    logger = logging.getLogger(__name__)
from astrbot.api.event import AstrMessageEvent, filter
from astrbot.api.star import Context, Star, register
from astrbot.core.agent.tool import FunctionTool

try:
    from astrbot.core.agent.run_context import ContextWrapper
except Exception:  # pragma: no cover - compatibility fallback
    ContextWrapper = None  # type: ignore[assignment]

try:
    import astrbot.api.message_components as Comp
except Exception:  # pragma: no cover - compatibility fallback
    Comp = None  # type: ignore[assignment]

from .image_paths import (
    collect_paths_from_mapping,
    collect_paths_from_text,
    is_maimai_subagent_tool_name,
    is_render_tool_name,
    is_subagent_tool_name,
    parse_lines,
    parse_prefix_mappings,
    strip_paths_from_text,
    unique_existing_paths,
)
from .direct_render import (
    DirectMcpClient,
    DirectRenderError,
    TargetContext,
    direct_mcp_config_from_mapping,
    handle_direct_command,
    normalize_command_text,
    parse_direct_render_command,
)


def _message_event_decorator() -> Callable[[Callable[..., Any]], Callable[..., Any]]:
    event_message_type = getattr(filter, "event_message_type", None)
    event_types = getattr(filter, "EventMessageType", None)
    all_messages = getattr(event_types, "ALL", None)
    if callable(event_message_type) and all_messages is not None:
        return event_message_type(all_messages)
    return lambda func: func


def _is_ambiguous_napcat_send_timeout(exc: BaseException) -> bool:
    """识别 NapCat 已提交消息、但等待发送回执超时的异常。"""

    parts = [str(exc)]
    for attribute in ("message", "wording"):
        value = getattr(exc, attribute, None)
        if value:
            parts.append(str(value))
    parts.extend(str(value) for value in getattr(exc, "args", ()) if value)
    text = " ".join(parts).casefold()
    compact = re.sub(r"\s+", "", text)
    retcode = str(getattr(exc, "retcode", "") or "").strip()
    has_retcode = retcode == "1200" or "retcode=1200" in compact or "retcode:1200" in compact
    has_send_method = (
        "nodeikernelmsgservice/sendmsg" in compact
        or "serviceandmethod:nodeikernelmsgservice/sendmsg" in compact
    )
    return has_retcode and "timeout" in text and has_send_method


@dataclass
class SentState:
    paths: list[str] = field(default_factory=list)
    expires_at: float = 0.0


class AstrBotToolMcpClient:
    def __init__(self, context: Context, config: AstrBotConfig, fallback: DirectMcpClient):
        self.context = context
        self.config = config
        self.fallback = fallback

    async def call_tool(self, server: str, tool_name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        if server == "upload":
            return await self.fallback.call_tool(server, tool_name, arguments)

        tool = self._tool(tool_name)
        if tool is None or ContextWrapper is None:
            return await self.fallback.call_tool(server, tool_name, arguments)

        timeout = int(self.config.get("direct_render_timeout_seconds", 90) or 90)
        try:
            result = await tool.call(ContextWrapper(context=None, tool_call_timeout=timeout), **arguments)
        except Exception as exc:
            raise DirectRenderError(f"AstrBot MCP 工具调用失败: {tool_name}: {exc}") from exc
        return self._tool_result_to_mapping(result)

    def _tool(self, tool_name: str) -> FunctionTool | None:
        getter = getattr(self.context, "get_llm_tool_manager", None)
        if not callable(getter):
            return None
        try:
            manager = getter()
        except Exception:
            return None
        get_func = getattr(manager, "get_func", None)
        if not callable(get_func):
            return None
        try:
            tool = get_func(tool_name)
        except Exception:
            return None
        return tool if isinstance(tool, FunctionTool) else tool

    def _tool_result_to_mapping(self, result: Any) -> dict[str, Any]:
        if isinstance(result, dict):
            return result

        data: dict[str, Any] = {}
        is_error = getattr(result, "isError", None)
        if is_error is None:
            is_error = getattr(result, "is_error", False)
        data["isError"] = bool(is_error)

        structured = (
            getattr(result, "structuredContent", None)
            or getattr(result, "structured_content", None)
        )
        if structured is not None:
            data["structuredContent"] = structured

        content: list[dict[str, Any]] = []
        for item in getattr(result, "content", []) or []:
            if isinstance(item, dict):
                content.append(item)
                continue
            item_type = getattr(item, "type", "text") or "text"
            text = getattr(item, "text", None)
            if isinstance(text, str):
                content.append({"type": str(item_type), "text": text})
        data["content"] = content
        return data


@register(
    "astrbot_plugin_maimai_auto_send_images",
    "maimai-mcp",
    "自动发送 maimai 绘图 MCP 生成的图片，并隐藏已发送的本地路径。",
    "0.1.0",
)
class MaimaiAutoSendImagesPlugin(Star):
    def __init__(self, context: Context, config: AstrBotConfig | None = None):
        super().__init__(context)
        self.config = config or {}
        self._sent_by_event: dict[str, SentState] = {}
        self._recent_path_keys: dict[str, float] = {}

    @_message_event_decorator()
    async def on_direct_render_message(self, event: AstrMessageEvent):
        started_at = time.perf_counter()
        if not self._config_bool("enabled", True):
            return
        if not self._config_bool("direct_render_enabled", True):
            return

        command_text = self._direct_command_text(event)
        if command_text is None:
            return
        parsed_at = time.perf_counter()

        whitelist_reply = self._handle_whitelist_command(event, command_text)
        if whitelist_reply is not None:
            self._disable_llm(event)
            await event.send(event.plain_result(whitelist_reply))
            self._stop_event(event)
            return

        context = TargetContext(
            sender_qq=self._sender_id(event),
            mention_qqs=tuple(self._mention_qqs(event)),
            self_qq=self._self_id(event),
            group_id=self._group_id(event),
            is_private=self._is_private_chat(event),
        )
        command = parse_direct_render_command(command_text, context)
        if command is None:
            if self._config_bool("direct_render_log_unmatched", False):
                logger.debug(
                    "maimai direct-render wake did not match a command: origin=%s text=%r",
                    getattr(event, "unified_msg_origin", ""),
                    command_text,
                )
            return

        tool_name = command.render_tool_name or command.tool_name
        logger.info(
            "maimai direct-render matched: origin=%s tool=%s text=%r args=%s",
            getattr(event, "unified_msg_origin", ""),
            tool_name,
            command_text,
            command.search_arguments or command.arguments,
        )
        self._disable_llm(event)
        client = self._direct_mcp_client()
        try:
            mcp_started_at = time.perf_counter()
            result = await handle_direct_command(
                command,
                client,
                path_prefix_mappings=self.config.get("path_prefix_mappings", ""),
                max_images=int(self.config.get("max_images_per_tool", 8) or 8),
                today_offset=self.config.get("direct_render_today_offset", 0),
            )
            mcp_finished_at = time.perf_counter()
        except DirectRenderError as exc:
            logger.warning("maimai direct-render MCP failed: tool=%s error=%s", tool_name, exc)
            await event.send(event.plain_result(f"直连绘图失败：{exc}"))
            self._stop_event(event)
            return

        sent_paths: list[str] = []
        uncertain_paths: list[str] = []
        send_started_at = time.perf_counter()
        for path in result.image_paths:
            if self._is_recent_duplicate(event, path):
                continue
            try:
                await self._send_image(event, path)
            except Exception as exc:
                if _is_ambiguous_napcat_send_timeout(exc):
                    uncertain_paths.append(path)
                    self._mark_recent_duplicate(event, path)
                    logger.warning(
                        "maimai direct-render image send acknowledgement timed out; delivery unknown, suppress fallback: path=%s error=%s",
                        path,
                        exc,
                    )
                    continue
                logger.warning("maimai direct-render image send failed: path=%s error=%s", path, exc)
                continue
            self._mark_recent_duplicate(event, path)
            sent_paths.append(path)

        handled_paths = [*sent_paths, *uncertain_paths]
        if handled_paths:
            self._remember_sent_paths(event, handled_paths)
        elif result.text:
            await event.send(event.plain_result(result.text))
        else:
            await event.send(event.plain_result("绘图完成，但没有找到可发送的图片。"))
        finished_at = time.perf_counter()
        if self._config_bool("direct_render_log_timing", True):
            logger.info(
                "maimai direct-render finished: tool=%s images=%d uncertain_images=%d text_len=%d parse_ms=%.1f mcp_ms=%.1f send_ms=%.1f total_ms=%.1f",
                tool_name,
                len(sent_paths),
                len(uncertain_paths),
                len(result.text or ""),
                (parsed_at - started_at) * 1000,
                (mcp_finished_at - mcp_started_at) * 1000,
                (finished_at - send_started_at) * 1000,
                (finished_at - started_at) * 1000,
            )
        self._stop_event(event)

    @filter.on_llm_tool_respond()
    async def on_llm_tool_respond(
        self,
        event: AstrMessageEvent,
        tool: FunctionTool,
        tool_args: dict | None,
        tool_result: CallToolResult | None,
    ):
        if not self._config_bool("enabled", True):
            return
        if tool_result is None or getattr(tool_result, "isError", False):
            return
        tool_name = getattr(tool, "name", "") or ""
        if not self._should_handle_tool(tool_name):
            return

        self._cleanup_state()
        paths = self._extract_image_paths(tool_result)
        max_images = max(1, int(self.config.get("max_images_per_tool", 8) or 8))
        sent_paths: list[str] = []
        uncertain_paths: list[str] = []
        for path in paths[:max_images]:
            if self._is_recent_duplicate(event, path):
                continue
            try:
                await self._send_image(event, path)
            except Exception as exc:
                if _is_ambiguous_napcat_send_timeout(exc):
                    uncertain_paths.append(path)
                    self._mark_recent_duplicate(event, path)
                    logger.warning(
                        "maimai auto-send tool image acknowledgement timed out; delivery unknown: tool=%s path=%s error=%s",
                        tool_name,
                        path,
                        exc,
                    )
                    continue
                logger.warning("maimai auto-send tool image send failed: tool=%s path=%s error=%s", tool_name, path, exc)
                continue
            self._mark_recent_duplicate(event, path)
            sent_paths.append(path)

        handled_paths = [*sent_paths, *uncertain_paths]
        if not handled_paths:
            return
        self._remember_sent_paths(event, handled_paths)
        if self._config_bool("suppress_tool_result_paths", True):
            self._suppress_tool_result_paths(tool_result, handled_paths)
        if self._config_bool("direct_render_log_timing", True):
            logger.info(
                "maimai auto-send tool result: tool=%s paths=%d sent=%d uncertain=%d",
                tool_name,
                len(paths),
                len(sent_paths),
                len(uncertain_paths),
            )

    @filter.on_using_llm_tool()
    async def on_using_llm_tool(
        self,
        event: AstrMessageEvent,
        tool: FunctionTool,
        tool_args: dict | None,
    ):
        del event, tool_args
        if not self._config_bool("enabled", True):
            return
        if not self._config_bool("suppress_duplicate_send_message_calls", True):
            return
        if (getattr(tool, "name", "") or "") != "send_message_to_user":
            return
        self._ensure_send_message_tool_wrapped(tool)

    @filter.on_decorating_result()
    async def on_decorating_result(self, event: AstrMessageEvent):
        if not self._config_bool("enabled", True):
            return
        if not self._config_bool("suppress_sent_paths", True):
            return
        state = self._sent_by_event.get(self._event_key(event))
        if not state or state.expires_at < time.time():
            return
        result = event.get_result()
        chain = getattr(result, "chain", None)
        if not isinstance(chain, list):
            return
        sent_paths = state.paths
        new_chain = []
        for item in chain:
            if self._is_sent_image_component(item, sent_paths):
                continue
            if self._is_plain_component(item):
                text = getattr(item, "text", "")
                cleaned = strip_paths_from_text(str(text), sent_paths)
                if not cleaned:
                    continue
                try:
                    item.text = cleaned
                except Exception:
                    if Comp is not None:
                        item = Comp.Plain(cleaned)
            new_chain.append(item)
        chain[:] = new_chain

    def _should_handle_tool(self, tool_name: str) -> bool:
        extra_patterns = parse_lines(self.config.get("tool_name_patterns", ""))
        if self._config_bool("send_direct_mcp_tools", True) and is_render_tool_name(tool_name, extra_patterns):
            return True
        if is_maimai_subagent_tool_name(tool_name):
            return self._config_bool("send_maimai_subagent_results", True)
        return self._config_bool("send_subagent_results", False) and is_subagent_tool_name(tool_name)

    def _direct_mcp_client(self) -> Any:
        fallback = DirectMcpClient(direct_mcp_config_from_mapping(self.config))
        if not self._config_bool("direct_render_use_astrbot_mcp", True):
            return fallback
        return AstrBotToolMcpClient(self.context, self.config, fallback)

    def _extract_image_paths(self, tool_result: CallToolResult) -> list[str]:
        raw_paths: list[str] = []
        structured = (
            getattr(tool_result, "structuredContent", None)
            or getattr(tool_result, "structured_content", None)
        )
        raw_paths.extend(collect_paths_from_mapping(structured))
        for item in getattr(tool_result, "content", []) or []:
            if isinstance(item, TextContent):
                raw_paths.extend(collect_paths_from_text(item.text))
            else:
                text = getattr(item, "text", None)
                if isinstance(text, str):
                    raw_paths.extend(collect_paths_from_text(text))
        mappings = parse_prefix_mappings(self.config.get("path_prefix_mappings", ""))
        return unique_existing_paths(raw_paths, mappings)

    def _suppress_tool_result_paths(self, tool_result: CallToolResult, sent_paths: list[str]) -> None:
        notice = "图片已由插件自动发送。不要再次调用 send_message_to_user，也不要复述本地图片路径。"
        content = getattr(tool_result, "content", None)
        if not isinstance(content, list):
            return
        for index, item in enumerate(content):
            text = None
            if isinstance(item, dict):
                value = item.get("text")
                if isinstance(value, str):
                    text = value
            else:
                value = getattr(item, "text", None)
                if isinstance(value, str):
                    text = value
            if not text:
                continue
            cleaned = strip_paths_from_text(text, sent_paths)
            if cleaned == text:
                continue
            replacement = notice if not cleaned else f"{cleaned}\n{notice}"
            if isinstance(item, dict):
                item["text"] = replacement
                continue
            try:
                item.text = replacement
            except Exception:
                if Comp is not None:
                    content[index] = Comp.Plain(replacement)

    def _ensure_send_message_tool_wrapped(self, tool: FunctionTool) -> None:
        if getattr(tool, "_maimai_auto_send_wrapped", False):
            return
        original_call = getattr(tool, "call", None)
        if not callable(original_call):
            return
        plugin = self

        async def wrapped_call(context_wrapper: Any, **kwargs: Any) -> Any:
            event = None
            try:
                event = context_wrapper.context.event
            except Exception:
                event = None
            if event is not None and plugin._config_bool("suppress_duplicate_send_message_calls", True):
                plugin._cleanup_state()
                state = plugin._sent_by_event.get(plugin._event_key(event))
                if state and state.expires_at >= time.time():
                    working_kwargs = dict(kwargs)
                    removed = plugin._remove_sent_image_messages(working_kwargs, state.paths)
                    if removed:
                        logger.info(
                            "maimai auto-send skipped duplicate send_message_to_user image call: removed=%d",
                            removed,
                        )
                        return "Message skipped because the maimai image was already sent by the plugin."
            return await original_call(context_wrapper, **kwargs)

        try:
            setattr(tool, "call", wrapped_call)
            setattr(tool, "_maimai_auto_send_wrapped", True)
        except Exception:
            logger.warning("maimai auto-send failed to wrap send_message_to_user tool", exc_info=True)

    def _remove_sent_image_messages(self, tool_args: dict[str, Any], sent_paths: list[str]) -> int:
        messages = tool_args.get("messages")
        if not isinstance(messages, list):
            return 0
        kept: list[Any] = []
        removed = 0
        for message in messages:
            if self._is_sent_image_message(message, sent_paths):
                removed += 1
                continue
            kept.append(message)
        if removed:
            tool_args["messages"] = kept
        return removed

    def _is_sent_image_message(self, message: Any, sent_paths: list[str]) -> bool:
        if not isinstance(message, dict):
            return False
        if str(message.get("type", "")).lower() != "image":
            return False
        value = message.get("path") or message.get("url")
        if not isinstance(value, str) or not value:
            return False
        normalized = value.removeprefix("file:///").removeprefix("$")
        real = os.path.realpath(normalized)
        for path in sent_paths:
            if normalized == path or real == os.path.realpath(path):
                return True
        return False

    async def _send_image(self, event: AstrMessageEvent, path: str) -> None:
        caption = str(self.config.get("send_caption", "") or "").strip()
        if caption:
            await event.send(event.plain_result(caption))
        await event.send(event.image_result(path))

    def _remember_sent_paths(self, event: AstrMessageEvent, paths: list[str]) -> None:
        ttl = max(30, int(self.config.get("dedupe_ttl_seconds", 300) or 300))
        key = self._event_key(event)
        state = self._sent_by_event.get(key)
        if state is None:
            state = SentState(expires_at=time.time() + ttl)
            self._sent_by_event[key] = state
        state.expires_at = time.time() + ttl
        for path in paths:
            if path not in state.paths:
                state.paths.append(path)

    def _event_key(self, event: AstrMessageEvent) -> str:
        message_obj = getattr(event, "message_obj", None)
        message_id = getattr(message_obj, "message_id", None) or getattr(message_obj, "messageId", None)
        origin = getattr(event, "unified_msg_origin", "")
        if origin or message_id:
            return f"{origin}:{message_id}"
        return str(id(event))

    def _path_key(self, event: AstrMessageEvent, path: str) -> str:
        return f"{self._event_key(event)}:{os.path.realpath(path)}"

    def _is_recent_duplicate(self, event: AstrMessageEvent, path: str) -> bool:
        key = self._path_key(event, path)
        expires_at = self._recent_path_keys.get(key)
        return bool(expires_at and expires_at >= time.time())

    def _mark_recent_duplicate(self, event: AstrMessageEvent, path: str) -> None:
        ttl = max(30, int(self.config.get("dedupe_ttl_seconds", 300) or 300))
        self._recent_path_keys[self._path_key(event, path)] = time.time() + ttl

    def _cleanup_state(self) -> None:
        now = time.time()
        self._sent_by_event = {
            key: value for key, value in self._sent_by_event.items() if value.expires_at >= now
        }
        self._recent_path_keys = {
            key: expires_at for key, expires_at in self._recent_path_keys.items() if expires_at >= now
        }

    def _config_bool(self, key: str, default: bool) -> bool:
        value = self.config.get(key, default)
        if isinstance(value, bool):
            return value
        if isinstance(value, str):
            return value.strip().lower() in {"1", "true", "yes", "on", "启用"}
        return bool(value)

    def _direct_command_text(self, event: AstrMessageEvent) -> str | None:
        text = normalize_command_text(self._plain_text_from_event(event))
        if not text:
            return None

        prefixed = self._strip_direct_prefix(text)
        if prefixed is not None:
            return prefixed

        if not self._config_bool("direct_render_require_wake", True):
            return text
        if self._config_bool("direct_render_allow_private_without_wake", True) and self._is_private_chat(event):
            return text
        if self._event_is_wake(event):
            return text
        if self._is_whitelist_command_text(text):
            return None
        if self._event_in_direct_whitelist(event):
            return text
        return None

    def _handle_whitelist_command(self, event: AstrMessageEvent, text: str) -> str | None:
        raw_text = text.strip()
        if not self._is_whitelist_command_text(raw_text):
            return None
        if not self._event_is_wake(event):
            return None
        if not self._sender_is_current_config_admin(event):
            return "只有当前适配器配置文件里的管理员可以修改直连群白名单。"

        add_match = re.fullmatch(r"whitelist\s+add\s+([A-Za-z0-9_.:-]+)\s+(\d{1,32})", raw_text, flags=re.IGNORECASE)
        del_match = re.fullmatch(r"whitelist\s+del\s+([A-Za-z0-9_.:-]+)\s+(\d{1,32})", raw_text, flags=re.IGNORECASE)
        list_match = re.fullmatch(r"whitelist\s+list(?:\s+([A-Za-z0-9_.:-]+))?", raw_text, flags=re.IGNORECASE)
        current_adapter = self._adapter_id(event)
        if not current_adapter:
            return "无法识别当前接收适配器，未修改白名单。"

        if add_match is None and del_match is None and list_match is None:
            return self._whitelist_usage()

        target_adapter = (
            (add_match or del_match).group(1)
            if (add_match or del_match) is not None
            else (list_match.group(1) or current_adapter)
        )
        if target_adapter != current_adapter:
            return f"只能管理当前接收适配器 {current_adapter} 的群白名单。"

        data = self._read_direct_whitelist()
        groups = self._direct_whitelist_groups(data, current_adapter)

        if list_match is not None:
            if not groups:
                return f"直连群白名单 {current_adapter}: 空"
            return f"直连群白名单 {current_adapter}:\n" + "\n".join(groups)

        target_group_id = (add_match or del_match).group(2)
        if add_match is not None:
            already_exists = target_group_id in groups
            if already_exists:
                return f"直连群白名单已存在：{current_adapter} {target_group_id}"
            groups.append(target_group_id)
            groups.sort(key=self._direct_whitelist_sort_key)
            self._write_direct_whitelist(data)
            return f"已加入直连群白名单：{current_adapter} {target_group_id}"

        if target_group_id not in groups:
            return f"直连群白名单不存在：{current_adapter} {target_group_id}"
        groups.remove(target_group_id)
        self._write_direct_whitelist(data)
        return f"已移出直连群白名单：{current_adapter} {target_group_id}"

    def _is_whitelist_command_text(self, text: str) -> bool:
        return bool(re.match(r"^\s*whitelist(?:\s|$)", text, flags=re.IGNORECASE))

    def _whitelist_usage(self) -> str:
        return "用法：whitelist add 适配器名 群号 / whitelist del 适配器名 群号 / whitelist list [适配器名]"

    def _strip_direct_prefix(self, text: str) -> str | None:
        prefixes = parse_lines(self.config.get("direct_render_prefixes", ""))
        for prefix in prefixes:
            prefix = prefix.strip()
            if not prefix:
                continue
            if text == prefix:
                return ""
            if text.startswith(prefix + " "):
                return normalize_command_text(text[len(prefix) :])
        return None

    def _event_in_direct_whitelist(self, event: AstrMessageEvent) -> bool:
        if self._is_private_chat(event):
            return False
        adapter_id = self._adapter_id(event)
        group_id = self._group_id(event)
        if not adapter_id or not group_id:
            return False
        data = self._read_direct_whitelist()
        return group_id in self._direct_whitelist_groups(data, adapter_id)

    def _direct_whitelist_file(self) -> Path:
        configured = str(self.config.get("direct_render_group_whitelist_file", "") or "").strip()
        if configured:
            return Path(configured).expanduser()
        data_dir = str(self.config.get("direct_render_data_dir", "/AstrBot/data") or "/AstrBot/data")
        return Path(data_dir) / "maimai-config" / "direct-render-group-whitelist.json"

    def _read_direct_whitelist(self) -> dict[str, Any]:
        path = self._direct_whitelist_file()
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
        except Exception:
            payload = {}
        if not isinstance(payload, dict):
            payload = {}
        payload.setdefault("version", 1)
        items = payload.get("items")
        if not isinstance(items, dict):
            payload["items"] = {}
        return payload

    def _write_direct_whitelist(self, data: dict[str, Any]) -> None:
        path = self._direct_whitelist_file()
        path.parent.mkdir(parents=True, exist_ok=True)
        temp_path = path.with_name(f".{path.name}.{os.getpid()}.{time.time_ns()}.tmp")
        temp_path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        os.replace(temp_path, path)

    def _direct_whitelist_groups(self, data: dict[str, Any], adapter_id: str) -> list[str]:
        items = data.setdefault("items", {})
        if not isinstance(items, dict):
            items = {}
            data["items"] = items
        groups = items.get(adapter_id)
        if isinstance(groups, list):
            normalized = sorted(
                {str(group).strip() for group in groups if str(group).strip()},
                key=self._direct_whitelist_sort_key,
            )
        else:
            normalized = []
        items[adapter_id] = normalized
        return normalized

    def _direct_whitelist_sort_key(self, item: str) -> tuple[int, int | str]:
        return (0, int(item)) if item.isdigit() else (1, item)

    def _sender_is_current_config_admin(self, event: AstrMessageEvent) -> bool:
        if str(getattr(event, "role", "") or "").lower() == "admin":
            return True
        sender_id = self._sender_id(event)
        if not sender_id:
            return False
        cfg: Any = None
        getter = getattr(getattr(self, "context", None), "get_config", None)
        if callable(getter):
            try:
                cfg = getter(umo=str(getattr(event, "unified_msg_origin", "") or ""))
            except TypeError:
                try:
                    cfg = getter()
                except Exception:
                    cfg = None
            except Exception:
                cfg = None
        admins = cfg.get("admins_id", []) if isinstance(cfg, dict) else []
        return str(sender_id) in {str(admin) for admin in admins}

    def _adapter_id(self, event: AstrMessageEvent) -> str:
        origin = str(getattr(event, "unified_msg_origin", "") or "")
        if origin:
            return origin.split(":", 1)[0]
        for name in ("get_platform_id", "get_platform_name"):
            getter = getattr(event, name, None)
            if callable(getter):
                try:
                    value = getter()
                    if value not in (None, ""):
                        return str(value)
                except Exception:
                    pass
        message_obj = getattr(event, "message_obj", None)
        for obj in (message_obj, getattr(message_obj, "platform", None)):
            for name in ("platform_id", "platformId", "adapter_id", "id", "name"):
                value = getattr(obj, name, None)
                if value not in (None, ""):
                    return str(value)
        return ""

    def _plain_text_from_event(self, event: AstrMessageEvent) -> str:
        text = self._event_message_str(event)
        chain_text = self._message_chain_text_with_mentions(event)
        if text:
            return self._merge_message_str_with_chain_mentions(text, chain_text)

        if chain_text:
            return chain_text

        return ""

    def _message_chain_text_with_mentions(self, event: AstrMessageEvent) -> str:
        pieces: list[str] = []
        self_qq = self._self_id(event)
        for item in self._message_chain(event):
            if self._is_plain_component(item):
                pieces.append(str(getattr(item, "text", "")))
                continue
            if not self._is_at_component(item):
                continue
            qq = self._at_component_qq(item)
            if not qq or qq == self_qq:
                pieces.append(" ")
                continue
            pieces.append(f"[CQ:at,qq={qq}]")
        return "".join(pieces)

    def _merge_message_str_with_chain_mentions(self, text: str, chain_text: str) -> str:
        if not chain_text:
            return text
        normalized_text = normalize_command_text(text)
        normalized_chain = normalize_command_text(chain_text)
        if not normalized_text or not normalized_chain:
            return text
        if normalized_text == normalized_chain:
            return normalized_chain
        if "@" not in normalized_chain and "[cq:at,qq=" not in normalized_chain.casefold():
            return text

        match_text = self._strip_display_mentions_for_match(normalized_text, normalized_chain)
        if not match_text:
            return text
        chain_folded = normalized_chain.casefold()
        text_folded = match_text.casefold()
        start = chain_folded.rfind(text_folded)
        if start < 0:
            parts = match_text.split(maxsplit=1)
            if parts:
                start = chain_folded.rfind(parts[0].casefold())
        if start < 0:
            start = self._compact_text_start(normalized_chain, match_text)
        if start < 0:
            return text

        prefix = normalized_chain[:start]
        for match in re.finditer(r"(?:@(?:\d{5,12}|all)|\[CQ:at,qq=(?:\d{5,12}|all)\])", prefix, flags=re.IGNORECASE):
            if prefix[match.end() :].strip() == "":
                start = match.start()
        return normalized_chain[start:]

    def _strip_display_mentions_for_match(self, text: str, chain_text: str) -> str:
        qqs = set(re.findall(r"\[CQ:at,qq=(\d{5,12})\]", chain_text, flags=re.IGNORECASE))
        for qq in qqs:
            text = re.sub(rf"@[^@\s]*[（(]{re.escape(qq)}[）)]", " ", text)
        return normalize_command_text(text)

    def _compact_text(self, text: str) -> str:
        return re.sub(r"\s+", "", normalize_command_text(text)).casefold()

    def _compact_text_start(self, chain_text: str, text: str) -> int:
        needle = self._compact_text(text)
        if not needle:
            return -1
        spans = list(re.finditer(r"(?:@(?:\d{5,12}|all)|\[CQ:at,qq=(?:\d{5,12}|all)\])", chain_text, flags=re.IGNORECASE))
        span_index = 0
        chars: list[str] = []
        positions: list[int] = []
        index = 0
        while index < len(chain_text):
            if span_index < len(spans) and index == spans[span_index].start():
                index = spans[span_index].end()
                span_index += 1
                continue
            char = chain_text[index]
            if not char.isspace():
                chars.append(char.casefold())
                positions.append(index)
            index += 1
        compact_chain = "".join(chars)
        compact_start = compact_chain.rfind(needle)
        if compact_start < 0 or compact_start >= len(positions):
            return -1
        return positions[compact_start]

    def _event_message_str(self, event: AstrMessageEvent) -> str:
        getter = getattr(event, "get_message_str", None)
        if callable(getter):
            try:
                text = str(getter() or "")
            except Exception:
                text = ""
        else:
            text = str(getattr(event, "message_str", "") or "")
        self_qq = self._self_id(event)
        if self_qq:
            text = re.sub(rf"\[CQ:at,qq={re.escape(self_qq)}\]", " ", text)
            text = re.sub(rf"<at[^>]+(?:id|qq)=[\"']?{re.escape(self_qq)}[\"']?[^>]*/?>", " ", text)
        return text

    def _message_chain(self, event: AstrMessageEvent) -> list[Any]:
        getter = getattr(event, "get_messages", None)
        if callable(getter):
            try:
                messages = getter()
            except Exception:
                messages = None
            if isinstance(messages, list):
                return messages
        message_obj = getattr(event, "message_obj", None)
        for name in ("message", "messages", "chain"):
            messages = getattr(message_obj, name, None)
            if isinstance(messages, list):
                return messages
        return []

    def _mention_qqs(self, event: AstrMessageEvent) -> list[str]:
        qqs: list[str] = []
        for item in self._message_chain(event):
            if not self._is_at_component(item):
                continue
            qq = self._at_component_qq(item)
            if qq:
                qqs.append(qq)
        if qqs:
            return qqs

        getter = getattr(event, "get_message_str", None)
        text = str(getter() if callable(getter) else "")
        return re.findall(r"\[CQ:at,qq=(\d+)\]", text)

    def _sender_id(self, event: AstrMessageEvent) -> str:
        getter = getattr(event, "get_sender_id", None)
        if callable(getter):
            try:
                value = getter()
                if value not in (None, ""):
                    return str(value)
            except Exception:
                pass
        message_obj = getattr(event, "message_obj", None)
        sender = getattr(message_obj, "sender", None)
        for obj in (sender, message_obj):
            for name in ("user_id", "sender_id", "qq", "id"):
                value = getattr(obj, name, None)
                if value not in (None, ""):
                    return str(value)
        return ""

    def _self_id(self, event: AstrMessageEvent) -> str:
        getter = getattr(event, "get_self_id", None)
        if callable(getter):
            try:
                value = getter()
                if value not in (None, ""):
                    return str(value)
            except Exception:
                pass
        message_obj = getattr(event, "message_obj", None)
        for name in ("self_id", "selfId", "bot_id", "botId"):
            value = getattr(message_obj, name, None)
            if value not in (None, ""):
                return str(value)
        return ""

    def _group_id(self, event: AstrMessageEvent) -> str:
        for name in ("get_group_id", "get_groupid", "get_group"):
            getter = getattr(event, name, None)
            if callable(getter):
                try:
                    value = getter()
                    if value not in (None, ""):
                        return str(value)
                except Exception:
                    pass
        message_obj = getattr(event, "message_obj", None)
        for obj in (message_obj, getattr(message_obj, "message", None)):
            for name in ("group_id", "groupId", "group", "group_qq"):
                value = getattr(obj, name, None)
                if value not in (None, ""):
                    return str(value)
        origin = str(getattr(event, "unified_msg_origin", "") or "")
        match = re.search(r"(?:^|:)GroupMessage:(\d+)(?:$|:)", origin)
        if match:
            return match.group(1)
        return ""

    def _is_private_chat(self, event: AstrMessageEvent) -> bool:
        checker = getattr(event, "is_private_chat", None)
        if callable(checker):
            try:
                return bool(checker())
            except Exception:
                return False
        return False

    def _event_is_wake(self, event: AstrMessageEvent) -> bool:
        value = getattr(event, "is_at_or_wake_command", None)
        if value is not None:
            try:
                if callable(value) and value():
                    return True
                if not callable(value) and value:
                    return True
            except Exception:
                return False
            return False

        # Compatibility fallback for non-standard event stubs. Real AstrBot sets
        # is_at_or_wake_command after @bot, wake-prefix, private, or reply wake.
        self_qq = self._self_id(event)
        return bool(self_qq and self_qq in self._mention_qqs(event))

    def _disable_llm(self, event: AstrMessageEvent) -> None:
        setter = getattr(event, "should_call_llm", None)
        if callable(setter):
            try:
                setter(False)
            except Exception:
                pass

    def _stop_event(self, event: AstrMessageEvent) -> None:
        stopper = getattr(event, "stop_event", None)
        if callable(stopper):
            try:
                stopper()
            except Exception:
                pass

    def _is_plain_component(self, item: Any) -> bool:
        if Comp is not None and isinstance(item, getattr(Comp, "Plain", ())):
            return True
        return hasattr(item, "text") and str(getattr(item, "type", "")).lower().endswith("plain")

    def _is_at_component(self, item: Any) -> bool:
        if Comp is not None and isinstance(item, getattr(Comp, "At", ())):
            return True
        item_type = str(getattr(item, "type", "")).lower()
        class_name = item.__class__.__name__.lower()
        return item_type.endswith("at") or class_name.endswith("at")

    def _at_component_qq(self, item: Any) -> str:
        for name in ("qq", "id", "user_id", "target", "uin"):
            value = getattr(item, name, None)
            if value not in (None, ""):
                return str(value)
        return ""

    def _is_sent_image_component(self, item: Any, sent_paths: list[str]) -> bool:
        file_value = getattr(item, "file", None) or getattr(item, "url", None) or getattr(item, "path", None)
        if not isinstance(file_value, str) or not file_value:
            return False
        normalized = file_value.removeprefix("file:///")
        real = os.path.realpath(normalized)
        for path in sent_paths:
            if normalized == path or real == os.path.realpath(path):
                return True
        return False
