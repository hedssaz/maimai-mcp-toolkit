from __future__ import annotations

import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from typing import Any

from scripts import maimai_update_records_workflow as workflow
from maimai_update_mcp import server as update_server


class FakeResponse:
    ok = True
    status_code = 200
    text = '{"updated":true}'

    def json(self) -> dict[str, Any]:
        return {"updated": True}


class MaimaiUpdateRecordsWorkflowTests(unittest.TestCase):
    def test_bind_import_token_writes_secret_store(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "bindings.json"

            result = workflow.bind_import_token("1000000001", "import-token-abcdef", path=path)
            data = json.loads(path.read_text(encoding="utf-8"))

        self.assertTrue(result["ok"])
        self.assertEqual(data["bindings"]["1000000001"]["importToken"], "import-token-abcdef")
        self.assertNotIn("import-token-abcdef", result["text"])

    def test_update_workflow_uses_raw_stdin_converts_and_uploads(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            bindings = root / "bindings.json"
            output_dir = root / "runs"
            workflow.bind_import_token("1000000001", "import-token-abcdef", path=bindings)
            calls: list[dict[str, Any]] = []

            def fake_runner(command: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
                calls.append({"command": command, **kwargs})
                if "sdgb155_full_dump_logout_tool.py" in command[1]:
                    output_arg = Path(command[command.index("--output-dir") + 1])
                    output_arg.mkdir(parents=True, exist_ok=True)
                    raw_path = output_arg / "raw.json"
                    raw_path.write_text(
                        json.dumps(
                            {
                                "GetUserMusicApi": {
                                    "userMusicList": [
                                        {
                                            "userMusicDetailList": [
                                                {
                                                    "musicId": 383,
                                                    "level": 3,
                                                    "achievement": 1000000,
                                                    "deluxscoreMax": 2458,
                                                    "comboStatus": 1,
                                                    "syncStatus": 5,
                                                }
                                            ]
                                        }
                                    ]
                                }
                            },
                            ensure_ascii=False,
                        ),
                        encoding="utf-8",
                    )
                    return subprocess.CompletedProcess(
                        command,
                        0,
                        stdout=f"user_id=10000001\nfull_json_path={raw_path}\nflow_success=true\n",
                        stderr="",
                    )
                output_path = Path(command[command.index("-o") + 1])
                report_path = Path(command[command.index("--report") + 1])
                output_path.write_text(
                    json.dumps(
                        [
                            {
                                "achievements": 100.0,
                                "dxScore": 2458,
                                "fc": "fc",
                                "fs": "sync",
                                "level_index": 3,
                                "title": "Link(CoF)",
                                "type": "SD",
                            }
                        ],
                        ensure_ascii=False,
                    ),
                    encoding="utf-8",
                )
                report_path.write_text(json.dumps({"skipped": 0, "skippedRecords": []}), encoding="utf-8")
                return subprocess.CompletedProcess(command, 0, stdout="", stderr="")

            seen_post: dict[str, Any] = {}

            def fake_post(url: str, **kwargs: Any) -> FakeResponse:
                seen_post["url"] = url
                seen_post.update(kwargs)
                return FakeResponse()

            result = workflow.update_records_workflow(
                qq="1000000001",
                qr_content="SGWCSDGB2606171200000123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                keyship="A63E01D90630000",
                logoutid=2,
                title_ver="1.55.00",
                bindings_file=bindings,
                output_dir=output_dir,
                runner=fake_runner,
                post=fake_post,
                python="/python",
            )

        raw_call = calls[0]
        self.assertNotIn(result["qq"], raw_call["command"])
        self.assertNotIn(result["rawJsonPath"], raw_call["command"])
        self.assertIn("--keyship-id", raw_call["command"])
        self.assertEqual(raw_call["input"], "SGWCSDGB2606171200000123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n")
        self.assertEqual(seen_post["headers"]["Import-Token"], "import-token-abcdef")
        self.assertEqual(seen_post["json"][0]["title"], "Link(CoF)")
        self.assertEqual(result["converted"], 1)
        self.assertEqual(result["skipped"], 0)

    def test_mcp_bind_call_returns_text_result(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            old_path = workflow.os.environ.get("MAIMAI_IMPORT_TOKEN_BINDINGS_FILE")
            workflow.os.environ["MAIMAI_IMPORT_TOKEN_BINDINGS_FILE"] = str(Path(tmp) / "bindings.json")
            try:
                response = update_server.handle_request(
                    {
                        "jsonrpc": "2.0",
                        "id": 1,
                        "method": "tools/call",
                        "params": {
                            "name": "maimai_bind_import_token",
                            "arguments": {"qq": "1000000001", "importToken": "import-token-abcdef"},
                        },
                    }
                )
            finally:
                if old_path is None:
                    workflow.os.environ.pop("MAIMAI_IMPORT_TOKEN_BINDINGS_FILE", None)
                else:
                    workflow.os.environ["MAIMAI_IMPORT_TOKEN_BINDINGS_FILE"] = old_path

        assert response is not None
        result = response["result"]
        self.assertFalse(result["isError"])
        self.assertIn("已绑定 QQ 1000000001", result["content"][0]["text"])


if __name__ == "__main__":
    unittest.main()
