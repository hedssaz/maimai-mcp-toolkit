from __future__ import annotations

import argparse
import json
import mimetypes
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any
from urllib.parse import unquote
from urllib.parse import parse_qs, urlparse

from .search import (
    SearchError,
    list_songs_by_id,
    list_versions,
    random_songs,
    save_custom_alias,
    search_songs,
)
from .source_refresh import RefreshError, refresh_sources


PACKAGE_ROOT = Path(__file__).resolve().parent.parent
STATIC_ROOT = PACKAGE_ROOT / "web"


def parse_limit(value: Any) -> int | None:
    if value in (None, ""):
        return None
    return int(value)


def parse_bool(value: Any) -> bool:
    if isinstance(value, bool):
        return value
    if value in (None, ""):
        return False
    text = str(value).strip().lower()
    return text in {"1", "true", "yes", "y", "on"}


def parse_args_from_query(query: str) -> dict[str, Any]:
    params = parse_qs(query, keep_blank_values=False)
    data: dict[str, Any] = {}
    for key in (
        "query",
        "level",
        "genre",
        "version",
        "ds",
        "fit_diff",
        "fit_delta",
        "fit_label",
        "region_has",
        "region_missing",
        "difficulty",
        "song_type",
        "artist",
        "charter",
        "tag",
        "tag_exclude",
        "released_after",
        "released_before",
        "sort",
        "order",
        "seed",
        "sources",
        "source",
    ):
        if key in params:
            data[key] = params[key][0]
    for key in (
        "ds_min",
        "ds_max",
        "fit_diff_min",
        "fit_diff_max",
        "fit_delta_min",
        "fit_delta_max",
    ):
        if key in params:
            data[key] = float(params[key][0])
    if "limit" in params:
        data["limit"] = parse_limit(params["limit"][0])
    if "count" in params:
        data["count"] = parse_limit(params["count"][0])
    if "force" in params:
        data["force"] = parse_bool(params["force"][0])
    if "check_only" in params:
        data["check_only"] = parse_bool(params["check_only"][0])
    if "ttl_days" in params:
        data["ttl_days"] = float(params["ttl_days"][0])
    if "timeout_seconds" in params:
        data["timeout_seconds"] = int(params["timeout_seconds"][0])
    return data


class MaimaiRequestHandler(BaseHTTPRequestHandler):
    server_version = "maimai-local-search/0.1"

    def log_message(self, fmt: str, *args: Any) -> None:
        return

    def send_json(self, status: HTTPStatus, payload: dict[str, Any]) -> None:
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Headers", "content-type")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.end_headers()
        self.wfile.write(body)

    def send_static(self, requested_path: str) -> bool:
        if requested_path in {"", "/"}:
            relative_path = "index.html"
        else:
            relative_path = unquote(requested_path.lstrip("/"))

        file_path = (STATIC_ROOT / relative_path).resolve()
        if STATIC_ROOT.resolve() not in file_path.parents and file_path != STATIC_ROOT.resolve():
            self.send_json(HTTPStatus.NOT_FOUND, {"error": "not_found"})
            return True
        if not file_path.is_file():
            return False

        body = file_path.read_bytes()
        content_type = mimetypes.guess_type(file_path.name)[0] or "application/octet-stream"
        if file_path.suffix == ".js":
            content_type = "text/javascript"
        self.send_response(HTTPStatus.OK)
        self.send_header("Content-Type", f"{content_type}; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)
        return True

    def do_OPTIONS(self) -> None:
        self.send_json(HTTPStatus.NO_CONTENT, {})

    def do_GET(self) -> None:
        parsed = urlparse(self.path)
        if parsed.path == "/health":
            self.send_json(HTTPStatus.OK, {"ok": True})
            return
        if parsed.path == "/search":
            self.handle_search(parse_args_from_query(parsed.query))
            return
        if parsed.path == "/random":
            self.handle_random(parse_args_from_query(parsed.query))
            return
        if parsed.path == "/songs-by-id":
            self.handle_songs_by_id(parse_args_from_query(parsed.query))
            return
        if parsed.path == "/versions":
            self.handle_versions(parse_args_from_query(parsed.query))
            return
        if parsed.path == "/source-status":
            arguments = parse_args_from_query(parsed.query)
            arguments["check_only"] = True
            self.handle_refresh_sources(arguments)
            return
        if self.send_static(parsed.path):
            return
        self.send_json(HTTPStatus.NOT_FOUND, {"error": "not_found"})

    def do_POST(self) -> None:
        parsed = urlparse(self.path)
        if parsed.path not in {"/search", "/random", "/songs-by-id", "/alias", "/refresh-sources"}:
            self.send_json(HTTPStatus.NOT_FOUND, {"error": "not_found"})
            return

        length = int(self.headers.get("Content-Length", "0") or 0)
        body = self.rfile.read(length) if length else b"{}"
        try:
            payload = json.loads(body.decode("utf-8"))
            if not isinstance(payload, dict):
                raise ValueError("JSON body must be an object")
        except ValueError as exc:
            self.send_json(HTTPStatus.BAD_REQUEST, {"error": str(exc)})
            return
        if parsed.path == "/search":
            self.handle_search(payload)
            return
        if parsed.path == "/random":
            self.handle_random(payload)
            return
        if parsed.path == "/songs-by-id":
            self.handle_songs_by_id(payload)
            return
        if parsed.path == "/refresh-sources":
            self.handle_refresh_sources(payload)
            return
        self.handle_alias(payload)

    def handle_search(self, arguments: dict[str, Any]) -> None:
        try:
            result = search_songs(
                query=arguments.get("query"),
                level=arguments.get("level"),
                genre=arguments.get("genre"),
                version=arguments.get("version"),
                ds=arguments.get("ds"),
                ds_min=arguments.get("ds_min"),
                ds_max=arguments.get("ds_max"),
                fit_diff=arguments.get("fit_diff"),
                fit_diff_min=arguments.get("fit_diff_min"),
                fit_diff_max=arguments.get("fit_diff_max"),
                fit_delta=arguments.get("fit_delta"),
                fit_delta_min=arguments.get("fit_delta_min"),
                fit_delta_max=arguments.get("fit_delta_max"),
                fit_label=arguments.get("fit_label"),
                region_has=arguments.get("region_has"),
                region_missing=arguments.get("region_missing"),
                difficulty=arguments.get("difficulty"),
                song_type=arguments.get("song_type"),
                artist=arguments.get("artist"),
                charter=arguments.get("charter"),
                tag=arguments.get("tag"),
                tag_exclude=arguments.get("tag_exclude"),
                released_after=arguments.get("released_after"),
                released_before=arguments.get("released_before"),
                sort=arguments.get("sort"),
                limit=parse_limit(arguments.get("limit")),
            )
        except (SearchError, ValueError) as exc:
            self.send_json(HTTPStatus.BAD_REQUEST, {"error": str(exc)})
            return
        self.send_json(HTTPStatus.OK, result)

    def handle_random(self, arguments: dict[str, Any]) -> None:
        try:
            result = random_songs(
                count=arguments.get("count"),
                level=arguments.get("level"),
                genre=arguments.get("genre"),
                version=arguments.get("version"),
                ds=arguments.get("ds"),
                ds_min=arguments.get("ds_min"),
                ds_max=arguments.get("ds_max"),
                fit_diff=arguments.get("fit_diff"),
                fit_diff_min=arguments.get("fit_diff_min"),
                fit_diff_max=arguments.get("fit_diff_max"),
                fit_delta=arguments.get("fit_delta"),
                fit_delta_min=arguments.get("fit_delta_min"),
                fit_delta_max=arguments.get("fit_delta_max"),
                fit_label=arguments.get("fit_label"),
                region_has=arguments.get("region_has"),
                region_missing=arguments.get("region_missing"),
                difficulty=arguments.get("difficulty"),
                song_type=arguments.get("song_type"),
                artist=arguments.get("artist"),
                charter=arguments.get("charter"),
                tag=arguments.get("tag"),
                tag_exclude=arguments.get("tag_exclude"),
                released_after=arguments.get("released_after"),
                released_before=arguments.get("released_before"),
                sort=arguments.get("sort"),
                seed=arguments.get("seed"),
            )
        except (SearchError, ValueError) as exc:
            self.send_json(HTTPStatus.BAD_REQUEST, {"error": str(exc)})
            return
        self.send_json(HTTPStatus.OK, result)

    def handle_songs_by_id(self, arguments: dict[str, Any]) -> None:
        try:
            result = list_songs_by_id(
                order=arguments.get("order", "asc"),
                limit=parse_limit(arguments.get("limit")),
                level=arguments.get("level"),
                genre=arguments.get("genre"),
                version=arguments.get("version"),
                ds=arguments.get("ds"),
                ds_min=arguments.get("ds_min"),
                ds_max=arguments.get("ds_max"),
                fit_diff=arguments.get("fit_diff"),
                fit_diff_min=arguments.get("fit_diff_min"),
                fit_diff_max=arguments.get("fit_diff_max"),
                fit_delta=arguments.get("fit_delta"),
                fit_delta_min=arguments.get("fit_delta_min"),
                fit_delta_max=arguments.get("fit_delta_max"),
                fit_label=arguments.get("fit_label"),
                region_has=arguments.get("region_has"),
                region_missing=arguments.get("region_missing"),
                difficulty=arguments.get("difficulty"),
                song_type=arguments.get("song_type"),
                artist=arguments.get("artist"),
                charter=arguments.get("charter"),
                tag=arguments.get("tag"),
                tag_exclude=arguments.get("tag_exclude"),
                released_after=arguments.get("released_after"),
                released_before=arguments.get("released_before"),
                sort=arguments.get("sort"),
            )
        except (SearchError, ValueError) as exc:
            self.send_json(HTTPStatus.BAD_REQUEST, {"error": str(exc)})
            return
        self.send_json(HTTPStatus.OK, result)

    def handle_versions(self, arguments: dict[str, Any]) -> None:
        try:
            result = list_versions(
                query=arguments.get("query"),
                limit=parse_limit(arguments.get("limit")),
            )
        except (SearchError, ValueError) as exc:
            self.send_json(HTTPStatus.BAD_REQUEST, {"error": str(exc)})
            return
        self.send_json(HTTPStatus.OK, result)

    def handle_alias(self, arguments: dict[str, Any]) -> None:
        try:
            result = save_custom_alias(
                song_id=arguments.get("song_id"),
                title=arguments.get("title"),
                alias=arguments.get("alias", ""),
            )
        except (SearchError, ValueError) as exc:
            self.send_json(HTTPStatus.BAD_REQUEST, {"error": str(exc)})
            return
        self.send_json(HTTPStatus.OK, result)

    def handle_refresh_sources(self, arguments: dict[str, Any]) -> None:
        try:
            result = refresh_sources(arguments)
        except (RefreshError, ValueError) as exc:
            self.send_json(HTTPStatus.BAD_REQUEST, {"error": str(exc)})
            return
        self.send_json(HTTPStatus.OK, result)


def main() -> None:
    parser = argparse.ArgumentParser(description="Run maimai local search HTTP server.")
    parser.add_argument("--host", default="0.0.0.0")
    parser.add_argument("--port", default=8000, type=int)
    args = parser.parse_args()

    server = ThreadingHTTPServer((args.host, args.port), MaimaiRequestHandler)
    print(f"maimai-local-search listening on http://{args.host}:{args.port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
