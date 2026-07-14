"""下载 OpenCC TSCharacters 繁→简单字表 → data/zh_t2s.json。

来源是 OpenCC 官方 TSCharacters.txt，格式为 TSV（傳統漢字\t简体汉字）。
用标准 urllib 拉取，失败时保留旧文件；成功时原子替换。
"""

from __future__ import annotations

import json
import os
import tempfile
import urllib.request
from pathlib import Path


URL = "https://raw.githubusercontent.com/BYVoid/OpenCC/master/data/dictionary/TSCharacters.txt"
ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "data" / "zh_t2s.json"
DEFAULT_TIMEOUT_SECONDS = 15.0


def http_timeout_seconds() -> float:
    configured = os.environ.get("MAIMAI_OPENCC_TIMEOUT_SECONDS") or os.environ.get(
        "MAIMAI_SOURCE_HTTP_TIMEOUT_SECONDS"
    )
    if configured in (None, ""):
        return DEFAULT_TIMEOUT_SECONDS
    return max(1.0, float(configured))


def download_t2s_table(url: str = URL, target: Path = TARGET) -> int:
    target.parent.mkdir(parents=True, exist_ok=True)
    req = urllib.request.Request(url, headers={"User-Agent": "maimai-bot opencc-t2s-updater"})
    with urllib.request.urlopen(req, timeout=http_timeout_seconds()) as response:
        lines = response.read().decode("utf-8").splitlines()

    mapping: dict[str, str] = {}
    for line in lines:
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) >= 2:
            trad, simp = parts[0], parts[1]
            if len(trad) == 1 and len(simp) == 1 and trad != simp:
                mapping[trad] = simp

    if not mapping:
        raise RuntimeError(f"No valid T→S entries parsed from {url}")

    with tempfile.NamedTemporaryFile("w", encoding="utf-8", delete=False, dir=target.parent) as tmp:
        json.dump(mapping, tmp, ensure_ascii=False)
        tmp_path = Path(tmp.name)
    tmp_path.replace(target)
    return len(mapping)


def main() -> None:
    count = download_t2s_table()
    print(f"Downloaded {count} T→S character mappings to {TARGET}")


if __name__ == "__main__":
    main()
