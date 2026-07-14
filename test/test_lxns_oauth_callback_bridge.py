from __future__ import annotations

import contextlib
import http.client
import io
import json
import sqlite3
import stat
import tempfile
import threading
import unittest
from pathlib import Path
from urllib.parse import quote

from scripts.lxns_oauth_callback_bridge import (
    MAX_CODE_LENGTH,
    MAX_STATE_LENGTH,
    CallbackHandler,
    CallbackServer,
    CallbackStore,
    signed_state_signature,
    utc_seconds,
    valid_signed_state,
)


TEST_TOKEN = "0123456789abcdef0123456789abcdef"


class RunningCallbackServer:
    def __init__(self, store: CallbackStore, *, token: str = TEST_TOKEN) -> None:
        self.server = CallbackServer(
            ("127.0.0.1", 0),
            CallbackHandler,
            store=store,
            callback_path="/lxns/callback",
            poll_path="/lxns/poll",
            token=token,
        )
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def __enter__(self) -> RunningCallbackServer:
        self.thread.start()
        return self

    def __exit__(self, *_args: object) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=3)

    def get(
        self,
        target: str,
        *,
        headers: dict[str, str] | None = None,
    ) -> tuple[int, dict[str, str], bytes]:
        connection = http.client.HTTPConnection(
            "127.0.0.1",
            self.server.server_port,
            timeout=3,
        )
        try:
            connection.request("GET", target, headers=headers or {})
            response = connection.getresponse()
            return response.status, dict(response.getheaders()), response.read()
        finally:
            connection.close()


def signed_state(*, issued_at: int | None = None, nonce: str | None = None) -> str:
    issued = utc_seconds() if issued_at is None else issued_at
    nonce_value = nonce or "callback-test-nonce-1234567890"
    signature = signed_state_signature(TEST_TOKEN, issued, nonce_value)
    return f"lxns.{issued}.{nonce_value}.{signature}"


class LxnsOAuthCallbackBridgeTests(unittest.TestCase):
    def test_state_requires_strong_token_and_rejects_tampering_or_bad_time(
        self,
    ) -> None:
        now = 1_800_000_000
        state = signed_state(issued_at=now)

        self.assertTrue(valid_signed_state(state, TEST_TOKEN, now=now, ttl_seconds=600))
        self.assertFalse(
            valid_signed_state(state, "too-short", now=now, ttl_seconds=600)
        )
        self.assertFalse(
            valid_signed_state(state + "0", TEST_TOKEN, now=now, ttl_seconds=600)
        )
        self.assertFalse(valid_signed_state(state, "f" * 32, now=now, ttl_seconds=600))
        self.assertFalse(
            valid_signed_state(state, TEST_TOKEN, now=now + 601, ttl_seconds=600)
        )
        self.assertFalse(
            valid_signed_state(state, TEST_TOKEN, now=now - 61, ttl_seconds=600)
        )

    def test_server_refuses_missing_or_short_shared_token(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            store = CallbackStore(Path(temp_dir) / "callbacks.sqlite3")
            for token in ("", "short-token"):
                with self.subTest(token=token):
                    with self.assertRaises(ValueError):
                        CallbackServer(
                            ("127.0.0.1", 0),
                            CallbackHandler,
                            store=store,
                            callback_path="/lxns/callback",
                            poll_path="/lxns/poll",
                            token=token,
                        )

    def test_first_code_wins_and_consumption_keeps_a_cleared_tombstone(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            db_path = Path(temp_dir) / "private" / "callbacks.sqlite3"
            store = CallbackStore(db_path, ttl_seconds=600)

            self.assertTrue(store.put("state-token", "first-code"))
            self.assertFalse(store.put("state-token", "replacement-code"))
            self.assertEqual(
                store.poll("state-token", consume=False)["code"], "first-code"
            )
            self.assertEqual(store.poll("state-token")["code"], "first-code")
            self.assertFalse(store.poll("state-token")["ready"])
            self.assertFalse(store.put("state-token", "late-replacement"))
            self.assertFalse(store.poll("state-token")["ready"])

            with sqlite3.connect(db_path) as connection:
                row = connection.execute(
                    "SELECT code, consumed_at FROM lxns_oauth_callbacks WHERE state = ?",
                    ("state-token",),
                ).fetchone()

        self.assertIsNotNone(row)
        self.assertEqual(row[0], "")
        self.assertIsNotNone(row[1])

    def test_consuming_poll_is_atomic_across_concurrent_workers(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            store = CallbackStore(Path(temp_dir) / "callbacks.sqlite3", ttl_seconds=600)
            store.put("state-token", "oauth-code")
            barrier = threading.Barrier(3)
            results: list[dict[str, object]] = []

            def poll_once() -> None:
                barrier.wait()
                results.append(store.poll("state-token"))

            workers = [threading.Thread(target=poll_once) for _ in range(2)]
            for worker in workers:
                worker.start()
            barrier.wait()
            for worker in workers:
                worker.join(timeout=3)

        self.assertEqual(len(results), 2)
        self.assertEqual(sum(bool(result["ready"]) for result in results), 1)
        winner = next(result for result in results if result["ready"])
        self.assertEqual(winner["code"], "oauth-code")

    def test_storage_directory_database_and_live_sidecars_are_private(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            private_dir = Path(temp_dir) / "callback-data"
            db_path = private_dir / "callbacks.sqlite3"
            store = CallbackStore(db_path)

            keeper = sqlite3.connect(db_path)
            try:
                keeper.execute("PRAGMA journal_mode=WAL")
                keeper.execute("BEGIN")
                keeper.execute("SELECT COUNT(*) FROM lxns_oauth_callbacks").fetchone()
                store.put("state-token", "oauth-code")

                self.assertEqual(stat.S_IMODE(private_dir.stat().st_mode), 0o700)
                self.assertEqual(stat.S_IMODE(db_path.stat().st_mode), 0o600)
                sidecars = [Path(f"{db_path}-wal"), Path(f"{db_path}-shm")]
                self.assertTrue(all(path.exists() for path in sidecars))
                self.assertTrue(
                    all(stat.S_IMODE(path.stat().st_mode) == 0o600 for path in sidecars)
                )
            finally:
                keeper.close()

    def test_store_refuses_a_shared_directory_without_changing_its_mode(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            shared_dir = Path(temp_dir) / "shared"
            shared_dir.mkdir(mode=0o755)
            shared_dir.chmod(0o755)

            with self.assertRaises(ValueError):
                CallbackStore(shared_dir / "callbacks.sqlite3")

            self.assertEqual(stat.S_IMODE(shared_dir.stat().st_mode), 0o755)

    def test_http_callback_and_authenticated_poll_are_one_shot(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            store = CallbackStore(Path(temp_dir) / "callbacks.sqlite3")
            state = signed_state()
            with RunningCallbackServer(store) as running:
                status, headers, body = running.get(
                    f"/lxns/callback?state={quote(state)}&code=oauth-code"
                )
                self.assertEqual(status, 200)
                self.assertIn("已成功授权", body.decode("utf-8"))
                self.assertEqual(headers["Cache-Control"], "no-store")
                self.assertEqual(headers["Pragma"], "no-cache")
                self.assertEqual(headers["X-Frame-Options"], "DENY")
                self.assertEqual(headers["X-Content-Type-Options"], "nosniff")
                self.assertIn("default-src 'none'", headers["Content-Security-Policy"])

                status, _, _ = running.get(f"/lxns/poll?state={quote(state)}")
                self.assertEqual(status, 401)

                auth = {"Authorization": f"Bearer {TEST_TOKEN}"}
                status, _, body = running.get(
                    f"/lxns/poll?state={quote(state)}&consume=0",
                    headers=auth,
                )
                self.assertEqual(status, 200)
                self.assertEqual(json.loads(body)["code"], "oauth-code")

                _, _, first_body = running.get(
                    f"/lxns/poll?state={quote(state)}",
                    headers=auth,
                )
                _, _, second_body = running.get(
                    f"/lxns/poll?state={quote(state)}",
                    headers=auth,
                )
                self.assertTrue(json.loads(first_body)["ready"])
                self.assertFalse(json.loads(second_body)["ready"])

                duplicate_status, _, _ = running.get(
                    f"/lxns/callback?state={quote(state)}&code=replacement-code"
                )
                self.assertEqual(duplicate_status, 200)
                _, _, after_duplicate = running.get(
                    f"/lxns/poll?state={quote(state)}",
                    headers=auth,
                )
                self.assertFalse(json.loads(after_duplicate)["ready"])

    def test_callback_and_poll_reject_oversized_or_ambiguous_parameters(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            store = CallbackStore(Path(temp_dir) / "callbacks.sqlite3")
            state = signed_state()
            auth = {"Authorization": f"Bearer {TEST_TOKEN}"}
            with RunningCallbackServer(store) as running:
                cases = (
                    f"/lxns/callback?state={quote('s' * (MAX_STATE_LENGTH + 1))}&code=code",
                    f"/lxns/callback?state={quote(state)}&code={'c' * (MAX_CODE_LENGTH + 1)}",
                    f"/lxns/callback?state={quote(state)}&state={quote(state)}&code=code",
                    f"/lxns/callback?state={quote(state)}&code=first&code=second",
                )
                for target in cases:
                    with self.subTest(target=target[:80]):
                        status, _, _ = running.get(target)
                        self.assertEqual(status, 400)

                status, _, _ = running.get(
                    f"/lxns/poll?state={'s' * (MAX_STATE_LENGTH + 1)}",
                    headers=auth,
                )
                self.assertEqual(status, 400)

    def test_request_logging_never_emits_query_secrets(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            store = CallbackStore(Path(temp_dir) / "callbacks.sqlite3")
            state = signed_state()
            stderr = io.StringIO()
            with (
                contextlib.redirect_stderr(stderr),
                RunningCallbackServer(store) as running,
            ):
                status, _, _ = running.get(
                    f"/lxns/callback?state={quote(state)}&code=secret-code-sentinel"
                )

        self.assertEqual(status, 200)
        self.assertNotIn("secret-code-sentinel", stderr.getvalue())
        self.assertNotIn(state, stderr.getvalue())

    def test_deployment_examples_are_generic_and_hardened(self) -> None:
        project_root = Path(__file__).resolve().parents[1]
        env_text = (project_root / "deploy/lxns-oauth-callback.env.example").read_text(
            "utf-8"
        )
        nginx_text = (project_root / "deploy/lxns-oauth-callback.nginx.conf").read_text(
            "utf-8"
        )
        service_text = (project_root / "deploy/lxns-oauth-callback.service").read_text(
            "utf-8"
        )
        combined = "\n".join((env_text, nginx_text, service_text))

        for forbidden in ("hedssaz", "/Users/", "/home/", "ICP备", "beian.miit.gov.cn"):
            with self.subTest(forbidden=forbidden):
                self.assertNotIn(forbidden, combined)
        self.assertIn("LXNS_CALLBACK_TOKEN=", env_text)
        self.assertIn("access_log off", nginx_text)
        self.assertGreaterEqual(nginx_text.count("access_log off"), 2)
        self.assertIn("DynamicUser=yes", service_text)
        self.assertIn("UMask=0077", service_text)
        self.assertNotIn("\nUser=", service_text)


if __name__ == "__main__":
    unittest.main()
