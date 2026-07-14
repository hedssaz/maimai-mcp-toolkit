from __future__ import annotations

import json
import os
import stat
import tempfile
import time
import unittest
from pathlib import Path
from urllib.parse import parse_qs, urlparse
from unittest.mock import patch

import lxns_oauth


class FakeResponse:
    def __init__(
        self, payload: object, *, status_code: int = 200, ok: bool = True
    ) -> None:
        self._payload = payload
        self.status_code = status_code
        self.ok = ok

    def json(self) -> object:
        return self._payload


class MutableClock:
    def __init__(self, value: int = 1_800_000_000) -> None:
        self.value = value

    def __call__(self) -> float:
        return float(self.value)


def token_response(access: str, refresh: str, *, expires_in: int = 900) -> FakeResponse:
    return FakeResponse(
        {
            "success": True,
            "data": {
                "access_token": access,
                "refresh_token": refresh,
                "expires_in": expires_in,
                "token_type": "Bearer",
                "scope": "read_user_profile",
            },
        }
    )


class LxnsOAuthTests(unittest.TestCase):
    def make_service(
        self,
        directory: str,
        *,
        post,
        clock: MutableClock | None = None,
    ) -> tuple[lxns_oauth.OAuthStore, lxns_oauth.OAuthService]:
        config = lxns_oauth.OAuthConfig(
            client_id="client-id",
            client_secret="client-secret",
            redirect_uri="https://bot.example.test/lxns/callback",
            authorize_url="https://maimai.example.test/oauth/authorize",
            token_url="https://maimai.example.test/api/v0/oauth/token",
            scopes="read_user_profile",
        )
        store = lxns_oauth.OAuthStore(Path(directory) / "oauth.sqlite3")
        service = lxns_oauth.OAuthService(
            config, store, post=post, clock=clock or MutableClock()
        )
        return store, service

    def test_environment_configuration_defaults_to_empty(self) -> None:
        names = [
            "LXNS_OAUTH_CLIENT_ID",
            "LXNS_CLIENT_ID",
            "LXNS_OAUTH_CLIENT_SECRET",
            "LXNS_CLIENT_SECRET",
            "LXNS_OAUTH_REDIRECT_URI",
            "LXNS_REDIRECT_URI",
            "LXNS_OAUTH_AUTHORIZE_URL",
            "LXNS_AUTHORIZE_URL",
            "LXNS_OAUTH_TOKEN_URL",
            "LXNS_API_BASE_URL",
            "LXNS_OAUTH_SCOPES",
            "LXNS_SCOPES",
            "LXNS_OAUTH_DB",
        ]
        with patch.dict(os.environ, {name: "" for name in names}, clear=True):
            config = lxns_oauth.OAuthConfig.from_env()

        self.assertEqual(config.client_id, "")
        self.assertEqual(config.client_secret, "")
        self.assertEqual(config.redirect_uri, "")
        self.assertEqual(config.authorize_url, lxns_oauth.DEFAULT_AUTHORIZE_URL)
        self.assertEqual(config.token_url, lxns_oauth.DEFAULT_TOKEN_URL)
        self.assertEqual(config.scopes, lxns_oauth.DEFAULT_SCOPES)

    def test_oauth_url_uses_opaque_state_and_does_not_return_state_field(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            store, service = self.make_service(temp_dir, post=lambda *a, **k: None)

            result = service.oauth_url(
                "user:123456789",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
            )

            query = parse_qs(urlparse(result["authorizationUrl"]).query)
            state = query["state"][0]
            serialized = json.dumps(result, ensure_ascii=False)

            self.assertGreaterEqual(len(state), 32)
            self.assertNotIn("123456789", state)
            self.assertNotIn("state", result)
            self.assertNotIn(str(store.path), serialized)
            self.assertEqual(store.state_count(), 1)

    def test_oauth_url_accepts_explicit_opaque_state_without_echoing_it_as_a_field(
        self,
    ) -> None:
        supplied_state = (
            "lxns.1800000000.synthetic-nonce-1234567890.synthetic-signature"
        )
        with tempfile.TemporaryDirectory() as temp_dir:
            _store, service = self.make_service(temp_dir, post=lambda *a, **k: None)

            result = service.oauth_url("subject-1", state=supplied_state)

            query = parse_qs(urlparse(result["authorizationUrl"]).query)

        self.assertEqual(query["state"], [supplied_state])
        self.assertNotIn("state", result)

    def test_normalize_oauth_submission_accepts_three_manual_forms(self) -> None:
        raw = lxns_oauth.normalize_oauth_submission("raw-code")
        assignment = lxns_oauth.normalize_oauth_submission("code=assigned-code")
        callback = lxns_oauth.normalize_oauth_submission(
            "https://bot.example.test/lxns/callback?code=url-code&state=opaque-state"
        )

        self.assertEqual(raw, ("raw-code", None))
        self.assertEqual(assignment, ("assigned-code", None))
        self.assertEqual(callback, ("url-code", "opaque-state"))

    def test_normalize_oauth_submission_rejects_ambiguous_or_unsafe_callbacks(
        self,
    ) -> None:
        invalid_values = [
            "code=first&code=second",
            "code=value&state=first&state=second",
            "code=value&error=first&error=second",
            "https://bot.example.test/callback?code=value&a=1&b=2&c=3&d=4&e=5&f=6&g=7&h=8",
            "ftp://bot.example.test/callback?code=value",
            "javascript://callback?code=value",
            "https://bot.example.test/callback",
            "https://bot.example.test/callback?code=value#state=hidden-state",
        ]
        for value in invalid_values:
            with (
                self.subTest(value=value),
                self.assertRaises(lxns_oauth.LxnsOAuthError) as caught,
            ):
                lxns_oauth.normalize_oauth_submission(value)
            self.assertEqual(caught.exception.code, "INVALID_INPUT")

    def test_bind_code_accepts_manual_forms_and_returns_no_secrets(self) -> None:
        calls: list[dict[str, object]] = []

        def fake_post(
            url: str, data: dict[str, object], timeout: float
        ) -> FakeResponse:
            calls.append(dict(data))
            return token_response("access-secret", "refresh-secret")

        with tempfile.TemporaryDirectory() as temp_dir:
            store, service = self.make_service(temp_dir, post=fake_post)
            service.oauth_url("subject-1")
            first = service.bind_code("subject-1", "code=first-code")
            service.oauth_url("subject-2")
            second = service.bind_code(
                "subject-2",
                "https://bot.example.test/lxns/callback?code=second-code",
            )
            service.oauth_url(
                "subject-3",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
            )
            third = service.bind_code(
                "subject-3",
                "third-code",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
            )
            fourth_url = service.oauth_url(
                "subject-4",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
            )
            fourth_state = parse_qs(urlparse(fourth_url["authorizationUrl"]).query)[
                "state"
            ][0]
            fourth = service.bind_code(
                "subject-4",
                f"https://bot.example.test/lxns/callback?code=fourth-code&state={fourth_state}",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
            )

            raw = store.read_token("subject-1")

        self.assertEqual(
            [call["code"] for call in calls],
            ["first-code", "second-code", "third-code", "fourth-code"],
        )
        self.assertEqual(raw["access_token"], "access-secret")
        self.assertEqual(raw["refresh_token"], "refresh-secret")
        for result in (first, second, third, fourth):
            serialized = json.dumps(result, ensure_ascii=False)
            self.assertNotIn("first-code", serialized)
            self.assertNotIn("second-code", serialized)
            self.assertNotIn("access-secret", serialized)
            self.assertNotIn("refresh-secret", serialized)
            self.assertNotIn("tokenPreview", result)
            self.assertNotIn("bindingsFile", result)
            self.assertNotIn("subject", result)

    def test_prepare_poke_rejects_wrong_event_context_before_token_exchange(
        self,
    ) -> None:
        calls = 0

        def fake_post(*args, **kwargs):
            nonlocal calls
            calls += 1
            return token_response("pending-access", "pending-refresh")

        with tempfile.TemporaryDirectory() as temp_dir:
            _store, service = self.make_service(temp_dir, post=fake_post)
            service.oauth_url(
                "subject-1",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
            )

            with self.assertRaises(lxns_oauth.LxnsOAuthError) as caught:
                service.prepare_poke(
                    "subject-1",
                    "pending-code",
                    adapter="napcat",
                    conversation="group:wrong",
                    bot="bot:20002",
                )

        self.assertEqual(caught.exception.code, "CONTEXT_MISMATCH")
        self.assertEqual(calls, 0)

    def test_pending_context_is_checked_before_expiry(self) -> None:
        clock = MutableClock()
        with tempfile.TemporaryDirectory() as temp_dir:
            store, service = self.make_service(
                temp_dir,
                post=lambda *a, **k: token_response(
                    "pending-access", "pending-refresh"
                ),
                clock=clock,
            )
            service.oauth_url(
                "subject-1",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
            )
            pending = service.prepare_poke(
                "subject-1",
                "pending-code",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
                ttl_seconds=60,
            )
            clock.value += 61

            wrong_context = service.confirm_poke(
                "subject-1",
                adapter="napcat",
                conversation="group:wrong",
                bot="bot:20002",
            )
            exact_context = service.confirm_poke(
                "subject-1",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
            )

        self.assertTrue(pending["pending"])
        self.assertNotIn("subject", pending)
        self.assertEqual(wrong_context["status"], "context_mismatch")
        self.assertEqual(exact_context["status"], "expired")

    def test_confirm_poke_atomically_activates_pending_token(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            store, service = self.make_service(
                temp_dir,
                post=lambda *a, **k: token_response(
                    "pending-access", "pending-refresh"
                ),
            )
            service.oauth_url(
                "subject-1",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
            )
            service.prepare_poke(
                "subject-1",
                "pending-code",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
            )

            before = service.status("subject-1")
            confirmed = service.confirm_poke(
                "subject-1",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
            )
            after = service.status("subject-1")
            repeated = service.confirm_poke(
                "subject-1",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
            )
            stored = store.read_token("subject-1")

        self.assertFalse(before["bound"])
        self.assertTrue(before["pending"])
        self.assertTrue(confirmed["confirmed"])
        self.assertNotIn("subject", confirmed)
        self.assertTrue(after["bound"])
        self.assertFalse(after["pending"])
        self.assertEqual(repeated["status"], "not_found")
        self.assertEqual(stored["refresh_token"], "pending-refresh")
        serialized = json.dumps(confirmed, ensure_ascii=False)
        self.assertNotIn("pending-access", serialized)
        self.assertNotIn("pending-refresh", serialized)

    def test_status_deletes_expired_pending_token_and_state(self) -> None:
        clock = MutableClock()
        with tempfile.TemporaryDirectory() as temp_dir:
            store, service = self.make_service(
                temp_dir,
                post=lambda *a, **k: token_response(
                    "expired-pending-access",
                    "expired-pending-refresh",
                ),
                clock=clock,
            )
            service.oauth_url(
                "subject-1",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
                ttl_seconds=60,
            )
            service.prepare_poke(
                "subject-1",
                "pending-code",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
                ttl_seconds=60,
            )
            clock.value += 61

            result = service.status("subject-1")
            pending_count = store.pending_count("subject-1")
            state_count = store.state_count("subject-1")

        self.assertFalse(result["pending"])
        self.assertEqual(pending_count, 0)
        self.assertEqual(state_count, 0)
        serialized = json.dumps(result, ensure_ascii=False)
        self.assertNotIn("expired-pending-access", serialized)
        self.assertNotIn("expired-pending-refresh", serialized)

    def test_prepare_poke_does_not_replace_existing_binding_until_confirmation(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            store, service = self.make_service(
                temp_dir,
                post=lambda *a, **k: token_response("new-access", "new-refresh"),
            )
            store.save_token(
                "subject-1",
                access_token="old-access",
                refresh_token="old-refresh",
                expires_at=1_900_000_000,
                token_type="Bearer",
                scope="read_user_profile",
            )
            service.oauth_url(
                "subject-1",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
            )
            service.prepare_poke(
                "subject-1",
                "pending-code",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
            )
            before = store.read_token("subject-1")

            service.confirm_poke(
                "subject-1",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
            )
            after = store.read_token("subject-1")

        self.assertEqual(before["access_token"], "old-access")
        self.assertEqual(before["refresh_token"], "old-refresh")
        self.assertEqual(after["access_token"], "new-access")
        self.assertEqual(after["refresh_token"], "new-refresh")

    def test_refresh_uses_rotated_token_and_compare_and_swap(self) -> None:
        clock = MutableClock()
        with tempfile.TemporaryDirectory() as temp_dir:
            store, service = self.make_service(
                temp_dir, post=lambda *a, **k: None, clock=clock
            )
            initial = store.save_token(
                "subject-1",
                access_token="old-access",
                refresh_token="old-refresh",
                expires_at=clock.value - 1,
                token_type="Bearer",
                scope="read_user_profile",
            )

            def racing_post(
                url: str, data: dict[str, object], timeout: float
            ) -> FakeResponse:
                self.assertEqual(data["refresh_token"], "old-refresh")
                store.save_token(
                    "subject-1",
                    access_token="winner-access",
                    refresh_token="winner-refresh",
                    expires_at=clock.value + 900,
                    token_type="Bearer",
                    scope="read_user_profile",
                    expected_revision=initial["revision"],
                )
                return token_response("loser-access", "loser-refresh")

            service.post = racing_post
            result = service.refresh("subject-1", force=True)
            stored = store.read_token("subject-1")

        self.assertEqual(stored["access_token"], "winner-access")
        self.assertEqual(stored["refresh_token"], "winner-refresh")
        self.assertEqual(result["revision"], stored["revision"])
        serialized = json.dumps(result, ensure_ascii=False)
        self.assertNotIn("winner-access", serialized)
        self.assertNotIn("winner-refresh", serialized)
        self.assertNotIn("loser-access", serialized)
        self.assertNotIn("loser-refresh", serialized)

    def test_refresh_rejects_missing_or_unrotated_refresh_token(self) -> None:
        for payload in (
            FakeResponse(
                {
                    "success": True,
                    "data": {"access_token": "new-access", "expires_in": 900},
                }
            ),
            token_response("new-access", "old-refresh"),
        ):
            with (
                self.subTest(payload=payload._payload),
                tempfile.TemporaryDirectory() as temp_dir,
            ):
                clock = MutableClock()
                store, service = self.make_service(
                    temp_dir, post=lambda *a, **k: payload, clock=clock
                )
                store.save_token(
                    "subject-1",
                    access_token="old-access",
                    refresh_token="old-refresh",
                    expires_at=clock.value - 1,
                    token_type="Bearer",
                    scope="read_user_profile",
                )

                with self.assertRaises(lxns_oauth.LxnsOAuthError) as caught:
                    service.refresh("subject-1", force=True)

                stored = store.read_token("subject-1")

            self.assertEqual(caught.exception.code, "INVALID_TOKEN_RESPONSE")
            self.assertEqual(stored["refresh_token"], "old-refresh")

    def test_refresh_invalid_grant_recovers_from_concurrent_cas_winner(self) -> None:
        clock = MutableClock()
        with tempfile.TemporaryDirectory() as temp_dir:
            store, service = self.make_service(
                temp_dir, post=lambda *a, **k: None, clock=clock
            )
            initial = store.save_token(
                "subject-1",
                access_token="old-access",
                refresh_token="old-refresh",
                expires_at=clock.value - 1,
                token_type="Bearer",
                scope="read_user_profile",
            )

            def racing_rejection(
                url: str, data: dict[str, object], timeout: float
            ) -> FakeResponse:
                store.save_token(
                    "subject-1",
                    access_token="winner-access",
                    refresh_token="winner-refresh",
                    expires_at=clock.value + 900,
                    token_type="Bearer",
                    scope="read_user_profile",
                    expected_revision=initial["revision"],
                )
                return FakeResponse(
                    {"error": "invalid_grant"}, status_code=400, ok=False
                )

            service.post = racing_rejection
            result = service.refresh("subject-1", force=True)
            stored = store.read_token("subject-1")

        self.assertEqual(result["revision"], stored["revision"])
        self.assertEqual(stored["refresh_token"], "winner-refresh")
        self.assertNotIn("winner-refresh", json.dumps(result, ensure_ascii=False))

    def test_unbind_removes_token_state_and_pending_without_paths(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            store, service = self.make_service(
                temp_dir,
                post=lambda *a, **k: token_response(
                    "pending-access", "pending-refresh"
                ),
            )
            service.oauth_url("subject-1")
            service.bind_code("subject-1", "bind-code")
            service.oauth_url(
                "subject-1",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
            )
            service.prepare_poke(
                "subject-1",
                "pending-code",
                adapter="napcat",
                conversation="group:10001",
                bot="bot:20002",
            )

            result = service.unbind("subject-1")
            stored = store.read_token("subject-1")
            state_count = store.state_count("subject-1")
            pending_count = store.pending_count("subject-1")

        self.assertTrue(result["changed"])
        self.assertNotIn("subject", result)
        self.assertNotIn("bindingsFile", result)
        self.assertNotIn(str(store.path), json.dumps(result, ensure_ascii=False))
        self.assertIsNone(stored)
        self.assertEqual(state_count, 0)
        self.assertEqual(pending_count, 0)

    def test_all_public_service_results_do_not_echo_subject(self) -> None:
        synthetic_subject = "123456789"
        with tempfile.TemporaryDirectory() as temp_dir:
            _store, service = self.make_service(
                temp_dir,
                post=lambda *a, **k: token_response("access-secret", "refresh-secret"),
            )
            results = [service.oauth_url(synthetic_subject)]
            results.append(service.bind_code(synthetic_subject, "manual-code"))
            results.append(service.status(synthetic_subject))
            results.append(service.refresh(synthetic_subject))
            results.append(service.unbind(synthetic_subject))

            results.append(
                service.oauth_url(
                    synthetic_subject,
                    adapter="napcat",
                    conversation="group:10001",
                    bot="bot:20002",
                )
            )
            results.append(
                service.prepare_poke(
                    synthetic_subject,
                    "pending-code",
                    adapter="napcat",
                    conversation="group:10001",
                    bot="bot:20002",
                )
            )
            results.append(
                service.confirm_poke(
                    synthetic_subject,
                    adapter="napcat",
                    conversation="group:10001",
                    bot="bot:20002",
                )
            )
            results.append(service.status(synthetic_subject))
            results.append(service.unbind(synthetic_subject))

        for result in results:
            self.assertNotIn(synthetic_subject, json.dumps(result, ensure_ascii=False))

    def test_store_protects_directory_database_wal_and_shm_under_public_umask(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            secure_dir = Path(temp_dir) / "oauth-private"
            db_path = secure_dir / "oauth.sqlite3"
            previous_umask = os.umask(0o022)
            try:
                store = lxns_oauth.OAuthStore(db_path)
                with store.connect() as conn:
                    conn.execute(
                        "INSERT OR REPLACE INTO oauth_tokens "
                        "(subject, access_token, refresh_token, token_type, scope, expires_at, revision, created_at, updated_at) "
                        "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                        (
                            "subject-1",
                            "access",
                            "refresh",
                            "Bearer",
                            "scope",
                            1,
                            1,
                            1,
                            1,
                        ),
                    )
                    store.protect_storage_files()
                    existing = [
                        path
                        for path in (
                            db_path,
                            Path(f"{db_path}-wal"),
                            Path(f"{db_path}-shm"),
                        )
                        if path.exists()
                    ]
                    modes = {
                        path.name: stat.S_IMODE(path.stat().st_mode)
                        for path in existing
                    }
                    directory_mode = stat.S_IMODE(secure_dir.stat().st_mode)
            finally:
                os.umask(previous_umask)

        self.assertEqual(directory_mode, 0o700)
        self.assertIn("oauth.sqlite3", modes)
        self.assertGreaterEqual(len(modes), 2)
        self.assertTrue(all(mode == 0o600 for mode in modes.values()), modes)

    def test_store_rejects_symlink_database_directory(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            real_directory = root / "real"
            real_directory.mkdir()
            linked_directory = root / "linked"
            linked_directory.symlink_to(real_directory, target_is_directory=True)

            with self.assertRaises(lxns_oauth.LxnsOAuthError) as caught:
                lxns_oauth.OAuthStore(linked_directory / "oauth.sqlite3")

        self.assertEqual(caught.exception.code, "UNSAFE_STORAGE")

    def test_store_rejects_shared_parent_without_changing_its_mode(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            shared_directory = Path(temp_dir) / "shared"
            shared_directory.mkdir(mode=0o755)
            shared_directory.chmod(0o755)

            with self.assertRaises(lxns_oauth.LxnsOAuthError) as caught:
                lxns_oauth.OAuthStore(shared_directory / "oauth.sqlite3")

            mode_after = stat.S_IMODE(shared_directory.stat().st_mode)

        self.assertEqual(caught.exception.code, "UNSAFE_STORAGE")
        self.assertEqual(mode_after, 0o755)

    def test_reopening_store_deletes_expired_state_and_pending_token(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            db_path = Path(temp_dir) / "oauth.sqlite3"
            store = lxns_oauth.OAuthStore(db_path)
            expired_at = int(time.time()) - 1
            with store.connect() as conn:
                conn.execute(
                    "INSERT INTO oauth_states "
                    "(state_digest, subject, adapter, conversation, bot, code_verifier, "
                    "created_at, expires_at, consumed_at) "
                    "VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL)",
                    (
                        "expired-state-digest",
                        "subject-1",
                        "napcat",
                        "group:10001",
                        "bot:20002",
                        "expired-verifier",
                        expired_at - 60,
                        expired_at,
                    ),
                )
                conn.execute(
                    "INSERT INTO oauth_pending "
                    "(subject, adapter, conversation, bot, access_token, refresh_token, "
                    "token_type, scope, token_expires_at, created_at, expires_at) "
                    "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    (
                        "subject-1",
                        "napcat",
                        "group:10001",
                        "bot:20002",
                        "expired-access",
                        "expired-refresh",
                        "Bearer",
                        "read_user_profile",
                        expired_at,
                        expired_at - 60,
                        expired_at,
                    ),
                )

            reopened = lxns_oauth.OAuthStore(db_path)
            state_count = reopened.state_count("subject-1")
            pending_count = reopened.pending_count("subject-1")

        self.assertEqual(state_count, 0)
        self.assertEqual(pending_count, 0)

    def test_store_rejects_symlink_database_and_sidecar_without_chmod_follow(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            secure_directory = Path(temp_dir) / "secure"
            secure_directory.mkdir(mode=0o700)
            target = Path(temp_dir) / "target"
            target.write_text("do not touch", encoding="utf-8")
            target.chmod(0o644)
            db_path = secure_directory / "oauth.sqlite3"
            db_path.symlink_to(target)

            with self.assertRaises(lxns_oauth.LxnsOAuthError) as db_error:
                lxns_oauth.OAuthStore(db_path)
            target_mode_after_db = stat.S_IMODE(target.stat().st_mode)

            db_path.unlink()
            store = lxns_oauth.OAuthStore(db_path)
            sidecar = Path(f"{db_path}-wal")
            sidecar.unlink(missing_ok=True)
            sidecar.symlink_to(target)
            with self.assertRaises(lxns_oauth.LxnsOAuthError) as sidecar_error:
                store.protect_storage_files()
            target_mode_after_sidecar = stat.S_IMODE(target.stat().st_mode)

        self.assertEqual(db_error.exception.code, "UNSAFE_STORAGE")
        self.assertEqual(sidecar_error.exception.code, "UNSAFE_STORAGE")
        self.assertEqual(target_mode_after_db, 0o644)
        self.assertEqual(target_mode_after_sidecar, 0o644)


if __name__ == "__main__":
    unittest.main()
