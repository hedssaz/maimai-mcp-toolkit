from __future__ import annotations

import json
import tempfile
import urllib.request
from pathlib import Path


URL = "https://www.diving-fish.com/api/maimaidxprober/chart_stats"
ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "data" / "divingfish_chart_stats.json"


def download_chart_stats(url: str = URL, target: Path = TARGET) -> tuple[int, int]:
    target.parent.mkdir(parents=True, exist_ok=True)
    with urllib.request.urlopen(url, timeout=30) as response:
        payload = response.read()

    data = json.loads(payload.decode("utf-8"))
    charts = data.get("charts") if isinstance(data, dict) else None
    diff_data = data.get("diff_data") if isinstance(data, dict) else None
    if not isinstance(charts, dict):
        raise RuntimeError(f"Downloaded JSON from {url} does not contain charts")
    if diff_data is not None and not isinstance(diff_data, dict):
        raise RuntimeError(f"Downloaded JSON from {url} has invalid diff_data")

    with tempfile.NamedTemporaryFile("wb", delete=False, dir=target.parent) as tmp:
        tmp.write(payload)
        tmp_path = Path(tmp.name)
    tmp_path.replace(target)
    return len(charts), len(diff_data or {})


def main() -> None:
    chart_count, diff_bucket_count = download_chart_stats()
    print(f"Downloaded chart stats for {chart_count} song IDs to {TARGET}")
    print(f"Downloaded {diff_bucket_count} difficulty buckets")


if __name__ == "__main__":
    main()
