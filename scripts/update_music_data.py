from __future__ import annotations

import json
import tempfile
import urllib.request
from pathlib import Path


URL = "https://maimai.lxns.net/api/v0/maimai/song/list?notes=true"
ALIAS_URL = "https://maimai.lxns.net/api/v0/maimai/alias/list"
ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "data" / "lxns_song_list.json"
ALIAS_TARGET = ROOT / "data" / "lxns_alias_list.json"


def extract_record_list(data: object, url: str) -> list[object]:
    if isinstance(data, list):
        return data
    if isinstance(data, dict):
        for key in ("content", "songs", "aliases"):
            content = data.get(key)
            if isinstance(content, list):
                return content
    raise RuntimeError(f"Downloaded JSON from {url} does not contain a supported list")


def download_json(url: str, target: Path) -> int:
    target.parent.mkdir(parents=True, exist_ok=True)
    with urllib.request.urlopen(url, timeout=30) as response:
        payload = response.read()

    data = json.loads(payload.decode("utf-8"))
    content = extract_record_list(data, url)

    with tempfile.NamedTemporaryFile("wb", delete=False, dir=target.parent) as tmp:
        tmp.write(payload)
        tmp_path = Path(tmp.name)
    tmp_path.replace(target)
    return len(content)


def main() -> None:
    song_count = download_json(URL, TARGET)
    alias_count = download_json(ALIAS_URL, ALIAS_TARGET)
    print(f"Downloaded {song_count} songs to {TARGET}")
    print(f"Downloaded {alias_count} alias records to {ALIAS_TARGET}")


if __name__ == "__main__":
    main()
