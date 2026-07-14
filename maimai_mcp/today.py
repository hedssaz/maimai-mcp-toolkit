from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from typing import Any
from zoneinfo import ZoneInfo


ACTIVITIES = [
    "拼机",
    "推分",
    "越级",
    "下埋",
    "夜勤",
    "练底力",
    "练手法",
    "打旧框",
    "干饭",
    "抓绝赞",
    "收歌",
]


@dataclass(frozen=True)
class Song:
    id: str
    title: str
    ds: tuple[str, ...]


def qqhash(qq: int | str, now: datetime | None = None, *, offset: int | str = 0) -> int:
    if now is None:
        now = datetime.now(ZoneInfo("Asia/Shanghai"))
    days = now.day + 31 * now.month + 77 + int(offset or 0)
    return (days * int(qq)) >> 8


def today_maimai(
    qq: int | str,
    songs: list[Song],
    now: datetime | None = None,
    *,
    offset: int | str = 0,
) -> dict[str, Any]:
    if not songs:
        raise ValueError("songs 不能为空")

    h = qqhash(qq, now, offset=offset)
    rp = h % 100

    good: list[str] = []
    bad: list[str] = []
    for activity in ACTIVITIES:
        value = h & 3
        if value == 3:
            good.append(activity)
        elif value == 0:
            bad.append(activity)
        h >>= 2

    song = songs[h % len(songs)]
    return {
        "rp": rp,
        "good": good,
        "bad": bad,
        "song": song,
        "offset": int(offset or 0),
    }


def format_today_maimai(
    bot_name: str,
    qq: int | str,
    songs: list[Song],
    now: datetime | None = None,
    *,
    offset: int | str = 0,
) -> str:
    result = today_maimai(qq, songs, now, offset=offset)
    song: Song = result["song"]

    lines = [f"今日人品值：{result['rp']}"]
    for item in result["good"]:
        lines.append(f"宜 {item}")
    for item in result["bad"]:
        lines.append(f"忌 {item}")

    lines.append(f"{bot_name}提醒您：以上内容均由程序自动生成，仅供娱乐参考")
    lines.append("今日推荐歌曲：")
    lines.append(f"ID.{song.id} - {song.title}")
    lines.append("/".join(song.ds))
    return "\n".join(lines)


def song_from_search_result(song: dict[str, Any]) -> Song | None:
    title = str(song.get("title") or "").strip()
    if not title:
        return None
    song_id = _numeric_song_id(song)
    if not song_id:
        return None
    return Song(
        id=song_id,
        title=title,
        ds=tuple(_chart_ds_values(song)),
    )


def _numeric_song_id(song: dict[str, Any]) -> str:
    candidates: list[Any] = [song.get("id"), song.get("source_id")]
    source_ids = song.get("source_ids")
    if isinstance(source_ids, dict):
        for key in ("divingfish", "cn"):
            candidates.append(source_ids.get(key))
    for candidate in candidates:
        value = str(candidate or "").strip()
        if value.isdigit():
            return value
    return ""


def _chart_ds_values(song: dict[str, Any]) -> list[str]:
    values: list[str] = []
    seen: set[tuple[str, int]] = set()
    charts = song.get("matched_charts")
    if isinstance(charts, list) and charts:
        for chart in charts:
            if not isinstance(chart, dict):
                continue
            key = (str(chart.get("chart_type") or ""), _int_value(chart.get("difficulty_index")))
            if key in seen:
                continue
            seen.add(key)
            ds = chart.get("ds")
            if ds not in (None, ""):
                values.append(str(ds))
    if values:
        return values

    raw_ds = song.get("ds")
    if isinstance(raw_ds, list):
        return [str(value) for value in raw_ds if value not in (None, "")]
    return []


def _int_value(value: Any) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return 0
