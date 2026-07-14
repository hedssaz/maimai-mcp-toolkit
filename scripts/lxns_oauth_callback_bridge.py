#!/usr/bin/env python3
"""Receive LXNS OAuth callbacks for a bot that polls from a private network."""

from __future__ import annotations

import argparse
import hashlib
import hmac
import html
import json
import os
import re
import secrets
import sqlite3
import stat
import time
from contextlib import contextmanager
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any, Iterator
from urllib.parse import parse_qs, urlsplit


DEFAULT_DB = Path(
    os.environ.get("LXNS_CALLBACK_DB") or "lxns-oauth-callback/callbacks.sqlite3"
)
DEFAULT_HOST = os.environ.get("LXNS_CALLBACK_HOST") or "127.0.0.1"
DEFAULT_PORT = int(os.environ.get("LXNS_CALLBACK_PORT") or "8788")
DEFAULT_PATH = os.environ.get("LXNS_CALLBACK_PATH") or "/lxns/callback"
DEFAULT_POLL_PATH = os.environ.get("LXNS_CALLBACK_POLL_PATH") or "/lxns/poll"
DEFAULT_TTL_SECONDS = int(os.environ.get("LXNS_CALLBACK_TTL_SECONDS") or "600")

MIN_TOKEN_BYTES = 32
MAX_STATE_LENGTH = 512
MAX_CODE_LENGTH = 2048
MAX_ERROR_LENGTH = 512
MAX_CONSUME_LENGTH = 8
MAX_QUERY_FIELDS = 8
MAX_REQUEST_TARGET_LENGTH = 4096
SIGNED_STATE_PREFIX = "lxns"
SIGNED_STATE_SIGNATURE_HEX_LENGTH = 32

_NONCE_RE = re.compile(r"[A-Za-z0-9_-]{20,128}\Z")
_SIGNATURE_RE = re.compile(rf"[0-9a-f]{{{SIGNED_STATE_SIGNATURE_HEX_LENGTH}}}\Z")

SECURITY_HEADERS = {
    "Content-Security-Policy": (
        "default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'; "
        "base-uri 'none'; form-action 'none'"
    ),
    "Cross-Origin-Resource-Policy": "same-origin",
    "Permissions-Policy": "camera=(), microphone=(), geolocation=(), payment=(), usb=()",
    "Referrer-Policy": "no-referrer",
    "X-Content-Type-Options": "nosniff",
    "X-Frame-Options": "DENY",
    "X-Permitted-Cross-Domain-Policies": "none",
}


def utc_seconds() -> int:
    return int(time.time())


def validate_shared_token(token: Any) -> str:
    value = str(token or "").strip()
    if len(value.encode("utf-8")) < MIN_TOKEN_BYTES:
        raise ValueError(
            f"LXNS callback token must contain at least {MIN_TOKEN_BYTES} bytes"
        )
    return value


def token_from_env() -> str:
    return str(os.environ.get("LXNS_CALLBACK_TOKEN") or "").strip()


def signed_state_signature(token: str, issued_at: int, nonce: str) -> str:
    secret = validate_shared_token(token)
    payload = f"{SIGNED_STATE_PREFIX}.{int(issued_at)}.{nonce}".encode("utf-8")
    return hmac.new(secret.encode("utf-8"), payload, hashlib.sha256).hexdigest()[
        :SIGNED_STATE_SIGNATURE_HEX_LENGTH
    ]


def valid_signed_state(state: str, token: str, *, now: int, ttl_seconds: int) -> bool:
    try:
        secret = validate_shared_token(token)
        if not state or len(state) > MAX_STATE_LENGTH:
            return False
        prefix, issued_text, nonce, signature = state.split(".")
        if prefix != SIGNED_STATE_PREFIX:
            return False
        if (
            not issued_text.isascii()
            or not issued_text.isdigit()
            or len(issued_text) > 12
        ):
            return False
        if (
            _NONCE_RE.fullmatch(nonce) is None
            or _SIGNATURE_RE.fullmatch(signature) is None
        ):
            return False
        issued_at = int(issued_text)
        ttl = max(60, int(ttl_seconds))
        if issued_at > int(now) + 60 or int(now) - issued_at > ttl:
            return False
        expected = signed_state_signature(secret, issued_at, nonce)
        return secrets.compare_digest(signature, expected)
    except (TypeError, ValueError):
        return False


def _bounded_text(value: Any, name: str, maximum: int, *, required: bool = True) -> str:
    text = str(value or "").strip()
    if required and not text:
        raise ValueError(f"{name} is required")
    if len(text) > maximum:
        raise ValueError(f"{name} is too long")
    return text


class CallbackStore:
    """SQLite-backed one-shot code store with consumed state tombstones."""

    def __init__(self, path: Path, ttl_seconds: int = DEFAULT_TTL_SECONDS) -> None:
        self.path = Path(path).expanduser()
        self.ttl_seconds = max(60, min(int(ttl_seconds), 3600))
        self._prepare_storage()
        self._init_db()

    def _prepare_storage(self) -> None:
        parent = self.path.parent
        if parent.is_symlink():
            raise ValueError("callback database directory must not be a symbolic link")
        parent_existed = parent.exists()
        parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        if parent.is_symlink():
            raise ValueError("callback database directory must not be a symbolic link")
        if not parent.is_dir():
            raise ValueError("callback database parent must be a directory")
        parent_mode = stat.S_IMODE(parent.stat().st_mode)
        if parent_existed and parent_mode & 0o077:
            raise ValueError(
                "callback database must use a dedicated directory with mode 0700"
            )
        os.chmod(parent, 0o700)
        if self.path.is_symlink():
            raise ValueError("callback database must not be a symbolic link")

        flags = os.O_CREAT | os.O_RDWR
        if hasattr(os, "O_NOFOLLOW"):
            flags |= os.O_NOFOLLOW
        descriptor = os.open(self.path, flags, 0o600)
        try:
            os.fchmod(descriptor, 0o600)
        finally:
            os.close(descriptor)
        self._protect_storage_files()

    def _protect_storage_files(self) -> None:
        for suffix in ("", "-wal", "-shm", "-journal"):
            candidate = Path(f"{self.path}{suffix}")
            if candidate.is_symlink():
                raise ValueError(
                    f"callback database file must not be a symbolic link: {candidate.name}"
                )
            try:
                os.chmod(candidate, 0o600)
            except FileNotFoundError:
                continue

    @contextmanager
    def _connect(self) -> Iterator[sqlite3.Connection]:
        if self.path.is_symlink():
            raise ValueError("callback database must not be a symbolic link")
        connection = sqlite3.connect(self.path, timeout=10, isolation_level=None)
        connection.row_factory = sqlite3.Row
        connection.execute("PRAGMA busy_timeout = 10000")
        try:
            self._protect_storage_files()
            yield connection
        finally:
            self._protect_storage_files()
            connection.close()
            self._protect_storage_files()

    def _init_db(self) -> None:
        with self._connect() as connection:
            connection.execute("PRAGMA journal_mode = WAL")
            connection.execute("PRAGMA synchronous = FULL")
            connection.execute(
                """
                CREATE TABLE IF NOT EXISTS lxns_oauth_callbacks (
                    state TEXT PRIMARY KEY,
                    code TEXT NOT NULL DEFAULT '',
                    received_at INTEGER NOT NULL,
                    consumed_at INTEGER
                )
                """
            )
            connection.execute(
                "CREATE INDEX IF NOT EXISTS idx_lxns_oauth_received "
                "ON lxns_oauth_callbacks(received_at)"
            )

    def _cleanup_locked(self, connection: sqlite3.Connection, now: int) -> None:
        cutoff = now - self.ttl_seconds
        connection.execute(
            "DELETE FROM lxns_oauth_callbacks WHERE received_at < ?",
            (cutoff,),
        )

    def put(self, state: str, code: str) -> bool:
        state_text = _bounded_text(state, "state", MAX_STATE_LENGTH)
        code_text = _bounded_text(code, "code", MAX_CODE_LENGTH)
        now = utc_seconds()
        with self._connect() as connection:
            connection.execute("BEGIN IMMEDIATE")
            try:
                self._cleanup_locked(connection, now)
                cursor = connection.execute(
                    """
                    INSERT INTO lxns_oauth_callbacks(state, code, received_at, consumed_at)
                    VALUES (?, ?, ?, NULL)
                    ON CONFLICT(state) DO NOTHING
                    """,
                    (state_text, code_text, now),
                )
                connection.commit()
            except Exception:
                connection.rollback()
                raise
        return cursor.rowcount == 1

    def poll(self, state: str, *, consume: bool = True) -> dict[str, Any]:
        state_text = _bounded_text(state, "state", MAX_STATE_LENGTH)
        now = utc_seconds()
        result: dict[str, Any] = {"ok": True, "ready": False}
        with self._connect() as connection:
            connection.execute("BEGIN IMMEDIATE")
            try:
                self._cleanup_locked(connection, now)
                row = connection.execute(
                    """
                    SELECT state, code, received_at, consumed_at
                    FROM lxns_oauth_callbacks
                    WHERE state = ?
                    """,
                    (state_text,),
                ).fetchone()
                if (
                    row is not None
                    and row["consumed_at"] is None
                    and str(row["code"] or "")
                ):
                    code = str(row["code"])
                    if consume:
                        cursor = connection.execute(
                            """
                            UPDATE lxns_oauth_callbacks
                            SET code = '', consumed_at = ?
                            WHERE state = ? AND consumed_at IS NULL AND code <> ''
                            """,
                            (now, state_text),
                        )
                        if cursor.rowcount == 1:
                            result = {
                                "ok": True,
                                "ready": True,
                                "code": code,
                                "receivedAt": int(row["received_at"]),
                            }
                    else:
                        result = {
                            "ok": True,
                            "ready": True,
                            "code": code,
                            "receivedAt": int(row["received_at"]),
                        }
                connection.commit()
            except Exception:
                connection.rollback()
                raise
        return result

    def cleanup(self, *, now: int | None = None) -> None:
        current = utc_seconds() if now is None else int(now)
        with self._connect() as connection:
            connection.execute("BEGIN IMMEDIATE")
            try:
                self._cleanup_locked(connection, current)
                connection.commit()
            except Exception:
                connection.rollback()
                raise


class RequestValidationError(ValueError):
    pass


def _parse_query(query: str) -> dict[str, list[str]]:
    try:
        return parse_qs(
            query,
            keep_blank_values=True,
            max_num_fields=MAX_QUERY_FIELDS,
        )
    except ValueError as exc:
        raise RequestValidationError("invalid query") from exc


def _single_parameter(
    values: dict[str, list[str]],
    name: str,
    *,
    maximum: int,
    required: bool = True,
) -> str:
    items = values.get(name) or []
    if len(items) > 1:
        raise RequestValidationError(f"duplicate {name}")
    if not items:
        if required:
            raise RequestValidationError(f"missing {name}")
        return ""
    try:
        return _bounded_text(items[0], name, maximum, required=required)
    except ValueError as exc:
        raise RequestValidationError(str(exc)) from exc


class CallbackHandler(BaseHTTPRequestHandler):
    server_version = "LXNS-OAuth-Bridge"
    sys_version = ""

    @property
    def store(self) -> CallbackStore:
        return self.server.store  # type: ignore[attr-defined]

    @property
    def callback_path(self) -> str:
        return self.server.callback_path  # type: ignore[attr-defined]

    @property
    def poll_path(self) -> str:
        return self.server.poll_path  # type: ignore[attr-defined]

    @property
    def token(self) -> str:
        return self.server.token  # type: ignore[attr-defined]

    def do_GET(self) -> None:  # noqa: N802 - stdlib handler API
        if len(self.path) > MAX_REQUEST_TARGET_LENGTH:
            self._send_json(
                {"ok": False, "error": "request target too long"},
                status=HTTPStatus.REQUEST_URI_TOO_LONG,
            )
            return
        try:
            parsed = urlsplit(self.path)
        except ValueError:
            self._send_json(
                {"ok": False, "error": "invalid request target"},
                status=HTTPStatus.BAD_REQUEST,
            )
            return
        if parsed.path == self.callback_path:
            self._handle_callback(parsed.query)
            return
        if parsed.path == self.poll_path:
            self._handle_poll(parsed.query)
            return
        if parsed.path == "/health" and not parsed.query:
            self._send_json({"ok": True})
            return
        self._send_json(
            {"ok": False, "error": "not found"}, status=HTTPStatus.NOT_FOUND
        )

    def _handle_callback(self, query: str) -> None:
        try:
            values = _parse_query(query)
            error = _single_parameter(
                values,
                "error",
                maximum=MAX_ERROR_LENGTH,
                required=False,
            )
            if error:
                self._send_html(
                    HTTPStatus.BAD_REQUEST,
                    "授权失败",
                    "落雪返回了授权错误，请回到 QQ 重新发起授权。",
                )
                return
            code = _single_parameter(values, "code", maximum=MAX_CODE_LENGTH)
            state = _single_parameter(values, "state", maximum=MAX_STATE_LENGTH)
        except RequestValidationError:
            self._send_html(HTTPStatus.BAD_REQUEST, "授权失败", "回调参数不正确。")
            return

        if any(character.isspace() for character in code + state):
            self._send_html(HTTPStatus.BAD_REQUEST, "授权失败", "回调参数不正确。")
            return
        if not valid_signed_state(
            state,
            self.token,
            now=utc_seconds(),
            ttl_seconds=self.store.ttl_seconds,
        ):
            self._send_html(
                HTTPStatus.BAD_REQUEST,
                "授权失败",
                "授权请求无效或已过期，请回到 QQ 重新发起授权。",
            )
            return

        self.store.put(state, code)
        self._send_html(
            HTTPStatus.OK,
            "已成功授权",
            "请回到发起授权的 QQ 会话，拍一拍机器人完成绑定。本页面可以关闭。",
        )

    def _handle_poll(self, query: str) -> None:
        supplied = self.headers.get("Authorization", "")
        if supplied.startswith("Bearer "):
            supplied = supplied[7:].strip()
        else:
            supplied = ""
        if not secrets.compare_digest(supplied, self.token):
            self._send_json(
                {"ok": False, "error": "unauthorized"},
                status=HTTPStatus.UNAUTHORIZED,
                extra_headers={"WWW-Authenticate": "Bearer"},
            )
            return

        try:
            values = _parse_query(query)
            state = _single_parameter(values, "state", maximum=MAX_STATE_LENGTH)
            consume_text = _single_parameter(
                values,
                "consume",
                maximum=MAX_CONSUME_LENGTH,
                required=False,
            ).casefold()
            if consume_text in {"", "1", "true", "yes"}:
                consume = True
            elif consume_text in {"0", "false", "no"}:
                consume = False
            else:
                raise RequestValidationError("invalid consume")
        except RequestValidationError:
            self._send_json(
                {"ok": False, "error": "invalid query"},
                status=HTTPStatus.BAD_REQUEST,
            )
            return

        self._send_json(self.store.poll(state, consume=consume))

    def _send_json(
        self,
        payload: dict[str, Any],
        *,
        status: HTTPStatus = HTTPStatus.OK,
        extra_headers: dict[str, str] | None = None,
    ) -> None:
        body = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode(
            "utf-8"
        )
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self._send_common_headers()
        for name, value in (extra_headers or {}).items():
            self.send_header(name, value)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _send_html(self, status: HTTPStatus, title: str, message: str) -> None:
        body = (
            "<!doctype html><html lang='zh-CN'><head><meta charset='utf-8'>"
            "<meta name='viewport' content='width=device-width,initial-scale=1'>"
            f"<title>{html.escape(title)}</title></head>"
            "<body style='font-family:system-ui,sans-serif;line-height:1.6;padding:2rem'>"
            f"<h1>{html.escape(title)}</h1><p>{html.escape(message)}</p></body></html>"
        ).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self._send_common_headers()
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _send_common_headers(self) -> None:
        self.send_header("Cache-Control", "no-store")
        self.send_header("Pragma", "no-cache")
        self.send_header("Expires", "0")
        for name, value in SECURITY_HEADERS.items():
            self.send_header(name, value)

    def log_message(self, _format: str, *_args: Any) -> None:
        # BaseHTTPRequestHandler logs the complete request target, including
        # OAuth code and state. Keep request logging disabled unconditionally.
        return


def _validated_endpoint_path(value: Any, name: str) -> str:
    path = str(value or "").strip()
    if not path.startswith("/") or "?" in path or "#" in path or len(path) > 256:
        raise ValueError(
            f"{name} must be an absolute URL path without a query or fragment"
        )
    return path


class CallbackServer(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(
        self,
        server_address: tuple[str, int],
        handler_class: type[CallbackHandler],
        *,
        store: CallbackStore,
        callback_path: str,
        poll_path: str,
        token: str,
        quiet: bool = True,
    ) -> None:
        self.store = store
        self.callback_path = _validated_endpoint_path(callback_path, "callback path")
        self.poll_path = _validated_endpoint_path(poll_path, "poll path")
        if self.callback_path == self.poll_path:
            raise ValueError("callback path and poll path must differ")
        self.token = validate_shared_token(token)
        self.quiet = bool(quiet)
        super().__init__(server_address, handler_class)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="LXNS OAuth callback bridge")
    parser.add_argument("--host", default=DEFAULT_HOST)
    parser.add_argument("--port", type=int, default=DEFAULT_PORT)
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--callback-path", default=DEFAULT_PATH)
    parser.add_argument("--poll-path", default=DEFAULT_POLL_PATH)
    parser.add_argument("--ttl-seconds", type=int, default=DEFAULT_TTL_SECONDS)
    parser.add_argument(
        "--token",
        default=token_from_env(),
        help="shared token (required; at least 32 UTF-8 bytes)",
    )
    parser.add_argument("--quiet", action="store_true", help=argparse.SUPPRESS)
    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    try:
        token = validate_shared_token(args.token)
        callback_path = _validated_endpoint_path(args.callback_path, "callback path")
        poll_path = _validated_endpoint_path(args.poll_path, "poll path")
    except ValueError as exc:
        parser.error(str(exc))

    os.umask(0o077)
    store = CallbackStore(args.db, ttl_seconds=args.ttl_seconds)
    server = CallbackServer(
        (args.host, args.port),
        CallbackHandler,
        store=store,
        callback_path=callback_path,
        poll_path=poll_path,
        token=token,
        quiet=True,
    )
    print(
        f"LXNS OAuth callback bridge listening on {args.host}:{args.port}",
        flush=True,
    )
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
