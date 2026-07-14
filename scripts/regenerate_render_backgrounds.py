"""Regenerate maimaidx static render backgrounds.

This rewrites static rating and plate templates after source data changes.
"""

from __future__ import annotations

import asyncio
import json
import sys
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))


def reset_render_data_cache() -> None:
    try:
        from maimai_mcp.search import clear_search_caches

        clear_search_caches()
    except Exception:
        pass

    from maimaidx_render_mcp.maimaidx import mai

    mai._loaded = False
    mai._ensure_loaded()


async def regenerate() -> dict:
    reset_render_data_cache()

    from maimaidx_render_mcp.maimaidx.maimaidx_update_table import (
        update_plate_table,
        update_rating_table,
    )

    rating_result = await update_rating_table()
    plate_result = await update_plate_table()
    ok = "失败" not in str(rating_result) and "失败" not in str(plate_result)
    return {
        "ok": ok,
        "rating": rating_result,
        "plate": plate_result,
    }


def main() -> None:
    started_at = datetime.now(timezone.utc).isoformat()
    result = asyncio.run(regenerate())
    result["startedAt"] = started_at
    result["finishedAt"] = datetime.now(timezone.utc).isoformat()
    print(json.dumps(result, ensure_ascii=False), flush=True)
    sys.exit(0 if result["ok"] else 1)


if __name__ == "__main__":
    main()
