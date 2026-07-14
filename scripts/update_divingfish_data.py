from __future__ import annotations

import json
import tempfile
import urllib.request
from pathlib import Path


URL = "https://www.diving-fish.com/api/maimaidxprober/music_data"
ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "data" / "divingfish_song_list.json"
ETAG_FILE = ROOT / "data" / ".divingfish_etag"


def load_etag() -> str | None:
    if ETAG_FILE.exists():
        return ETAG_FILE.read_text(encoding="utf-8").strip().strip('"')
    return None


def save_etag(etag: str) -> None:
    ETAG_FILE.write_text(etag.strip('"'), encoding="utf-8")


def download_json() -> tuple[int, bool]:
    """Download DivingFish music data. Returns (count, updated)."""
    headers: dict[str, str] = {}
    etag = load_etag()
    if etag:
        headers["If-None-Match"] = f'"{etag}"'

    req = urllib.request.Request(URL, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            payload = response.read()
            new_etag = response.headers.get("etag", "").strip().strip('"')
    except urllib.error.HTTPError as exc:
        if exc.code == 304:
            if not TARGET.exists():
                raise RuntimeError("DivingFish returned 304 Not Modified but local data file is missing")
            TARGET.touch()
            print("DivingFish data is up to date (304 Not Modified)")
            return 0, False
        raise

    data = json.loads(payload.decode("utf-8"))
    if not isinstance(data, list):
        raise RuntimeError(f"DivingFish API returned non-list: {type(data)}")

    TARGET.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile("wb", delete=False, dir=TARGET.parent) as tmp:
        tmp.write(payload)
        tmp_path = Path(tmp.name)
    tmp_path.replace(TARGET)

    if new_etag:
        save_etag(new_etag)

    return len(data), True


def main() -> None:
    count, updated = download_json()
    if updated:
        print(f"Downloaded {count} DivingFish songs to {TARGET}")
    else:
        print("No update needed")


if __name__ == "__main__":
    main()
