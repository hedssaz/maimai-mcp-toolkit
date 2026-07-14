"""maimaiDX 兼容层：替换 maimaidx_music.MaiMusic，数据源为 maimai_mcp"""

import json
import os
from collections import defaultdict
from copy import deepcopy
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple, Union

from ..maimaidx.maimaidx_model import (
    Music,
    BasicInfo,
    Chart,
    Stats,
    Notes1,
    Notes2,
    RaMusic,
    PlanInfo,
    PlayInfoDefault,
    PlayInfoDev,
)


def _cross(
    checker: Union[List[str], List[float]],
    elem: Optional[Union[str, float, List[str], List[float], Tuple[float, float]]],
    diff: Any,
) -> Tuple[bool, Any]:
    if elem is Ellipsis or elem in (None, ""):
        return True, diff

    candidates: list[Any] | tuple[Any, Any]
    is_range = isinstance(elem, tuple)
    if is_range:
        candidates = elem
    elif isinstance(elem, list):
        candidates = elem
    else:
        candidates = [elem]

    diff_ret: list[int] = []
    indexes = range(len(checker)) if diff is Ellipsis else diff
    for index in indexes:
        if index >= len(checker):
            continue
        value = checker[index]
        if is_range:
            low, high = candidates
            if low <= value <= high:
                diff_ret.append(index)
        elif value in candidates:
            diff_ret.append(index)
    return bool(diff_ret), diff_ret


def _in_or_equal(
    checker: Union[str, int, float],
    elem: Optional[Union[str, float, List[str], List[float], Tuple[float, float]]],
) -> bool:
    if elem is Ellipsis or elem in (None, ""):
        return True
    if isinstance(elem, list):
        return checker in elem
    if isinstance(elem, tuple):
        return elem[0] <= checker <= elem[1]
    return checker == elem


def _search_charts(checker: List[Chart], elem: Optional[str], diff: Any) -> Tuple[bool, Any]:
    if elem is Ellipsis or not elem:
        return True, diff
    diff_ret: list[int] = []
    indexes = range(len(checker)) if diff is Ellipsis else diff
    needle = str(elem).lower()
    for index in indexes:
        if index >= len(checker):
            continue
        if needle in str(checker[index].charter or "").lower():
            diff_ret.append(index)
    return bool(diff_ret), diff_ret


# ============================================================
# MusicList — 从 maimaidx_music 精简来的（只保留数据方法）
# ============================================================

class MusicList(List[Music]):

    def by_id(self, music_id: Union[str, int]) -> Optional[Music]:
        exact_id = str(music_id)
        normalized_ids = [exact_id]
        try:
            numeric_id = int(music_id)
            if numeric_id > 10000:
                normalized_ids.append(str(numeric_id - 10000))
            elif 1000 < numeric_id < 10000:
                normalized_ids.append(str(numeric_id + 10000))
        except (TypeError, ValueError):
            pass
        for music in self:
            if music.id == exact_id:
                return music
        for music in self:
            if music.id in normalized_ids:
                return music
        return None

    def by_title(self, music_title: str) -> Optional[Music]:
        for music in self:
            if music.title == music_title:
                return music
        return None

    def by_plan(self, level: str) -> Dict[str, Union[PlanInfo, RaMusic, Dict]]:
        lv = defaultdict(dict)

        def create_ra_music(music: Music, index: int) -> RaMusic:
            return RaMusic(
                id=music.id,
                ds=music.ds[index],
                lv=str(index),
                lvp=music.level[index],
                type=music.type,
                image_name=music.image_name,
            )

        for music in self:
            if level not in music.level:
                continue
            numeric_id = _positive_int(music.id)
            if numeric_id is not None and numeric_id >= 100000:
                continue
            if music.level.count(level) > 1:
                lv[music.id] = {
                    index: create_ra_music(music, index)
                    for index, _lv in enumerate(music.level)
                    if _lv == level
                }
            else:
                index = music.level.index(level)
                lv[music.id] = create_ra_music(music, index)
        return dict(lv)

    def by_id_list(self, music_id_list: List[int]) -> Optional[List[Music]]:
        musicList = []
        for music in self:
            numeric_id = _positive_int(music.id)
            if numeric_id is not None and numeric_id in music_id_list:
                musicList.append(music)
        return musicList

    def filter(
        self,
        *,
        level: Optional[Union[str, List[str]]] = ...,
        ds: Optional[Union[float, List[float], Tuple[float, float]]] = ...,
        title_search: Optional[str] = ...,
        artist_search: Optional[str] = ...,
        charter_search: Optional[str] = ...,
        genre: Optional[Union[str, List[str]]] = ...,
        bpm: Optional[Union[float, List[float], Tuple[float, float]]] = ...,
        type: Optional[Union[str, List[str]]] = ...,
        diff: Any = ...,
        version: Union[str, List[str]] = ...,
    ) -> "MusicList":
        new_list = MusicList()
        for original in self:
            diff2 = diff
            music = deepcopy(original)
            ret, diff2 = _cross(music.level, level, diff2)
            if not ret:
                continue
            ret, diff2 = _cross(music.ds, ds, diff2)
            if not ret:
                continue
            ret, diff2 = _search_charts(music.charts, charter_search, diff2)
            if not ret:
                continue
            if not _in_or_equal(music.basic_info.genre, genre):
                continue
            if not _in_or_equal(music.type, type):
                continue
            if not _in_or_equal(music.basic_info.bpm, bpm):
                continue
            if not _in_or_equal(music.basic_info.version, version):
                continue
            if title_search is not Ellipsis and str(title_search).lower() not in music.title.lower():
                continue
            if artist_search is not Ellipsis and str(artist_search).lower() not in music.basic_info.artist.lower():
                continue
            music.diff = diff2 if diff2 is not Ellipsis else list(range(len(music.ds)))
            new_list.append(music)
        return new_list


# ============================================================
# CN numeric version code → maimaiDX asset version name
# ============================================================

CN_VERSION_MAP: Dict[int, str] = {
    10000: "maimai",
    10001: "maimai PLUS",
    10002: "maimai PLUS",
    11000: "maimai GreeN",
    11001: "maimai GreeN",
    11002: "maimai GreeN",
    11003: "maimai GreeN",
    11004: "maimai GreeN",
    11005: "maimai GreeN",
    11006: "maimai GreeN",
    11007: "maimai GreeN",
    12000: "maimai GreeN PLUS",
    12001: "maimai GreeN PLUS",
    12002: "maimai GreeN PLUS",
    12003: "maimai GreeN PLUS",
    12004: "maimai GreeN PLUS",
    12005: "maimai GreeN PLUS",
    12006: "maimai GreeN PLUS",
    12007: "maimai GreeN PLUS",
    12008: "maimai GreeN PLUS",
    12009: "maimai GreeN PLUS",
    13000: "maimai ORANGE",
    13001: "maimai ORANGE",
    13002: "maimai ORANGE",
    13003: "maimai ORANGE",
    13004: "maimai ORANGE",
    13005: "maimai ORANGE",
    13006: "maimai ORANGE",
    13007: "maimai ORANGE",
    13008: "maimai ORANGE",
    13009: "maimai ORANGE",
    13010: "maimai ORANGE",
    13011: "maimai ORANGE",
    14000: "maimai ORANGE PLUS",
    14001: "maimai ORANGE PLUS",
    14002: "maimai ORANGE PLUS",
    14003: "maimai ORANGE PLUS",
    14004: "maimai ORANGE PLUS",
    14006: "maimai ORANGE PLUS",
    14007: "maimai ORANGE PLUS",
    14008: "maimai ORANGE PLUS",
    14009: "maimai ORANGE PLUS",
    14010: "maimai ORANGE PLUS",
    15000: "maimai PiNK",
    15003: "maimai PiNK",
    15004: "maimai PiNK",
    15005: "maimai PiNK",
    15006: "maimai PiNK",
    15007: "maimai PiNK",
    15008: "maimai PiNK",
    15009: "maimai PiNK",
    15010: "maimai PiNK",
    15011: "maimai PiNK",
    15013: "maimai PiNK",
    15014: "maimai PiNK",
    15017: "maimai PiNK",
    15018: "maimai PiNK",
    15019: "maimai PiNK",
    16000: "maimai PiNK PLUS",
    16001: "maimai PiNK PLUS",
    16002: "maimai PiNK PLUS",
    16004: "maimai PiNK PLUS",
    16005: "maimai PiNK PLUS",
    16006: "maimai PiNK PLUS",
    16007: "maimai PiNK PLUS",
    16008: "maimai PiNK PLUS",
    16009: "maimai PiNK PLUS",
    16011: "maimai PiNK PLUS",
    16012: "maimai PiNK PLUS",
    16013: "maimai PiNK PLUS",
    16014: "maimai PiNK PLUS",
    17000: "maimai MURASAKi",
    17001: "maimai MURASAKi",
    17002: "maimai MURASAKi",
    17003: "maimai MURASAKi",
    17004: "maimai MURASAKi",
    17005: "maimai MURASAKi",
    17006: "maimai MURASAKi",
    17007: "maimai MURASAKi",
    17008: "maimai MURASAKi",
    17009: "maimai MURASAKi",
    17010: "maimai MURASAKi",
    17011: "maimai MURASAKi",
    17012: "maimai MURASAKi",
    17013: "maimai MURASAKi",
    17015: "maimai MURASAKi",
    17016: "maimai MURASAKi",
    17017: "maimai MURASAKi",
    17018: "maimai MURASAKi",
    18000: "maimai MURASAKi PLUS",
    18001: "maimai MURASAKi PLUS",
    18002: "maimai MURASAKi PLUS",
    18003: "maimai MURASAKi PLUS",
    18005: "maimai MURASAKi PLUS",
    18006: "maimai MURASAKi PLUS",
    18007: "maimai MURASAKi PLUS",
    18008: "maimai MURASAKi PLUS",
    18009: "maimai MURASAKi PLUS",
    18010: "maimai MURASAKi PLUS",
    18011: "maimai MURASAKi PLUS",
    18012: "maimai MURASAKi PLUS",
    18014: "maimai MURASAKi PLUS",
    18015: "maimai MURASAKi PLUS",
    18017: "maimai MURASAKi PLUS",
    18018: "maimai MURASAKi PLUS",
    18019: "maimai MURASAKi PLUS",
    18020: "maimai MURASAKi PLUS",
    18021: "maimai MURASAKi PLUS",
    18022: "maimai MURASAKi PLUS",
    18023: "maimai MURASAKi PLUS",
    18500: "maimai MiLK",
    18501: "maimai MiLK",
    18502: "maimai MiLK",
    18503: "maimai MiLK",
    18504: "maimai MiLK",
    18505: "maimai MiLK",
    18506: "maimai MiLK",
    18507: "maimai MiLK",
    18508: "maimai MiLK",
    18509: "maimai MiLK",
    18511: "maimai MiLK",
    18512: "maimai MiLK",
    18599: "maimai MiLK",
    19000: "MiLK PLUS",
    19001: "MiLK PLUS",
    19003: "MiLK PLUS",
    19004: "MiLK PLUS",
    19005: "MiLK PLUS",
    19006: "MiLK PLUS",
    19007: "MiLK PLUS",
    19008: "MiLK PLUS",
    19009: "MiLK PLUS",
    19010: "MiLK PLUS",
    19011: "MiLK PLUS",
    19012: "MiLK PLUS",
    19013: "MiLK PLUS",
    19500: "maimai FiNALE",
    19501: "maimai FiNALE",
    19502: "maimai FiNALE",
    19503: "maimai FiNALE",
    19504: "maimai FiNALE",
    19505: "maimai FiNALE",
    19507: "maimai FiNALE",
    19508: "maimai FiNALE",
    19509: "maimai FiNALE",
    19510: "maimai FiNALE",
    19511: "maimai FiNALE",
    19512: "maimai FiNALE",
    19513: "maimai FiNALE",
    19514: "maimai FiNALE",
    19900: "maimai FiNALE",
    19901: "maimai FiNALE",
    19902: "maimai FiNALE",
    19903: "maimai FiNALE",
    19904: "maimai FiNALE",
    19905: "maimai FiNALE",
    19906: "maimai FiNALE",
    19907: "maimai FiNALE",
    19909: "maimai FiNALE",
    19910: "maimai FiNALE",
    19911: "maimai FiNALE",
    19912: "maimai FiNALE",
    19992: "maimai FiNALE",
    19993: "maimai FiNALE",
    19994: "maimai FiNALE",
    19995: "maimai FiNALE",
    19996: "maimai FiNALE",
    19997: "maimai FiNALE",
    19998: "maimai FiNALE",
    19999: "maimai FiNALE",
    20000: "maimai でらっくす",
    20002: "maimai でらっくす",
    20003: "maimai でらっくす",
    20005: "maimai でらっくす",
    20007: "maimai でらっくす",
    20100: "maimai でらっくす",
    20105: "maimai でらっくす",
    20106: "maimai でらっくす",
    21000: "maimai でらっくす PLUS",
    21001: "maimai でらっくす PLUS",
    21002: "maimai でらっくす PLUS",
    21003: "maimai でらっくす PLUS",
    21004: "maimai でらっくす PLUS",
    21005: "maimai でらっくす PLUS",
    21006: "maimai でらっくす PLUS",
    21007: "maimai でらっくす PLUS",
    22000: "maimai でらっくす Splash",
    22001: "maimai でらっくす Splash",
    22002: "maimai でらっくす Splash",
    22003: "maimai でらっくす Splash",
    22004: "maimai でらっくす Splash",
    22005: "maimai でらっくす Splash",
    22006: "maimai でらっくす Splash",
    22007: "maimai でらっくす Splash",
    23000: "maimai でらっくす Splash PLUS",
    23001: "maimai でらっくす Splash PLUS",
    23002: "maimai でらっくす Splash PLUS",
    23003: "maimai でらっくす Splash PLUS",
    23004: "maimai でらっくす Splash PLUS",
    23005: "maimai でらっくす Splash PLUS",
    23006: "maimai でらっくす Splash PLUS",
    23007: "maimai でらっくす Splash PLUS",
    23008: "maimai でらっくす Splash PLUS",
    24000: "maimai でらっくす UNiVERSE",
    24001: "maimai でらっくす UNiVERSE",
    24002: "maimai でらっくす UNiVERSE",
    24003: "maimai でらっくす UNiVERSE",
    24004: "maimai でらっくす UNiVERSE",
    24005: "maimai でらっくす UNiVERSE",
    24006: "maimai でらっくす UNiVERSE",
    24007: "maimai でらっくす UNiVERSE",
    24010: "maimai でらっくす UNiVERSE",
    24015: "maimai でらっくす UNiVERSE",
    24500: "maimai でらっくす UNiVERSE",
    24505: "maimai でらっくす UNiVERSE",
    24506: "maimai でらっくす UNiVERSE",
    24511: "maimai でらっくす UNiVERSE",
    25000: "maimai でらっくす FESTiVAL",
    25001: "maimai でらっくす FESTiVAL",
    25002: "maimai でらっくす FESTiVAL",
    25003: "maimai でらっくす FESTiVAL",
    25004: "maimai でらっくす FESTiVAL",
    25005: "maimai でらっくす FESTiVAL",
    25006: "maimai でらっくす FESTiVAL",
    25007: "maimai でらっくす FESTiVAL",
    25008: "maimai でらっくす FESTiVAL",
    25009: "maimai でらっくす FESTiVAL",
    25010: "maimai でらっくす FESTiVAL",
    25013: "maimai でらっくす FESTiVAL",
}


def _cn_version_name(code: int) -> str:
    return CN_VERSION_MAP.get(code, f"unknown_{code}")


# ============================================================
# Source genre → maimaiDX category mapping
# ============================================================

GENRE_MAP: Dict[str, str] = {
    "POPSアニメ": "anime",
    "niconicoボーカロイド": "niconico",
    "東方Project": "touhou",
    "ゲームバラエティ": "game",
    "maimai": "maimai",
    "オンゲキCHUNITHM": "ongeki",
    "宴会場": "宴会场",
}


def _source_genre_to_category(genre: str) -> str:
    return GENRE_MAP.get(genre, genre)


def _positive_int(value: Any) -> Optional[int]:
    try:
        number = int(value)
    except (TypeError, ValueError):
        return None
    return number if number > 0 else None


def _equivalent_song_ids(value: Any) -> set[str]:
    ids = {str(value)}
    number = _positive_int(value)
    if number is None:
        return ids
    ids.add(str(number))
    if number > 100000:
        ids.add(str(number - 100000))
    if number > 10000:
        ids.add(str(number - 10000))
    if 1000 < number < 10000:
        ids.add(str(number + 10000))
    if number >= 10000:
        ids.add(str(number % 10000))
    return ids


def _normalized_chart_type(value: Any) -> str | None:
    text = str(value or "").strip().lower()
    if text in {"dx", "でらっくす"}:
        return "dx"
    if text in {"sd", "st", "std", "standard", "標準", "标准"}:
        return "standard"
    return None


def _plate_song_render_id(song: dict) -> str:
    if song.get("render_id"):
        return str(song["render_id"])
    song_id = _positive_int(song.get("song_id"))
    if song_id:
        return str(song_id)
    return f"custom:{song.get('title', '')}:{song.get('type', '')}"


def _coerce_plate_values(song: dict) -> tuple[list[float], list[str]]:
    ds_vals = list(song.get("ds_values") or [])
    lvls = list(song.get("level_values") or [])
    while ds_vals and ds_vals[-1] == 0:
        ds_vals.pop()
        if lvls:
            lvls.pop()
    while len(ds_vals) < 4:
        ds_vals.append(0)
    while len(lvls) < len(ds_vals):
        lvls.append("")
    return ds_vals, lvls


RENDER_VERSION_MAP: Dict[str, str] = {
    "maimai": "maimai",
    "maimai PLUS": "maimai PLUS",
    "GreeN": "maimai GreeN",
    "GreeN PLUS": "maimai GreeN PLUS",
    "ORANGE": "maimai ORANGE",
    "ORANGE PLUS": "maimai ORANGE PLUS",
    "PiNK": "maimai PiNK",
    "PiNK PLUS": "maimai PiNK PLUS",
    "MURASAKi": "maimai MURASAKi",
    "MURASAKi PLUS": "maimai MURASAKi PLUS",
    "MiLK": "maimai MiLK",
    "MiLK PLUS": "MiLK PLUS",
    "FiNALE": "maimai FiNALE",
    "maimaiでらっくす": "maimai でらっくす",
    "maimaiでらっくす PLUS": "maimai でらっくす PLUS",
    "Splash": "maimai でらっくす Splash",
    "Splash PLUS": "maimai でらっくす Splash PLUS",
    "UNiVERSE": "maimai でらっくす UNiVERSE",
    "UNiVERSE PLUS": "maimai でらっくす UNiVERSE PLUS",
    "FESTiVAL": "maimai でらっくす FESTiVAL",
    "FESTiVAL PLUS": "maimai でらっくす FESTiVAL PLUS",
    "BUDDiES": "maimai でらっくす BUDDiES",
    "BUDDiES PLUS": "maimai でらっくす BUDDiES PLUS",
    "PRiSM": "maimai でらっくす PRiSM",
    # The bundled maimaiDX asset pack does not currently include PRiSM PLUS/CiRCLE badges.
    "PRiSM PLUS": "maimai でらっくす PRiSM",
}


def _render_version_name_from_search(value: Any) -> str:
    number = _positive_int(value)
    if number is not None:
        return _cn_version_name(number)
    text = str(value or "").strip()
    return RENDER_VERSION_MAP.get(text, text or "maimai でらっくす")


def _search_source_field(song: dict[str, Any], source: str, field: str) -> Any:
    source_fields = song.get("source_fields") if isinstance(song.get("source_fields"), dict) else {}
    source_data = source_fields.get(source) if isinstance(source_fields.get(source), dict) else {}
    return source_data.get(field)


def _search_note_value(notes: dict[str, Any], *keys: str) -> int | None:
    for key in keys:
        value = notes.get(key)
        if value is None:
            continue
        try:
            return int(value)
        except (TypeError, ValueError):
            continue
    return None


def _search_chart_notes(notes: Any) -> Notes1 | Notes2:
    if not isinstance(notes, dict):
        notes = {}
    tap = _search_note_value(notes, "tap")
    hold = _search_note_value(notes, "hold")
    slide = _search_note_value(notes, "slide")
    touch = _search_note_value(notes, "touch")
    brk = _search_note_value(notes, "break", "brk")
    return Notes2(tap, hold, slide, touch, brk) if (touch or 0) > 0 else Notes1(tap, hold, slide, brk)


def _search_chart_total_notes(notes: Any) -> int | None:
    if not isinstance(notes, dict):
        return None
    total = _search_note_value(notes, "total")
    if total is not None and total > 0:
        return total
    values = [
        _search_note_value(notes, "tap"),
        _search_note_value(notes, "hold"),
        _search_note_value(notes, "slide"),
        _search_note_value(notes, "touch"),
        _search_note_value(notes, "break", "brk"),
    ]
    if any(value is not None for value in values):
        return sum(value or 0 for value in values)
    return None


def _search_chart_note_detail_score(chart: dict) -> tuple[int, int]:
    notes = chart.get("notes") if isinstance(chart.get("notes"), dict) else {}
    note_parts = [
        _search_note_value(notes, "tap"),
        _search_note_value(notes, "hold"),
        _search_note_value(notes, "slide"),
        _search_note_value(notes, "touch"),
        _search_note_value(notes, "break", "brk"),
    ]
    return (
        sum(value is not None for value in note_parts),
        int(_search_chart_total_notes(notes) is not None),
    )


def _merge_search_chart_note_details(primary: dict, candidates: list[dict]) -> dict:
    primary_type = _normalized_chart_type(primary.get("chart_type"))
    compatible = [
        chart
        for chart in candidates
        if not primary_type
        or not _normalized_chart_type(chart.get("chart_type"))
        or _normalized_chart_type(chart.get("chart_type")) == primary_type
    ]
    best = max(compatible or [primary], key=_search_chart_note_detail_score)
    if best is primary or _search_chart_note_detail_score(best) <= _search_chart_note_detail_score(primary):
        return primary

    merged = dict(primary)
    merged_notes = dict(best.get("notes") if isinstance(best.get("notes"), dict) else {})
    primary_notes = primary.get("notes") if isinstance(primary.get("notes"), dict) else {}
    primary_total = _search_note_value(primary_notes, "total")
    if primary_total is not None:
        merged_notes["total"] = primary_total
    merged["notes"] = merged_notes
    if not merged.get("charter") and best.get("charter") not in (None, "", "-"):
        merged["charter"] = best.get("charter")
    return merged


def _search_chart_stats(chart: dict) -> Stats | None:
    raw_stats = chart.get("fit_stats") if isinstance(chart.get("fit_stats"), dict) else {}
    stats = dict(raw_stats)
    if chart.get("fit_diff") is not None:
        stats["fit_diff"] = chart.get("fit_diff")
    if chart.get("level") is not None and "diff" not in stats:
        stats["diff"] = chart.get("level")
    if not stats:
        return None
    return Stats.model_validate(stats)


def _search_song_bpm(value: Any) -> int:
    try:
        return int(float(value))
    except (TypeError, ValueError):
        return 0


def music_from_search_song(song: dict[str, Any]) -> Optional[Music]:
    """Build a renderable Music model from maimai-local-search JSON output.

    Numeric Diving-Fish IDs remain the preferred render key. String IDs are
    accepted only for local/custom compatibility.
    """
    title = str(song.get("title") or "").strip()
    sid = str(song.get("id") or song.get("source_id") or title).strip()
    if not sid or not title:
        return None

    charts = [c for c in song.get("matched_charts") or [] if isinstance(c, dict)]
    if not charts:
        return None

    slot_candidates: Dict[int, list[dict]] = {}
    for chart in charts:
        try:
            difficulty_index = int(chart.get("difficulty_index", len(slot_candidates)))
        except (TypeError, ValueError):
            continue
        slot_candidates.setdefault(difficulty_index, []).append(chart)

    if not slot_candidates:
        return None

    slots = {
        difficulty_index: _merge_search_chart_note_details(candidates[0], candidates)
        for difficulty_index, candidates in slot_candidates.items()
    }
    max_index = max(slots)
    ds_list = [0.0] * (max_index + 1)
    level_list = [""] * (max_index + 1)
    chart_list: List[Chart] = []
    stats_list: List[Stats | None] = [None] * (max_index + 1)

    for index in range(max_index + 1):
        chart = slots.get(index)
        if not chart:
            chart_list.append(Chart(notes=Notes1(0, 0, 0, 0), charter="-"))
            continue
        try:
            ds_list[index] = float(chart.get("ds", 0) or 0)
        except (TypeError, ValueError):
            ds_list[index] = 0.0
        level_list[index] = str(chart.get("level") or "")
        chart_list.append(
            Chart(
                notes=_search_chart_notes(chart.get("notes")),
                charter=str(chart.get("charter") or ""),
                total_notes=_search_chart_total_notes(chart.get("notes")),
            )
        )
        stats_list[index] = _search_chart_stats(chart)

    while level_list and level_list[-1] == "":
        level_list.pop()
        ds_list.pop()
        chart_list.pop()
        stats_list.pop()

    if not level_list:
        return None

    dx_count = sum(1 for c in charts if str(c.get("chart_type", "")).lower() == "dx")
    std_count = sum(1 for c in charts if str(c.get("chart_type", "")).lower() in {"standard", "sd", "std"})
    music_type = "DX" if dx_count >= std_count else "SD"
    version = _search_source_field(song, "divingfish", "version") or song.get("version")
    if version in (None, ""):
        version = next((c.get("version") for c in charts if c.get("version") not in (None, "")), None)
    is_new = _search_source_field(song, "divingfish", "is_new")
    if is_new in (None, ""):
        is_new = song.get("is_new", False)

    return Music(
        id=sid,
        title=title,
        type=music_type,
        ds=ds_list,
        level=level_list,
        cids=[0] * len(level_list),
        charts=chart_list,
        basic_info=BasicInfo.model_validate({
            "title": title,
            "artist": str(song.get("artist") or ""),
            "genre": _source_genre_to_category(str(song.get("genre") or "")),
            "bpm": _search_song_bpm(song.get("bpm")),
            "from": _render_version_name_from_search(version),
            "is_new": bool(is_new),
        }),
        stats=stats_list,
        image_name=song.get("imageName") or song.get("image_name"),
    )


def _base_numeric_song_id(song: dict[str, Any]) -> Optional[int]:
    source_ids = song.get("source_ids") if isinstance(song.get("source_ids"), dict) else {}
    for raw_id in (source_ids.get("cn"), song.get("id"), song.get("source_id")):
        numeric_id = _positive_int(raw_id)
        if numeric_id is not None:
            return numeric_id
    return None


def render_id_from_search_song(song: dict[str, Any], song_type: Any = None) -> str:
    """Return the chart-specific render/score id for a local-search song.

    For merged ST/DX songs the song-level Diving-Fish id may point at only one
    side of the pair. Standard charts must stay on the base id, while DX charts
    use the +10000 score id.
    """
    source_ids = song.get("source_ids") if isinstance(song.get("source_ids"), dict) else {}
    waterfish_id = _positive_int(source_ids.get("divingfish"))
    base_id = _base_numeric_song_id(song)
    chart_type = _normalized_chart_type(song_type)

    if chart_type == "standard":
        if base_id is not None:
            return str(base_id)
        if waterfish_id is not None and waterfish_id > 10000:
            return str(waterfish_id - 10000)
        if waterfish_id is not None:
            return str(waterfish_id)
    elif chart_type == "dx":
        if waterfish_id is not None and waterfish_id > 10000:
            return str(waterfish_id)
        if base_id is not None:
            return str(base_id + 10000)
        if waterfish_id is not None:
            return str(waterfish_id + 10000 if waterfish_id < 10000 else waterfish_id)

    raw_id = source_ids.get("divingfish") or song.get("id") or song.get("source_id")
    numeric_id = _positive_int(raw_id)
    return str(numeric_id) if numeric_id is not None else str(raw_id or "").strip()


def _search_song_chart_types(song: dict[str, Any]) -> list[str | None]:
    chart_types: list[str | None] = []
    for chart in song.get("matched_charts") or []:
        if not isinstance(chart, dict):
            continue
        chart_type = _normalized_chart_type(chart.get("chart_type"))
        if chart_type and chart_type not in chart_types:
            chart_types.append(chart_type)
    return chart_types or [None]


def _search_song_variant(song: dict[str, Any], chart_type: str | None) -> dict[str, Any]:
    if not chart_type:
        return song
    variant = dict(song)
    variant["matched_charts"] = [
        chart
        for chart in song.get("matched_charts") or []
        if isinstance(chart, dict) and _normalized_chart_type(chart.get("chart_type")) == chart_type
    ]
    variant["available_chart_types"] = [chart_type]
    return variant


def _custom_plate_data_path() -> Path:
    raw_path = (
        os.environ.get("MAIMAIDX_CUSTOM_PLATES_PATH")
        or os.environ.get("MAIMAIDX_CUSTOM_PLATE_PATH")
    )
    if raw_path:
        return Path(raw_path)
    return Path(__file__).parent.parent.parent / "data" / "custom_plates.json"


def _load_custom_plate_content() -> dict[str, Any]:
    path = _custom_plate_data_path()
    if not path.exists():
        return {}
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}
    if not isinstance(data, dict):
        return {}
    content = data.get("content", data.get("plates", data))
    return content if isinstance(content, dict) else {}


def _custom_plate_entry(plate_name: str) -> Any:
    text = str(plate_name or "").strip()
    if not text:
        return None
    content = _load_custom_plate_content()
    if text in content:
        return content[text]
    for key, value in content.items():
        if str(key).strip() == text:
            return value
    return None


def custom_plate_exists(plate_name: str) -> bool:
    return _custom_plate_entry(plate_name) is not None


def _custom_plate_song_items(plate_name: str) -> list[Any]:
    entry = _custom_plate_entry(plate_name)
    if entry is None:
        return []
    if isinstance(entry, list):
        return entry
    if isinstance(entry, dict):
        for key in ("songs", "music", "musicIds", "music_ids", "ids"):
            value = entry.get(key)
            if isinstance(value, list):
                return value
    return []


def _custom_song_item_id(item: Any) -> Any:
    if isinstance(item, dict):
        for key in ("song_id", "songId", "music_id", "musicId", "df_id", "id"):
            value = item.get(key)
            if value not in (None, ""):
                return value
        return None
    return item


def _custom_song_type(value: Any) -> str:
    text = str(value or "DX").strip().lower()
    if text in {"sd", "st", "std", "standard", "标准", "標準"}:
        return "SD"
    return "DX"


def _custom_song_values(item: dict[str, Any], *keys: str) -> list[Any]:
    for key in keys:
        value = item.get(key)
        if isinstance(value, list):
            return value
    return []


def _custom_song_from_search(query: str, song_type: Any = None) -> Optional[dict[str, Any]]:
    try:
        from maimai_mcp.search import search_songs
    except Exception:
        return None

    try:
        result = search_songs(query=query, song_type=_normalized_chart_type(song_type), limit=5)
    except Exception:
        return None
    songs = result.get("songs") if isinstance(result, dict) else None
    if not songs:
        return None

    source_song = songs[0]
    if not isinstance(source_song, dict):
        return None
    chart_type = _normalized_chart_type(song_type)
    if chart_type is None:
        chart_type = _search_song_chart_types(source_song)[0]
    variant = _search_song_variant(source_song, chart_type)
    music = music_from_search_song(variant)
    if music is None:
        return None
    render_id = render_id_from_search_song(source_song, chart_type)
    return {
        "title": music.title,
        "type": music.type,
        "df_id": _positive_int(render_id),
        "song_id": _positive_int(render_id) or render_id,
        "render_id": render_id,
        "image_name": source_song.get("imageName") or source_song.get("image_name"),
        "ds_values": list(music.ds),
        "level_values": list(music.level),
    }


def _custom_song_from_item(item: dict[str, Any], df_music: Music | None = None) -> Optional[dict[str, Any]]:
    raw_id = _custom_song_item_id(item)
    render_id = str(raw_id).strip() if raw_id not in (None, "") else ""
    if df_music is not None:
        return {
            "title": df_music.title,
            "type": df_music.type,
            "df_id": _positive_int(raw_id),
            "song_id": _positive_int(raw_id) or render_id,
            "render_id": render_id or df_music.id,
            "image_name": item.get("image_name") or item.get("imageName"),
            "ds_values": list(df_music.ds),
            "level_values": list(df_music.level),
        }

    query = str(item.get("query") or item.get("title") or item.get("name") or render_id).strip()
    if query and not (_custom_song_values(item, "ds_values", "ds") and _custom_song_values(item, "level_values", "level")):
        searched = _custom_song_from_search(query, item.get("type") or item.get("songType"))
        if searched:
            if item.get("image_name") or item.get("imageName"):
                searched["image_name"] = item.get("image_name") or item.get("imageName")
            return searched

    title = str(item.get("title") or item.get("name") or query).strip()
    if not title:
        return None
    ds_values = _custom_song_values(item, "ds_values", "ds")
    level_values = _custom_song_values(item, "level_values", "level")
    if not ds_values or not level_values:
        return None
    return {
        "title": title,
        "type": _custom_song_type(item.get("type") or item.get("songType")),
        "df_id": _positive_int(raw_id),
        "song_id": _positive_int(raw_id) or (render_id if render_id else None),
        "render_id": render_id or f"custom:{title}:{_custom_song_type(item.get('type'))}",
        "image_name": item.get("image_name") or item.get("imageName"),
        "ds_values": ds_values,
        "level_values": level_values,
    }


# ============================================================
# MaiMusic shim
# ============================================================

class _MaiMusic:
    """替换 maimaiDX 的 MaiMusic，数据源为 maimai_mcp"""

    def __init__(self):
        self.total_list: MusicList = MusicList()
        self.total_plate_id_list: Dict[str, List[int]] = {}
        self.total_level_data: Dict[str, Dict[str, List[RaMusic]]] = {}
        self.music_regions_by_id: Dict[str, Dict[str, bool]] = {}
        self._loaded = False

    def _ensure_loaded(self):
        if self._loaded:
            return
        self._load_from_maimai_mcp()
        self._load_plate_data()
        self._loaded = True

    def _load_from_maimai_mcp(self):
        """从 maimai_mcp 合并搜索结果加载曲库并转成 Music 模型。"""
        self.total_list = MusicList()
        self.music_regions_by_id = {}
        self._augment_from_search_database()
        self._build_level_data()

    def _remember_music_regions(self, render_id: str, song: dict[str, Any]) -> None:
        regions = song.get("regions") if isinstance(song.get("regions"), dict) else {}
        source_labels = song.get("source_labels") if isinstance(song.get("source_labels"), list) else []
        normalized = {
            "cn": bool(regions.get("cn")) or "cn" in source_labels or "divingfish" in source_labels,
        }
        for equivalent_id in _equivalent_song_ids(render_id):
            self.music_regions_by_id[equivalent_id] = normalized

    def _music_matches_server(self, music: Music, server: str) -> bool:
        if server != "cn":
            return False
        regions = self.music_regions_by_id.get(str(music.id))
        if regions is None:
            for equivalent_id in _equivalent_song_ids(music.id):
                regions = self.music_regions_by_id.get(equivalent_id)
                if regions is not None:
                    break
        if regions is None:
            return _positive_int(music.id) is not None
        return bool(regions.get("cn"))

    def _augment_from_search_database(self):
        """从 maimai_mcp 合并搜索结果构建渲染曲库。

        数字 ID 曲目继续用水鱼 ID；无数字 ID 的自定义曲目只在自定义牌子等
        本地场景里使用。
        """
        try:
            from maimai_mcp.search import collect_song_results
        except Exception:
            return

        try:
            songs, _criteria = collect_song_results()
        except Exception:
            return

        existing: set[str] = set()
        for music in self.total_list:
            existing.update(_equivalent_song_ids(music.id))
        for song in songs:
            if not isinstance(song, dict):
                continue
            for chart_type in _search_song_chart_types(song):
                variant_song = _search_song_variant(song, chart_type)
                render_id = render_id_from_search_song(song, chart_type)
                if not render_id or render_id in existing:
                    continue
                music = music_from_search_song(variant_song)
                if music is None:
                    continue
                music.id = render_id
                self.total_list.append(music)
                self._remember_music_regions(render_id, song)
                existing.add(render_id)

    def _convert_divingfish_to_music(self, raw: dict) -> Optional[Music]:
        """Convert a Diving-Fish song-list record into the maimaiDX Music model."""
        sid = str(raw.get("id", ""))
        if not sid:
            return None

        title = str(raw.get("title", ""))
        music_type = str(raw.get("type", "SD"))
        raw_ds = raw.get("ds") if isinstance(raw.get("ds"), list) else []
        raw_levels = raw.get("level") if isinstance(raw.get("level"), list) else []
        ds_list: List[float] = []
        level_list: List[str] = []
        for value in raw_ds:
            try:
                ds_list.append(float(value))
            except (TypeError, ValueError):
                ds_list.append(0.0)
        for value in raw_levels:
            level_list.append(str(value))
        while len(level_list) < len(ds_list):
            level_list.append("")

        raw_charts = raw.get("charts") if isinstance(raw.get("charts"), list) else []
        chart_list: List[Chart] = []
        for index in range(len(ds_list)):
            chart = raw_charts[index] if index < len(raw_charts) and isinstance(raw_charts[index], dict) else {}
            notes = chart.get("notes", [])
            if isinstance(notes, dict):
                tp = int(notes.get("tap", 0) or 0)
                hd = int(notes.get("hold", 0) or 0)
                sl = int(notes.get("slide", 0) or 0)
                tc = int(notes.get("touch", 0) or 0)
                br = int(notes.get("break", notes.get("brk", 0)) or 0)
            elif isinstance(notes, list):
                values = [int(v or 0) for v in notes]
                tp = values[0] if len(values) > 0 else 0
                hd = values[1] if len(values) > 1 else 0
                sl = values[2] if len(values) > 2 else 0
                if len(values) >= 5:
                    tc = values[3]
                    br = values[4]
                else:
                    tc = 0
                    br = values[3] if len(values) > 3 else 0
            else:
                tp = hd = sl = tc = br = 0
            ntuple = Notes2(tp, hd, sl, tc, br) if tc > 0 else Notes1(tp, hd, sl, br)
            chart_list.append(Chart(notes=ntuple, charter=str(chart.get("charter", "-"))))

        if not ds_list:
            return None

        basic = raw.get("basic_info") if isinstance(raw.get("basic_info"), dict) else {}
        bpm_val = basic.get("bpm", 0)
        try:
            bpm = int(bpm_val)
        except (TypeError, ValueError):
            bpm = 150

        return Music(
            id=sid,
            title=title,
            type=music_type,
            ds=ds_list,
            level=level_list,
            cids=list(raw.get("cids", [0] * len(ds_list))),
            charts=chart_list,
            basic_info=BasicInfo.model_validate({
                "title": str(basic.get("title", title)),
                "artist": str(basic.get("artist", "")),
                "genre": str(basic.get("genre", "")),
                "bpm": bpm,
                "from": str(basic.get("from", "")),
                "is_new": bool(basic.get("is_new", False)),
            }),
            stats=[None] * len(ds_list),
            image_name=raw.get("image_name") or raw.get("imageName"),
        )

    def _build_level_data(self):
        from ..maimaidx import levelList

        def level_range(lv: str) -> range:
            if lv == "15":
                return range(1)
            if lv.endswith("+"):
                return range(9, 5, -1)
            return range(9, -1, -1) if int(lv) <= 5 else range(5, -1, -1)

        _level: Dict[str, Dict[str, List[RaMusic]]] = {
            lv: {f"{lv.rstrip('+')}.{i}": [] for i in level_range(lv)}
            for lv in levelList
        }
        for music in self.total_list:
            for index, ds in enumerate(music.ds):
                if ds < 7:
                    continue
                lv = music.level[index] if index < len(music.level) else ""
                if not lv:
                    continue
                ra = RaMusic(
                    id=music.id,
                    ds=ds,
                    lv=str(index),
                    lvp=lv,
                    type=music.type,
                    image_name=music.image_name,
                )
                ds_key = str(ds)
                if lv in _level and ds_key in _level[lv]:
                    _level[lv][ds_key].append(ra)
        self.total_level_data = _level

    def _load_plate_data(self):
        # 从主 data/ 目录读（update_plate_data.py 下载到这里）
        plate_path = Path(__file__).parent.parent.parent / "data" / "maimaidxplate.json"
        if plate_path.exists():
            try:
                data = json.loads(plate_path.read_text(encoding="utf-8"))
                self.total_plate_id_list = data.get("content", data)
            except Exception:
                pass

    # 代理 total_list 的方法以便直接调用
    def by_id(self, music_id):
        self._ensure_loaded()
        return self.total_list.by_id(music_id)

    def by_plan(self, level, server: str = "cn"):
        self._ensure_loaded()
        if server != "cn":
            return {}
        return MusicList(
            [music for music in self.total_list if self._music_matches_server(music, "cn")]
        ).by_plan(level)

    def by_id_list(self, ids):
        self._ensure_loaded()
        return self.total_list.by_id_list(ids)

    def get_songs_by_df_ids(self, df_ids: List[int]) -> Dict[int, Music]:
        """按水鱼 DivingFish ID 批量查歌，返回 {df_id: Music} 字典。

        直接从 Diving-Fish 曲库记录转换，避免把其他来源的 canonical id / 谱面混进
        牌子底图和水鱼成绩匹配链路。
        """
        self._ensure_loaded()
        from maimai_mcp.search import DEFAULT_DIVINGFISH_DATA_PATH, load_single_music_data

        raw_by_df: Dict[int, dict] = {}
        for raw in load_single_music_data(DEFAULT_DIVINGFISH_DATA_PATH):
            df_id = _positive_int(raw.get("id"))
            if df_id:
                raw_by_df[df_id] = raw

        result: Dict[int, Music] = {}
        for df_id in df_ids:
            normalized_id = _positive_int(df_id)
            if not normalized_id:
                continue
            raw = raw_by_df.get(normalized_id)
            if not raw:
                continue
            music = self._convert_divingfish_to_music(raw)
            if music:
                result[normalized_id] = music
        return result


def get_custom_plate_songs(plate_name: str) -> List[dict]:
    """自定义牌子 → 曲目列表。

    `data/custom_plates.json` 支持：
      {"content": {"牌子名": [8, 11475]}}
      {"content": {"牌子名": {"songs": [{"id": 8}, {"title": "...", "type": "DX", "ds": [...], "level": [...]}]}}}
    """
    items = _custom_plate_song_items(plate_name)
    if not items:
        return []

    numeric_ids: list[int] = []
    for item in items:
        song_id = _positive_int(_custom_song_item_id(item))
        if song_id:
            numeric_ids.append(song_id)
    waterfish_music = mai.get_songs_by_df_ids(numeric_ids) if numeric_ids else {}

    result: list[dict[str, Any]] = []
    seen: set[str] = set()
    for item in items:
        raw_id = _custom_song_item_id(item)
        song_id = _positive_int(raw_id)
        df_music = waterfish_music.get(song_id) if song_id else None
        if isinstance(item, dict):
            song = _custom_song_from_item(item, df_music)
        elif df_music is not None:
            song = {
                "title": df_music.title,
                "type": df_music.type,
                "df_id": song_id,
                "song_id": song_id,
                "render_id": str(song_id),
                "ds_values": list(df_music.ds),
                "level_values": list(df_music.level),
            }
        else:
            query = str(item or "").strip()
            song = _custom_song_from_search(query) if query else None
        if not song:
            continue
        key = str(song.get("render_id") or song.get("song_id") or f"{song.get('title')}:{song.get('type')}")
        if key in seen:
            continue
        seen.add(key)
        result.append(song)
    return result


def build_custom_plate_music(plate_songs: List[dict]) -> List[Music]:
    """Build renderable Music objects from plate song dictionaries."""
    song_ids = [
        song_id for song_id in (_positive_int(song.get("song_id")) for song in plate_songs)
        if song_id
    ]
    waterfish_music = mai.get_songs_by_df_ids(song_ids) if song_ids else {}
    result: List[Music] = []
    seen: set[str] = set()
    for song in plate_songs:
        song_id = _positive_int(song.get("song_id"))
        music = waterfish_music.get(song_id) if song_id else None
        if not music:
            ds_vals, lvls = _coerce_plate_values(song)
            music = Music(
                id=_plate_song_render_id(song),
                title=str(song.get("title", "")),
                type=str(song.get("type", "DX")),
                ds=ds_vals,
                level=lvls,
                cids=[0] * len(ds_vals),
                charts=[],
                basic_info=BasicInfo.model_validate({
                    "title": str(song.get("title", "")),
                    "artist": "",
                    "genre": "",
                    "bpm": 0,
                    "from": "",
                    "is_new": False,
                }),
                stats=[],
                image_name=song.get("image_name") or song.get("imageName"),
            )
        if music.id in seen:
            continue
        seen.add(music.id)
        result.append(music)
    return result


mai = _MaiMusic()
