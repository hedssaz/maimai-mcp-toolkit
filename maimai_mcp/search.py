from __future__ import annotations

import argparse
import csv
import json
import math
import random
import re
import time
import unicodedata
from decimal import Decimal, ROUND_FLOOR
from functools import lru_cache
from pathlib import Path
from typing import Any


PACKAGE_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_DATA_PATH = PACKAGE_ROOT / "data" / "lxns_song_list.json"
DEFAULT_ALIAS_PATH = PACKAGE_ROOT / "data" / "lxns_alias_list.json"
DEFAULT_CUSTOM_ALIAS_PATH = PACKAGE_ROOT / "data" / "custom_aliases.json"
DEFAULT_YUZU_ALIAS_PATH = PACKAGE_ROOT / "data" / "music_alias.json"
DEFAULT_LEGACY_ALIAS_CSV_PATH = PACKAGE_ROOT / "data" / "aliases.csv"
DEFAULT_PINYIN_ALIAS_PATH = PACKAGE_ROOT / "data" / "pinyin_aliases.json"
DEFAULT_DIVINGFISH_DATA_PATH = PACKAGE_ROOT / "data" / "divingfish_song_list.json"
DEFAULT_CHART_STATS_PATH = PACKAGE_ROOT / "data" / "divingfish_chart_stats.json"
DEFAULT_S2T_PATH = PACKAGE_ROOT / "data" / "zh_s2t.json"
DEFAULT_T2S_PATH = PACKAGE_ROOT / "data" / "zh_t2s.json"
DEFAULT_ARTIST_ALIAS_PATH = PACKAGE_ROOT / "data" / "artist_aliases.json"
DEFAULT_CHARTER_ALIAS_PATH = PACKAGE_ROOT / "data" / "charter_aliases.json"
PINYIN_ALIAS_TTL_SECONDS = 30 * 60
PINYIN_ALIAS_FORMAT_VERSION = 2
_SEARCH_SOURCE_REVISION: tuple[tuple[str, int, int], ...] | None = None

DIFFICULTIES = ("Basic", "Advanced", "Expert", "Master", "Re:MASTER")
DIFFICULTY_INDEX_BY_NAME = {
    "basic": 0,
    "advanced": 1,
    "expert": 2,
    "master": 3,
    "remaster": 4,
    "re:master": 4,
}
SONG_TYPE_ALIASES = {
    "sd": "standard",
    "st": "standard",
    "std": "standard",
    "standard": "standard",
    "标准": "standard",
    "标": "standard",
    "dx": "dx",
    # 单人宴：内部统一叫 utage1p（跟 utage2p 平级，避免 AI 把"utage"误以为是"全部宴谱"）。
    # 旧名 utage / 宴 / 宴会场 作输入别名保留，依然映到 utage1p。
    "utage1p": "utage1p",
    "1p": "utage1p",
    "单人宴": "utage1p",
    "单人": "utage1p",
    "utage": "utage1p",
    "宴": "utage1p",
    "宴会场": "utage1p",
    # 合奏/双人宴
    "utage2p": "utage2p",
    "2p": "utage2p",
    "双人宴": "utage2p",
    "合奏宴": "utage2p",
    "合奏": "utage2p",
}
DIFFICULTY_ALIASES = {
    "basic": 0,
    "bas": 0,
    "green": 0,
    "绿": 0,
    "advanced": 1,
    "adv": 1,
    "yellow": 1,
    "黄": 1,
    "expert": 2,
    "exp": 2,
    "red": 2,
    "红": 2,
    "master": 3,
    "mst": 3,
    "purple": 3,
    "紫": 3,
    "remaster": 4,
    "re:master": 4,
    "rem": 4,
    "white": 4,
    "白": 4,
}
SOURCE_NAMES = {
    "cn": "lxns",
    "divingfish": "cndivingfish",
}
SOURCE_PRIORITY = ("cn", "divingfish")

# 统一字段名 → 各源的 key 路径（tuple 表示嵌套路径如 ("basic_info", "genre")）
# 空 tuple 表示该源不提供这个字段，由 load_music_data 阶段从其他源派生。
SOURCE_FIELD_MAPS = {
    "cn": {  # LXNS
        "title":         ("title",),
        "artist":        ("artist",),
        "genre":         ("genre",),
        "bpm":           ("bpm",),
        "version":       ("version",),
        "release_date":  (),
        "is_new":        (),
    },
    "divingfish": {  # DivingFish
        "title":         ("basic_info", "title"),
        "artist":        ("basic_info", "artist"),
        "genre":         ("basic_info", "genre"),
        "bpm":           ("basic_info", "bpm"),
        "version":       ("basic_info", "from"),
        "release_date":  ("basic_info", "release_date"),
        "is_new":        ("basic_info", "is_new"),
    },
}
REGION_KEYS = ("cn",)
CN_REGION_SOURCE_LABELS = {"cn", "divingfish"}
REGION_ALIASES = {
    "cn": "cn",
    "china": "cn",
    "国服": "cn",
    "国行": "cn",
    "国版": "cn",
    "中二": "cn",
    "中国": "cn",
}
FIT_LABEL_ALIASES = {
    "虚高": "虚高",
    "高": "虚高",
    "over": "虚高",
    "overrated": "虚高",
    "high": "虚高",
    "plus": "虚高",
    "虚低": "虚低",
    "低": "虚低",
    "under": "虚低",
    "underrated": "虚低",
    "low": "虚低",
    "minus": "虚低",
}
SORT_ALIASES = {
    "fit_delta_desc": "fit_delta_desc",
    "delta_desc": "fit_delta_desc",
    "most_over": "fit_delta_desc",
    "虚高": "fit_delta_desc",
    "最虚高": "fit_delta_desc",
    "fit_delta_asc": "fit_delta_asc",
    "delta_asc": "fit_delta_asc",
    "most_under": "fit_delta_asc",
    "虚低": "fit_delta_asc",
    "最虚低": "fit_delta_asc",
    "fit_diff_asc": "fit_diff_asc",
    "fit_asc": "fit_diff_asc",
    "fit_diff_desc": "fit_diff_desc",
    "fit_desc": "fit_diff_desc",
}
NUMBER_PATTERN = r"[+-]?(?:\d+(?:\.\d*)?|\.\d+)"


class SearchError(ValueError):
    """Raised when a query cannot be searched."""


_S2T_TABLE: dict[int, int] | None = None


def _s2t_translation_table() -> dict[int, int]:
    """懒加载简体→繁体单字映射，转成 str.translate() 用的 codepoint 表。
    保持表只构造一次，clear_search_caches 不重置它（s2t 是静态数据）。"""
    global _S2T_TABLE
    if _S2T_TABLE is None:
        if DEFAULT_S2T_PATH.exists():
            with DEFAULT_S2T_PATH.open("r", encoding="utf-8") as file:
                mapping = json.load(file)
            if isinstance(mapping, dict):
                _S2T_TABLE = str.maketrans({
                    str(k): str(v)
                    for k, v in mapping.items()
                    if isinstance(k, str) and isinstance(v, str) and len(k) == 1
                })
            else:
                _S2T_TABLE = {}
        else:
            _S2T_TABLE = {}
    return _S2T_TABLE


def normalize_text(value: Any) -> str:
    """归一化文本：NFKC 全半角统一 → casefold 大小写不敏感 → strip → 简→繁。
    繁体 canonical 让简繁、半全角、英文大小写都能落到同一桶里互相匹配。"""
    text = "" if value is None else str(value)
    text = unicodedata.normalize("NFKC", text).casefold().strip()
    table = _s2t_translation_table()
    if table:
        text = text.translate(table)
    return text


_T2S_TABLE: dict[int, int] | None = None


def _t2s_translation_table() -> dict[int, int]:
    """繁→简映射表，跟 _s2t_translation_table 同构，仅在输出层用，存储不动。"""
    global _T2S_TABLE
    if _T2S_TABLE is None:
        if DEFAULT_T2S_PATH.exists():
            with DEFAULT_T2S_PATH.open("r", encoding="utf-8") as file:
                mapping = json.load(file)
            if isinstance(mapping, dict):
                _T2S_TABLE = str.maketrans({
                    str(k): str(v)
                    for k, v in mapping.items()
                    if isinstance(k, str) and isinstance(v, str) and len(k) == 1
                })
            else:
                _T2S_TABLE = {}
        else:
            _T2S_TABLE = {}
    return _T2S_TABLE


def to_simplified(value: Any) -> str:
    """繁→简单字翻译。仅用于输出展示（如 aliases 显示），不影响存储或匹配。"""
    if value is None:
        return ""
    text = str(value)
    table = _t2s_translation_table()
    if table:
        text = text.translate(table)
    return text


def normalize_level(value: Any) -> str:
    text = normalize_text(value)
    text = text.replace("级", "").replace("等級", "")
    text = re.sub(r"\s+", "", text)
    return text


@lru_cache(maxsize=16)
def load_single_music_data(data_path: Path) -> list[dict[str, Any]]:
    if not data_path.exists():
        raise SearchError(
            f"Missing music data: {data_path}. Run scripts/update_music_data.py first."
        )
    with data_path.open("r", encoding="utf-8") as file:
        payload = json.load(file)
    data = payload.get("songs", payload) if isinstance(payload, dict) else payload
    if not isinstance(data, list):
        raise SearchError(f"Invalid music data format in {data_path}")
    return data


@lru_cache(maxsize=8)
def load_music_data(
    data_path: Path = DEFAULT_DATA_PATH,
    divingfish_path: Path = DEFAULT_DIVINGFISH_DATA_PATH,
) -> list[dict[str, Any]]:
    data: list[dict[str, Any]] = []
    songs_by_title: dict[str, list[dict[str, Any]]] = {}
    songs_by_id: dict[str, dict[str, Any]] = {}

    def source_numeric_id(music: dict[str, Any]) -> str:
        source_id = raw_source_id(music)
        if raw_numeric_song_id(source_id) is None:
            return ""
        return normalize_song_id(source_id)

    def add_title_index(title: Any, music: dict[str, Any]) -> None:
        title_key = normalize_text(title)
        if not title_key:
            return
        bucket = songs_by_title.setdefault(title_key, [])
        if music not in bucket:
            bucket.append(music)

    def index_music(source_music: dict[str, Any], merged_music: dict[str, Any]) -> None:
        add_title_index(source_music.get("title", ""), merged_music)
        source_id = source_numeric_id(source_music)
        if source_id and source_id not in songs_by_id:
            songs_by_id[source_id] = merged_music

    for source_music in load_single_music_data(data_path):
        music = dict(source_music)
        music["_source_records"] = {"cn": source_music}
        data.append(music)
        index_music(source_music, music)

    def title_match_score(source_label: str, candidate: dict[str, Any], title_key: str) -> tuple[int, int]:
        del source_label
        records = source_records(candidate)
        if normalize_text(records.get("cn", {}).get("title", "")) == title_key:
            return (2, len(records))
        return (1, len(records))

    def title_merge_target(source_label: str, music: dict[str, Any]) -> dict[str, Any] | None:
        title = normalize_text(music.get("title", ""))
        if not title:
            return None
        candidates = [
            candidate
            for candidate in songs_by_title.get(title, [])
            if source_label not in source_records(candidate)
        ]
        if not candidates:
            return None
        return max(candidates, key=lambda candidate: title_match_score(source_label, candidate, title))

    def attach_source(source_label: str, target: dict[str, Any], music: dict[str, Any]) -> bool:
        records = target.setdefault("_source_records", {})
        if source_label in records:
            return False
        records[source_label] = music
        index_music(music, target)
        return True

    def merge_source(source_label: str, music: dict[str, Any], *, prefer_numeric_id: bool = False) -> None:
        source_id = source_numeric_id(music)
        if source_id and source_id in songs_by_id and attach_source(source_label, songs_by_id[source_id], music):
            return
        if not prefer_numeric_id:
            target = title_merge_target(source_label, music)
            if target is not None and attach_source(source_label, target, music):
                return
        # Rare duplicate title/source or a numeric source ID that does not exist
        # in CN: keep it as a separate searchable row instead of title-merging a
        # remaster/revival song with an older same-title record.
        merged = dict(music)
        merged["_source_records"] = {source_label: music}
        data.append(merged)
        index_music(music, merged)

    if divingfish_path.exists():
        for music in load_single_music_data(divingfish_path):
            merge_source("divingfish", music, prefer_numeric_id=True)

    return data


@lru_cache(maxsize=8)
def load_chart_stats(chart_stats_path: Path = DEFAULT_CHART_STATS_PATH) -> dict[str, list[dict[str, Any]]]:
    if not chart_stats_path.exists():
        return {}
    with chart_stats_path.open("r", encoding="utf-8") as file:
        payload = json.load(file)
    charts = payload.get("charts", payload) if isinstance(payload, dict) else payload
    if not isinstance(charts, dict):
        raise SearchError(f"Invalid chart stats format in {chart_stats_path}")
    result: dict[str, list[dict[str, Any]]] = {}
    for raw_id, chart_items in charts.items():
        if not isinstance(chart_items, list):
            continue
        normalized_items: list[dict[str, Any]] = []
        for item in chart_items:
            normalized_items.append(item if isinstance(item, dict) else {})
        result[str(raw_id)] = normalized_items
    return result


@lru_cache(maxsize=8)
def load_name_alias_map(alias_path: Path) -> dict[str, list[str]]:
    """加载 `{canonical: [alias, ...]}` 形式的别名字典（artist / charter 共用同一格式）。"""
    if not alias_path.exists():
        return {}
    with alias_path.open("r", encoding="utf-8") as file:
        payload = json.load(file)
    if not isinstance(payload, dict):
        return {}
    result: dict[str, list[str]] = {}
    for key, value in payload.items():
        canonical = str(key).strip()
        if not canonical or not isinstance(value, list):
            continue
        aliases = [str(item).strip() for item in value if str(item).strip()]
        if aliases:
            result[canonical] = aliases
    return result


def _name_fuzzy_hit(needle: str, names_norm: list[str]) -> bool:
    """双向子串匹配的单点定义。needle 命中任一别名（任一方向 substring）即返回 True。

    bidirectional 是有意为之：除了"用户输入是别名的子串"（如 'sasakure' 命中
    'sasakure.UK'）外，反向也支持"已存别名是用户输入的子串"（如已存别名
    'sasakure' 命中用户输入 'sasakure.UK 笹倉'）。代价是手存单字符别名时
    会过度展开（如 'j' → 命中所有含 j 的查询），但这只在用户主动选短别名
    时触发，属于他自己的决策。
    """
    if not needle or not names_norm:
        return False
    return any((needle in n) or (n in needle) for n in names_norm if n)


def expand_name_query(needle: str, alias_map: dict[str, list[str]]) -> set[str]:
    """needle 命中任一别名组就把整组成员都纳入候选，返回 normalize 后的候选集合。"""
    if not needle or not alias_map:
        return set()
    candidates: set[str] = set()
    for canonical, aliases in alias_map.items():
        names_norm = [n for n in (normalize_text(canonical), *map(normalize_text, aliases)) if n]
        if _name_fuzzy_hit(needle, names_norm):
            candidates.update(names_norm)
    return candidates


@lru_cache(maxsize=8)
def load_search_context(
    data_path: Path = DEFAULT_DATA_PATH,
    alias_path: Path = DEFAULT_ALIAS_PATH,
    chart_stats_path: Path = DEFAULT_CHART_STATS_PATH,
    pinyin_alias_path: Path = DEFAULT_PINYIN_ALIAS_PATH,
    pinyin_alias_bucket: int | None = None,
) -> tuple[
    list[dict[str, Any]],
    dict[str, list[str]],
    dict[str, list[str]],
    dict[str, list[dict[str, Any]]],
]:
    _ = pinyin_alias_bucket  # lru_cache key: rotate the pinyin alias library every TTL bucket.
    data = load_music_data(data_path)
    alias_map = load_alias_map(data, alias_path=alias_path)
    pinyin_alias_map = load_pinyin_alias_map(
        data,
        alias_map,
        pinyin_alias_path=pinyin_alias_path,
        source_paths=_pinyin_alias_source_paths(data_path=data_path, alias_path=alias_path),
    )
    chart_stats = load_chart_stats(chart_stats_path)
    return data, alias_map, pinyin_alias_map, chart_stats


def _source_file_revision(path: Path) -> tuple[str, int, int]:
    resolved = Path(path).expanduser().resolve(strict=False)
    try:
        stat = resolved.stat()
    except OSError:
        return str(resolved), 0, 0
    return str(resolved), int(stat.st_mtime_ns), int(stat.st_size)


def search_source_revision(
    *,
    data_path: Path = DEFAULT_DATA_PATH,
    alias_path: Path = DEFAULT_ALIAS_PATH,
    chart_stats_path: Path = DEFAULT_CHART_STATS_PATH,
    pinyin_alias_path: Path = DEFAULT_PINYIN_ALIAS_PATH,
    artist_alias_path: Path = DEFAULT_ARTIST_ALIAS_PATH,
    charter_alias_path: Path = DEFAULT_CHARTER_ALIAS_PATH,
) -> tuple[tuple[str, int, int], ...]:
    """返回公开曲库、别名和统计文件的版本，用于常驻进程失效缓存。"""

    paths = (
        data_path,
        alias_path,
        chart_stats_path,
        pinyin_alias_path,
        artist_alias_path,
        charter_alias_path,
        DEFAULT_DIVINGFISH_DATA_PATH,
        DEFAULT_CUSTOM_ALIAS_PATH,
        DEFAULT_YUZU_ALIAS_PATH,
        DEFAULT_LEGACY_ALIAS_CSV_PATH,
        DEFAULT_S2T_PATH,
        DEFAULT_T2S_PATH,
    )
    revisions: dict[str, tuple[str, int, int]] = {}
    for path in paths:
        revision = _source_file_revision(path)
        revisions[revision[0]] = revision
    return tuple(revisions[key] for key in sorted(revisions))


def refresh_search_caches_if_sources_changed(
    *,
    data_path: Path = DEFAULT_DATA_PATH,
    alias_path: Path = DEFAULT_ALIAS_PATH,
    chart_stats_path: Path = DEFAULT_CHART_STATS_PATH,
    pinyin_alias_path: Path = DEFAULT_PINYIN_ALIAS_PATH,
    artist_alias_path: Path = DEFAULT_ARTIST_ALIAS_PATH,
    charter_alias_path: Path = DEFAULT_CHARTER_ALIAS_PATH,
) -> bool:
    """源文件被外部刷新后清空内存缓存；返回本次是否发生失效。"""

    global _SEARCH_SOURCE_REVISION
    revision = search_source_revision(
        data_path=data_path,
        alias_path=alias_path,
        chart_stats_path=chart_stats_path,
        pinyin_alias_path=pinyin_alias_path,
        artist_alias_path=artist_alias_path,
        charter_alias_path=charter_alias_path,
    )
    previous = _SEARCH_SOURCE_REVISION
    if previous is None:
        _SEARCH_SOURCE_REVISION = revision
        return False
    if previous == revision:
        return False
    clear_search_caches()
    _SEARCH_SOURCE_REVISION = revision
    return True


def clear_search_caches() -> None:
    global _SEARCH_SOURCE_REVISION
    load_single_music_data.cache_clear()
    load_music_data.cache_clear()
    load_chart_stats.cache_clear()
    load_name_alias_map.cache_clear()
    load_search_context.cache_clear()
    pinyin_query_needles.cache_clear()
    _SEARCH_SOURCE_REVISION = None


def source_label_for_music(music: dict[str, Any]) -> str:
    if "difficulties" in music:
        return "cn"
    return "divingfish"


def source_records(music: dict[str, Any]) -> dict[str, dict[str, Any]]:
    records = music.get("_source_records")
    if isinstance(records, dict) and records:
        return {
            str(label): record
            for label, record in records.items()
            if isinstance(record, dict)
        }
    return {source_label_for_music(music): music}


def primary_source_record(music: dict[str, Any]) -> tuple[str, dict[str, Any]]:
    records = source_records(music)
    for label in SOURCE_PRIORITY:
        if label in records:
            return label, records[label]
    label, record = next(iter(records.items()))
    return label, record


def source_name(label: str) -> str:
    return SOURCE_NAMES.get(label, label)


def unique_preserve_order(values: list[str]) -> list[str]:
    seen: set[str] = set()
    result: list[str] = []
    for value in values:
        normalized = normalize_text(value)
        if not normalized or normalized in seen:
            continue
        seen.add(normalized)
        result.append(value)
    return result


def unique_values(values: list[Any]) -> list[Any]:
    seen: set[str] = set()
    result: list[Any] = []
    for value in values:
        if value in (None, ""):
            continue
        normalized = normalize_text(value)
        if not normalized or normalized in seen:
            continue
        seen.add(normalized)
        result.append(value)
    return result


def load_alias_map(
    music_data: list[dict[str, Any]],
    alias_path: Path = DEFAULT_ALIAS_PATH,
    custom_alias_path: Path = DEFAULT_CUSTOM_ALIAS_PATH,
    yuzu_alias_path: Path = DEFAULT_YUZU_ALIAS_PATH,
    legacy_alias_csv_path: Path = DEFAULT_LEGACY_ALIAS_CSV_PATH,
) -> dict[str, list[str]]:
    aliases_by_id: dict[str, list[str]] = {}

    for music in music_data:
        for source_music in source_records(music).values():
            embedded_aliases = source_music.get("aliases")
            if isinstance(embedded_aliases, list):
                song_id = normalize_song_id(source_music.get("id", source_music.get("songId", "")))
                aliases_by_id.setdefault(song_id, []).extend(
                    str(alias) for alias in embedded_aliases if str(alias).strip()
                )

    if alias_path.exists():
        with alias_path.open("r", encoding="utf-8") as file:
            payload = json.load(file)
        content = payload.get("aliases", payload.get("content", payload)) if isinstance(payload, dict) else payload
        if isinstance(content, list):
            for item in content:
                if not isinstance(item, dict):
                    continue
                raw_id = item.get("song_id", item.get("SongID"))
                if raw_id is None:
                    continue
                song_id = normalize_song_id(raw_id)
                aliases = item.get("aliases", item.get("Alias", []))
                aliases = [str(alias) for alias in aliases if str(alias).strip()]
                aliases_by_id.setdefault(song_id, []).extend(aliases)

    if yuzu_alias_path.exists():
        with yuzu_alias_path.open("r", encoding="utf-8") as file:
            payload = json.load(file)
        content = payload.get("content", payload) if isinstance(payload, dict) else payload
        if isinstance(content, list):
            for item in content:
                if not isinstance(item, dict) or "SongID" not in item:
                    continue
                song_id = normalize_song_id(item["SongID"])
                aliases = [str(alias) for alias in item.get("Alias", []) if str(alias).strip()]
                aliases_by_id.setdefault(song_id, []).extend(aliases)

    if custom_alias_path.exists():
        with custom_alias_path.open("r", encoding="utf-8") as file:
            payload = json.load(file)
        if isinstance(payload, dict):
            for raw_id, aliases in payload.items():
                song_id = normalize_song_id(raw_id)
                if isinstance(aliases, list):
                    aliases_by_id.setdefault(song_id, []).extend(
                        str(alias) for alias in aliases if str(alias).strip()
                    )

    # title → canonical song_id 反查表，供 legacy CSV 别名导入使用。
    title_to_ids: dict[str, list[str]] | None = None

    def _ensure_title_index() -> dict[str, list[str]]:
        nonlocal title_to_ids
        if title_to_ids is None:
            title_to_ids = {}
            for music in music_data:
                ids = music_id_values(music)
                for title in music_title_values(music):
                    title_to_ids.setdefault(normalize_text(title), []).extend(ids)
        return title_to_ids

    if legacy_alias_csv_path.exists():
        index = _ensure_title_index()
        with legacy_alias_csv_path.open("r", encoding="utf-8-sig", newline="") as file:
            for row in csv.reader(file):
                if not row or row[0] == "歌名":
                    continue
                title = normalize_text(row[0])
                song_ids = index.get(title, [])
                if not song_ids:
                    continue
                row_aliases = [alias.strip() for alias in row[1:] if alias.strip()]
                for song_id in song_ids:
                    aliases_by_id.setdefault(song_id, []).extend(row_aliases)

    return {
        song_id: unique_preserve_order(aliases)
        for song_id, aliases in aliases_by_id.items()
    }


def current_pinyin_alias_bucket(now: float | None = None) -> int:
    timestamp = time.time() if now is None else now
    return int(timestamp // PINYIN_ALIAS_TTL_SECONDS)


def _pinyin_alias_source_paths(
    data_path: Path = DEFAULT_DATA_PATH,
    alias_path: Path = DEFAULT_ALIAS_PATH,
    custom_alias_path: Path = DEFAULT_CUSTOM_ALIAS_PATH,
    yuzu_alias_path: Path = DEFAULT_YUZU_ALIAS_PATH,
    legacy_alias_csv_path: Path = DEFAULT_LEGACY_ALIAS_CSV_PATH,
) -> list[Path]:
    return [
        data_path,
        DEFAULT_DIVINGFISH_DATA_PATH,
        alias_path,
        custom_alias_path,
        yuzu_alias_path,
        legacy_alias_csv_path,
    ]


def _load_pinyin_alias_file(path: Path) -> dict[str, list[str]]:
    if not path.exists():
        return {}
    with path.open("r", encoding="utf-8") as file:
        payload = json.load(file)
    content = payload.get("aliases", payload) if isinstance(payload, dict) else payload
    aliases_by_id: dict[str, list[str]] = {}
    if isinstance(content, dict):
        for raw_id, aliases in content.items():
            if isinstance(aliases, list):
                aliases_by_id[normalize_song_id(raw_id)] = [
                    str(alias) for alias in aliases if str(alias).strip()
                ]
    elif isinstance(content, list):
        for item in content:
            if not isinstance(item, dict):
                continue
            raw_id = item.get("song_id", item.get("SongID"))
            aliases = item.get("aliases", item.get("Alias", []))
            if raw_id is None or not isinstance(aliases, list):
                continue
            aliases_by_id.setdefault(normalize_song_id(raw_id), []).extend(
                str(alias) for alias in aliases if str(alias).strip()
            )
    return {
        song_id: unique_preserve_order(aliases)
        for song_id, aliases in aliases_by_id.items()
    }


def _pinyin_alias_file_current(
    pinyin_alias_path: Path,
    source_paths: list[Path],
    *,
    ttl_seconds: int = PINYIN_ALIAS_TTL_SECONDS,
) -> bool:
    if not pinyin_alias_path.exists():
        return False
    try:
        alias_mtime = pinyin_alias_path.stat().st_mtime
    except OSError:
        return False
    try:
        with pinyin_alias_path.open("r", encoding="utf-8") as file:
            payload = json.load(file)
    except (OSError, json.JSONDecodeError):
        return False
    if isinstance(payload, dict) and payload.get("version") != PINYIN_ALIAS_FORMAT_VERSION:
        return False
    if time.time() - alias_mtime >= ttl_seconds:
        return False
    source_mtimes = [
        path.stat().st_mtime
        for path in source_paths
        if path.exists()
    ]
    return not source_mtimes or alias_mtime >= max(source_mtimes)


_CJK_RE = re.compile(r"[\u3400-\u9fff]")


def _valid_pinyin_alias(value: str) -> bool:
    compact = re.sub(r"\s+", "", normalize_text(value))
    return len(compact) >= 3 and any("a" <= char <= "z" for char in compact)


def _pinyin_forms_for_text(value: Any, lazy_pinyin: Any, style: Any) -> list[str]:
    text = str(value).strip()
    if not text or not _CJK_RE.search(text):
        return []
    syllables: list[str] = []
    initials: list[str] = []
    for char in text:
        if _CJK_RE.fullmatch(char):
            char_syllables = [
                item for item in lazy_pinyin(char, style=style.NORMAL, errors="ignore")
                if item
            ]
            char_initials = [
                item for item in lazy_pinyin(char, style=style.FIRST_LETTER, errors="ignore")
                if item
            ]
            if char_syllables:
                syllables.extend(char_syllables)
            if char_initials:
                initials.extend(char_initials)
            continue
        if char.isascii() and char.isalnum():
            normalized = normalize_text(char)
            if normalized:
                syllables.append(normalized)
                initials.append(normalized)
    if not syllables or not any(any("a" <= c <= "z" for c in token) for token in syllables):
        return []
    candidates = [
        "".join(syllables),
        " ".join(syllables),
        "".join(initials),
    ]
    return unique_preserve_order(
        normalize_text(candidate)
        for candidate in candidates
        if _valid_pinyin_alias(candidate)
    )


@lru_cache(maxsize=1024)
def pinyin_query_needles(query: str | None) -> tuple[str, ...]:
    needle = normalize_text(query) if query else ""
    if not needle:
        return tuple()
    candidates = [needle]
    if query and _CJK_RE.search(str(query)):
        try:
            from pypinyin import Style, lazy_pinyin
        except ImportError:
            pass
        else:
            candidates.extend(_pinyin_forms_for_text(query, lazy_pinyin, Style))
    return tuple(unique_preserve_order(candidate for candidate in candidates if candidate))


def build_pinyin_alias_map(
    music_data: list[dict[str, Any]],
    alias_map: dict[str, list[str]],
) -> dict[str, list[str]] | None:
    try:
        from pypinyin import Style, lazy_pinyin
    except ImportError:
        return None

    aliases_by_id: dict[str, list[str]] = {}
    for music in music_data:
        song_ids = music_id_values(music)
        text_values: list[Any] = [*music_title_values(music)]
        for song_id in song_ids:
            text_values.extend(alias_map.get(song_id, []))
        pinyin_aliases: list[str] = []
        for value in unique_preserve_order(str(item) for item in text_values if str(item).strip()):
            pinyin_aliases.extend(_pinyin_forms_for_text(value, lazy_pinyin, Style))
        pinyin_aliases = unique_preserve_order(pinyin_aliases)
        if not pinyin_aliases:
            continue
        for song_id in song_ids:
            aliases_by_id.setdefault(song_id, []).extend(pinyin_aliases)

    return {
        song_id: unique_preserve_order(aliases)
        for song_id, aliases in aliases_by_id.items()
    }


def write_pinyin_alias_file(
    pinyin_alias_path: Path,
    pinyin_alias_map: dict[str, list[str]],
) -> None:
    pinyin_alias_path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "generated_at": int(time.time()),
        "ttl_seconds": PINYIN_ALIAS_TTL_SECONDS,
        "version": PINYIN_ALIAS_FORMAT_VERSION,
        "aliases": [
            {"song_id": song_id, "aliases": aliases}
            for song_id, aliases in sorted(pinyin_alias_map.items())
        ],
    }
    temp = pinyin_alias_path.with_name(
        f".{pinyin_alias_path.name}.{int(time.time() * 1000)}.tmp"
    )
    with temp.open("w", encoding="utf-8") as file:
        json.dump(payload, file, ensure_ascii=False, indent=2, sort_keys=True)
        file.write("\n")
    temp.replace(pinyin_alias_path)


def load_pinyin_alias_map(
    music_data: list[dict[str, Any]],
    alias_map: dict[str, list[str]],
    *,
    pinyin_alias_path: Path = DEFAULT_PINYIN_ALIAS_PATH,
    source_paths: list[Path] | None = None,
) -> dict[str, list[str]]:
    sources = source_paths or _pinyin_alias_source_paths()
    if _pinyin_alias_file_current(pinyin_alias_path, sources):
        return _load_pinyin_alias_file(pinyin_alias_path)

    generated = build_pinyin_alias_map(music_data, alias_map)
    if generated is None:
        return _load_pinyin_alias_file(pinyin_alias_path)

    try:
        write_pinyin_alias_file(pinyin_alias_path, generated)
    except OSError:
        pass
    return generated


def save_custom_alias(
    *,
    song_id: str | int | None = None,
    title: str | None = None,
    alias: str,
    data_path: Path = DEFAULT_DATA_PATH,
    custom_alias_path: Path = DEFAULT_CUSTOM_ALIAS_PATH,
) -> dict[str, Any]:
    alias = str(alias).strip()
    if not alias:
        raise SearchError("alias is required")
    if song_id in (None, "") and not title:
        raise SearchError("song_id or title is required")

    data = load_music_data(data_path)
    target: dict[str, Any] | None = None
    if song_id not in (None, ""):
        normalized_id = normalize_song_id(song_id)
        target = next(
            (
                music
                for music in data
                if normalized_id in music_id_values(music)
            ),
            None,
        )
    else:
        title_key = normalize_text(title)
        matches = [
            music
            for music in data
            if any(normalize_text(value) == title_key for value in music_title_values(music))
        ]
        if len(matches) > 1:
            raise SearchError(f"multiple songs matched title {title!r}; use song_id instead")
        target = matches[0] if matches else None

    if target is None:
        raise SearchError("song not found")

    target_id = canonical_song_id(target)
    custom_alias_path.parent.mkdir(parents=True, exist_ok=True)
    if custom_alias_path.exists():
        with custom_alias_path.open("r", encoding="utf-8") as file:
            payload = json.load(file)
        if not isinstance(payload, dict):
            payload = {}
    else:
        payload = {}

    aliases = payload.setdefault(target_id, [])
    if not isinstance(aliases, list):
        aliases = []
        payload[target_id] = aliases

    existed = normalize_text(alias) in {normalize_text(item) for item in aliases}
    if not existed:
        aliases.append(alias)
        with custom_alias_path.open("w", encoding="utf-8") as file:
            json.dump(payload, file, ensure_ascii=False, indent=2, sort_keys=True)
            file.write("\n")
        clear_search_caches()

    return {
        "song_id": target_id,
        "source_id": primary_source_id(target),
        "source_ids": source_ids(target),
        "title": primary_title(target),
        "alias": alias,
        "existed": existed,
        "aliases": [to_simplified(a) for a in aliases],
        "document": str(custom_alias_path),
    }


def delete_custom_alias(
    *,
    song_id: str | int | None = None,
    title: str | None = None,
    alias: str,
    data_path: Path = DEFAULT_DATA_PATH,
    custom_alias_path: Path = DEFAULT_CUSTOM_ALIAS_PATH,
) -> dict[str, Any]:
    alias = str(alias).strip()
    if not alias:
        raise SearchError("alias is required")
    if song_id in (None, "") and not title:
        raise SearchError("song_id or title is required")

    data = load_music_data(data_path)
    target: dict[str, Any] | None = None
    if song_id not in (None, ""):
        normalized_id = normalize_song_id(song_id)
        target = next(
            (
                music
                for music in data
                if normalized_id in music_id_values(music)
            ),
            None,
        )
    else:
        title_key = normalize_text(title)
        matches = [
            music
            for music in data
            if any(normalize_text(value) == title_key for value in music_title_values(music))
        ]
        if len(matches) > 1:
            raise SearchError(f"multiple songs matched title {title!r}; use song_id instead")
        target = matches[0] if matches else None

    if target is None:
        raise SearchError("song not found")

    target_id = canonical_song_id(target)

    if not custom_alias_path.exists():
        raise SearchError("no custom aliases file found")

    with custom_alias_path.open("r", encoding="utf-8") as file:
        payload = json.load(file)
    if not isinstance(payload, dict):
        raise SearchError("invalid custom aliases file")

    aliases = payload.get(target_id)
    if not isinstance(aliases, list) or not aliases:
        raise SearchError(f"no custom aliases for song {target_id}")

    normalized_alias = normalize_text(alias)
    matching = [item for item in aliases if normalize_text(item) == normalized_alias]
    if not matching:
        raise SearchError(f"alias {alias!r} not found for song {target_id}")

    removed = matching[0]
    aliases.remove(removed)
    if not aliases:
        del payload[target_id]
    with custom_alias_path.open("w", encoding="utf-8") as file:
        json.dump(payload, file, ensure_ascii=False, indent=2, sort_keys=True)
        file.write("\n")
    clear_search_caches()

    return {
        "song_id": target_id,
        "source_id": primary_source_id(target),
        "source_ids": source_ids(target),
        "title": primary_title(target),
        "removed_alias": removed,
        "remaining_aliases": [to_simplified(a) for a in aliases],
        "document": str(custom_alias_path),
    }


_ALIAS_KIND_ARTIST_KEYS = {normalize_text(s).replace(" ", "") for s in
    ("artist", "曲师", "曲作者", "作者", "曲師")}
_ALIAS_KIND_CHARTER_KEYS = {normalize_text(s).replace(" ", "") for s in
    ("charter", "notedesigner", "谱师", "谱面作者", "譜師", "譜面作者")}


def _normalize_name_alias_kind(kind: Any) -> tuple[str, Path]:
    # 比较时一律走 normalize_text 后的形式，这样 s2t 归一也不会让"曲师"漏掉
    text = normalize_text(kind).replace(" ", "")
    if text in _ALIAS_KIND_ARTIST_KEYS:
        return "artist", DEFAULT_ARTIST_ALIAS_PATH
    if text in _ALIAS_KIND_CHARTER_KEYS:
        return "charter", DEFAULT_CHARTER_ALIAS_PATH
    raise SearchError("kind must be artist or charter")


def _canonical_known(
    kind_norm: str,
    canonical: str,
    music_data: list[dict[str, Any]],
) -> bool:
    """检查 canonical 是否能匹配 music_data 里任何 artist / charter 字段。
    用 normalize_text 后的双向子串（跟实际 search 时 expand_name_query 同语义），
    所以"sasakure"能匹配上"sasakure.UK"——给用户主动用缩写命名 canonical 的余地。
    """
    needle = normalize_text(canonical)
    if not needle:
        return False
    if kind_norm == "artist":
        for music in music_data:
            for value in music_artist_values(music):
                v = normalize_text(value)
                if v and ((needle in v) or (v in needle)):
                    return True
    else:  # charter
        for music in music_data:
            for chart in all_charts(music):
                v = normalize_text(chart.get("charter") or "")
                if v and ((needle in v) or (v in needle)):
                    return True
    return False


def save_name_alias(
    *,
    kind: Any,
    canonical: str,
    alias: str,
    alias_path: Path | None = None,
    data_path: Path = DEFAULT_DATA_PATH,
) -> dict[str, Any]:
    kind_norm, default_path = _normalize_name_alias_kind(kind)
    path = alias_path or default_path
    canonical = str(canonical).strip()
    alias = str(alias).strip()
    if not canonical:
        raise SearchError(f"{kind_norm} canonical name is required")
    if not alias:
        raise SearchError("alias is required")

    path.parent.mkdir(parents=True, exist_ok=True)
    payload: dict[str, list[str]] = {}
    if path.exists():
        with path.open("r", encoding="utf-8") as file:
            loaded = json.load(file)
        if isinstance(loaded, dict):
            payload = loaded

    aliases = payload.setdefault(canonical, [])
    if not isinstance(aliases, list):
        aliases = []
        payload[canonical] = aliases

    normalized_alias = normalize_text(alias)
    existed = normalized_alias in {normalize_text(item) for item in aliases}
    # 如果 alias 等同于 canonical 自己，也算冗余
    if normalized_alias == normalize_text(canonical):
        existed = True

    if not existed:
        aliases.append(alias)
        with path.open("w", encoding="utf-8") as file:
            json.dump(payload, file, ensure_ascii=False, indent=2, sort_keys=True)
            file.write("\n")
        clear_search_caches()

    # canonical 存在性提示：写入不阻断，但回显 warning 让 caller 知道这个 alias
    # 永远命不中（典型场景：canonical 拼写错误，例如 'sasakuxe.UK'）。
    warning: str | None = None
    try:
        music_data = load_music_data(data_path)
        if not _canonical_known(kind_norm, canonical, music_data):
            warning = (
                f"warning: canonical {canonical!r} did not match any {kind_norm} "
                f"in the local music library — this alias will never be hit unless "
                f"the canonical name is exactly typed as it appears in the data."
            )
    except SearchError:
        # 曲库加载不上就跳过校验，不阻断写入
        pass

    result: dict[str, Any] = {
        "kind": kind_norm,
        "canonical": canonical,
        "alias": alias,
        "existed": existed,
        "aliases": [to_simplified(a) for a in aliases],
        "document": str(path),
    }
    if warning:
        result["warning"] = warning
    return result


def delete_name_alias(
    *,
    kind: Any,
    canonical: str,
    alias: str,
    alias_path: Path | None = None,
) -> dict[str, Any]:
    kind_norm, default_path = _normalize_name_alias_kind(kind)
    path = alias_path or default_path
    canonical = str(canonical).strip()
    alias = str(alias).strip()
    if not canonical:
        raise SearchError(f"{kind_norm} canonical name is required")
    if not alias:
        raise SearchError("alias is required")

    if not path.exists():
        raise SearchError(f"no {kind_norm} aliases file found")

    with path.open("r", encoding="utf-8") as file:
        payload = json.load(file)
    if not isinstance(payload, dict):
        raise SearchError(f"invalid {kind_norm} aliases file")

    aliases = payload.get(canonical)
    if not isinstance(aliases, list) or not aliases:
        raise SearchError(f"no aliases for {kind_norm} {canonical!r}")

    normalized_alias = normalize_text(alias)
    matching = [item for item in aliases if normalize_text(item) == normalized_alias]
    if not matching:
        raise SearchError(f"alias {alias!r} not found for {kind_norm} {canonical!r}")

    removed = matching[0]
    aliases.remove(removed)
    if not aliases:
        del payload[canonical]
    with path.open("w", encoding="utf-8") as file:
        json.dump(payload, file, ensure_ascii=False, indent=2, sort_keys=True)
        file.write("\n")
    clear_search_caches()

    return {
        "kind": kind_norm,
        "canonical": canonical,
        "removed_alias": removed,
        "remaining_aliases": [to_simplified(a) for a in aliases],
        "document": str(path),
    }


def list_name_aliases(
    *,
    kind: Any,
    query: str | None = None,
    alias_path: Path | None = None,
) -> dict[str, Any]:
    kind_norm, default_path = _normalize_name_alias_kind(kind)
    path = alias_path or default_path
    if not path.exists():
        return {"kind": kind_norm, "count": 0, "entries": [], "document": str(path)}
    with path.open("r", encoding="utf-8") as file:
        payload = json.load(file)
    if not isinstance(payload, dict):
        return {"kind": kind_norm, "count": 0, "entries": [], "document": str(path)}

    needle = normalize_text(query) if query else ""
    entries: list[dict[str, Any]] = []
    for canonical, aliases in sorted(payload.items()):
        if not isinstance(aliases, list):
            continue
        names_norm = [normalize_text(canonical), *[normalize_text(a) for a in aliases]]
        if needle and not _name_fuzzy_hit(needle, names_norm):
            continue
        # canonical 保持原文（用户主动选定的官方名），别名显示时繁→简
        display_aliases = [to_simplified(a) for a in aliases]
        entries.append({"canonical": canonical, "aliases": display_aliases, "count": len(aliases)})
    return {
        "kind": kind_norm,
        "count": len(entries),
        "entries": entries,
        "document": str(path),
    }


def parse_difficulty(value: Any) -> int | None:
    if value is None or value == "":
        return None
    key = normalize_text(value).replace(" ", "")
    if key not in DIFFICULTY_ALIASES:
        raise SearchError(
            "difficulty must be one of Basic/Advanced/Expert/Master/Re:MASTER "
            "or 绿/黄/红/紫/白"
        )
    return DIFFICULTY_ALIASES[key]


def parse_ds_range(
    *,
    ds: Any = None,
    ds_min: float | int | str | None = None,
    ds_max: float | int | str | None = None,
) -> tuple[float | None, float | None]:
    if ds_min is not None or ds_max is not None:
        return (
            float(ds_min) if ds_min is not None else None,
            float(ds_max) if ds_max is not None else None,
        )

    if ds is None or ds == "":
        return None, None

    if isinstance(ds, int | float):
        value = float(ds)
        return value, value

    text = normalize_text(ds)
    text = text.replace("~", "-").replace("～", "-").replace("..", "-")
    if "-" in text:
        left, right = text.split("-", 1)
        return (float(left) if left else None, float(right) if right else None)

    value = float(text)
    return value, value


def parse_float_range(
    *,
    value: Any = None,
    low: float | int | str | None = None,
    high: float | int | str | None = None,
) -> tuple[float | None, float | None]:
    if low is not None or high is not None:
        return (
            float(low) if low is not None else None,
            float(high) if high is not None else None,
        )
    if value is None or value == "":
        return None, None
    if isinstance(value, int | float):
        parsed = float(value)
        return parsed, parsed
    text = normalize_text(value)
    for separator in ("..", "~", "～"):
        if separator in text:
            left, right = text.split(separator, 1)
            return (float(left) if left else None, float(right) if right else None)
    range_match = re.fullmatch(rf"({NUMBER_PATTERN})\s*-\s*({NUMBER_PATTERN})?", text)
    if range_match:
        left, right = range_match.groups()
        return (float(left), float(right) if right else None)
    parsed = float(text)
    return parsed, parsed


def parse_fit_diff_filter(
    *,
    fit_diff: Any = None,
    fit_diff_min: float | int | str | None = None,
    fit_diff_max: float | int | str | None = None,
) -> tuple[float | None, float | None, bool]:
    if fit_diff_min is not None or fit_diff_max is not None:
        return (
            float(fit_diff_min) if fit_diff_min is not None else None,
            float(fit_diff_max) if fit_diff_max is not None else None,
            True,
        )
    if fit_diff is None or fit_diff == "":
        return None, None, True
    if isinstance(fit_diff, str):
        text = normalize_text(fit_diff)
        if any(separator in text for separator in ("-", "~", "～", "..")):
            low, high = parse_float_range(value=fit_diff)
            return low, high, True
    bucket = Decimal(str(fit_diff)).quantize(Decimal("0.1"), rounding=ROUND_FLOOR)
    return float(bucket), float(bucket + Decimal("0.1")), False


def parse_regions(value: Any) -> set[str]:
    if value in (None, ""):
        return set()
    raw_values = value if isinstance(value, list) else re.split(r"[,/，、\s]+", str(value))
    regions: set[str] = set()
    for raw in raw_values:
        if raw in (None, ""):
            continue
        key = normalize_text(raw).replace(" ", "")
        region = REGION_ALIASES.get(key)
        if region is None:
            raise SearchError("region must be cn or 国服 in this branch")
        regions.add(region)
    return regions


def parse_fit_label(value: Any) -> str | None:
    if value in (None, ""):
        return None
    key = normalize_text(value).replace(" ", "")
    label = FIT_LABEL_ALIASES.get(key)
    if label is None:
        raise SearchError("fit_label must be 虚高 or 虚低")
    return label


def parse_sort(value: Any) -> str | None:
    if value in (None, ""):
        return None
    key = normalize_text(value).replace(" ", "")
    sort = SORT_ALIASES.get(key)
    if sort is None:
        raise SearchError("sort must be fit_delta_desc, fit_delta_asc, fit_diff_asc, or fit_diff_desc")
    return sort


def parse_tag(value: Any, *, tags_path: Path | None = None) -> list[int]:
    """dxrating tags are not available in this branch."""
    _ = tags_path
    if value in (None, ""):
        return []
    if isinstance(value, list) and not any(item not in (None, "") for item in value):
        return []
    raise SearchError("dxrating tag filters are not supported in this branch")


def describe_tags(tag_ids: list[int], *, tags_path: Path | None = None) -> list[dict[str, Any]]:
    _ = tag_ids, tags_path
    return []


_DATE_BOUND_RE = re.compile(r"^(\d{4})(?:[-/](\d{1,2}))?(?:[-/](\d{1,2}))?$")
_MONTH_LAST_DAY = {1: 31, 2: 29, 3: 31, 4: 30, 5: 31, 6: 30,
                   7: 31, 8: 31, 9: 30, 10: 31, 11: 30, 12: 31}


def parse_date_bound(value: Any, *, mode: str) -> str | None:
    """把 "2024" / "2024-03" / "2024-03-15" 归一成可字符串比较的 YYYY-MM-DD。

    mode='after'  时把"2024"展开成 2024-01-01（>=）
    mode='before' 时把"2024"展开成 2024-12-31（<=）
    """
    if value in (None, ""):
        return None
    text = str(value).strip()
    match = _DATE_BOUND_RE.match(text)
    if not match:
        raise SearchError(f"Unrecognized date {value!r}. Use YYYY / YYYY-MM / YYYY-MM-DD.")
    year = int(match.group(1))
    month = int(match.group(2)) if match.group(2) else None
    day = int(match.group(3)) if match.group(3) else None
    if mode == "after":
        m = month or 1
        d = day or 1
    elif mode == "before":
        m = month or 12
        if day:
            d = day
        else:
            d = _MONTH_LAST_DAY.get(m, 31)
    else:
        raise SearchError(f"Internal: invalid date bound mode {mode!r}")
    if not (1 <= m <= 12 and 1 <= d <= 31):
        raise SearchError(f"Invalid date {value!r}.")
    return f"{year:04d}-{m:02d}-{d:02d}"


def chart_release_date(chart: dict[str, Any]) -> str | None:
    value = chart.get("release_date")
    if isinstance(value, str) and value:
        return value[:10]
    return None


def release_date_in_range(value: str | None, low: str | None, high: str | None) -> bool:
    if low is None and high is None:
        return True
    if value is None:
        return False
    if low is not None and value < low:
        return False
    if high is not None and value > high:
        return False
    return True


def chart_tag_ids(
    music: dict[str, Any],
    chart: dict[str, Any],
    tag_index: dict[str, Any] | None,
) -> frozenset[int]:
    _ = music, chart, tag_index
    return frozenset()


def tags_match(
    chart_tag_ids_value: frozenset[int],
    required: list[int],
    excluded: list[int],
) -> bool:
    if required:
        for tid in required:
            if tid not in chart_tag_ids_value:
                return False
    if excluded:
        for tid in excluded:
            if tid in chart_tag_ids_value:
                return False
    return True


def parse_song_type(value: Any) -> str | None:
    if value is None or value == "":
        return None
    song_type = normalize_text(value).replace(" ", "")
    if song_type not in SONG_TYPE_ALIASES:
        raise SearchError("song_type must be standard/SD, dx, or utage/宴")
    return SONG_TYPE_ALIASES[song_type]


def normalize_song_id(value: Any) -> str:
    try:
        song_id = int(value)
    except (TypeError, ValueError):
        return str(value)
    if 10000 < song_id < 100000:
        song_id %= 10000
    return str(song_id)


def raw_numeric_song_id(value: Any) -> int | None:
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def raw_source_id(music: dict[str, Any]) -> str:
    return str(music.get("songId", music.get("id", "")))


def music_id_values(music: dict[str, Any]) -> list[str]:
    ids: list[str] = []
    for source_music in source_records(music).values():
        for key in ("id", "songId"):
            if source_music.get(key) not in (None, ""):
                ids.append(normalize_song_id(source_music.get(key)))
    return unique_preserve_order(ids)


def source_ids(music: dict[str, Any]) -> dict[str, str]:
    ids: dict[str, str] = {}
    for label, source_music in source_records(music).items():
        source_id = raw_source_id(source_music)
        if source_id:
            ids[label] = source_id
    return ids


def canonical_song_id(music: dict[str, Any]) -> str:
    label, source_music = primary_source_record(music)
    for key in ("id", "songId"):
        if source_music.get(key) not in (None, ""):
            return normalize_song_id(source_music.get(key))
    ids = music_id_values(music)
    return ids[0] if ids else ""


def primary_source_id(music: dict[str, Any]) -> str:
    _, source_music = primary_source_record(music)
    return raw_source_id(source_music)


def source_basic_info(music: dict[str, Any]) -> dict[str, Any]:
    basic_info = music.get("basic_info")
    return basic_info if isinstance(basic_info, dict) else {}


def _resolve_field(records: dict[str, Any], field: str) -> Any:
    """从 primary 源取统一字段值。"""
    for label in SOURCE_PRIORITY:
        music = records.get(label)
        if not music:
            continue
        path = SOURCE_FIELD_MAPS.get(label, {}).get(field, ())
        if not path:
            continue
        val: Any = music
        for key in path:
            if isinstance(val, dict):
                val = val.get(key)
            else:
                val = None
                break
        if val not in (None, ""):
            return val
    return ""


def _collect_unique_fields(records: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """收集所有源的字段值，删除重复项。返回 {field: {value: source_label}}。"""
    field_values: dict[str, dict[str, Any]] = {}
    for label in SOURCE_PRIORITY:
        music = records.get(label)
        if not music:
            continue
        field_map = SOURCE_FIELD_MAPS.get(label, {})
        for field, path in field_map.items():
            if not path:
                continue
            val: Any = music
            for key in path:
                if isinstance(val, dict):
                    val = val.get(key)
                else:
                    val = None
                    break
            if val in (None, ""):
                continue
            if field not in field_values:
                field_values[field] = {}
            # 如果值已存在（来自更高优先级源），跳过
            if val not in field_values[field]:
                field_values[field][val] = label
    return field_values


def music_title_values(music: dict[str, Any]) -> list[str]:
    values: list[str] = []
    for source_music in source_records(music).values():
        basic_info = source_basic_info(source_music)
        values.extend([source_music.get("title", ""), basic_info.get("title", "")])
    return unique_preserve_order([str(value) for value in values if value not in (None, "")])


def primary_title(music: dict[str, Any]) -> str:
    _, source_music = primary_source_record(music)
    basic_info = source_basic_info(source_music)
    return str(source_music.get("title", basic_info.get("title", "")))


def resolved_display_title(records: dict[str, dict[str, Any]], primary: dict[str, Any], basic_info: dict[str, Any]) -> str:
    return str(_resolve_field(records, "title") or primary.get("title", basic_info.get("title", "")))


def aliases_from_id_map(alias_map: dict[str, list[str]], music: dict[str, Any]) -> list[str]:
    aliases: list[str] = []
    for song_id in music_id_values(music):
        aliases.extend(alias_map.get(song_id, []))
    return unique_preserve_order(aliases)


def aliases_for_music(alias_map: dict[str, list[str]], music: dict[str, Any]) -> list[str]:
    aliases: list[str] = aliases_from_id_map(alias_map, music)
    for source_music in source_records(music).values():
        for acronym in source_music.get("searchAcronyms") or []:
            if isinstance(acronym, str) and acronym:
                aliases.append(acronym)
    return unique_preserve_order(aliases)


def extract_query_id(query: str | None) -> str | None:
    if not query:
        return None
    normalized = normalize_text(query)
    match = re.fullmatch(r"(?:id)?\s*(\d+)", normalized)
    return normalize_song_id(match.group(1)) if match else None


def title_matches(
    music: dict[str, Any],
    aliases: list[str],
    pinyin_aliases: list[str],
    query: str | None,
) -> bool:
    if not query:
        return True
    query_id = extract_query_id(query)
    if query_id is not None:
        needle = normalize_text(query)
        exact_text_values = [
            *music_title_values(music),
            *music_song_id_string_values(music),
            *aliases,
        ]
        return query_id in music_id_values(music) or any(
            needle == normalize_text(value) for value in exact_text_values
        )

    needle = normalize_text(query)
    haystacks = [*music_title_values(music), *music_keyword_values(music), *aliases]
    if any(needle in normalize_text(value) for value in haystacks):
        return True
    pinyin_needles = pinyin_query_needles(query)
    return any(
        pinyin_needle in normalize_text(value)
        for pinyin_needle in pinyin_needles
        for value in pinyin_aliases
    )


def music_artist_values(music: dict[str, Any]) -> list[str]:
    values: list[str] = []
    for source_music in source_records(music).values():
        artist = source_music.get("artist")
        if isinstance(artist, str) and artist:
            values.append(artist)
        basic_info = source_music.get("basic_info")
        if isinstance(basic_info, dict):
            bi_artist = basic_info.get("artist")
            if isinstance(bi_artist, str) and bi_artist:
                values.append(bi_artist)
    return unique_preserve_order(values)


def artist_matches(
    music: dict[str, Any],
    expected: str | None,
    alias_map: dict[str, list[str]] | None = None,
) -> bool:
    if expected in (None, ""):
        return True
    needle = normalize_text(expected)
    if not needle:
        return True
    candidates = {needle}
    if alias_map:
        candidates |= expand_name_query(needle, alias_map)
    for artist in music_artist_values(music):
        artist_norm = normalize_text(artist)
        if any(c in artist_norm for c in candidates):
            return True
    return False


def charter_matches(
    chart: dict[str, Any],
    expected: str | None,
    alias_map: dict[str, list[str]] | None = None,
) -> bool:
    if expected in (None, ""):
        return True
    needle = normalize_text(expected)
    if not needle:
        return True
    charter = chart.get("charter")
    if charter in (None, ""):
        return False
    charter_norm = normalize_text(charter)
    candidates = {needle}
    if alias_map:
        candidates |= expand_name_query(needle, alias_map)
    return any(c in charter_norm for c in candidates)


def music_keyword_values(music: dict[str, Any]) -> list[str]:
    values: list[str] = []
    for source_music in source_records(music).values():
        values.append(source_music.get("keyword", ""))
        for acronym in source_music.get("searchAcronyms") or []:
            if isinstance(acronym, str) and acronym:
                values.append(acronym)
    return unique_preserve_order([str(value) for value in values if value not in (None, "")])


def music_song_id_string_values(music: dict[str, Any]) -> list[str]:
    """收集所有源里的歌曲级 ID 字符串形式。"""
    values: list[str] = []
    for source_music in source_records(music).values():
        for key in ("id", "songId"):
            raw = source_music.get(key)
            if raw in (None, ""):
                continue
            values.append(str(raw))
    return unique_preserve_order(values)


def music_int_ids(music: dict[str, Any]) -> list[int]:
    """收集所有源里能解析成整数的歌曲 ID。"""
    int_ids: list[int] = []
    for raw in music_song_id_string_values(music):
        try:
            int_ids.append(int(raw))
        except ValueError:
            continue
    return int_ids


def music_bpm_value(music: dict[str, Any]) -> float | None:
    """从三个源里挑第一个能解析成 float 的 bpm。"""
    records = source_records(music)
    for label in SOURCE_PRIORITY:
        source_music = records.get(label)
        if not isinstance(source_music, dict):
            continue
        for path_keys in (("bpm",), ("basic_info", "bpm")):
            value: Any = source_music
            for key in path_keys:
                if not isinstance(value, dict):
                    value = None
                    break
                value = value.get(key)
            if value in (None, ""):
                continue
            try:
                return float(value)
            except (TypeError, ValueError):
                continue
    return None


def music_is_locked(music: dict[str, Any]) -> bool | None:
    """The remaining public data sources do not expose lock status."""
    _ = music
    return None


def _maybe_truncated(value: bool) -> dict[str, Any]:
    """Helper：只在结果真的被截断时才返回 {"truncated": True}，否则返回空 dict。

    跟 fit_diff_high_inclusive 同样的问题：truncated=False 是"无事发生"，
    每次响应都带这个字段，Agent 看到就开始把 truncated 当参数往请求里塞。
    解决：用 spread 语法 `**_maybe_truncated(x)` 把它做成"有信息才出现"的字段。
    现有 consumer（server.py 的格式化、maimai_score_mcp）都用 .get('truncated', False)
    兜底，缺失时自动当 False，零兼容性影响。
    """
    return {"truncated": True} if value else {}


def parse_int_range(
    *,
    value: Any = None,
    low: int | str | None = None,
    high: int | str | None = None,
) -> tuple[int | None, int | None]:
    """解析 id 这类整数范围：支持 id_min/id_max；或单值；或字符串区间 '100-200'。"""
    if low is not None or high is not None:
        return (
            int(low) if low is not None else None,
            int(high) if high is not None else None,
        )
    if value in (None, ""):
        return None, None
    if isinstance(value, int):
        return value, value
    text = normalize_text(value)
    for separator in ("..", "~", "～"):
        if separator in text:
            left, right = text.split(separator, 1)
            return (int(left) if left else None, int(right) if right else None)
    range_match = re.fullmatch(r"(-?\d+)\s*-\s*(-?\d+)?", text)
    if range_match:
        left, right = range_match.groups()
        return (int(left), int(right) if right else None)
    return int(text), int(text)


def int_in_range(value: int | None, low: int | None, high: int | None) -> bool:
    if value is None:
        return low is None and high is None
    if low is not None and value < low:
        return False
    if high is not None and value > high:
        return False
    return True


def float_in_range(value: float | None, low: float | None, high: float | None) -> bool:
    if value is None:
        return low is None and high is None
    if low is not None and value < low:
        return False
    if high is not None and value > high:
        return False
    return True


def music_genre_values(music: dict[str, Any]) -> list[str]:
    values: list[str] = []
    for source_music in source_records(music).values():
        basic_info = source_basic_info(source_music)
        values.extend(
            [
                source_music.get("genre", ""),
                source_music.get("category", ""),
                basic_info.get("genre", ""),
            ]
        )
    return unique_preserve_order([str(value) for value in values if value not in (None, "")])


def music_genre(music: dict[str, Any]) -> str:
    values = music_genre_values(music)
    return values[0] if values else ""


def genre_matches(music: dict[str, Any], genre: str | None) -> bool:
    if not genre:
        return True
    return any(normalize_text(genre) in normalize_text(value) for value in music_genre_values(music))


def music_version_values(music: dict[str, Any], *, include_charts: bool = True) -> list[str]:
    values: list[Any] = []
    for source_music in source_records(music).values():
        basic_info = source_basic_info(source_music)
        values.extend(
            [
                source_music.get("version"),
                basic_info.get("from"),
                basic_info.get("version"),
                source_music.get("releaseVersion"),
            ]
        )
    if include_charts:
        values.extend(chart.get("version") for chart in all_charts(music))
    return unique_preserve_order(
        [str(value) for value in values if value not in (None, "")]
    )


def value_matches_filter(value: Any, expected: str | None) -> bool:
    if expected in (None, ""):
        return True
    return normalize_text(expected) in normalize_text(value)


def split_version_terms(expected: str | None) -> list[str]:
    if expected in (None, ""):
        return []
    return [
        term
        for term in re.split(r"[/,，、|]+", str(expected))
        if term.strip()
    ]


def normalize_lxns_version_term(expected: str | None) -> str:
    if expected in (None, ""):
        return ""
    text = normalize_text(expected).replace(" ", "")
    text = re.sub(r"(?:版本|版|年)$", "", text)
    for prefix in ("舞萌dx", "舞萌", "maimaidx", "maimai", "dx"):
        if text.startswith(prefix) and len(text) > len(prefix) and text[len(prefix)].isdigit():
            return text[len(prefix):]
    return text


def lxns_year_prefix(expected: str | None) -> str | None:
    text = normalize_lxns_version_term(expected)
    if not text:
        return None
    if re.fullmatch(r"\d{5}", text):
        return None
    if re.fullmatch(r"\d{2}", text):
        return text
    if re.fullmatch(r"20\d{2}", text):
        return text[-2:]
    match = re.search(r"(?<!\d)20(\d{2})(?!\d)", text)
    return match.group(1) if match else None


def lxns_exact_version_code(expected: str | None) -> str | None:
    text = normalize_lxns_version_term(expected)
    if not text:
        return None
    if re.fullmatch(r"\d{5}", text):
        return text
    match = re.fullmatch(r"(?:20)?(?P<year>\d{2})(?:[-_]0*(?P<num_sub>\d{1,3})|[-_]?(?P<letter_sub>[a-z]))", text)
    if not match:
        return None
    year = match.group("year")
    if match.group("letter_sub"):
        sub_number = ord(match.group("letter_sub")) - ord("a")
    else:
        sub_number = int(match.group("num_sub"))
    if not 0 <= sub_number <= 999:
        return None
    return f"{year}{sub_number:03d}"


def version_value_matches_filter(value: Any, expected: str | None) -> bool:
    if expected in (None, ""):
        return True
    value_text = normalize_text(value)
    for term in split_version_terms(expected) or [str(expected)]:
        term_text = normalize_text(term)
        exact_code = lxns_exact_version_code(term_text)
        if exact_code and value_text == exact_code:
            return True
        if exact_code:
            continue
        year_prefix = lxns_year_prefix(term_text)
        if year_prefix and re.fullmatch(r"\d{5}", value_text) and value_text.startswith(year_prefix):
            return True
        if year_prefix:
            continue
        if term_text in value_text:
            return True
    return False


def music_version_matches(music: dict[str, Any], version: str | None) -> bool:
    if not version:
        return True
    values = music_version_values(music, include_charts=True)
    return any(_music_version_term_matches(music, values, term) for term in split_version_terms(version))


def music_base_version_matches(music: dict[str, Any], version: str | None) -> bool:
    if not version:
        return True
    values = music_version_values(music, include_charts=False)
    return any(_music_version_term_matches(music, values, term) for term in split_version_terms(version))


def _is_lxns_numeric_version_term(term: str) -> bool:
    return bool(lxns_exact_version_code(term) or lxns_year_prefix(term))


def _music_cn_version_matches(music: dict[str, Any], term: str) -> bool:
    cn = source_records(music).get("cn")
    if not cn:
        return False
    return version_value_matches_filter(cn.get("version"), term)


def _music_version_term_matches(music: dict[str, Any], values: list[str], term: str) -> bool:
    if _is_lxns_numeric_version_term(term):
        # Numeric LXNS terms like 2025/25/25010 are CN song-version filters.
        return _music_cn_version_matches(music, term)
    return any(version_value_matches_filter(value, term) for value in values)


def query_rank(
    music: dict[str, Any],
    aliases: list[str],
    pinyin_aliases: list[str],
    query: str | None,
) -> tuple[int, str]:
    match = query_match(music, aliases, pinyin_aliases, query)
    return int(match.get("rank", 10)), normalize_text(match.get("sort_title", ""))


def _first_matching_value(values: list[str], needle: str, mode: str) -> str | None:
    for value in values:
        normalized = normalize_text(value)
        if not normalized:
            continue
        if mode == "exact" and normalized == needle:
            return value
        if mode == "prefix" and normalized.startswith(needle):
            return value
        if mode == "contains" and needle in normalized:
            return value
    return None


def _first_matching_query_value(values: list[str], needles: tuple[str, ...], mode: str) -> str | None:
    for needle in needles:
        value = _first_matching_value(values, needle, mode)
        if value is not None:
            return value
    return None


def _match_result(
    *,
    rank: int,
    field: str,
    label: str,
    mode: str,
    value: Any,
    sort_title: str,
) -> dict[str, Any]:
    return {
        "rank": rank,
        "field": field,
        "label": label,
        "mode": mode,
        "value": to_simplified(value),
        "sort_title": sort_title,
    }


def query_match(
    music: dict[str, Any],
    aliases: list[str],
    pinyin_aliases: list[str],
    query: str | None,
) -> dict[str, Any]:
    titles_raw = music_title_values(music)
    sort_title = normalize_text(titles_raw[0]) if titles_raw else ""
    if not query:
        return _match_result(
            rank=0,
            field="none",
            label="无查询",
            mode="none",
            value="",
            sort_title=normalize_text(primary_title(music)),
        )
    query_id = extract_query_id(query)
    if query_id is not None:
        # 数字 query 优先走 ID 精确匹配，但不能屏蔽完全相等的数字别名。
        needle = normalize_text(query)
        if query_id in music_id_values(music):
            return _match_result(
                rank=0,
                field="song_id",
                label="歌曲ID命中",
                mode="exact",
                value=query_id,
                sort_title=sort_title,
            )
        title_value = _first_matching_value(titles_raw, needle, "exact")
        if title_value is not None:
            return _match_result(
                rank=1,
                field="title",
                label="歌名命中",
                mode="exact",
                value=title_value,
                sort_title=sort_title,
            )
        source_id_value = _first_matching_value(music_song_id_string_values(music), needle, "exact")
        if source_id_value is not None:
            return _match_result(
                rank=1,
                field="source_id",
                label="源ID命中",
                mode="exact",
                value=source_id_value,
                sort_title=sort_title,
            )
        alias_value = _first_matching_value(aliases, needle, "exact")
        if alias_value is not None:
            return _match_result(
                rank=2,
                field="alias",
                label="别名命中",
                mode="exact",
                value=alias_value,
                sort_title=sort_title,
            )
        return _match_result(
            rank=9,
            field="unknown",
            label="兜底命中",
            mode="unknown",
            value=query,
            sort_title=sort_title,
        )

    needle = normalize_text(query)
    title_exact = _first_matching_value(titles_raw, needle, "exact")
    source_id_exact = _first_matching_value(music_song_id_string_values(music), needle, "exact")
    if title_exact is not None:
        return _match_result(rank=0, field="title", label="歌名命中", mode="exact", value=title_exact, sort_title=sort_title)
    if source_id_exact is not None:
        return _match_result(rank=0, field="source_id", label="源ID命中", mode="exact", value=source_id_exact, sort_title=sort_title)
    pinyin_needles = pinyin_query_needles(query)
    for rank, field, label, values, mode in (
        (1, "alias", "别名命中", aliases, "exact"),
        (2, "title", "歌名命中", titles_raw, "prefix"),
        (3, "title", "歌名命中", titles_raw, "contains"),
        (4, "alias", "别名命中", aliases, "prefix"),
        (5, "alias", "别名命中", aliases, "contains"),
        (6, "pinyin", "拼音命中", pinyin_aliases, "exact"),
        (7, "pinyin", "拼音命中", pinyin_aliases, "prefix"),
        (8, "pinyin", "拼音命中", pinyin_aliases, "contains"),
        (9, "keyword", "关键字命中", music_keyword_values(music), "contains"),
    ):
        value = (
            _first_matching_query_value(values, pinyin_needles, mode)
            if field == "pinyin"
            else _first_matching_value(values, needle, mode)
        )
        if value is not None:
            return _match_result(
                rank=rank,
                field=field,
                label=label,
                mode=mode,
                value=value,
                sort_title=sort_title,
            )
    return _match_result(
        rank=10,
        field="unknown",
        label="兜底命中",
        mode="unknown",
        value=query,
        sort_title=sort_title,
    )


def level_matches(candidate: Any, expected: str | None) -> bool:
    if not expected:
        return True
    candidate_level = normalize_level(candidate)
    expected_level = normalize_level(expected)
    return candidate_level == expected_level or candidate_level.rstrip("?") == expected_level


def ds_matches(candidate: Any, ds_low: float | None, ds_high: float | None) -> bool:
    if ds_low is None and ds_high is None:
        return True
    value = float(candidate)
    if ds_low is not None and value < ds_low:
        return False
    if ds_high is not None and value > ds_high:
        return False
    return True


def notes_to_dict(notes: list[int] | None) -> dict[str, int]:
    if not notes:
        return {}
    if len(notes) == 4:
        tap, hold, slide, brk = notes
        return {"tap": tap, "hold": hold, "slide": slide, "break": brk}
    tap, hold, slide, touch, brk = notes[:5]
    return {"tap": tap, "hold": hold, "slide": slide, "touch": touch, "break": brk}


def iter_lxns_charts(music: dict[str, Any]) -> list[dict[str, Any]]:
    difficulties = music.get("difficulties")
    if not isinstance(difficulties, dict):
        return []

    # lxns 源 difficulties 字典 key 仍叫 "utage"，输出统一用 utage1p，
    # 避免把单人宴和双人宴混成同一个类型。
    LXNS_BUCKET_TO_CHART_TYPE = {"standard": "standard", "dx": "dx", "utage": "utage1p"}
    charts: list[dict[str, Any]] = []
    for bucket_key in ("standard", "dx", "utage"):
        default_chart_type = LXNS_BUCKET_TO_CHART_TYPE[bucket_key]
        for chart in difficulties.get(bucket_key, []) or []:
            if not isinstance(chart, dict):
                continue
            difficulty_index = int(chart.get("difficulty", 0))
            # 若 chart 自带 type 字段，normalize 一下：历史 "utage" → utage1p。
            raw_inline_type = chart.get("type")
            if isinstance(raw_inline_type, str) and normalize_text(raw_inline_type) == "utage":
                inline_type: Any = "utage1p"
            else:
                inline_type = raw_inline_type
            charts.append(
                {
                    "chart_type": inline_type if inline_type else default_chart_type,
                    "difficulty_index": difficulty_index,
                    "difficulty": (
                        DIFFICULTIES[difficulty_index]
                        if difficulty_index < len(DIFFICULTIES)
                        else str(difficulty_index)
                    ),
                    "level": chart.get("level"),
                    "ds": chart.get("level_value"),
                    "charter": chart.get("note_designer"),
                    "version": chart.get("version"),
                    "notes": chart.get("notes") or {},
                    "kanji": chart.get("kanji"),
                    "description": chart.get("description"),
                    "is_buddy": chart.get("is_buddy"),
                }
            )
    return charts


def iter_diving_fish_charts(music: dict[str, Any]) -> list[dict[str, Any]]:
    levels = music.get("level") or []
    ds_values = music.get("ds") or []
    raw_charts = music.get("charts") or []
    chart_type = "dx" if normalize_text(music.get("type", "")).upper() == "DX" else "standard"
    charts: list[dict[str, Any]] = []

    for index, chart in enumerate(raw_charts):
        if index >= len(levels) or index >= len(ds_values):
            continue
        chart = chart if isinstance(chart, dict) else {}
        charts.append(
            {
                "chart_type": chart_type,
                "difficulty": DIFFICULTIES[index] if index < len(DIFFICULTIES) else str(index),
                "difficulty_index": index,
                "level": levels[index],
                "ds": ds_values[index],
                "charter": chart.get("charter"),
                "version": None,
                "notes": notes_to_dict(chart.get("notes")),
            }
        )
    return charts


def charts_from_single_music(music: dict[str, Any]) -> list[dict[str, Any]]:
    if "difficulties" in music:
        return iter_lxns_charts(music)
    if "sheets" in music:
        return []
    return iter_diving_fish_charts(music)


def all_charts(music: dict[str, Any]) -> list[dict[str, Any]]:
    charts: list[dict[str, Any]] = []
    for label, source_music in source_records(music).items():
        for chart in charts_from_single_music(source_music):
            item = dict(chart)
            item["source"] = label
            item["source_name"] = source_name(label)
            charts.append(item)
    return charts


def chart_stats_lookup_ids(music: dict[str, Any], chart: dict[str, Any]) -> list[str]:
    ids: list[str] = []
    chart_is_dx = chart.get("chart_type") == "dx"
    for label, source_music in source_records(music).items():
        for key in ("id", "songId"):
            numeric_id = raw_numeric_song_id(source_music.get(key))
            if numeric_id is None:
                continue
            if label == "divingfish":
                is_dx_id = numeric_id >= 10000
                if is_dx_id == chart_is_dx:
                    ids.append(str(numeric_id))
            elif chart_is_dx:
                if numeric_id < 10000:
                    ids.append(str(numeric_id + 10000))
            else:
                ids.append(str(numeric_id))
                ids.append(normalize_song_id(numeric_id))
    return unique_preserve_order(ids)


def fit_label_for_delta(delta: float) -> str | None:
    if delta > 0:
        return "虚高"
    if delta < 0:
        return "虚低"
    return None


def attach_fit_stats(
    music: dict[str, Any],
    charts: list[dict[str, Any]],
    chart_stats: dict[str, list[dict[str, Any]]],
) -> list[dict[str, Any]]:
    if not chart_stats:
        return charts
    attached: list[dict[str, Any]] = []
    for chart in charts:
        item = dict(chart)
        if item.get("chart_type") in ("utage1p", "utage2p"):
            # 宴谱（单人 utage1p、合奏 utage2p）不在 divingfish chart_stats 里，直接透传。
            attached.append(item)
            continue
        stat: dict[str, Any] | None = None
        stat_source_id: str | None = None
        difficulty_index = int(item.get("difficulty_index", 0))
        for lookup_id in chart_stats_lookup_ids(music, item):
            stats_for_song = chart_stats.get(lookup_id)
            if not stats_for_song or difficulty_index >= len(stats_for_song):
                continue
            candidate = stats_for_song[difficulty_index]
            if candidate.get("fit_diff") is None:
                continue
            cand_diff = candidate.get("diff")
            if cand_diff is not None and not level_matches(item.get("level"), str(cand_diff)):
                continue
            stat = candidate
            stat_source_id = lookup_id
            break
        if stat is not None:
            fit_diff = float(stat["fit_diff"])
            try:
                ds_value = float(item.get("ds"))
            except (TypeError, ValueError):
                ds_value = math.nan
            fit_delta = ds_value - fit_diff if math.isfinite(ds_value) else None
            item["fit_diff"] = fit_diff
            item["fit_delta"] = fit_delta
            item["fit_label"] = fit_label_for_delta(fit_delta) if fit_delta is not None else None
            item["fit_stats"] = {
                key: stat.get(key)
                for key in ("cnt", "diff", "avg", "avg_dx", "std_dev", "dist", "fc_dist")
                if stat.get(key) is not None
            }
            item["fit_source_id"] = stat_source_id
        attached.append(item)
    return attached


def chart_regions(chart: dict[str, Any]) -> dict[str, bool]:
    regions = {
        key: bool((chart.get("regions") or {}).get(key))
        for key in REGION_KEYS
    }
    regions["cn"] = chart.get("source") in CN_REGION_SOURCE_LABELS
    return regions


def regions_match(regions: dict[str, bool], region_has: set[str], region_missing: set[str]) -> bool:
    if any(not regions.get(region) for region in region_has):
        return False
    if any(regions.get(region) for region in region_missing):
        return False
    return True


def fit_value_matches(
    value: Any,
    low: float | None,
    high: float | None,
    *,
    high_inclusive: bool = True,
) -> bool:
    if low is None and high is None:
        return True
    if value is None:
        return False
    numeric = float(value)
    if low is not None and numeric < low:
        return False
    if high is not None:
        if high_inclusive:
            if numeric > high:
                return False
        elif numeric >= high:
            return False
    return True


def chart_fit_matches(
    chart: dict[str, Any],
    *,
    fit_diff_low: float | None,
    fit_diff_high: float | None,
    fit_diff_high_inclusive: bool,
    fit_delta_low: float | None,
    fit_delta_high: float | None,
    fit_label: str | None,
) -> bool:
    if not fit_value_matches(
        chart.get("fit_diff"),
        fit_diff_low,
        fit_diff_high,
        high_inclusive=fit_diff_high_inclusive,
    ):
        return False
    if not fit_value_matches(chart.get("fit_delta"), fit_delta_low, fit_delta_high):
        return False
    if fit_label is not None and chart.get("fit_label") != fit_label:
        return False
    return True


def matched_charts(
    music: dict[str, Any],
    *,
    level: str | None,
    version: str | None,
    ds_low: float | None,
    ds_high: float | None,
    fit_diff_low: float | None = None,
    fit_diff_high: float | None = None,
    fit_diff_high_inclusive: bool = True,
    fit_delta_low: float | None = None,
    fit_delta_high: float | None = None,
    fit_label: str | None = None,
    difficulty_index: int | None,
    song_type: str | None,
    charter: str | None = None,
    required_tag_ids: list[int] | None = None,
    excluded_tag_ids: list[int] | None = None,
    released_after: str | None = None,
    released_before: str | None = None,
    chart_stats: dict[str, list[dict[str, Any]]] | None = None,
    tag_index: dict[str, Any] | None = None,
    charter_alias_map: dict[str, list[str]] | None = None,
) -> list[dict[str, Any]]:
    matches: list[dict[str, Any]] = []
    charts = attach_fit_stats(music, all_charts(music), chart_stats or {})
    needs_tag_lookup = bool(required_tag_ids) or bool(excluded_tag_ids)
    for chart in charts:
        if song_type is not None and chart["chart_type"] != song_type:
            continue
        if difficulty_index is not None and chart["difficulty_index"] != difficulty_index:
            continue
        if not level_matches(chart.get("level"), level):
            continue
        if not version_value_matches_filter(chart.get("version", ""), version):
            continue
        if not ds_matches(chart.get("ds", 0), ds_low, ds_high):
            continue
        if not charter_matches(chart, charter, charter_alias_map):
            continue
        if not chart_fit_matches(
            chart,
            fit_diff_low=fit_diff_low,
            fit_diff_high=fit_diff_high,
            fit_diff_high_inclusive=fit_diff_high_inclusive,
            fit_delta_low=fit_delta_low,
            fit_delta_high=fit_delta_high,
            fit_label=fit_label,
        ):
            continue
        if released_after is not None or released_before is not None:
            if not release_date_in_range(chart_release_date(chart), released_after, released_before):
                continue
        if needs_tag_lookup:
            ids = chart_tag_ids(music, chart, tag_index)
            if not tags_match(ids, required_tag_ids or [], excluded_tag_ids or []):
                continue
        matches.append(chart)
    return matches


def source_regions(charts: list[dict[str, Any]]) -> dict[str, bool]:
    return {
        key: any(chart_regions(chart).get(key) for chart in charts)
        for key in REGION_KEYS
    }


def source_field_summary(label: str, music: dict[str, Any]) -> dict[str, Any]:
    basic_info = source_basic_info(music)
    charts = charts_from_single_music(music)
    fields: dict[str, Any] = {
        "source": source_name(label),
        "id": normalize_song_id(music.get("id", music.get("songId", ""))),
        "source_id": raw_source_id(music),
    }
    # 用映射表读统一字段
    field_map = SOURCE_FIELD_MAPS.get(label, {})
    for unified_field, path in field_map.items():
        if not path:
            continue
        val: Any = music
        for key in path:
            if isinstance(val, dict):
                val = val.get(key)
            else:
                val = None
                break
        if val not in (None, ""):
            fields[unified_field] = val
    # 源特有字段
    fields["is_locked"] = music.get("isLocked", music.get("locked"))
    fields["rights"] = music.get("rights")
    fields["map"] = music.get("map")
    fields["slug"] = music.get("slug")
    fields["keyword"] = music.get("keyword")
    fields["comment"] = music.get("comment")
    fields["available_chart_types"] = sorted({chart["chart_type"] for chart in charts})
    fields["levels"] = unique_values([chart.get("level") for chart in charts])
    fields["ds"] = unique_values([chart.get("ds") for chart in charts])
    regions = source_regions(charts)
    if any(regions.values()):
        fields["regions"] = regions
    return {
        key: value
        for key, value in fields.items()
        if value not in (None, "", [], {})
    }


def source_field_summaries(music: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {
        label: source_field_summary(label, source_music)
        for label, source_music in source_records(music).items()
    }


def serialize_music(
    music: dict[str, Any],
    charts: list[dict[str, Any]],
    aliases: list[str],
) -> dict[str, Any]:
    primary_label, primary = primary_source_record(music)
    basic_info = source_basic_info(primary)
    all_song_charts = all_charts(music)
    records = source_records(music)
    labels = sorted(
        records,
        key=lambda label: SOURCE_PRIORITY.index(label)
        if label in SOURCE_PRIORITY
        else len(SOURCE_PRIORITY),
    )
    charts = sorted(
        charts,
        key=lambda chart: (
            SOURCE_PRIORITY.index(chart.get("source", "divingfish"))
            if chart.get("source", "divingfish") in SOURCE_PRIORITY
            else len(SOURCE_PRIORITY),
            {"standard": 0, "dx": 1, "utage1p": 2, "utage2p": 3}.get(str(chart.get("chart_type")), 9),
            int(chart.get("difficulty_index", 0)),
        ),
    )
    is_locked = music_is_locked(music)
    return {
        "id": canonical_song_id(music),
        "source_id": raw_source_id(primary),
        "source_ids": source_ids(music),
        "title": resolved_display_title(records, primary, basic_info),
        "available_chart_types": sorted({chart["chart_type"] for chart in all_song_charts}),
        "artist": _resolve_field(records, "artist") or primary.get("artist", basic_info.get("artist", "")),
        "genre": _resolve_field(records, "genre") or primary.get("genre", primary.get("category", basic_info.get("genre", ""))),
        "bpm": _resolve_field(records, "bpm") or primary.get("bpm", basic_info.get("bpm")),
        "version": _resolve_field(records, "version") or primary.get("version", basic_info.get("from", "")),
        "release_date": _resolve_field(records, "release_date") or primary.get("releaseDate", basic_info.get("release_date", "")),
        "is_new": _resolve_field(records, "is_new") if _resolve_field(records, "is_new") not in (None, "") else primary.get("isNew", basic_info.get("is_new", False)),
        "is_locked": is_locked,
        "source": "+".join(source_name(label) for label in labels),
        "source_labels": labels,
        "primary_source": primary_label,
        "source_fields": source_field_summaries(music),
        "regions": source_regions(all_song_charts),
        "levels": unique_values([chart.get("level") for chart in all_song_charts]),
        "ds": unique_values([chart.get("ds") for chart in all_song_charts]),
        # 输出层一律繁→简，让 UI 显示统一用简体；存储仍保留原文
        "aliases": unique_preserve_order([to_simplified(a) for a in aliases]),
        "matched_charts": charts,
    }


def sorted_source_labels(labels: list[Any]) -> list[str]:
    values = unique_preserve_order([str(label) for label in labels if str(label)])
    return sorted(
        values,
        key=lambda label: (
            SOURCE_PRIORITY.index(label)
            if label in SOURCE_PRIORITY
            else len(SOURCE_PRIORITY)
        ),
    )


def song_result_dedupe_key(song: dict[str, Any]) -> tuple[str, str] | None:
    song_id = song.get("id")
    title = normalize_text(song.get("title"))
    if song_id in (None, "") or not title:
        return None
    return normalize_song_id(song_id), title


def merge_region_maps(left: Any, right: Any) -> dict[str, bool]:
    left_map = left if isinstance(left, dict) else {}
    right_map = right if isinstance(right, dict) else {}
    return {key: bool(left_map.get(key) or right_map.get(key)) for key in REGION_KEYS}


def merge_source_id_maps(target: dict[str, Any], incoming: dict[str, Any]) -> None:
    target_ids = target.setdefault("source_ids", {})
    if not isinstance(target_ids, dict):
        target_ids = {}
        target["source_ids"] = target_ids
    aliases = target.setdefault("source_id_aliases", {})
    if not isinstance(aliases, dict):
        aliases = {}
        target["source_id_aliases"] = aliases
    for label, raw_id in (incoming.get("source_ids") or {}).items():
        if label not in target_ids:
            target_ids[label] = raw_id
            continue
        if target_ids[label] == raw_id:
            continue
        aliases[label] = unique_preserve_order([str(target_ids[label]), str(raw_id), *aliases.get(label, [])])
    if not aliases:
        target.pop("source_id_aliases", None)


def merge_source_field_maps(target: dict[str, Any], incoming: dict[str, Any]) -> None:
    target_fields = target.setdefault("source_fields", {})
    if not isinstance(target_fields, dict):
        target_fields = {}
        target["source_fields"] = target_fields
    for label, fields in (incoming.get("source_fields") or {}).items():
        if not isinstance(fields, dict):
            continue
        current = target_fields.get(label)
        if not isinstance(current, dict):
            target_fields[label] = dict(fields)
            continue
        for key, value in fields.items():
            if key in {"available_chart_types", "levels", "ds"}:
                current[key] = unique_values([*(current.get(key) or []), *(value or [])])
            elif key == "regions":
                current[key] = merge_region_maps(current.get(key), value)
            elif current.get(key) in (None, "", [], {}):
                current[key] = value


def chart_result_dedupe_key(chart: dict[str, Any]) -> tuple[Any, ...]:
    return (
        chart.get("source"),
        chart.get("chart_type"),
        chart.get("difficulty_index", chart.get("difficulty")),
        chart.get("level"),
        chart.get("ds"),
        chart.get("fit_source_id"),
        chart.get("internal_id"),
    )


def sort_chart_results(charts: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return sorted(
        charts,
        key=lambda chart: (
            SOURCE_PRIORITY.index(chart.get("source", "divingfish"))
            if chart.get("source", "divingfish") in SOURCE_PRIORITY
            else len(SOURCE_PRIORITY),
            {"standard": 0, "dx": 1, "utage1p": 2, "utage2p": 3}.get(str(chart.get("chart_type")), 9),
            int(chart.get("difficulty_index", 0)),
        ),
    )


def merge_duplicate_song_result(target: dict[str, Any], incoming: dict[str, Any]) -> None:
    merge_source_id_maps(target, incoming)
    merge_source_field_maps(target, incoming)
    labels = sorted_source_labels([*(target.get("source_labels") or []), *(incoming.get("source_labels") or [])])
    target["source_labels"] = labels
    target["source"] = "+".join(source_name(label) for label in labels)
    target["available_chart_types"] = sorted(
        set(target.get("available_chart_types") or []) | set(incoming.get("available_chart_types") or [])
    )
    target["levels"] = unique_values([*(target.get("levels") or []), *(incoming.get("levels") or [])])
    target["ds"] = unique_values([*(target.get("ds") or []), *(incoming.get("ds") or [])])
    target["aliases"] = unique_preserve_order([*(target.get("aliases") or []), *(incoming.get("aliases") or [])])
    target["regions"] = merge_region_maps(target.get("regions"), incoming.get("regions"))
    if "_rank" in incoming:
        target["_rank"] = min(target.get("_rank", incoming["_rank"]), incoming["_rank"])

    charts_by_key = {
        chart_result_dedupe_key(chart): chart
        for chart in (target.get("matched_charts") or [])
        if isinstance(chart, dict)
    }
    for chart in incoming.get("matched_charts") or []:
        if isinstance(chart, dict):
            charts_by_key.setdefault(chart_result_dedupe_key(chart), chart)
    target["matched_charts"] = sort_chart_results(list(charts_by_key.values()))


def merge_duplicate_song_results(results: list[dict[str, Any]]) -> list[dict[str, Any]]:
    merged: dict[tuple[str, str], dict[str, Any]] = {}
    output: list[dict[str, Any]] = []
    for song in results:
        key = song_result_dedupe_key(song)
        if key is None:
            output.append(song)
            continue
        existing = merged.get(key)
        if existing is None:
            merged[key] = song
            output.append(song)
            continue
        merge_duplicate_song_result(existing, song)
    return output


def flatten_note_counts(notes: Any) -> dict[str, int]:
    counts = {"tap": 0, "hold": 0, "slide": 0, "touch": 0, "break": 0, "total": 0}
    if not isinstance(notes, dict):
        return counts

    nested_sides = [notes.get("left"), notes.get("right")]
    if any(isinstance(side, dict) for side in nested_sides):
        for side in nested_sides:
            side_counts = flatten_note_counts(side)
            for key, value in side_counts.items():
                counts[key] += value
        return counts

    for key in counts:
        value = notes.get(key)
        if isinstance(value, int | float):
            counts[key] += int(value)

    if counts["total"] == 0:
        counts["total"] = (
            counts["tap"]
            + counts["hold"]
            + counts["slide"]
            + counts["touch"]
            + counts["break"]
        )
    return counts


def scoring_note_totals_from_chart(chart: dict[str, Any]) -> dict[str, int]:
    counts = flatten_note_counts(chart.get("notes") or {})
    return {
        "tap": counts["tap"],
        "touch": counts["touch"],
        "hold": counts["hold"],
        "slide": counts["slide"],
        "break": counts["break"],
    }


def chart_choice_summary(chart: dict[str, Any]) -> dict[str, Any]:
    original_counts = flatten_note_counts(chart.get("notes") or {})
    return {
        "source": chart.get("source"),
        "chart_type": chart.get("chart_type"),
        "difficulty": chart.get("difficulty"),
        "level": chart.get("level"),
        "ds": chart.get("ds"),
        "charter": chart.get("charter"),
        "version": chart.get("version"),
        "original_note_counts": original_counts,
        "scoring_note_totals": scoring_note_totals_from_chart(chart),
        "touch_scored_as_tap": original_counts["touch"] > 0,
    }


def dedupe_equivalent_charts(charts: list[dict[str, Any]]) -> list[dict[str, Any]]:
    selected: dict[tuple[str, str, str, str], dict[str, Any]] = {}
    for chart in charts:
        key = (
            str(chart.get("chart_type", "")),
            str(chart.get("difficulty", "")),
            str(chart.get("level", "")),
            str(chart.get("ds", "")),
        )
        current = selected.get(key)
        if current is None or (
            current.get("source") != "cn" and chart.get("source") == "cn"
        ):
            selected[key] = chart
    return list(selected.values())


def song_choice_summary(song: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": song.get("id"),
        "title": song.get("title"),
        "artist": song.get("artist"),
        "source": song.get("source"),
        "version": song.get("version"),
        "matched_chart_count": len(song.get("matched_charts") or []),
        "matched_charts": [
            {
                "source": chart.get("source"),
                "chart_type": chart.get("chart_type"),
                "difficulty": chart.get("difficulty"),
                "level": chart.get("level"),
                "ds": chart.get("ds"),
            }
            for chart in (song.get("matched_charts") or [])[:10]
        ],
    }


def resolve_single_chart_note_totals(
    *,
    query: str | None = None,
    level: str | None = None,
    genre: str | None = None,
    version: str | None = None,
    ds: float | int | str | None = None,
    ds_min: float | int | str | None = None,
    ds_max: float | int | str | None = None,
    fit_diff: float | int | str | None = None,
    fit_diff_min: float | int | str | None = None,
    fit_diff_max: float | int | str | None = None,
    fit_delta: float | int | str | None = None,
    fit_delta_min: float | int | str | None = None,
    fit_delta_max: float | int | str | None = None,
    fit_label: str | None = None,
    region_has: Any = None,
    region_missing: Any = None,
    difficulty: str | None = None,
    song_type: str | None = None,
    artist: str | None = None,
    charter: str | None = None,
    candidate_limit: int = 20,
    data_path: Path = DEFAULT_DATA_PATH,
    alias_path: Path = DEFAULT_ALIAS_PATH,
    chart_stats_path: Path = DEFAULT_CHART_STATS_PATH,
) -> dict[str, Any]:
    results, criteria = collect_song_results(
        query=query,
        level=level,
        genre=genre,
        version=version,
        ds=ds,
        ds_min=ds_min,
        ds_max=ds_max,
        fit_diff=fit_diff,
        fit_diff_min=fit_diff_min,
        fit_diff_max=fit_diff_max,
        fit_delta=fit_delta,
        fit_delta_min=fit_delta_min,
        fit_delta_max=fit_delta_max,
        fit_label=fit_label,
        region_has=region_has,
        region_missing=region_missing,
        difficulty=difficulty,
        song_type=song_type,
        artist=artist,
        charter=charter,
        data_path=data_path,
        alias_path=alias_path,
        chart_stats_path=chart_stats_path,
        require_criteria=True,
    )

    if query:
        results.sort(key=lambda item: item.pop("_rank", (0, "")))
    else:
        for item in results:
            item.pop("_rank", None)

    if not results:
        return {
            "resolved": False,
            "reason": "no_song_match",
            "calculated": False,
            "criteria": criteria,
            "songs": [],
        }

    if len(results) > 1:
        candidates = [song_choice_summary(song) for song in results[:candidate_limit]]
        return {
            "resolved": False,
            "reason": "multiple_song_matches",
            "calculated": False,
            "criteria": criteria,
            "total_matches": len(results),
            **_maybe_truncated(len(results) > candidate_limit),
            "songs": candidates,
        }

    song = results[0]
    charts = dedupe_equivalent_charts(song.get("matched_charts") or [])
    if len(charts) != 1:
        return {
            "resolved": False,
            "reason": "multiple_chart_matches" if charts else "no_chart_match",
            "calculated": False,
            "criteria": criteria,
            "song": song_choice_summary(song),
            "charts": [chart_choice_summary(chart) for chart in charts[:candidate_limit]],
            "total_charts": len(charts),
            **_maybe_truncated(len(charts) > candidate_limit),
        }

    chart = charts[0]
    original_counts = flatten_note_counts(chart.get("notes") or {})
    return {
        "resolved": True,
        "calculated": False,
        "criteria": criteria,
        "song": {
            "id": song.get("id"),
            "title": song.get("title"),
            "artist": song.get("artist"),
            "source": song.get("source"),
            "source_ids": song.get("source_ids"),
        },
        "chart": chart_choice_summary(chart),
        "note_totals": scoring_note_totals_from_chart(chart),
        "original_note_counts": original_counts,
        "touch_scored_as_tap": original_counts["touch"] > 0,
    }


def has_search_criteria(
    *,
    query: str | None = None,
    level: str | None = None,
    genre: str | None = None,
    version: str | None = None,
    ds: float | int | str | None = None,
    ds_min: float | int | str | None = None,
    ds_max: float | int | str | None = None,
    fit_diff: float | int | str | None = None,
    fit_diff_min: float | int | str | None = None,
    fit_diff_max: float | int | str | None = None,
    fit_delta: float | int | str | None = None,
    fit_delta_min: float | int | str | None = None,
    fit_delta_max: float | int | str | None = None,
    fit_label: str | None = None,
    region_has: Any = None,
    region_missing: Any = None,
    difficulty: str | None = None,
    song_type: str | None = None,
    is_new: bool | None = None,
    is_new_source: str | None = None,
    id: int | str | None = None,
    id_min: int | str | None = None,
    id_max: int | str | None = None,
    bpm: float | int | str | None = None,
    bpm_min: float | int | str | None = None,
    bpm_max: float | int | str | None = None,
    is_locked: bool | None = None,
    artist: str | None = None,
    charter: str | None = None,
    tag: Any = None,
    tag_exclude: Any = None,
    released_after: str | None = None,
    released_before: str | None = None,
    sort: str | None = None,
) -> bool:
    return any(
        value not in (None, "")
        for value in (
            query,
            level,
            genre,
            version,
            ds,
            ds_min,
            ds_max,
            fit_diff,
            fit_diff_min,
            fit_diff_max,
            fit_delta,
            fit_delta_min,
            fit_delta_max,
            fit_label,
            region_has,
            region_missing,
            difficulty,
            song_type,
            is_new,
            is_new_source,
            id,
            id_min,
            id_max,
            bpm,
            tag,
            tag_exclude,
            released_after,
            released_before,
            bpm_min,
            bpm_max,
            is_locked,
            artist,
            charter,
            sort,
        )
    )


@lru_cache(maxsize=8)
def cn_latest_versions(
    data_path: Path = DEFAULT_DATA_PATH,
) -> list[int]:
    """返回国服最新版本号列表（最新版若全是宴谱则回退到上一个版本）。"""
    data = load_music_data(data_path)
    versions: dict[int, list[dict[str, Any]]] = {}
    for music in data:
        records = source_records(music)
        cn = records.get("cn")
        if not cn:
            continue
        ver = cn.get("version")
        if not isinstance(ver, int):
            continue
        versions.setdefault(ver, []).append(music)

    if not versions:
        return []

    sorted_vers = sorted(versions.keys(), reverse=True)
    latest_ver = sorted_vers[0]
    latest_songs = versions[latest_ver]

    def _is_all_utage(songs: list[dict[str, Any]]) -> bool:
        for s in songs:
            cn = source_records(s).get("cn")
            if not cn:
                continue
            diffs = cn.get("difficulties", {})
            if diffs.get("standard") or diffs.get("dx"):
                return False
        return True

    if _is_all_utage(latest_songs) and len(sorted_vers) > 1:
        return [latest_ver, sorted_vers[1]]
    return [latest_ver]


def collect_song_results(
    *,
    query: str | None = None,
    level: str | None = None,
    genre: str | None = None,
    version: str | None = None,
    ds: float | int | str | None = None,
    ds_min: float | int | str | None = None,
    ds_max: float | int | str | None = None,
    fit_diff: float | int | str | None = None,
    fit_diff_min: float | int | str | None = None,
    fit_diff_max: float | int | str | None = None,
    fit_delta: float | int | str | None = None,
    fit_delta_min: float | int | str | None = None,
    fit_delta_max: float | int | str | None = None,
    fit_label: str | None = None,
    region_has: Any = None,
    region_missing: Any = None,
    difficulty: str | None = None,
    song_type: str | None = None,
    is_new: bool | None = None,
    is_new_source: str | None = None,
    id: int | str | None = None,
    id_min: int | str | None = None,
    id_max: int | str | None = None,
    bpm: float | int | str | None = None,
    bpm_min: float | int | str | None = None,
    bpm_max: float | int | str | None = None,
    is_locked: bool | None = None,
    artist: str | None = None,
    charter: str | None = None,
    tag: Any = None,
    tag_exclude: Any = None,
    released_after: str | None = None,
    released_before: str | None = None,
    sort: str | None = None,
    limit: int | None = None,
    require_criteria: bool = False,
    data_path: Path = DEFAULT_DATA_PATH,
    alias_path: Path = DEFAULT_ALIAS_PATH,
    pinyin_alias_path: Path = DEFAULT_PINYIN_ALIAS_PATH,
    chart_stats_path: Path = DEFAULT_CHART_STATS_PATH,
    tags_path: Path | None = None,
    artist_alias_path: Path = DEFAULT_ARTIST_ALIAS_PATH,
    charter_alias_path: Path = DEFAULT_CHARTER_ALIAS_PATH,
) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    if require_criteria and not has_search_criteria(
        query=query,
        level=level,
        genre=genre,
        version=version,
        ds=ds,
        ds_min=ds_min,
        ds_max=ds_max,
        fit_diff=fit_diff,
        fit_diff_min=fit_diff_min,
        fit_diff_max=fit_diff_max,
        fit_delta=fit_delta,
        fit_delta_min=fit_delta_min,
        fit_delta_max=fit_delta_max,
        fit_label=fit_label,
        region_has=region_has,
        region_missing=region_missing,
        difficulty=difficulty,
        song_type=song_type,
        is_new=is_new,
        is_new_source=is_new_source,
        id=id,
        id_min=id_min,
        id_max=id_max,
        bpm=bpm,
        bpm_min=bpm_min,
        bpm_max=bpm_max,
        is_locked=is_locked,
        artist=artist,
        charter=charter,
        tag=tag,
        tag_exclude=tag_exclude,
        released_after=released_after,
        released_before=released_before,
        sort=sort,
    ):
        raise SearchError("At least one search criterion is required.")

    if limit is not None and limit < 1:
        raise SearchError("limit must be a positive integer.")
    if is_new_source not in (None, "", "cn"):
        raise SearchError("is_new_source only supports cn in this branch")
    if released_after not in (None, "") or released_before not in (None, ""):
        raise SearchError("released_after/released_before filters require dxdata and are not supported in this branch")

    refresh_search_caches_if_sources_changed(
        data_path=data_path,
        alias_path=alias_path,
        chart_stats_path=chart_stats_path,
        pinyin_alias_path=pinyin_alias_path,
        artist_alias_path=artist_alias_path,
        charter_alias_path=charter_alias_path,
    )

    ds_low, ds_high = parse_ds_range(ds=ds, ds_min=ds_min, ds_max=ds_max)
    fit_diff_low, fit_diff_high, fit_diff_high_inclusive = parse_fit_diff_filter(
        fit_diff=fit_diff,
        fit_diff_min=fit_diff_min,
        fit_diff_max=fit_diff_max,
    )
    fit_delta_low, fit_delta_high = parse_float_range(
        value=fit_delta,
        low=fit_delta_min,
        high=fit_delta_max,
    )
    id_low, id_high = parse_int_range(value=id, low=id_min, high=id_max)
    bpm_low, bpm_high = parse_float_range(value=bpm, low=bpm_min, high=bpm_max)
    parsed_fit_label = parse_fit_label(fit_label)
    parsed_region_has = parse_regions(region_has)
    parsed_region_missing = parse_regions(region_missing)
    parsed_sort = parse_sort(sort)
    difficulty_index = parse_difficulty(difficulty)
    parsed_type = parse_song_type(song_type)
    required_tag_ids = parse_tag(tag, tags_path=tags_path)
    excluded_tag_ids = parse_tag(tag_exclude, tags_path=tags_path)
    parsed_released_after = None
    parsed_released_before = None
    data, alias_map, pinyin_alias_map, chart_stats = load_search_context(
        data_path,
        alias_path,
        chart_stats_path,
        pinyin_alias_path,
        current_pinyin_alias_bucket(),
    )
    tag_index = None
    artist_alias_map = load_name_alias_map(artist_alias_path) if artist else {}
    charter_alias_map = load_name_alias_map(charter_alias_path) if charter else {}

    results: list[dict[str, Any]] = []
    for music in data:
        aliases = aliases_for_music(alias_map, music)
        pinyin_aliases = aliases_from_id_map(pinyin_alias_map, music)
        # 歌曲级过滤：id 范围、bpm 范围、is_locked
        if id_low is not None or id_high is not None:
            int_ids = music_int_ids(music)
            if not int_ids:
                continue
            if not any(int_in_range(v, id_low, id_high) for v in int_ids):
                continue
        if bpm_low is not None or bpm_high is not None:
            if not float_in_range(music_bpm_value(music), bpm_low, bpm_high):
                continue
        if is_locked is not None:
            locked = music_is_locked(music)
            if locked is None:
                locked = False
            if bool(locked) != bool(is_locked):
                continue
        if not genre_matches(music, genre):
            continue
        if not artist_matches(music, artist, artist_alias_map):
            continue
        if not music_version_matches(music, version):
            continue
        if is_new is not None:
            records = source_records(music)
            if is_new_source == "cn":
                # 国服：用最新版本号判断
                cn = records.get("cn")
                if cn:
                    cn_ver = cn.get("version")
                    latest_vers = cn_latest_versions(data_path)
                    music_is_new = isinstance(cn_ver, int) and cn_ver in latest_vers
                else:
                    music_is_new = False
            elif is_new_source:
                # 指定其他源：从指定源取 is_new
                source_music = records.get(is_new_source)
                if source_music:
                    basic_info = source_basic_info(source_music)
                    music_is_new = source_music.get("isNew", basic_info.get("is_new", False))
                else:
                    music_is_new = False
            else:
                # 未指定源：先查国服最新版本，再查其他源的 is_new 字段
                cn = records.get("cn")
                cn_ver = cn.get("version") if cn else None
                latest_vers = cn_latest_versions(data_path)
                music_is_new = isinstance(cn_ver, int) and cn_ver in latest_vers
                if not music_is_new:
                    for label in SOURCE_PRIORITY:
                        if label == "cn":
                            continue
                        source_music = records.get(label)
                        if not source_music:
                            continue
                        basic_info = source_basic_info(source_music)
                        if source_music.get("isNew", basic_info.get("is_new", False)):
                            music_is_new = True
                            break
            if is_new and not music_is_new:
                continue
            if not is_new and music_is_new:
                continue
        if not title_matches(music, aliases, pinyin_aliases, query):
            continue
        all_song_charts = attach_fit_stats(music, all_charts(music), chart_stats)
        if not regions_match(source_regions(all_song_charts), parsed_region_has, parsed_region_missing):
            continue
        chart_version_filter = None if music_base_version_matches(music, version) else version
        charts = matched_charts(
            music,
            level=level,
            version=chart_version_filter,
            ds_low=ds_low,
            ds_high=ds_high,
            fit_diff_low=fit_diff_low,
            fit_diff_high=fit_diff_high,
            fit_diff_high_inclusive=fit_diff_high_inclusive,
            fit_delta_low=fit_delta_low,
            fit_delta_high=fit_delta_high,
            fit_label=parsed_fit_label,
            difficulty_index=difficulty_index,
            song_type=parsed_type,
            charter=charter,
            required_tag_ids=required_tag_ids,
            excluded_tag_ids=excluded_tag_ids,
            released_after=parsed_released_after,
            released_before=parsed_released_before,
            chart_stats=chart_stats,
            tag_index=tag_index,
            charter_alias_map=charter_alias_map,
        )
        if not charts:
            continue
        serialized = serialize_music(music, charts, aliases)
        if query:
            match = query_match(music, aliases, pinyin_aliases, query)
            serialized["match"] = {
                key: value for key, value in match.items()
                if key not in {"rank", "sort_title"}
            }
            serialized["_rank"] = (match["rank"], match["sort_title"])
        results.append(serialized)

    results = merge_duplicate_song_results(results)

    # 不把 fit_diff_high_inclusive 暴露到 criteria 输出里。它纯粹是 parse_fit_diff_filter
    # 内部用来区分"单值分桶（< 上界）"和"显式区间（<= 上界）"的标志位，对外没有意义；
    # 之前漏出去之后 Agent 在 tool call 上下文里看到它，会反复把这个字段塞回参数里。
    criteria = {
        "query": query,
        "level": level,
        "genre": genre,
        "version": version,
        "ds": ds,
        "ds_min": ds_low,
        "ds_max": ds_high,
        "fit_diff": fit_diff,
        "fit_diff_min": fit_diff_low,
        "fit_diff_max": fit_diff_high,
        "fit_delta": fit_delta,
        "fit_delta_min": fit_delta_low,
        "fit_delta_max": fit_delta_high,
        "fit_label": parsed_fit_label,
        "region_has": sorted(parsed_region_has),
        "region_missing": sorted(parsed_region_missing),
        "difficulty": difficulty,
        "song_type": parsed_type,
        "id": id,
        "id_min": id_low,
        "id_max": id_high,
        "bpm": bpm,
        "bpm_min": bpm_low,
        "bpm_max": bpm_high,
        "is_locked": is_locked,
        "artist": artist,
        "charter": charter,
        "tag": describe_tags(required_tag_ids, tags_path=tags_path) or None,
        "tag_exclude": describe_tags(excluded_tag_ids, tags_path=tags_path) or None,
        "released_after": parsed_released_after,
        "released_before": parsed_released_before,
        "sort": parsed_sort,
        "limit": limit,
    }
    if parsed_sort:
        sort_results_by_fit(results, parsed_sort)
    return results, criteria


def chart_sort_value(chart: dict[str, Any], key: str) -> float | None:
    value = chart.get(key)
    if value is None:
        return None
    return float(value)


def song_sort_value(song: dict[str, Any], sort: str) -> float | None:
    charts = song.get("matched_charts") or []
    if not charts:
        return None
    if sort.startswith("fit_delta"):
        values = [chart_sort_value(chart, "fit_delta") for chart in charts]
    else:
        values = [chart_sort_value(chart, "fit_diff") for chart in charts]
    values = [value for value in values if value is not None]
    if not values:
        return None
    if sort == "fit_delta_desc":
        return max(values)
    if sort == "fit_delta_asc":
        return min(values)
    if sort == "fit_diff_desc":
        return max(values)
    return min(values)


def sort_results_by_fit(results: list[dict[str, Any]], sort: str) -> None:
    descending = sort.endswith("_desc")
    for song in results:
        charts = song.get("matched_charts") or []
        key_name = "fit_delta" if sort.startswith("fit_delta") else "fit_diff"
        charts.sort(
            key=lambda chart: (
                chart_sort_value(chart, key_name) is None,
                -float(chart_sort_value(chart, key_name) or 0)
                if descending
                else float(chart_sort_value(chart, key_name) or 0),
            ),
        )
    results.sort(
        key=lambda song: (
            song_sort_value(song, sort) is None,
            -float(song_sort_value(song, sort) or 0)
            if descending
            else float(song_sort_value(song, sort) or 0),
            normalize_text(song.get("title", "")),
        ),
    )


def search_songs(
    *,
    query: str | None = None,
    level: str | None = None,
    genre: str | None = None,
    version: str | None = None,
    ds: float | int | str | None = None,
    ds_min: float | int | str | None = None,
    ds_max: float | int | str | None = None,
    fit_diff: float | int | str | None = None,
    fit_diff_min: float | int | str | None = None,
    fit_diff_max: float | int | str | None = None,
    fit_delta: float | int | str | None = None,
    fit_delta_min: float | int | str | None = None,
    fit_delta_max: float | int | str | None = None,
    fit_label: str | None = None,
    region_has: Any = None,
    region_missing: Any = None,
    difficulty: str | None = None,
    song_type: str | None = None,
    is_new: bool | None = None,
    is_new_source: str | None = None,
    id: int | str | None = None,
    id_min: int | str | None = None,
    id_max: int | str | None = None,
    bpm: float | int | str | None = None,
    bpm_min: float | int | str | None = None,
    bpm_max: float | int | str | None = None,
    is_locked: bool | None = None,
    artist: str | None = None,
    charter: str | None = None,
    tag: Any = None,
    tag_exclude: Any = None,
    released_after: str | None = None,
    released_before: str | None = None,
    sort: str | None = None,
    limit: int | None = None,
    data_path: Path = DEFAULT_DATA_PATH,
    alias_path: Path = DEFAULT_ALIAS_PATH,
    pinyin_alias_path: Path = DEFAULT_PINYIN_ALIAS_PATH,
    chart_stats_path: Path = DEFAULT_CHART_STATS_PATH,
    tags_path: Path | None = None,
) -> dict[str, Any]:
    results, criteria = collect_song_results(
        query=query,
        level=level,
        genre=genre,
        version=version,
        ds=ds,
        ds_min=ds_min,
        ds_max=ds_max,
        fit_diff=fit_diff,
        fit_diff_min=fit_diff_min,
        fit_diff_max=fit_diff_max,
        fit_delta=fit_delta,
        fit_delta_min=fit_delta_min,
        fit_delta_max=fit_delta_max,
        fit_label=fit_label,
        region_has=region_has,
        region_missing=region_missing,
        difficulty=difficulty,
        song_type=song_type,
        is_new=is_new,
        is_new_source=is_new_source,
        id=id,
        id_min=id_min,
        id_max=id_max,
        bpm=bpm,
        bpm_min=bpm_min,
        bpm_max=bpm_max,
        is_locked=is_locked,
        artist=artist,
        charter=charter,
        tag=tag,
        tag_exclude=tag_exclude,
        released_after=released_after,
        released_before=released_before,
        sort=sort,
        limit=limit,
        require_criteria=True,
        data_path=data_path,
        alias_path=alias_path,
        pinyin_alias_path=pinyin_alias_path,
        chart_stats_path=chart_stats_path,
        tags_path=tags_path,
    )

    if query and not criteria.get("sort"):
        results.sort(key=lambda item: item.pop("_rank", (0, "")))
    else:
        for item in results:
            item.pop("_rank", None)

    total_matches = len(results)
    truncated = limit is not None and total_matches > limit
    returned_results = results[:limit] if limit is not None else results

    return {
        "count": len(returned_results),
        "total_matches": total_matches,
        **_maybe_truncated(truncated),
        "criteria": criteria,
        "songs": returned_results,
    }


def parse_count(value: Any, *, default: int, maximum: int) -> int:
    if value in (None, ""):
        return default
    count = int(value)
    if count < 1:
        raise SearchError("count/limit must be a positive integer.")
    if count > maximum:
        raise SearchError(f"count/limit must be <= {maximum}.")
    return count


def random_songs(
    *,
    count: int | str | None = 1,
    level: str | None = None,
    genre: str | None = None,
    version: str | None = None,
    ds: float | int | str | None = None,
    ds_min: float | int | str | None = None,
    ds_max: float | int | str | None = None,
    fit_diff: float | int | str | None = None,
    fit_diff_min: float | int | str | None = None,
    fit_diff_max: float | int | str | None = None,
    fit_delta: float | int | str | None = None,
    fit_delta_min: float | int | str | None = None,
    fit_delta_max: float | int | str | None = None,
    fit_label: str | None = None,
    region_has: Any = None,
    region_missing: Any = None,
    difficulty: str | None = None,
    song_type: str | None = None,
    artist: str | None = None,
    charter: str | None = None,
    tag: Any = None,
    tag_exclude: Any = None,
    released_after: str | None = None,
    released_before: str | None = None,
    sort: str | None = None,
    seed: int | str | None = None,
    data_path: Path = DEFAULT_DATA_PATH,
    alias_path: Path = DEFAULT_ALIAS_PATH,
    chart_stats_path: Path = DEFAULT_CHART_STATS_PATH,
    tags_path: Path | None = None,
) -> dict[str, Any]:
    requested_count = parse_count(count, default=1, maximum=100)
    has_chart_filter = any(
        value not in (None, "")
        for value in (
            level,
            ds,
            ds_min,
            ds_max,
            fit_diff,
            fit_diff_min,
            fit_diff_max,
            fit_delta,
            fit_delta_min,
            fit_delta_max,
            fit_label,
            difficulty,
            song_type,
            charter,
            tag,
            tag_exclude,
            released_after,
            released_before,
        )
    )

    if has_chart_filter:
        results, criteria = collect_song_results(
            level=level,
            genre=genre,
            version=version,
            ds=ds,
            ds_min=ds_min,
            ds_max=ds_max,
            fit_diff=fit_diff,
            fit_diff_min=fit_diff_min,
            fit_diff_max=fit_diff_max,
            fit_delta=fit_delta,
            fit_delta_min=fit_delta_min,
            fit_delta_max=fit_delta_max,
            fit_label=fit_label,
            region_has=region_has,
            region_missing=region_missing,
            difficulty=difficulty,
            song_type=song_type,
            artist=artist,
            charter=charter,
            tag=tag,
            tag_exclude=tag_exclude,
            released_after=released_after,
            released_before=released_before,
            sort=sort,
            data_path=data_path,
            alias_path=alias_path,
            chart_stats_path=chart_stats_path,
            tags_path=tags_path,
        )
        random_mode = "chart_filtered"
    else:
        results, criteria = collect_song_results(
            genre=genre,
            version=version,
            region_has=region_has,
            region_missing=region_missing,
            artist=artist,
            sort=sort,
            data_path=data_path,
            alias_path=alias_path,
            chart_stats_path=chart_stats_path,
            tags_path=tags_path,
        )
        random_mode = "song_id"

    if not results:
        raise SearchError("No songs matched the random criteria.")

    rng = random.Random(seed) if seed not in (None, "") else random
    if requested_count >= len(results):
        selected = list(results)
        rng.shuffle(selected)
    else:
        selected = rng.sample(results, requested_count)

    criteria.update({"count": requested_count, "seed": seed, "random_mode": random_mode})
    return {
        "count": len(selected),
        "total_candidates": len(results),
        "criteria": criteria,
        "songs": selected,
    }


def id_sort_parts(song: dict[str, Any]) -> tuple[int | None, str, str]:
    song_id = normalize_song_id(song.get("id", ""))
    try:
        numeric_id: int | None = int(song_id)
    except ValueError:
        numeric_id = None
    return numeric_id, normalize_text(song_id), normalize_text(song.get("title", ""))


def sort_results_by_id(results: list[dict[str, Any]], *, descending: bool) -> list[dict[str, Any]]:
    numeric_results: list[dict[str, Any]] = []
    nonnumeric_results: list[dict[str, Any]] = []
    for song in results:
        numeric_id, _, _ = id_sort_parts(song)
        if numeric_id is None:
            nonnumeric_results.append(song)
        else:
            numeric_results.append(song)

    numeric_results.sort(
        key=lambda song: (id_sort_parts(song)[0] or 0, id_sort_parts(song)[2]),
        reverse=descending,
    )
    nonnumeric_results.sort(
        key=lambda song: (id_sort_parts(song)[1], id_sort_parts(song)[2]),
        reverse=descending,
    )
    return [*numeric_results, *nonnumeric_results]


def list_songs_by_id(
    *,
    order: str = "asc",
    limit: int | str | None = 20,
    level: str | None = None,
    genre: str | None = None,
    version: str | None = None,
    ds: float | int | str | None = None,
    ds_min: float | int | str | None = None,
    ds_max: float | int | str | None = None,
    fit_diff: float | int | str | None = None,
    fit_diff_min: float | int | str | None = None,
    fit_diff_max: float | int | str | None = None,
    fit_delta: float | int | str | None = None,
    fit_delta_min: float | int | str | None = None,
    fit_delta_max: float | int | str | None = None,
    fit_label: str | None = None,
    region_has: Any = None,
    region_missing: Any = None,
    difficulty: str | None = None,
    song_type: str | None = None,
    artist: str | None = None,
    charter: str | None = None,
    tag: Any = None,
    tag_exclude: Any = None,
    released_after: str | None = None,
    released_before: str | None = None,
    sort: str | None = None,
    data_path: Path = DEFAULT_DATA_PATH,
    alias_path: Path = DEFAULT_ALIAS_PATH,
    chart_stats_path: Path = DEFAULT_CHART_STATS_PATH,
    tags_path: Path | None = None,
) -> dict[str, Any]:
    normalized_order = normalize_text(order or "asc")
    if normalized_order in {"asc", "ascending", "正序", "升序"}:
        descending = False
        order_value = "asc"
    elif normalized_order in {"desc", "descending", "倒序", "降序"}:
        descending = True
        order_value = "desc"
    else:
        raise SearchError("order must be asc/desc, 正序/倒序, or 升序/降序.")

    requested_limit = parse_count(limit, default=20, maximum=2000)
    results, criteria = collect_song_results(
        level=level,
        genre=genre,
        version=version,
        ds=ds,
        ds_min=ds_min,
        ds_max=ds_max,
        fit_diff=fit_diff,
        fit_diff_min=fit_diff_min,
        fit_diff_max=fit_diff_max,
        fit_delta=fit_delta,
        fit_delta_min=fit_delta_min,
        fit_delta_max=fit_delta_max,
        fit_label=fit_label,
        region_has=region_has,
        region_missing=region_missing,
        difficulty=difficulty,
        song_type=song_type,
        artist=artist,
        charter=charter,
        tag=tag,
        tag_exclude=tag_exclude,
        released_after=released_after,
        released_before=released_before,
        sort=sort,
        data_path=data_path,
        alias_path=alias_path,
        chart_stats_path=chart_stats_path,
        tags_path=tags_path,
    )
    if sort is None:
        results = sort_results_by_id(results, descending=descending)
    total_matches = len(results)
    returned_results = results[:requested_limit]
    criteria.update({"order": order_value, "limit": requested_limit})
    return {
        "count": len(returned_results),
        "total_matches": total_matches,
        **_maybe_truncated(total_matches > requested_limit),
        "criteria": criteria,
        "songs": returned_results,
    }


def version_sort_key(version: str) -> tuple[int, int | str]:
    try:
        return 0, int(version)
    except ValueError:
        return 1, normalize_text(version)


def list_versions(
    *,
    query: str | None = None,
    limit: int | str | None = None,
    data_path: Path = DEFAULT_DATA_PATH,
) -> dict[str, Any]:
    requested_limit = parse_count(limit, default=2000, maximum=5000) if limit not in (None, "") else None
    versions: dict[str, dict[str, Any]] = {}
    for music in load_music_data(data_path):
        for label, source_music in source_records(music).items():
            song_seen: set[str] = set()
            for value in music_version_values(source_music, include_charts=False):
                if query and not version_value_matches_filter(value, query):
                    continue
                item = versions.setdefault(
                    value,
                    {"version": value, "song_count": 0, "chart_count": 0, "sources": set()},
                )
                if value not in song_seen:
                    item["song_count"] += 1
                    song_seen.add(value)
                item["sources"].add(source_name(label))

            for chart in charts_from_single_music(source_music):
                value = chart.get("version")
                if value in (None, "") or (query and not version_value_matches_filter(value, query)):
                    continue
                key = str(value)
                item = versions.setdefault(
                    key,
                    {"version": key, "song_count": 0, "chart_count": 0, "sources": set()},
                )
                item["chart_count"] += 1
                if key not in song_seen:
                    item["song_count"] += 1
                    song_seen.add(key)
                item["sources"].add(source_name(label))

    results = sorted(versions.values(), key=lambda item: version_sort_key(item["version"]))
    for item in results:
        item["sources"] = sorted(item["sources"])

    total_matches = len(results)
    returned = results[:requested_limit] if requested_limit is not None else results
    latest_cn_versions = cn_latest_versions(data_path)
    latest_cn_years = unique_preserve_order([str(version)[:2] for version in latest_cn_versions])
    return {
        "count": len(returned),
        "total_matches": total_matches,
        **_maybe_truncated(requested_limit is not None and total_matches > requested_limit),
        "criteria": {"query": query, "limit": requested_limit},
        "latest_cn_versions": latest_cn_versions,
        "latest_cn_years": latest_cn_years,
        "versions": returned,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Search local maimai DX song data.")
    parser.add_argument("query", nargs="?", help="Song ID or fuzzy song title.")
    parser.add_argument("--level", help="Chart level, for example 13, 13+, 14+.")
    parser.add_argument("--genre", help="Song genre/category filter, for example niconico or POPS.")
    parser.add_argument("--version", help="Song/chart version filter, for example PRiSM PLUS.")
    parser.add_argument("--ds", help="Chart constant, for example 13.7 or 13.4-13.8.")
    parser.add_argument("--ds-min", type=float, help="Minimum chart constant.")
    parser.add_argument("--ds-max", type=float, help="Maximum chart constant.")
    parser.add_argument("--fit-diff", help="Fitted chart constant, for example 13.1 or 13.13-13.27.")
    parser.add_argument("--fit-diff-min", type=float, help="Minimum fitted chart constant.")
    parser.add_argument("--fit-diff-max", type=float, help="Maximum fitted chart constant.")
    parser.add_argument("--fit-delta", help="Actual ds minus fitted chart constant, e.g. -0.3-0.")
    parser.add_argument("--fit-delta-min", type=float, help="Minimum ds - fit_diff.")
    parser.add_argument("--fit-delta-max", type=float, help="Maximum ds - fit_diff.")
    parser.add_argument("--fit-label", help="虚高 or 虚低.")
    parser.add_argument("--region-has", help="Required region: cn or 国服.")
    parser.add_argument("--region-missing", help="Missing region: cn or 国服.")
    parser.add_argument("--sort", help="fit_delta_desc, fit_delta_asc, fit_diff_asc, or fit_diff_desc.")
    parser.add_argument("--difficulty", help="Difficulty, for example 紫, Master, white.")
    parser.add_argument("--type", dest="song_type", help="Song type: SD or DX.")
    parser.add_argument(
        "--tag",
        help="Required tag(s). Use comma/slash to separate, e.g. 爆发,纵连. Accepts zh-Hans/en/ja/ko names or ID.",
    )
    parser.add_argument(
        "--tag-exclude",
        help="Tag(s) that the chart must NOT have. Same input format as --tag.",
    )
    parser.add_argument(
        "--released-after",
        help="Earliest chart release date (YYYY / YYYY-MM / YYYY-MM-DD).",
    )
    parser.add_argument(
        "--released-before",
        help="Latest chart release date (YYYY / YYYY-MM / YYYY-MM-DD).",
    )
    parser.add_argument("--limit", type=int, help="Maximum result count.")
    args = parser.parse_args()

    try:
        result = search_songs(
            query=args.query,
            level=args.level,
            genre=args.genre,
            version=args.version,
            ds=args.ds,
            ds_min=args.ds_min,
            ds_max=args.ds_max,
            fit_diff=args.fit_diff,
            fit_diff_min=args.fit_diff_min,
            fit_diff_max=args.fit_diff_max,
            fit_delta=args.fit_delta,
            fit_delta_min=args.fit_delta_min,
            fit_delta_max=args.fit_delta_max,
            fit_label=args.fit_label,
            region_has=args.region_has,
            region_missing=args.region_missing,
            difficulty=args.difficulty,
            song_type=args.song_type,
            tag=args.tag,
            tag_exclude=args.tag_exclude,
            released_after=args.released_after,
            released_before=args.released_before,
            sort=args.sort,
            limit=args.limit,
        )
    except SearchError as exc:
        raise SystemExit(str(exc)) from exc

    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
