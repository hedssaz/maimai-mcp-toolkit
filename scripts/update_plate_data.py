"""下载 CN plate 数据（maimaidxplate.json）。

数据源: https://www.yuzuchan.moe/api/maimaidx/maimaidxplate
用途: 牌子进度查询时按版本匹配歌曲 ID 白名单。

注意: 当前分支仅支持国服（CN）牌子数据。
"""

from __future__ import annotations

import json
import os
import tempfile
import urllib.request
from pathlib import Path


URL = "https://www.yuzuchan.moe/api/maimaidx/maimaidxplate"
ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "data" / "maimaidxplate.json"
DEFAULT_TIMEOUT_SECONDS = 10.0


def http_timeout_seconds() -> float:
    configured = os.environ.get("MAIMAI_PLATE_TIMEOUT_SECONDS") or os.environ.get(
        "MAIMAI_SOURCE_HTTP_TIMEOUT_SECONDS"
    )
    if configured in (None, ""):
        return DEFAULT_TIMEOUT_SECONDS
    return max(1.0, float(configured))


def download_plate_data(url: str = URL, target: Path = TARGET) -> int:
    target.parent.mkdir(parents=True, exist_ok=True)
    req = urllib.request.Request(url, headers={"User-Agent": "maimai-bot plate-updater"})
    with urllib.request.urlopen(req, timeout=http_timeout_seconds()) as response:
        payload = response.read()

    data = json.loads(payload.decode("utf-8"))
    content = data.get("content", data)
    if not isinstance(content, dict):
        raise RuntimeError(f"Plate data from {URL} has unexpected format")

    # 写入 API 返回的完整 JSON（保留 code + content 结构）
    target.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile("wb", delete=False, dir=target.parent) as tmp:
        tmp.write(payload)
        tmp_path = Path(tmp.name)
    tmp_path.replace(target)

    plate_count = len(content)
    song_counts = [(k, len(v)) for k, v in content.items()]
    for name, count in sorted(song_counts):
        print(f"  {name}: {count} songs")
    return plate_count


def main() -> None:
    plate_count = download_plate_data()
    print(f"\nDownloaded {plate_count} plate entries to {TARGET}")


if __name__ == "__main__":
    main()
