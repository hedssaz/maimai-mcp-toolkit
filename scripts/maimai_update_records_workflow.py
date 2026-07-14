#!/usr/bin/env python3
"""Bind Diving-Fish Import-Token and run QR -> raw -> update_records workflow."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable

try:
    import requests
except ImportError as exc:  # pragma: no cover - operator-facing dependency check
    raise SystemExit("missing dependency: requests. Install with: python -m pip install requests") from exc


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_RAW_DUMP_SCRIPT = REPO_ROOT / "scripts" / "sdgb155_full_dump_logout_tool.py"
DEFAULT_CONVERT_SCRIPT = REPO_ROOT / "scripts" / "convert_official_raw_records.py"
DEFAULT_DIVING_FISH_URL = "https://www.diving-fish.com/api/maimaidxprober/player/update_records"
DEFAULT_BINDINGS_FILE = (
    Path("/AstrBot/data/maimai-config/.maimai-import-token-bindings.json")
    if Path("/AstrBot/data").exists()
    else REPO_ROOT / "data" / ".maimai-import-token-bindings.json"
)
DEFAULT_OUTPUT_DIR = (
    Path("/AstrBot/data/maimai-record-imports")
    if Path("/AstrBot/data").exists()
    else Path("/tmp/maimai-record-imports")
)
QQ_RE = re.compile(r"^\d{5,12}$")


class WorkflowError(RuntimeError):
    pass


@dataclass(frozen=True)
class CommandResult:
    returncode: int
    stdout: str
    stderr: str


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def mask_secret(value: str) -> str:
    if len(value) <= 8:
        return "*" * len(value)
    return f"{value[:3]}...{value[-4:]}"


def normalize_qq(value: Any) -> str:
    qq = str(value or "").strip()
    if not QQ_RE.fullmatch(qq):
        raise WorkflowError("QQ 号格式不正确。")
    return qq


def normalize_token(value: Any) -> str:
    token = str(value or "").strip()
    if not token or any(ch.isspace() for ch in token):
        raise WorkflowError("Import-Token 不能为空，也不能包含空白字符。")
    return token


def bindings_file_from_env() -> Path:
    return Path(os.environ.get("MAIMAI_IMPORT_TOKEN_BINDINGS_FILE") or DEFAULT_BINDINGS_FILE).expanduser()


def output_dir_from_env() -> Path:
    return Path(os.environ.get("MAIMAI_UPDATE_RECORDS_OUTPUT_DIR") or DEFAULT_OUTPUT_DIR).expanduser()


def load_bindings(path: Path | None = None) -> dict[str, Any]:
    target = path or bindings_file_from_env()
    if not target.exists():
        return {"version": 1, "bindings": {}}
    try:
        data = json.loads(target.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise WorkflowError(f"读取 Import-Token 绑定文件失败：{exc}") from exc
    if not isinstance(data, dict):
        raise WorkflowError("Import-Token 绑定文件格式不是对象。")
    bindings = data.setdefault("bindings", {})
    if not isinstance(bindings, dict):
        raise WorkflowError("Import-Token 绑定文件 bindings 字段格式错误。")
    data.setdefault("version", 1)
    return data


def save_bindings(data: dict[str, Any], path: Path | None = None) -> None:
    target = path or bindings_file_from_env()
    target.parent.mkdir(parents=True, exist_ok=True)
    try:
        target.parent.chmod(0o700)
    except OSError:
        pass
    temp = target.with_suffix(target.suffix + ".tmp")
    temp.write_text(json.dumps(data, ensure_ascii=False, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    try:
        temp.chmod(0o600)
    except OSError:
        pass
    temp.replace(target)
    try:
        target.chmod(0o600)
    except OSError:
        pass


def bind_import_token(qq: Any, import_token: Any, *, path: Path | None = None) -> dict[str, Any]:
    qq_text = normalize_qq(qq)
    token = normalize_token(import_token)
    data = load_bindings(path)
    bindings = data["bindings"]
    now = utc_now()
    old = bindings.get(qq_text) if isinstance(bindings.get(qq_text), dict) else {}
    bindings[qq_text] = {
        "importToken": token,
        "boundAt": old.get("boundAt") or now,
        "updatedAt": now,
    }
    save_bindings(data, path)
    return {
        "ok": True,
        "qq": qq_text,
        "tokenPreview": mask_secret(token),
        "bindingsFile": str((path or bindings_file_from_env()).resolve()),
        "text": f"已绑定 QQ {qq_text} 的水鱼成绩导入 token（{mask_secret(token)}）。",
    }


def get_import_token(qq: Any, *, path: Path | None = None) -> str:
    qq_text = normalize_qq(qq)
    data = load_bindings(path)
    record = data.get("bindings", {}).get(qq_text)
    if not isinstance(record, dict):
        raise WorkflowError("还没有绑定水鱼成绩导入 token。请先发送 mai bind <水鱼成绩导入token>。")
    return normalize_token(record.get("importToken"))


def run_command(
    command: list[str],
    *,
    input_text: str | None = None,
    timeout: float = 180.0,
    cwd: Path = REPO_ROOT,
    runner: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run,
) -> CommandResult:
    completed = runner(
        command,
        input=input_text,
        text=True,
        cwd=str(cwd),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        check=False,
    )
    return CommandResult(
        returncode=int(completed.returncode),
        stdout=str(completed.stdout or ""),
        stderr=str(completed.stderr or ""),
    )


def parse_key_value_lines(text: str) -> dict[str, str]:
    values: dict[str, str] = {}
    for raw in text.splitlines():
        if "=" not in raw:
            continue
        key, value = raw.split("=", 1)
        values[key.strip()] = value.strip()
    return values


def make_run_dir(base: Path, qq: str) -> Path:
    stamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    path = base / f"{qq}_{stamp}_{os.getpid()}"
    path.mkdir(parents=True, exist_ok=True)
    return path


def run_raw_dump(
    *,
    qr_content: str,
    run_dir: Path,
    keyship: str | None = None,
    logoutid: int | None = None,
    title_ver: str | None = None,
    timeout: float = 180.0,
    python: str = sys.executable,
    raw_script: Path = DEFAULT_RAW_DUMP_SCRIPT,
    runner: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run,
) -> tuple[Path, dict[str, str], CommandResult]:
    command = [python, str(raw_script), "--output-dir", str(run_dir / "raw")]
    if keyship:
        command.extend(["--keyship-id", keyship])
    if logoutid is not None:
        if logoutid not in (1, 2):
            raise WorkflowError("--logoutid 只能是 1 或 2。")
        command.extend(["--logout-type", str(logoutid)])
    if title_ver:
        command.extend(["--title-ver", title_ver])

    result = run_command(command, input_text=qr_content.strip() + "\n", timeout=timeout, runner=runner)
    values = parse_key_value_lines(result.stdout)
    raw_path_text = values.get("full_json_path")
    raw_path = Path(raw_path_text).expanduser() if raw_path_text else Path()
    if result.returncode != 0 or values.get("flow_success") != "true" or not raw_path.is_file():
        detail = values.get("flow_error") or result.stderr.strip() or "raw dump 未成功生成 JSON。"
        raise WorkflowError(f"获取原始成绩失败：{detail}")
    return raw_path, values, result


def run_convert(
    *,
    raw_json: Path,
    run_dir: Path,
    timeout: float = 60.0,
    python: str = sys.executable,
    convert_script: Path = DEFAULT_CONVERT_SCRIPT,
    runner: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run,
) -> tuple[Path, Path, list[dict[str, Any]], dict[str, Any]]:
    output_path = run_dir / "update_records.json"
    report_path = run_dir / "update_records_report.json"
    command = [
        python,
        str(convert_script),
        str(raw_json),
        "-o",
        str(output_path),
        "--report",
        str(report_path),
        "--pretty",
    ]
    result = run_command(command, timeout=timeout, runner=runner)
    if result.returncode != 0 or not output_path.is_file() or not report_path.is_file():
        detail = result.stderr.strip() or result.stdout.strip() or "转换脚本未生成输出。"
        raise WorkflowError(f"转换成绩失败：{detail}")
    payload = json.loads(output_path.read_text(encoding="utf-8"))
    report = json.loads(report_path.read_text(encoding="utf-8"))
    if not isinstance(payload, list) or not isinstance(report, dict):
        raise WorkflowError("转换输出格式异常。")
    return output_path, report_path, payload, report


def upload_update_records(
    *,
    import_token: str,
    payload: list[dict[str, Any]],
    api_url: str = DEFAULT_DIVING_FISH_URL,
    timeout: float = 60.0,
    post: Callable[..., Any] = requests.post,
) -> dict[str, Any]:
    response = post(
        api_url,
        headers={
            "Accept": "application/json",
            "Content-Type": "application/json",
            "Import-Token": import_token,
            "User-Agent": "maimai-update-records-workflow/0.1.0",
        },
        json=payload,
        timeout=timeout,
    )
    text = str(getattr(response, "text", "") or "")
    status_code = int(getattr(response, "status_code", 0) or 0)
    ok = bool(getattr(response, "ok", False))
    data: Any = None
    try:
        data = response.json()
    except Exception:
        data = text
    if not ok:
        raise WorkflowError(f"上传水鱼失败：HTTP {status_code} {text[:500]}")
    return {"statusCode": status_code, "data": data}


def update_records_workflow(
    *,
    qq: Any,
    qr_content: Any,
    keyship: str | None = None,
    logoutid: int | None = None,
    title_ver: str | None = None,
    timeout: float = 240.0,
    bindings_file: Path | None = None,
    output_dir: Path | None = None,
    python: str = sys.executable,
    raw_script: Path = DEFAULT_RAW_DUMP_SCRIPT,
    convert_script: Path = DEFAULT_CONVERT_SCRIPT,
    runner: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run,
    post: Callable[..., Any] = requests.post,
) -> dict[str, Any]:
    qq_text = normalize_qq(qq)
    qr_text = str(qr_content or "").strip()
    if not qr_text:
        raise WorkflowError("二维码解析内容不能为空。")
    import_token = get_import_token(qq_text, path=bindings_file)
    run_dir = make_run_dir((output_dir or output_dir_from_env()), qq_text)

    raw_json, raw_values, _ = run_raw_dump(
        qr_content=qr_text,
        run_dir=run_dir,
        keyship=keyship,
        logoutid=logoutid,
        title_ver=title_ver,
        timeout=timeout,
        python=python,
        raw_script=raw_script,
        runner=runner,
    )
    payload_path, report_path, payload, report = run_convert(
        raw_json=raw_json,
        run_dir=run_dir,
        timeout=min(max(timeout / 4, 30), 120),
        python=python,
        convert_script=convert_script,
        runner=runner,
    )
    upload = upload_update_records(
        import_token=import_token,
        payload=payload,
        timeout=min(max(timeout / 4, 30), 120),
        post=post,
    )
    skipped = int(report.get("skipped") or 0)
    converted = len(payload)
    user_id = raw_values.get("user_id") or raw_values.get("userId") or ""
    text = f"成绩上传完成：转换 {converted} 条，跳过 {skipped} 条。"
    if user_id:
        text += f"\nSEGA userId: {user_id}"
    if skipped:
        text += "\n有跳过记录，详情见转换 report。"
    return {
        "ok": True,
        "qq": qq_text,
        "segaUserId": user_id,
        "converted": converted,
        "skipped": skipped,
        "rawJsonPath": str(raw_json),
        "payloadPath": str(payload_path),
        "reportPath": str(report_path),
        "upload": upload,
        "text": text,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Bind token and upload maimai records from SDGB155 QR raw dump.")
    sub = parser.add_subparsers(dest="command", required=True)

    bind = sub.add_parser("bind", help="Bind QQ to Diving-Fish Import-Token.")
    bind.add_argument("--qq", required=True)
    bind.add_argument("--token", required=True)
    bind.add_argument("--bindings-file", type=Path)

    update = sub.add_parser("update", help="QR login, dump records, convert and upload to Diving-Fish.")
    update.add_argument("--qq", required=True)
    update.add_argument("--qr-content", required=True)
    update.add_argument("--keyship")
    update.add_argument("--logoutid", type=int)
    update.add_argument("--title-ver")
    update.add_argument("--timeout", type=float, default=240.0)
    update.add_argument("--bindings-file", type=Path)
    update.add_argument("--output-dir", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.command == "bind":
            result = bind_import_token(args.qq, args.token, path=args.bindings_file)
        else:
            result = update_records_workflow(
                qq=args.qq,
                qr_content=args.qr_content,
                keyship=args.keyship,
                logoutid=args.logoutid,
                title_ver=args.title_ver,
                timeout=args.timeout,
                bindings_file=args.bindings_file,
                output_dir=args.output_dir,
            )
    except WorkflowError as exc:
        print(json.dumps({"ok": False, "error": str(exc)}, ensure_ascii=False))
        return 1
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
