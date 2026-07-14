"""Independent LXNS OAuth state, token, and poke-confirmation storage.

This module intentionally depends only on the Python standard library and
``requests``.  It does not import score providers, region data, or the shared
maimai binding store.
"""

from __future__ import annotations

import base64
import hashlib
import os
import secrets
import sqlite3
import stat
import threading
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable
from urllib.parse import parse_qsl, urlencode, urlparse

import requests


DEFAULT_AUTHORIZE_URL = "https://maimai.lxns.net/oauth/authorize"
DEFAULT_TOKEN_URL = "https://maimai.lxns.net/api/v0/oauth/token"
DEFAULT_SCOPES = "write_player read_user_profile read_player"
DEFAULT_STATE_TTL_SECONDS = 600
DEFAULT_PENDING_TTL_SECONDS = 300
TOKEN_REFRESH_SKEW_SECONDS = 90
MAX_SUBJECT_LENGTH = 256
MAX_CONTEXT_LENGTH = 512
MAX_CODE_LENGTH = 2048
MAX_STATE_LENGTH = 1024
MAX_CALLBACK_FIELDS = 8


class LxnsOAuthError(RuntimeError):
    """An operator-safe OAuth error that never carries response bodies."""

    def __init__(
        self, message: str, *, code: str = "LXNS_OAUTH_ERROR", status: int | None = None
    ) -> None:
        super().__init__(message)
        self.code = code
        self.status = status

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {"code": self.code, "message": str(self)}
        if self.status is not None:
            result["status"] = self.status
        return result


def _first_env(*names: str) -> str:
    for name in names:
        value = str(os.environ.get(name) or "").strip()
        if value:
            return value
    return ""


@dataclass(frozen=True)
class OAuthConfig:
    client_id: str = ""
    client_secret: str = field(default="", repr=False)
    redirect_uri: str = ""
    authorize_url: str = DEFAULT_AUTHORIZE_URL
    token_url: str = DEFAULT_TOKEN_URL
    scopes: str = DEFAULT_SCOPES

    @classmethod
    def from_env(cls) -> "OAuthConfig":
        configured_token_url = _first_env("LXNS_OAUTH_TOKEN_URL")
        api_base = _first_env("LXNS_API_BASE_URL")
        token_url = configured_token_url or (
            f"{api_base.rstrip('/')}/oauth/token" if api_base else DEFAULT_TOKEN_URL
        )
        return cls(
            client_id=_first_env("LXNS_OAUTH_CLIENT_ID", "LXNS_CLIENT_ID"),
            client_secret=_first_env("LXNS_OAUTH_CLIENT_SECRET", "LXNS_CLIENT_SECRET"),
            redirect_uri=_first_env("LXNS_OAUTH_REDIRECT_URI", "LXNS_REDIRECT_URI"),
            authorize_url=_first_env("LXNS_OAUTH_AUTHORIZE_URL", "LXNS_AUTHORIZE_URL")
            or DEFAULT_AUTHORIZE_URL,
            token_url=token_url,
            scopes=_first_env("LXNS_OAUTH_SCOPES", "LXNS_SCOPES") or DEFAULT_SCOPES,
        )

    def require_authorization(self) -> None:
        if not self.client_id:
            raise LxnsOAuthError("未配置落雪 OAuth client_id。", code="CONFIG_MISSING")
        if not self.authorize_url:
            raise LxnsOAuthError(
                "未配置落雪 OAuth authorization endpoint。", code="CONFIG_MISSING"
            )

    def require_token_exchange(self) -> None:
        if not self.client_id:
            raise LxnsOAuthError("未配置落雪 OAuth client_id。", code="CONFIG_MISSING")
        if not self.token_url:
            raise LxnsOAuthError(
                "未配置落雪 OAuth token endpoint。", code="CONFIG_MISSING"
            )


def normalize_subject(value: Any) -> str:
    text = str(value or "").strip()
    if (
        not text
        or len(text) > MAX_SUBJECT_LENGTH
        or any(ord(char) < 32 for char in text)
    ):
        raise LxnsOAuthError("subject 格式不正确。", code="INVALID_INPUT")
    return text


def normalize_context(value: Any, field: str, *, required: bool = False) -> str:
    text = str(value or "").strip()
    if required and not text:
        raise LxnsOAuthError(f"缺少 {field}。", code="INVALID_INPUT")
    if len(text) > MAX_CONTEXT_LENGTH or any(ord(char) < 32 for char in text):
        raise LxnsOAuthError(f"{field} 格式不正确。", code="INVALID_INPUT")
    return text


def normalize_opaque_state(value: Any) -> str:
    text = str(value or "").strip()
    if not text or len(text) > MAX_STATE_LENGTH or any(char.isspace() for char in text):
        raise LxnsOAuthError("OAuth state 格式不正确。", code="INVALID_STATE")
    return text


def _bounded_seconds(value: Any, *, default: int, maximum: int, field: str) -> int:
    if value is None:
        return default
    if (
        isinstance(value, bool)
        or not isinstance(value, int)
        or value < 1
        or value > maximum
    ):
        raise LxnsOAuthError(
            f"{field} 必须是 1 到 {maximum} 之间的整数。", code="INVALID_INPUT"
        )
    return value


def normalize_oauth_submission(value: Any) -> tuple[str, str | None]:
    """Accept a raw code, ``code=...``, or a complete callback URL."""

    text = str(value or "").strip()
    if not text or len(text) > MAX_CODE_LENGTH:
        raise LxnsOAuthError("OAuth code 格式不正确。", code="INVALID_INPUT")

    parsed = urlparse(text)
    pairs: list[tuple[str, str]] = []
    callback_url = bool(parsed.scheme or parsed.netloc)
    if callback_url:
        if parsed.scheme.lower() not in {"http", "https"} or not parsed.netloc:
            raise LxnsOAuthError(
                "OAuth callback URL 格式不正确。", code="INVALID_INPUT"
            )
        if parsed.query and parsed.fragment:
            raise LxnsOAuthError(
                "OAuth callback URL 参数存在歧义。", code="INVALID_INPUT"
            )
        encoded_fields = parsed.query or parsed.fragment
        try:
            pairs = parse_qsl(
                encoded_fields,
                keep_blank_values=True,
                max_num_fields=MAX_CALLBACK_FIELDS,
            )
        except ValueError as exc:
            raise LxnsOAuthError(
                "OAuth callback 参数过多。", code="INVALID_INPUT"
            ) from exc
    elif (
        text.startswith("code=")
        or text.startswith("state=")
        or text.startswith("error=")
    ):
        try:
            pairs = parse_qsl(
                text, keep_blank_values=True, max_num_fields=MAX_CALLBACK_FIELDS
            )
        except ValueError as exc:
            raise LxnsOAuthError(
                "OAuth callback 参数过多。", code="INVALID_INPUT"
            ) from exc

    values: dict[str, list[str]] = {}
    for key, item in pairs:
        values.setdefault(key, []).append(item)
    if any(len(values.get(key, [])) > 1 for key in ("code", "state", "error")):
        raise LxnsOAuthError("OAuth callback 参数存在歧义。", code="INVALID_INPUT")

    if values:
        if values.get("error"):
            raise LxnsOAuthError("落雪 OAuth 授权未完成。", code="OAUTH_REJECTED")
        code = str((values.get("code") or [""])[0]).strip()
        state = str((values.get("state") or [""])[0]).strip() or None
    elif callback_url:
        raise LxnsOAuthError("OAuth callback URL 缺少 code。", code="INVALID_INPUT")
    else:
        code = text
        state = None

    if not code or len(code) > MAX_CODE_LENGTH or any(char.isspace() for char in code):
        raise LxnsOAuthError("OAuth code 格式不正确。", code="INVALID_INPUT")
    if state is not None:
        state = normalize_opaque_state(state)
    return code, state


def _state_digest(state: str) -> str:
    return hashlib.sha256(state.encode("utf-8")).hexdigest()


def _iso_time(value: int | None) -> str | None:
    if value is None or value <= 0:
        return None
    return datetime.fromtimestamp(value, tz=timezone.utc).isoformat()


def _expiry_epoch(data: dict[str, Any], now: int) -> int | None:
    raw = data.get("expires_at", data.get("expiresAt"))
    if isinstance(raw, (int, float)) and int(raw) > 0:
        return int(raw)
    if isinstance(raw, str) and raw.strip():
        text = raw.strip()
        try:
            if text.endswith("Z"):
                text = f"{text[:-1]}+00:00"
            parsed = datetime.fromisoformat(text)
            if parsed.tzinfo is None:
                parsed = parsed.replace(tzinfo=timezone.utc)
            return int(parsed.timestamp())
        except ValueError:
            pass
    expires_in = data.get("expires_in", data.get("expiresIn"))
    try:
        seconds = int(expires_in)
    except (TypeError, ValueError):
        return None
    return now + seconds if seconds > 0 else None


class OAuthStore:
    """Private SQLite store for OAuth state, active tokens, and pending tokens."""

    def __init__(self, path: str | Path) -> None:
        self.path = Path(path).expanduser()
        self._ensure_safe_storage()
        self._init_db()

    @staticmethod
    def _safe_lstat(path: Path) -> os.stat_result | None:
        try:
            return path.lstat()
        except FileNotFoundError:
            return None

    @staticmethod
    def _reject_link_or_wrong_type(
        path: Path, *, directory: bool
    ) -> os.stat_result | None:
        metadata = OAuthStore._safe_lstat(path)
        if metadata is None:
            return None
        valid_type = (
            stat.S_ISDIR(metadata.st_mode)
            if directory
            else stat.S_ISREG(metadata.st_mode)
        )
        if stat.S_ISLNK(metadata.st_mode) or not valid_type:
            raise LxnsOAuthError("OAuth 存储路径不安全。", code="UNSAFE_STORAGE")
        return metadata

    def _ensure_safe_storage(self) -> None:
        parent = self.path.parent
        parent_metadata = self._reject_link_or_wrong_type(parent, directory=True)
        if parent_metadata is None:
            try:
                parent.mkdir(parents=True, mode=0o700)
            except OSError as exc:
                raise LxnsOAuthError(
                    "OAuth 存储目录创建失败。", code="STORE_ERROR"
                ) from exc
            os.chmod(parent, 0o700, follow_symlinks=False)
            parent_metadata = self._reject_link_or_wrong_type(parent, directory=True)
        if parent_metadata is None or stat.S_IMODE(parent_metadata.st_mode) != 0o700:
            raise LxnsOAuthError(
                "OAuth 数据库必须使用权限为 0700 的专用目录。",
                code="UNSAFE_STORAGE",
            )

        db_metadata = self._reject_link_or_wrong_type(self.path, directory=False)
        if db_metadata is None:
            flags = os.O_CREAT | os.O_EXCL | os.O_WRONLY
            flags |= getattr(os, "O_CLOEXEC", 0)
            flags |= getattr(os, "O_NOFOLLOW", 0)
            try:
                descriptor = os.open(self.path, flags, 0o600)
            except FileExistsError as exc:
                raise LxnsOAuthError(
                    "OAuth 存储路径不安全。", code="UNSAFE_STORAGE"
                ) from exc
            except OSError as exc:
                raise LxnsOAuthError(
                    "OAuth 数据库创建失败。", code="STORE_ERROR"
                ) from exc
            else:
                os.close(descriptor)

        for sidecar in (Path(f"{self.path}-wal"), Path(f"{self.path}-shm")):
            self._reject_link_or_wrong_type(sidecar, directory=False)
        self.protect_storage_files()

    def connect(self) -> sqlite3.Connection:
        self._ensure_safe_storage()
        conn = sqlite3.connect(self.path, timeout=30)
        conn.row_factory = sqlite3.Row
        conn.execute("PRAGMA journal_mode=WAL")
        conn.execute("PRAGMA foreign_keys=ON")
        conn.execute("PRAGMA busy_timeout=30000")
        self.protect_storage_files()
        return conn

    def protect_storage_files(self) -> None:
        parent_metadata = self._reject_link_or_wrong_type(
            self.path.parent,
            directory=True,
        )
        if parent_metadata is None or stat.S_IMODE(parent_metadata.st_mode) != 0o700:
            raise LxnsOAuthError(
                "OAuth 数据库必须使用权限为 0700 的专用目录。",
                code="UNSAFE_STORAGE",
            )
        for candidate in (
            self.path,
            Path(f"{self.path}-wal"),
            Path(f"{self.path}-shm"),
        ):
            metadata = self._reject_link_or_wrong_type(candidate, directory=False)
            if metadata is not None:
                os.chmod(candidate, 0o600, follow_symlinks=False)

    def _init_db(self) -> None:
        with self.connect() as conn:
            conn.executescript(
                """
                CREATE TABLE IF NOT EXISTS oauth_states (
                    state_digest TEXT PRIMARY KEY,
                    subject TEXT NOT NULL,
                    adapter TEXT NOT NULL,
                    conversation TEXT NOT NULL,
                    bot TEXT NOT NULL,
                    code_verifier TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    expires_at INTEGER NOT NULL,
                    consumed_at INTEGER
                );
                CREATE INDEX IF NOT EXISTS idx_oauth_states_subject
                    ON oauth_states(subject, adapter, conversation, bot, created_at DESC);

                CREATE TABLE IF NOT EXISTS oauth_tokens (
                    subject TEXT PRIMARY KEY,
                    access_token TEXT NOT NULL,
                    refresh_token TEXT NOT NULL,
                    token_type TEXT NOT NULL,
                    scope TEXT NOT NULL,
                    expires_at INTEGER,
                    revision INTEGER NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS oauth_pending (
                    subject TEXT PRIMARY KEY,
                    adapter TEXT NOT NULL,
                    conversation TEXT NOT NULL,
                    bot TEXT NOT NULL,
                    access_token TEXT NOT NULL,
                    refresh_token TEXT NOT NULL,
                    token_type TEXT NOT NULL,
                    scope TEXT NOT NULL,
                    token_expires_at INTEGER,
                    created_at INTEGER NOT NULL,
                    expires_at INTEGER NOT NULL
                );
                """
            )
            now = int(time.time())
            conn.execute("DELETE FROM oauth_pending WHERE expires_at <= ?", (now,))
            conn.execute("DELETE FROM oauth_states WHERE expires_at <= ?", (now,))
            self.protect_storage_files()
        self.protect_storage_files()

    def put_state(
        self,
        state: str,
        *,
        subject: str,
        adapter: str,
        conversation: str,
        bot: str,
        code_verifier: str,
        created_at: int,
        expires_at: int,
    ) -> None:
        digest = _state_digest(state)
        try:
            with self.connect() as conn:
                conn.execute(
                    "DELETE FROM oauth_states WHERE expires_at <= ?", (created_at,)
                )
                conn.execute(
                    """
                    INSERT INTO oauth_states (
                        state_digest, subject, adapter, conversation, bot,
                        code_verifier, created_at, expires_at, consumed_at
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL)
                    """,
                    (
                        digest,
                        subject,
                        adapter,
                        conversation,
                        bot,
                        code_verifier,
                        created_at,
                        expires_at,
                    ),
                )
                self.protect_storage_files()
        except sqlite3.IntegrityError as exc:
            raise LxnsOAuthError("OAuth state 已存在。", code="STATE_CONFLICT") from exc
        except sqlite3.Error as exc:
            raise LxnsOAuthError("OAuth 状态存储失败。", code="STORE_ERROR") from exc

    def resolve_state(
        self,
        *,
        subject: str,
        state: str | None,
        adapter: str,
        conversation: str,
        bot: str,
        now: int,
    ) -> dict[str, Any]:
        try:
            with self.connect() as conn:
                if state is not None:
                    row = conn.execute(
                        """
                        SELECT * FROM oauth_states
                        WHERE state_digest = ? AND consumed_at IS NULL
                        """,
                        (_state_digest(state),),
                    ).fetchone()
                else:
                    row = conn.execute(
                        """
                        SELECT * FROM oauth_states
                        WHERE subject = ? AND adapter = ? AND conversation = ? AND bot = ?
                          AND consumed_at IS NULL
                        ORDER BY created_at DESC
                        LIMIT 1
                        """,
                        (subject, adapter, conversation, bot),
                    ).fetchone()
                    if row is None:
                        row = conn.execute(
                            """
                            SELECT * FROM oauth_states
                            WHERE subject = ? AND consumed_at IS NULL
                            ORDER BY created_at DESC
                            LIMIT 1
                            """,
                            (subject,),
                        ).fetchone()
        except sqlite3.Error as exc:
            raise LxnsOAuthError("OAuth 状态读取失败。", code="STORE_ERROR") from exc

        if row is None:
            raise LxnsOAuthError("OAuth state 不存在或已使用。", code="INVALID_STATE")
        result = dict(row)
        if (
            result["subject"] != subject
            or result["adapter"] != adapter
            or result["conversation"] != conversation
            or result["bot"] != bot
        ):
            raise LxnsOAuthError("OAuth state 上下文不匹配。", code="CONTEXT_MISMATCH")
        if int(result["expires_at"]) <= now:
            raise LxnsOAuthError("OAuth state 已过期。", code="STATE_EXPIRED")
        return result

    @staticmethod
    def _write_token(
        conn: sqlite3.Connection,
        *,
        subject: str,
        access_token: str,
        refresh_token: str,
        token_type: str,
        scope: str,
        expires_at: int | None,
        now: int,
        expected_revision: int | None,
    ) -> dict[str, Any] | None:
        current = conn.execute(
            "SELECT revision, created_at FROM oauth_tokens WHERE subject = ?",
            (subject,),
        ).fetchone()
        if expected_revision is not None:
            if current is None or int(current["revision"]) != expected_revision:
                return None
        revision = (int(current["revision"]) + 1) if current is not None else 1
        created_at = int(current["created_at"]) if current is not None else now
        conn.execute(
            """
            INSERT INTO oauth_tokens (
                subject, access_token, refresh_token, token_type, scope,
                expires_at, revision, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(subject) DO UPDATE SET
                access_token = excluded.access_token,
                refresh_token = excluded.refresh_token,
                token_type = excluded.token_type,
                scope = excluded.scope,
                expires_at = excluded.expires_at,
                revision = excluded.revision,
                updated_at = excluded.updated_at
            """,
            (
                subject,
                access_token,
                refresh_token,
                token_type,
                scope,
                expires_at,
                revision,
                created_at,
                now,
            ),
        )
        row = conn.execute(
            "SELECT * FROM oauth_tokens WHERE subject = ?", (subject,)
        ).fetchone()
        return dict(row) if row is not None else None

    def save_token(
        self,
        subject: str,
        *,
        access_token: str,
        refresh_token: str,
        token_type: str,
        scope: str,
        expires_at: int | None,
        expected_revision: int | None = None,
        now: int | None = None,
    ) -> dict[str, Any] | None:
        current_time = int(time.time()) if now is None else int(now)
        try:
            with self.connect() as conn:
                conn.execute("BEGIN IMMEDIATE")
                result = self._write_token(
                    conn,
                    subject=subject,
                    access_token=access_token,
                    refresh_token=refresh_token,
                    token_type=token_type,
                    scope=scope,
                    expires_at=expires_at,
                    now=current_time,
                    expected_revision=expected_revision,
                )
                conn.commit()
                self.protect_storage_files()
                return result
        except sqlite3.Error as exc:
            raise LxnsOAuthError("OAuth token 存储失败。", code="STORE_ERROR") from exc

    def activate_from_state(
        self,
        state_digest: str,
        *,
        subject: str,
        token: dict[str, Any],
        now: int,
    ) -> dict[str, Any]:
        try:
            with self.connect() as conn:
                conn.execute("BEGIN IMMEDIATE")
                consumed = conn.execute(
                    """
                    UPDATE oauth_states SET consumed_at = ?
                    WHERE state_digest = ? AND subject = ? AND consumed_at IS NULL
                    """,
                    (now, state_digest, subject),
                )
                if consumed.rowcount != 1:
                    conn.rollback()
                    raise LxnsOAuthError(
                        "OAuth state 不存在或已使用。", code="INVALID_STATE"
                    )
                stored = self._write_token(
                    conn,
                    subject=subject,
                    access_token=token["access_token"],
                    refresh_token=token["refresh_token"],
                    token_type=token["token_type"],
                    scope=token["scope"],
                    expires_at=token["expires_at"],
                    now=now,
                    expected_revision=None,
                )
                conn.commit()
                self.protect_storage_files()
        except LxnsOAuthError:
            raise
        except sqlite3.Error as exc:
            raise LxnsOAuthError("OAuth token 存储失败。", code="STORE_ERROR") from exc
        if stored is None:  # pragma: no cover - guarded by the transaction above
            raise LxnsOAuthError("OAuth token 存储失败。", code="STORE_ERROR")
        return stored

    def save_pending_from_state(
        self,
        state_digest: str,
        *,
        subject: str,
        adapter: str,
        conversation: str,
        bot: str,
        token: dict[str, Any],
        created_at: int,
        expires_at: int,
    ) -> None:
        try:
            with self.connect() as conn:
                conn.execute("BEGIN IMMEDIATE")
                consumed = conn.execute(
                    """
                    UPDATE oauth_states SET consumed_at = ?
                    WHERE state_digest = ? AND subject = ? AND consumed_at IS NULL
                    """,
                    (created_at, state_digest, subject),
                )
                if consumed.rowcount != 1:
                    conn.rollback()
                    raise LxnsOAuthError(
                        "OAuth state 不存在或已使用。", code="INVALID_STATE"
                    )
                conn.execute(
                    """
                    INSERT INTO oauth_pending (
                        subject, adapter, conversation, bot, access_token,
                        refresh_token, token_type, scope, token_expires_at,
                        created_at, expires_at
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    ON CONFLICT(subject) DO UPDATE SET
                        adapter = excluded.adapter,
                        conversation = excluded.conversation,
                        bot = excluded.bot,
                        access_token = excluded.access_token,
                        refresh_token = excluded.refresh_token,
                        token_type = excluded.token_type,
                        scope = excluded.scope,
                        token_expires_at = excluded.token_expires_at,
                        created_at = excluded.created_at,
                        expires_at = excluded.expires_at
                    """,
                    (
                        subject,
                        adapter,
                        conversation,
                        bot,
                        token["access_token"],
                        token["refresh_token"],
                        token["token_type"],
                        token["scope"],
                        token["expires_at"],
                        created_at,
                        expires_at,
                    ),
                )
                conn.commit()
                self.protect_storage_files()
        except LxnsOAuthError:
            raise
        except sqlite3.Error as exc:
            raise LxnsOAuthError(
                "待确认 OAuth token 存储失败。", code="STORE_ERROR"
            ) from exc

    def confirm_pending(
        self,
        subject: str,
        *,
        adapter: str,
        conversation: str,
        bot: str,
        now: int,
    ) -> dict[str, Any]:
        try:
            with self.connect() as conn:
                conn.execute("BEGIN IMMEDIATE")
                row = conn.execute(
                    "SELECT * FROM oauth_pending WHERE subject = ?", (subject,)
                ).fetchone()
                if row is None:
                    conn.commit()
                    return {"status": "not_found"}
                pending = dict(row)

                # Context comparison deliberately precedes expiry handling.  This
                # prevents a caller from probing another context's pending state.
                if (
                    pending["adapter"] != adapter
                    or pending["conversation"] != conversation
                    or pending["bot"] != bot
                ):
                    conn.commit()
                    return {"status": "context_mismatch"}
                if int(pending["expires_at"]) <= now:
                    conn.execute(
                        "DELETE FROM oauth_pending WHERE subject = ?", (subject,)
                    )
                    conn.commit()
                    self.protect_storage_files()
                    return {"status": "expired"}

                stored = self._write_token(
                    conn,
                    subject=subject,
                    access_token=pending["access_token"],
                    refresh_token=pending["refresh_token"],
                    token_type=pending["token_type"],
                    scope=pending["scope"],
                    expires_at=pending["token_expires_at"],
                    now=now,
                    expected_revision=None,
                )
                conn.execute(
                    """
                    DELETE FROM oauth_pending
                    WHERE subject = ? AND adapter = ? AND conversation = ? AND bot = ?
                    """,
                    (subject, adapter, conversation, bot),
                )
                conn.commit()
                self.protect_storage_files()
        except sqlite3.Error as exc:
            raise LxnsOAuthError(
                "待确认 OAuth token 读取失败。", code="STORE_ERROR"
            ) from exc

        if stored is None:  # pragma: no cover - guarded by the transaction above
            raise LxnsOAuthError("OAuth token 存储失败。", code="STORE_ERROR")
        return {"status": "confirmed", "token": stored}

    def read_token(self, subject: str) -> dict[str, Any] | None:
        try:
            with self.connect() as conn:
                row = conn.execute(
                    "SELECT * FROM oauth_tokens WHERE subject = ?", (subject,)
                ).fetchone()
        except sqlite3.Error as exc:
            raise LxnsOAuthError("OAuth token 读取失败。", code="STORE_ERROR") from exc
        return dict(row) if row is not None else None

    def read_pending(self, subject: str) -> dict[str, Any] | None:
        try:
            with self.connect() as conn:
                row = conn.execute(
                    "SELECT * FROM oauth_pending WHERE subject = ?", (subject,)
                ).fetchone()
        except sqlite3.Error as exc:
            raise LxnsOAuthError(
                "待确认 OAuth token 读取失败。", code="STORE_ERROR"
            ) from exc
        return dict(row) if row is not None else None

    def cleanup_expired(self, now: int) -> tuple[int, int]:
        """删除过期 state 与待确认 token，返回各自删除数量。"""

        try:
            with self.connect() as conn:
                conn.execute("BEGIN IMMEDIATE")
                pending_rows = conn.execute(
                    "DELETE FROM oauth_pending WHERE expires_at <= ?",
                    (int(now),),
                ).rowcount
                state_rows = conn.execute(
                    "DELETE FROM oauth_states WHERE expires_at <= ?",
                    (int(now),),
                ).rowcount
                conn.commit()
                self.protect_storage_files()
                return pending_rows, state_rows
        except sqlite3.Error as exc:
            raise LxnsOAuthError(
                "OAuth 过期记录清理失败。", code="STORE_ERROR"
            ) from exc

    def unbind(self, subject: str) -> bool:
        try:
            with self.connect() as conn:
                conn.execute("BEGIN IMMEDIATE")
                changed_rows = conn.execute(
                    "DELETE FROM oauth_tokens WHERE subject = ?", (subject,)
                ).rowcount
                changed_rows += conn.execute(
                    "DELETE FROM oauth_pending WHERE subject = ?", (subject,)
                ).rowcount
                changed_rows += conn.execute(
                    "DELETE FROM oauth_states WHERE subject = ?", (subject,)
                ).rowcount
                conn.commit()
                self.protect_storage_files()
                return changed_rows > 0
        except sqlite3.Error as exc:
            raise LxnsOAuthError("OAuth 解绑失败。", code="STORE_ERROR") from exc

    def state_count(self, subject: str | None = None) -> int:
        with self.connect() as conn:
            if subject is None:
                row = conn.execute(
                    "SELECT COUNT(*) AS count FROM oauth_states"
                ).fetchone()
            else:
                row = conn.execute(
                    "SELECT COUNT(*) AS count FROM oauth_states WHERE subject = ?",
                    (subject,),
                ).fetchone()
        return int(row["count"])

    def pending_count(self, subject: str | None = None) -> int:
        with self.connect() as conn:
            if subject is None:
                row = conn.execute(
                    "SELECT COUNT(*) AS count FROM oauth_pending"
                ).fetchone()
            else:
                row = conn.execute(
                    "SELECT COUNT(*) AS count FROM oauth_pending WHERE subject = ?",
                    (subject,),
                ).fetchone()
        return int(row["count"])


_REFRESH_LOCKS_GUARD = threading.Lock()
_REFRESH_LOCKS: dict[tuple[str, str], threading.Lock] = {}


def _refresh_lock(store: OAuthStore, subject: str) -> threading.Lock:
    key = (str(store.path), subject)
    with _REFRESH_LOCKS_GUARD:
        return _REFRESH_LOCKS.setdefault(key, threading.Lock())


class OAuthService:
    def __init__(
        self,
        config: OAuthConfig,
        store: OAuthStore,
        *,
        post: Callable[..., Any] = requests.post,
        clock: Callable[[], float] = time.time,
        timeout: float = 30.0,
    ) -> None:
        self.config = config
        self.store = store
        self.post = post
        self.clock = clock
        self.timeout = timeout

    def _now(self) -> int:
        return int(self.clock())

    def oauth_url(
        self,
        subject: Any,
        *,
        adapter: Any = "",
        conversation: Any = "",
        bot: Any = "",
        state: Any = None,
        scopes: Any = None,
        ttl_seconds: int = DEFAULT_STATE_TTL_SECONDS,
    ) -> dict[str, Any]:
        self.config.require_authorization()
        subject_text = normalize_subject(subject)
        adapter_text = normalize_context(adapter, "adapter")
        conversation_text = normalize_context(conversation, "conversation")
        bot_text = normalize_context(bot, "bot")
        state_text = (
            normalize_opaque_state(state)
            if state not in (None, "")
            else secrets.token_urlsafe(32)
        )
        if (
            state not in (None, "")
            and len(subject_text) >= 5
            and subject_text in state_text
        ):
            raise LxnsOAuthError(
                "OAuth state 必须是不含 subject 的 opaque 值。", code="INVALID_STATE"
            )
        verifier = secrets.token_urlsafe(64)
        challenge = (
            base64.urlsafe_b64encode(hashlib.sha256(verifier.encode("ascii")).digest())
            .rstrip(b"=")
            .decode("ascii")
        )
        now = self._now()
        self.store.cleanup_expired(now)
        ttl = _bounded_seconds(
            ttl_seconds,
            default=DEFAULT_STATE_TTL_SECONDS,
            maximum=3600,
            field="ttl_seconds",
        )
        self.store.put_state(
            state_text,
            subject=subject_text,
            adapter=adapter_text,
            conversation=conversation_text,
            bot=bot_text,
            code_verifier=verifier,
            created_at=now,
            expires_at=now + ttl,
        )
        query = {
            "response_type": "code",
            "client_id": self.config.client_id,
            "scope": str(scopes or self.config.scopes).strip(),
            "state": state_text,
            "code_challenge": challenge,
            "code_challenge_method": "S256",
        }
        if self.config.redirect_uri:
            query["redirect_uri"] = self.config.redirect_uri
        authorization_url = (
            f"{self.config.authorize_url.rstrip('/')}?{urlencode(query)}"
        )
        return {
            "ok": True,
            "authorizationUrl": authorization_url,
            "expiresAt": _iso_time(now + ttl),
        }

    def _resolve_submission(
        self,
        subject: str,
        submission: Any,
        *,
        explicit_state: Any,
        adapter: str,
        conversation: str,
        bot: str,
    ) -> tuple[str, dict[str, Any]]:
        code, callback_state = normalize_oauth_submission(submission)
        supplied_state = (
            normalize_opaque_state(explicit_state)
            if explicit_state not in (None, "")
            else None
        )
        if (
            callback_state is not None
            and supplied_state is not None
            and not secrets.compare_digest(callback_state, supplied_state)
        ):
            raise LxnsOAuthError("OAuth state 不一致。", code="INVALID_STATE")
        state = callback_state or supplied_state
        state_record = self.store.resolve_state(
            subject=subject,
            state=state,
            adapter=adapter,
            conversation=conversation,
            bot=bot,
            now=self._now(),
        )
        return code, state_record

    def _request_token(
        self, data: dict[str, Any], *, refreshing: bool = False
    ) -> dict[str, Any]:
        self.config.require_token_exchange()
        try:
            response = self.post(self.config.token_url, data=data, timeout=self.timeout)
        except Exception as exc:
            raise LxnsOAuthError(
                "落雪 OAuth token 请求失败。", code="TOKEN_REQUEST_FAILED"
            ) from exc

        status = int(getattr(response, "status_code", 0) or 0)
        try:
            payload = response.json()
        except Exception as exc:
            raise LxnsOAuthError(
                "落雪 OAuth token 响应格式不正确。",
                code="INVALID_TOKEN_RESPONSE",
                status=status,
            ) from exc
        if not isinstance(payload, dict):
            raise LxnsOAuthError(
                "落雪 OAuth token 响应格式不正确。",
                code="INVALID_TOKEN_RESPONSE",
                status=status,
            )
        ok = bool(getattr(response, "ok", 200 <= status < 300))
        if not ok or payload.get("success") is False:
            error_name = str(payload.get("error") or "").strip()
            if refreshing and error_name == "invalid_grant":
                raise LxnsOAuthError(
                    "落雪 OAuth 刷新授权已失效。",
                    code="REFRESH_REJECTED",
                    status=status,
                )
            raise LxnsOAuthError(
                "落雪 OAuth token 请求被拒绝。", code="TOKEN_REJECTED", status=status
            )
        data_payload = (
            payload.get("data") if isinstance(payload.get("data"), dict) else payload
        )
        if not isinstance(data_payload, dict):
            raise LxnsOAuthError(
                "落雪 OAuth token 响应格式不正确。",
                code="INVALID_TOKEN_RESPONSE",
                status=status,
            )
        return data_payload

    def _exchange_code(self, code: str, state_record: dict[str, Any]) -> dict[str, Any]:
        data: dict[str, Any] = {
            "grant_type": "authorization_code",
            "client_id": self.config.client_id,
            "code": code,
            "code_verifier": state_record["code_verifier"],
        }
        if self.config.client_secret:
            data["client_secret"] = self.config.client_secret
        if self.config.redirect_uri:
            data["redirect_uri"] = self.config.redirect_uri
        payload = self._request_token(data)
        return self._normalize_token_payload(payload, now=self._now())

    @staticmethod
    def _normalize_token_payload(
        payload: dict[str, Any],
        *,
        now: int,
        previous_refresh_token: str | None = None,
    ) -> dict[str, Any]:
        access_token = str(
            payload.get("access_token") or payload.get("accessToken") or ""
        ).strip()
        refresh_token = str(
            payload.get("refresh_token") or payload.get("refreshToken") or ""
        ).strip()
        if not access_token or not refresh_token:
            raise LxnsOAuthError(
                "落雪 OAuth token 响应缺少轮换凭证。", code="INVALID_TOKEN_RESPONSE"
            )
        if previous_refresh_token is not None and secrets.compare_digest(
            refresh_token, previous_refresh_token
        ):
            raise LxnsOAuthError(
                "落雪 OAuth refresh token 未轮换。", code="INVALID_TOKEN_RESPONSE"
            )
        return {
            "access_token": access_token,
            "refresh_token": refresh_token,
            "token_type": str(
                payload.get("token_type") or payload.get("tokenType") or "Bearer"
            ).strip()
            or "Bearer",
            "scope": str(payload.get("scope") or "").strip(),
            "expires_at": _expiry_epoch(payload, now),
        }

    @staticmethod
    def _safe_token_metadata(token: dict[str, Any]) -> dict[str, Any]:
        return {
            "ok": True,
            "bound": True,
            "hasRefreshToken": bool(token.get("refresh_token")),
            "expiresAt": _iso_time(token.get("expires_at")),
            "revision": int(token.get("revision") or 0),
        }

    def bind_code(
        self,
        subject: Any,
        submission: Any,
        *,
        adapter: Any = "",
        conversation: Any = "",
        bot: Any = "",
        state: Any = None,
    ) -> dict[str, Any]:
        subject_text = normalize_subject(subject)
        adapter_text = normalize_context(adapter, "adapter")
        conversation_text = normalize_context(conversation, "conversation")
        bot_text = normalize_context(bot, "bot")
        code, state_record = self._resolve_submission(
            subject_text,
            submission,
            explicit_state=state,
            adapter=adapter_text,
            conversation=conversation_text,
            bot=bot_text,
        )
        token = self._exchange_code(code, state_record)
        stored = self.store.activate_from_state(
            state_record["state_digest"],
            subject=subject_text,
            token=token,
            now=self._now(),
        )
        return self._safe_token_metadata(stored)

    def prepare_poke(
        self,
        subject: Any,
        submission: Any,
        *,
        adapter: Any,
        conversation: Any,
        bot: Any,
        state: Any = None,
        ttl_seconds: int = DEFAULT_PENDING_TTL_SECONDS,
    ) -> dict[str, Any]:
        subject_text = normalize_subject(subject)
        adapter_text = normalize_context(adapter, "adapter", required=True)
        conversation_text = normalize_context(
            conversation, "conversation", required=True
        )
        bot_text = normalize_context(bot, "bot", required=True)
        code, state_record = self._resolve_submission(
            subject_text,
            submission,
            explicit_state=state,
            adapter=adapter_text,
            conversation=conversation_text,
            bot=bot_text,
        )
        token = self._exchange_code(code, state_record)
        now = self._now()
        ttl = _bounded_seconds(
            ttl_seconds,
            default=DEFAULT_PENDING_TTL_SECONDS,
            maximum=1800,
            field="ttl_seconds",
        )
        self.store.save_pending_from_state(
            state_record["state_digest"],
            subject=subject_text,
            adapter=adapter_text,
            conversation=conversation_text,
            bot=bot_text,
            token=token,
            created_at=now,
            expires_at=now + ttl,
        )
        return {
            "ok": True,
            "pending": True,
            "confirmationExpiresAt": _iso_time(now + ttl),
        }

    def confirm_poke(
        self,
        subject: Any,
        *,
        adapter: Any,
        conversation: Any,
        bot: Any,
    ) -> dict[str, Any]:
        subject_text = normalize_subject(subject)
        adapter_text = normalize_context(adapter, "adapter", required=True)
        conversation_text = normalize_context(
            conversation, "conversation", required=True
        )
        bot_text = normalize_context(bot, "bot", required=True)
        result = self.store.confirm_pending(
            subject_text,
            adapter=adapter_text,
            conversation=conversation_text,
            bot=bot_text,
            now=self._now(),
        )
        status = result["status"]
        if status != "confirmed":
            return {"ok": True, "confirmed": False, "status": status}
        metadata = self._safe_token_metadata(result["token"])
        return {**metadata, "confirmed": True, "status": "confirmed"}

    def status(self, subject: Any) -> dict[str, Any]:
        subject_text = normalize_subject(subject)
        now = self._now()
        self.store.cleanup_expired(now)
        token = self.store.read_token(subject_text)
        pending = self.store.read_pending(subject_text)
        pending_active = pending is not None and int(pending["expires_at"]) > now
        return {
            "ok": True,
            "bound": token is not None,
            "pending": pending_active,
            "expiresAt": _iso_time(token.get("expires_at")) if token else None,
            "confirmationExpiresAt": _iso_time(pending.get("expires_at"))
            if pending_active and pending
            else None,
            "revision": int(token.get("revision") or 0) if token else 0,
        }

    def unbind(self, subject: Any) -> dict[str, Any]:
        subject_text = normalize_subject(subject)
        changed = self.store.unbind(subject_text)
        return {"ok": True, "changed": changed, "bound": False, "pending": False}

    def refresh(self, subject: Any, *, force: bool = False) -> dict[str, Any]:
        subject_text = normalize_subject(subject)
        initial = self.store.read_token(subject_text)
        if initial is None:
            raise LxnsOAuthError("尚未绑定落雪 OAuth。", code="AUTH_REQUIRED")
        now = self._now()
        expires_at = initial.get("expires_at")
        if not force and (
            expires_at is None or int(expires_at) - now > TOKEN_REFRESH_SKEW_SECONDS
        ):
            return self._safe_token_metadata(initial)

        with _refresh_lock(self.store, subject_text):
            current = self.store.read_token(subject_text)
            if current is None:
                raise LxnsOAuthError("尚未绑定落雪 OAuth。", code="AUTH_REQUIRED")
            if int(current["revision"]) != int(initial["revision"]):
                return self._safe_token_metadata(current)
            current_expiry = current.get("expires_at")
            if not force and (
                current_expiry is None
                or int(current_expiry) - self._now() > TOKEN_REFRESH_SKEW_SECONDS
            ):
                return self._safe_token_metadata(current)

            old_refresh = str(current["refresh_token"])
            data: dict[str, Any] = {
                "grant_type": "refresh_token",
                "client_id": self.config.client_id,
                "refresh_token": old_refresh,
            }
            if self.config.client_secret:
                data["client_secret"] = self.config.client_secret
            try:
                payload = self._request_token(data, refreshing=True)
            except LxnsOAuthError as exc:
                latest = self.store.read_token(subject_text)
                if latest is not None and int(latest["revision"]) != int(
                    current["revision"]
                ):
                    return self._safe_token_metadata(latest)
                raise exc
            try:
                token = self._normalize_token_payload(
                    payload,
                    now=self._now(),
                    previous_refresh_token=old_refresh,
                )
            except LxnsOAuthError as exc:
                latest = self.store.read_token(subject_text)
                if latest is not None and int(latest["revision"]) != int(
                    current["revision"]
                ):
                    return self._safe_token_metadata(latest)
                raise exc
            stored = self.store.save_token(
                subject_text,
                access_token=token["access_token"],
                refresh_token=token["refresh_token"],
                token_type=token["token_type"],
                scope=token["scope"],
                expires_at=token["expires_at"],
                expected_revision=int(current["revision"]),
                now=self._now(),
            )
            if stored is None:
                latest = self.store.read_token(subject_text)
                if latest is None or int(latest["revision"]) == int(
                    current["revision"]
                ):
                    raise LxnsOAuthError(
                        "OAuth token 刷新发生并发冲突。", code="REFRESH_CONFLICT"
                    )
                stored = latest
            return self._safe_token_metadata(stored)


# Explicit names for callers that prefer the longer domain-specific spelling.
LxnsOAuthStore = OAuthStore
LxnsOAuthService = OAuthService


__all__ = [
    "DEFAULT_AUTHORIZE_URL",
    "DEFAULT_PENDING_TTL_SECONDS",
    "DEFAULT_SCOPES",
    "DEFAULT_STATE_TTL_SECONDS",
    "DEFAULT_TOKEN_URL",
    "LxnsOAuthError",
    "LxnsOAuthService",
    "LxnsOAuthStore",
    "OAuthConfig",
    "OAuthService",
    "OAuthStore",
    "normalize_oauth_submission",
    "normalize_opaque_state",
    "normalize_subject",
]
