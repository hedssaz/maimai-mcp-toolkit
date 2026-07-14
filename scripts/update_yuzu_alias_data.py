from __future__ import annotations

import json
import os
import tempfile
import urllib.request
from pathlib import Path


URL = "https://www.yuzuchan.moe/api/maimaidx/maimaidxalias"
ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "data" / "music_alias.json"
DEFAULT_TIMEOUT_SECONDS = 5.0


def http_timeout_seconds() -> float:
    configured = os.environ.get("MAIMAI_YUZU_TIMEOUT_SECONDS") or os.environ.get(
        "MAIMAI_SOURCE_HTTP_TIMEOUT_SECONDS"
    )
    if configured in (None, ""):
        return DEFAULT_TIMEOUT_SECONDS
    return max(1.0, float(configured))


def extract_alias_list(data: object) -> list[object]:
    if isinstance(data, list):
        return data
    if isinstance(data, dict):
        content = data.get("content", data.get("aliases"))
        if isinstance(content, list):
            return content
    raise RuntimeError(f"Downloaded JSON from {URL} does not contain a supported alias list")


def download_yuzu_aliases(url: str = URL, target: Path = TARGET) -> int:
    target.parent.mkdir(parents=True, exist_ok=True)
    req = urllib.request.Request(url, headers={"User-Agent": "maimai-bot yuzu-alias-updater"})
    with urllib.request.urlopen(req, timeout=http_timeout_seconds()) as response:
        payload = response.read()

    data = json.loads(payload.decode("utf-8"))
    aliases = extract_alias_list(data)

    with tempfile.NamedTemporaryFile("wb", delete=False, dir=target.parent) as tmp:
        tmp.write(payload)
        tmp_path = Path(tmp.name)
    tmp_path.replace(target)
    return len(aliases)


def main() -> None:
    alias_count = download_yuzu_aliases()
    print(f"Downloaded {alias_count} Yuzu alias records to {TARGET}")


if __name__ == "__main__":
    main()
