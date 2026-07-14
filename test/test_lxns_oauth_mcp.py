from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from lxns_oauth import OAuthConfig, OAuthService, OAuthStore
from lxns_oauth_mcp import server


class FakeResponse:
    ok = True
    status_code = 200

    def json(self) -> object:
        return {
            "success": True,
            "data": {
                "access_token": "access-secret",
                "refresh_token": "refresh-secret",
                "expires_in": 900,
            },
        }


class LxnsOAuthMcpTests(unittest.TestCase):
    def make_service(self, directory: str) -> OAuthService:
        return OAuthService(
            OAuthConfig(
                client_id="client-id",
                client_secret="client-secret",
                redirect_uri="https://bot.example.test/lxns/callback",
                authorize_url="https://maimai.example.test/oauth/authorize",
                token_url="https://maimai.example.test/api/v0/oauth/token",
                scopes="read_user_profile",
            ),
            OAuthStore(Path(directory) / "oauth.sqlite3"),
            post=lambda *a, **k: FakeResponse(),
        )

    def test_tool_list_has_required_surface_and_no_legacy_output_fields(self) -> None:
        tools = {tool["name"]: tool for tool in server.TOOLS}

        self.assertEqual(
            set(tools),
            {
                "maimai_lxns_oauth_url",
                "maimai_lxns_bind_code",
                "maimai_lxns_prepare_poke",
                "maimai_lxns_confirm_poke",
                "maimai_lxns_status",
                "maimai_lxns_unbind",
            },
        )
        serialized = json.dumps(server.TOOLS, ensure_ascii=False)
        self.assertNotIn("bindingsFile", serialized)
        self.assertNotIn("tokenPreview", serialized)

    def test_initialize_and_tools_list_follow_mcp_shape(self) -> None:
        initialized = server.handle_request(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {"protocolVersion": "2024-11-05"},
            }
        )
        listed = server.handle_request(
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list"}
        )

        self.assertEqual(initialized["result"]["serverInfo"]["name"], "lxns-oauth")
        self.assertEqual(len(listed["result"]["tools"]), 6)

    def test_schemas_use_existing_astrbot_context_fields_and_allow_state(self) -> None:
        tools = {tool["name"]: tool for tool in server.TOOLS}
        url_properties = tools["maimai_lxns_oauth_url"]["inputSchema"]["properties"]
        prepare_properties = tools["maimai_lxns_prepare_poke"]["inputSchema"][
            "properties"
        ]

        for field in ("qq", "adapterId", "groupId", "botQq", "state"):
            self.assertIn(field, url_properties)
            self.assertIn(field, prepare_properties)
        self.assertIn("ttlSeconds", url_properties)

    def test_unprefixed_tool_aliases_are_not_part_of_the_public_surface(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            service = self.make_service(temp_dir)
            response = server.handle_tool_call(
                1,
                {"name": "status", "arguments": {"qq": "subject-1"}},
                service=service,
            )

        self.assertEqual(response["error"]["code"], -32602)
        self.assertIn("Unknown tool", response["error"]["message"])

    def test_oauth_url_and_binding_results_do_not_expose_secrets(self) -> None:
        synthetic_qq = "123456789"
        context = {
            "qq": synthetic_qq,
            "adapterId": "napcat",
            "groupId": "group:10001",
            "botQq": "bot:20002",
        }
        with tempfile.TemporaryDirectory() as temp_dir:
            service = self.make_service(temp_dir)
            url_response = server.handle_tool_call(
                1,
                {"name": "maimai_lxns_oauth_url", "arguments": context},
                service=service,
            )
            bind_response = server.handle_tool_call(
                2,
                {
                    "name": "maimai_lxns_bind_code",
                    "arguments": {**context, "code": "oauth-code"},
                },
                service=service,
            )

        url_structured = url_response["result"]["structuredContent"]
        self.assertIn("authorizationUrl", url_structured)
        self.assertNotIn("state", url_structured)
        serialized = json.dumps(bind_response, ensure_ascii=False)
        self.assertNotIn("oauth-code", serialized)
        self.assertNotIn("access-secret", serialized)
        self.assertNotIn("refresh-secret", serialized)
        self.assertNotIn("oauth.sqlite3", serialized)
        self.assertNotIn(synthetic_qq, json.dumps(url_response, ensure_ascii=False))
        self.assertNotIn(synthetic_qq, serialized)
        self.assertFalse(bind_response["result"]["isError"])

    def test_prepare_confirm_status_and_unbind_dispatch(self) -> None:
        context = {
            "qq": "subject-1",
            "adapterId": "napcat",
            "groupId": "group:10001",
            "botQq": "bot:20002",
        }
        with tempfile.TemporaryDirectory() as temp_dir:
            service = self.make_service(temp_dir)
            server.handle_tool_call(
                1,
                {"name": "maimai_lxns_oauth_url", "arguments": context},
                service=service,
            )
            pending = server.handle_tool_call(
                2,
                {
                    "name": "maimai_lxns_prepare_poke",
                    "arguments": {**context, "code": "oauth-code"},
                },
                service=service,
            )
            confirmed = server.handle_tool_call(
                3,
                {"name": "maimai_lxns_confirm_poke", "arguments": context},
                service=service,
            )
            status = server.handle_tool_call(
                4,
                {"name": "maimai_lxns_status", "arguments": {"qq": "subject-1"}},
                service=service,
            )
            unbound = server.handle_tool_call(
                5,
                {"name": "maimai_lxns_unbind", "arguments": {"qq": "subject-1"}},
                service=service,
            )

        self.assertTrue(pending["result"]["structuredContent"]["pending"])
        self.assertTrue(confirmed["result"]["structuredContent"]["confirmed"])
        self.assertTrue(status["result"]["structuredContent"]["bound"])
        self.assertTrue(unbound["result"]["structuredContent"]["changed"])

    def test_errors_do_not_echo_code_or_configuration(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            service = OAuthService(
                OAuthConfig(), OAuthStore(Path(temp_dir) / "secret-db.sqlite3")
            )
            response = server.handle_tool_call(
                1,
                {
                    "name": "maimai_lxns_bind_code",
                    "arguments": {"qq": "subject-1", "code": "do-not-echo-this-code"},
                },
                service=service,
            )

        serialized = json.dumps(response, ensure_ascii=False)
        self.assertTrue(response["result"]["isError"])
        self.assertNotIn("do-not-echo-this-code", serialized)
        self.assertNotIn("secret-db.sqlite3", serialized)
        self.assertNotIn("tokenPreview", serialized)
        self.assertNotIn("bindingsFile", serialized)


if __name__ == "__main__":
    unittest.main()
