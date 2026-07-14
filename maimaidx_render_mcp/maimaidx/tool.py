import asyncio
import json
import os
import shutil
import time
from pathlib import Path
from typing import Any, Union

from . import SNAPSHOT_JS, pie_html_file


def qqhash(qq: int):
    days = int(time.strftime("%d", time.localtime(time.time()))) + 31 * int(
        time.strftime("%m", time.localtime(time.time()))) + 77
    return (days * qq) >> 8


async def openfile(file: Path) -> Union[dict, list]:
    data = json.loads(file.read_text(encoding='utf-8'))
    return data


async def writefile(file: Path, data: Any) -> bool:
    file.write_text(json.dumps(data, ensure_ascii=False, indent=4), encoding='utf-8')
    return True


async def run_chrome_to_base64() -> str:
    try:
        from playwright.async_api import async_playwright
    except ImportError:
        return ""
    async with async_playwright() as p:
        launch_args = {
            "headless": True,
            "args": ["--no-sandbox", "--disable-dev-shm-usage"],
        }
        executable_path = _chromium_executable_path()
        if executable_path:
            launch_args["executable_path"] = executable_path
        browers = await p.chromium.launch(**launch_args)
        page = await browers.new_page(java_script_enabled=True)
        await page.goto('file://' + str(pie_html_file))
        await asyncio.sleep(2)

        content: str = await page.evaluate(SNAPSHOT_JS)
        await browers.close()

    content_array = content.split(',')
    if len(content_array) != 2:
        raise OSError(content_array)

    return 'base64://' + content_array[-1]


def _chromium_executable_path() -> str | None:
    configured = os.environ.get("MAIMAIDX_CHROMIUM_EXECUTABLE") or os.environ.get(
        "PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH"
    )
    if configured and Path(configured).exists():
        return configured

    for command in ("chromium", "chromium-browser", "google-chrome", "google-chrome-stable"):
        found = shutil.which(command)
        if found:
            return found

    for candidate in (
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/usr/bin/google-chrome",
        "/usr/bin/google-chrome-stable",
    ):
        if Path(candidate).exists():
            return candidate
    return None
