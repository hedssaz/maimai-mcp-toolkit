import random
import time
import traceback
from collections import defaultdict
from typing import Callable, DefaultDict, List, Optional, Tuple, Union

try:
    import pyecharts.options as opts
    from pyecharts.charts import Pie
except ModuleNotFoundError:
    opts = None
    Pie = None

from . import *
from .image import *
from .maimai_best_50 import ScoreBaseImage, changeColumnWidth, coloumWidth, computeRa
from .maimaidx_error import *
from .maimaidx_model import PlanInfo, PlayInfoDefault, PlayInfoDev, RaMusic, RiseScore, ChartInfo, UserInfoDev
from .maimaidx_model import Music
from . import mai, maiApi, normalize_level_value
from .tool import run_chrome_to_base64

Filter = Tuple[
    List[PlayInfoDefault],
    List[PlayInfoDefault],
    List[PlayInfoDefault],
    List[PlayInfoDefault],
    List[PlayInfoDefault]
]
Condition = Callable[[PlayInfoDefault], bool]
GLOBAL_STATS_FONT_FAMILY = "ResourceHanRoundedCN, Microsoft YaHei, WenQuanYi Micro Hei, sans-serif"
GLOBAL_STATS_FALLBACK_FOOTER = "PIL fallback renderer"
RISE_SCORE_ALGORITHM_EXPECTED = "expected"
RISE_SCORE_ALGORITHM_LEGACY = "legacy"
RISE_SCORE_ALGORITHM_DEFAULT = RISE_SCORE_ALGORITHM_LEGACY
RISE_SCORE_ALGORITHMS = {RISE_SCORE_ALGORITHM_EXPECTED, RISE_SCORE_ALGORITHM_LEGACY}
RISE_SCORE_TARGETS = tuple(achievementList[-4:])


def _patch_global_stats_html_font() -> None:
    """Load the bundled CJK font before ECharts snapshots the canvas."""
    try:
        html = pie_html_file.read_text(encoding="utf-8")
    except OSError:
        return
    if "font-family:'ResourceHanRoundedCN'" in html:
        return
    style = (
        "<style>"
        "@font-face{font-family:'ResourceHanRoundedCN';"
        f"src:url('./{SIYUAN.name}') format('truetype');font-weight:700;}}"
        f"body{{font-family:{GLOBAL_STATS_FONT_FAMILY};}}"
        "</style>"
    )
    if "</head>" in html:
        html = html.replace("</head>", f"{style}</head>", 1)
    else:
        html = f"{style}{html}"
    pie_html_file.write_text(html, encoding="utf-8")


def _positive_song_id(value) -> Optional[int]:
    try:
        number = int(value)
    except (TypeError, ValueError):
        return None
    return number if number > 0 else None


def _display_song_id(value) -> str:
    song_id = _positive_song_id(value)
    return str(song_id) if song_id is not None else ""


def _equivalent_song_ids(value) -> set[int]:
    song_id = _positive_song_id(value)
    if song_id is None:
        return set()
    song_ids = {song_id}
    if song_id > 10000:
        song_ids.add(song_id - 10000)
    elif 1000 < song_id < 10000:
        song_ids.add(song_id + 10000)
    return song_ids


def _old_record_achievement(
    old_records: DefaultDict[int, Dict[int, float]],
    song_id: int,
    level_index: int
) -> Optional[float]:
    for equivalent_id in _equivalent_song_ids(song_id):
        if equivalent_id in old_records and level_index in old_records[equivalent_id]:
            return old_records[equivalent_id][level_index]
    return None


def _is_cn_music(music: Music) -> bool:
    matcher = getattr(mai, "_music_matches_server", None)
    if callable(matcher):
        return bool(matcher(music, "cn"))
    return _positive_song_id(music.id) is not None


def _cn_current_rise_versions() -> List[str]:
    versions: List[str] = []
    seen: set[str] = set()
    for music in mai.total_list:
        if not _is_cn_music(music):
            continue
        if not getattr(music.basic_info, "is_new", False):
            continue
        version = getattr(music.basic_info, "version", None)
        if version and version not in seen:
            versions.append(version)
            seen.add(version)
    return versions


def _cn_legacy_rise_versions(current_versions: List[str]) -> List[str]:
    current = set(current_versions)
    versions: List[str] = []
    seen: set[str] = set()
    for music in mai.total_list:
        if not _is_cn_music(music):
            continue
        version = getattr(music.basic_info, "version", None)
        if not version or version in current or version in seen:
            continue
        versions.append(version)
        seen.add(version)
    return versions


def _rise_score_candidate_versions(type: str, info: Optional[List[ChartInfo]]) -> List[str]:
    mai._ensure_loaded()
    current_versions = _cn_current_rise_versions()
    if type == 'DX':
        if current_versions:
            return current_versions
        return list(plate_to_dx_version.values())[-2:]
    versions = _cn_legacy_rise_versions(current_versions)
    if versions:
        return versions
    return list(plate_to_dx_version.values())[:-2]


def _rise_score_fit_delta(music: Music, level_index: int) -> Optional[float]:
    stats = getattr(music, "stats", None) or []
    if level_index >= len(stats):
        return None
    stat = stats[level_index]
    if stat is None:
        return None
    fit_diff = getattr(stat, "fit_diff", None)
    if fit_diff is None:
        return None
    try:
        return round(float(music.ds[level_index]) - float(fit_diff), 10)
    except (TypeError, ValueError, IndexError):
        return None


def _rise_score_fit_delta_bucket(fit_delta: Optional[float]) -> Optional[int]:
    if fit_delta is None:
        return None
    if fit_delta > 0.2:
        return 0
    if fit_delta >= 0:
        return 1
    if fit_delta >= -0.2:
        return 2
    return 3


def _select_rise_score_candidates(
    candidates: List[Tuple[int, RiseScore]],
    limit: int = 5
) -> List[RiseScore]:
    selected: List[RiseScore] = []
    for bucket in range(5):
        if len(selected) >= limit:
            break
        bucket_items = [item for item_bucket, item in candidates if item_bucket == bucket]
        remaining = limit - len(selected)
        if len(bucket_items) <= remaining:
            selected.extend(bucket_items)
        else:
            selected.extend(random.sample(bucket_items, remaining))
            break
    selected.sort(key=lambda x: x.song_id, reverse=True)
    return selected


def _rise_score_section_size(type: str) -> int:
    return 15 if type == 'DX' else 35


def _chart_ra(info: Optional[List[ChartInfo]]) -> int:
    if not info:
        return 0
    try:
        return int(info[-1].ra or 0)
    except (TypeError, ValueError):
        return 0


def _rise_score_replacement_floor(type: str, info: Optional[List[ChartInfo]]) -> int:
    if not info:
        return 0
    if len(info) < _rise_score_section_size(type):
        return 0
    return _chart_ra(info)


def _rise_score_candidate_floor(
    type: str,
    info: Optional[List[ChartInfo]],
    fallback_info: Optional[List[ChartInfo]] = None,
) -> int:
    current_floor = _chart_ra(info)
    fallback_floor = _chart_ra(fallback_info)
    if not info or len(info) < _rise_score_section_size(type):
        return max(current_floor, fallback_floor, 250)
    return current_floor or fallback_floor or 250


def _clamp_float(value: float, minimum: float, maximum: float) -> float:
    return min(max(value, minimum), maximum)


def _percentile(values: List[float], ratio: float) -> float:
    ordered = sorted(values)
    if not ordered:
        return 0.0
    if len(ordered) == 1:
        return ordered[0]
    position = (len(ordered) - 1) * ratio
    lower = int(position)
    upper = min(lower + 1, len(ordered) - 1)
    fraction = position - lower
    return ordered[lower] * (1 - fraction) + ordered[upper] * fraction


def _achievement_ra_ratio(achievement: float) -> float:
    return max(computeRa(100.0, achievement) / 100.0, 1.0)


def _floor_ds_for_achievement(ra: int, achievement: float) -> float:
    return max(0.0, ra / _achievement_ra_ratio(achievement))


def _record_ds(value) -> Optional[float]:
    try:
        ds = float(getattr(value, "ds", 0) or 0)
    except (TypeError, ValueError):
        return None
    return ds if ds > 0 else None


def _record_achievement(value) -> Optional[float]:
    try:
        return float(getattr(value, "achievements", 0) or 0)
    except (TypeError, ValueError):
        return None


def _rise_score_profile_records(
    records: Optional[List[PlayInfoDev]],
    info: Optional[List[ChartInfo]],
    fallback_info: Optional[List[ChartInfo]],
) -> List[Union[PlayInfoDev, ChartInfo]]:
    seen: set[tuple[int, int]] = set()
    merged: List[Union[PlayInfoDev, ChartInfo]] = []
    for source in (records or [], info or [], fallback_info or []):
        song_id = getattr(source, "song_id", None)
        level_index = getattr(source, "level_index", None)
        key: tuple[int, int] | None = None
        try:
            if song_id is not None and level_index is not None:
                key = (int(song_id), int(level_index))
        except (TypeError, ValueError):
            key = None
        if key is not None:
            if key in seen:
                continue
            seen.add(key)
        merged.append(source)
    return merged


def _rise_score_target_ability_ds(
    records: List[Union[PlayInfoDev, ChartInfo]],
    target_achievement: float,
    recommendation_floor: int,
) -> float:
    achieved_ds = [
        ds
        for record in records
        if (ds := _record_ds(record)) is not None
        and (_record_achievement(record) or 0) >= target_achievement
    ]
    floor_ds = _floor_ds_for_achievement(recommendation_floor, target_achievement)
    if len(achieved_ds) >= 8:
        record_ds = _percentile(achieved_ds, 0.82)
    elif len(achieved_ds) >= 3:
        record_ds = _percentile(achieved_ds, 0.70)
    elif achieved_ds:
        record_ds = max(achieved_ds) - 0.2
    else:
        record_ds = 0.0
    return max(record_ds, floor_ds)


def _rise_score_target_probability(
    ds: float,
    target_achievement: float,
    ability_ds: float,
    old_achievement: Optional[float],
) -> float:
    margin = ability_ds - ds
    probability = 0.62 + margin * 0.95
    if target_achievement >= 100.5:
        probability -= 0.05
    elif target_achievement <= 99.0:
        probability += 0.06
    if old_achievement:
        if old_achievement >= target_achievement - 0.1:
            probability += 0.18
        elif old_achievement >= target_achievement - 0.6:
            probability += 0.10
        elif old_achievement >= target_achievement - 1.5:
            probability += 0.04
    return _clamp_float(probability, 0.05, 0.98)


def _rise_score_target_margin_limit(target_achievement: float, old_achievement: Optional[float]) -> float:
    if old_achievement is None or old_achievement <= 0:
        if target_achievement >= 100.5:
            return 0.0
        if target_achievement >= 100.0:
            return 0.15
        if target_achievement >= 99.5:
            return 0.0
        return 0.25
    bonus = 0.15 if old_achievement and old_achievement >= target_achievement - 0.8 else 0.0
    if target_achievement >= 100.5:
        return 0.25 + bonus
    if target_achievement >= 100.0:
        return 0.35 + bonus
    if target_achievement >= 99.5:
        return 0.55 + bonus
    return 0.75 + bonus


def _rise_score_target_allowed_by_history(
    target_achievement: float,
    old_achievement: Optional[float],
) -> bool:
    if old_achievement is None or old_achievement <= 0:
        return target_achievement <= 100.0
    if old_achievement < 97.0:
        return target_achievement <= 99.5
    if old_achievement < 99.5:
        return target_achievement <= 100.0
    return True


def _rise_score_effective_candidate_floor(
    replacement_floor: int,
    recommendation_floor: int,
) -> int:
    if replacement_floor == 0 and recommendation_floor > 0:
        return max(0, recommendation_floor - 18)
    return recommendation_floor


def _rise_score_fit_bonus(fit_delta_bucket: int) -> float:
    return {
        0: 24.0,
        1: 12.0,
        2: 0.0,
        3: -18.0,
        4: -8.0,
    }.get(fit_delta_bucket, -8.0)


def _rise_score_expected_candidate_score(
    item: RiseScore,
    *,
    actual_gain: int,
    over_floor: int,
    replacement_floor: int,
    recommendation_floor: int,
    ability_ds: float,
    probability: float,
    fit_delta_bucket: int,
) -> float:
    under_challenge = max(0.0, ability_ds - float(item.ds) - 0.45)
    over_challenge = max(0.0, float(item.ds) - ability_ds - 0.55)
    fill_weight = 0.24 if replacement_floor == 0 and recommendation_floor > 0 else 0.85
    return (
        actual_gain * probability * fill_weight
        + over_floor * 5.5
        + _rise_score_fit_bonus(fit_delta_bucket)
        - under_challenge * 72.0
        - over_challenge * 110.0
    )


def _rise_score_candidate_ds_range(candidate_floor: int, score: Optional[int]) -> Tuple[float, float]:
    score_bonus = int(score or 0)
    lower = round(_floor_ds_for_achievement(candidate_floor, 100.5) - 0.15, 1)
    upper = round(_floor_ds_for_achievement(candidate_floor + score_bonus, 99.0) + 0.35, 1)
    return max(1.0, lower), min(15.0, max(lower, upper))


def _weighted_sample_expected_rise_scores(
    candidates: List[Tuple[float, int, int, RiseScore]],
    limit: int = 5,
) -> List[RiseScore]:
    if len(candidates) <= limit:
        selected = sorted(
            candidates,
            key=lambda candidate: (candidate[0], candidate[1], candidate[2], candidate[3].ra),
            reverse=True,
        )
    else:
        candidates = sorted(
            candidates,
            key=lambda item: (item[0], item[1], item[2], item[3].ra, item[3].song_id),
            reverse=True,
        )
        pool = candidates[:max(limit * 8, 30)]
        remaining = list(pool)
        selected: List[Tuple[float, int, int, RiseScore]] = []
        while remaining and len(selected) < limit:
            pool_size = len(pool)
            weights = []
            for item in remaining:
                rank = pool.index(item)
                quality = (pool_size - rank) / max(pool_size, 1)
                weights.append(0.28 + quality ** 1.15)
            chosen = random.choices(remaining, weights=weights, k=1)[0]
            selected.append(chosen)
            remaining.remove(chosen)
    selected.sort(key=lambda item: (item[3].song_id, item[3].level_index), reverse=True)
    return [item[3] for item in selected]


def _plate_plan_condition(plan: str) -> Condition:
    if plan in ['将', '者']:
        achievement = 100 if plan == '将' else 80
        return lambda x: x.achievements < achievement
    if plan in ['極', '极']:
        return lambda x: not x.fc
    if plan == '舞舞':
        return lambda x: x.fs not in ['fsd', 'fdx', 'fsdp', 'fdxp']
    if plan == '神':
        return lambda x: x.fc not in ['ap', 'app']
    raise ValueError


def _plate_song_key(value) -> str:
    return str(value or "").strip()


def _cn_plate_data_key(version: str) -> str | None:
    if version in version_map:
        return version_map[version][1]
    if version in plate_to_dx_version:
        return version
    return None


def _cn_plate_supported(version: str) -> bool:
    data_key = _cn_plate_data_key(version)
    return bool(data_key and data_key in getattr(mai, "total_plate_id_list", {}))


async def music_global_data(music: Music, level_index: int) -> MessageSegment:
    """
    绘制曲目游玩详情
    
    Params:
        `music`: :class:Music
        `level_index`: 难度
    Returns:
        `MessageSegment`
    """
    if opts is None or Pie is None:
        return _draw_music_global_data_fallback(music, level_index)

    stats = music.stats[level_index]
    fc_data_pair = [list(z) for z in zip([c.upper() if c else 'Not FC' for c in [''] + comboRank], stats.fc_dist)]
    acc_data_pair = [list(z) for z in zip([s.upper() for s in scoreRank], stats.dist)]

    initopts = opts.InitOpts(width='1000px', height='800px', bg_color='#fff', js_host='./')
    labelopts = opts.LabelOpts(
        position='outside',
        formatter='{a|{a}}{abg|}\n{hr|}\n {b|{b}: }{c}  {per|{d}%}  ',
        background_color='#eee',
        border_color='#aaa',
        border_width=1,
        border_radius=4,
        rich={
            'a': {'color': '#999', 'lineHeight': 22, 'align': 'center'},
            'abg': {
                'backgroundColor': '#e3e3e3',
                'width': '100%',
                'align': 'right',
                'height': 22,
                'borderRadius': [4, 4, 0, 0],
            },
            'hr': {
                'borderColor': '#aaa',
                'width': '100%',
                'borderWidth': 0.5,
                'height': 0,
            },
            'b': {'fontSize': 16, 'lineHeight': 33},
            'per': {
                'color': '#eee',
                'backgroundColor': '#334455',
                'padding': [2, 4],
                'borderRadius': 2,
            },
        },
    )
    for rich in labelopts.opts.get("rich", {}).values():
        rich["fontFamily"] = GLOBAL_STATS_FONT_FAMILY
    display_id = _display_song_id(music.id)
    title_text = " ".join(part for part in (display_id, music.title) if part)
    titleopts = opts.TitleOpts(
        title=f'{title_text} 「{diffs[level_index]}」',
        pos_left='center',
        pos_top='20',
        title_textstyle_opts=opts.TextStyleOpts(color='#2c343c', font_family=GLOBAL_STATS_FONT_FAMILY),
    )
    legendopts = opts.LegendOpts(
        pos_left=15,
        pos_top=10,
        orient='vertical',
        textstyle_opts=opts.TextStyleOpts(font_family=GLOBAL_STATS_FONT_FAMILY),
    )

    pie = Pie(initopts)
    pie.add('全连等级', fc_data_pair, radius=[0, '30%'], label_opts=labelopts)
    pie.add('达成率等级', acc_data_pair, radius=['50%', '70%'], is_clockwise=True, label_opts=labelopts)
    pie.set_global_opts(title_opts=titleopts, legend_opts=legendopts)
    pie.set_series_opts(tooltip_opts=opts.TooltipOpts(trigger='item', formatter='{a} <br/>{b}: {c} ({d}%)'))
    pie.render(str(pie_html_file))
    _patch_global_stats_html_font()
    try:
        base64 = await run_chrome_to_base64()
    except Exception:
        # Browser rendering is optional; the PIL renderer below is the supported fallback.
        base64 = ""
    if not base64:
        return _draw_music_global_data_fallback(music, level_index)

    return MessageSegment.image(base64)


def _draw_music_global_data_fallback(music: Music, level_index: int) -> MessageSegment:
    stats = music.stats[level_index]
    fc_data_pair = [("Not FC", stats.fc_dist[0])] + [
        (label.upper(), value) for label, value in zip(comboRank, stats.fc_dist[1:])
    ]
    acc_data_pair = [(label.upper(), value) for label, value in zip(scoreRank, stats.dist)]

    im = Image.new("RGBA", (1000, 800), (255, 255, 255, 255))
    dr = ImageDraw.Draw(im)
    mr = DrawText(dr, SIYUAN)
    tb = DrawText(dr, TBFONT)
    display_id = _display_song_id(music.id)
    title_text = " ".join(part for part in (display_id, music.title) if part)
    title = f"{title_text} [{diffs[level_index]}]"
    if coloumWidth(title) > 60:
        title = changeColumnWidth(title, 59) + "..."
    mr.draw(500, 42, 26, title, (44, 52, 64, 255), "mm")
    _draw_stats_pie(dr, tb, "FC 分布", fc_data_pair, (280, 320), 160, title_font=mr)
    _draw_stats_pie(dr, tb, "达成率分布", acc_data_pair, (720, 320), 160, title_font=mr)
    mr.draw(500, 772, 12, GLOBAL_STATS_FALLBACK_FOOTER, (148, 163, 184, 255), "mm")
    return MessageSegment.image(image_to_base64(im))


def _draw_stats_pie(
    dr: ImageDraw.ImageDraw,
    tb: DrawText,
    title: str,
    data_pair: list[tuple[str, int]],
    center: tuple[int, int],
    radius: int,
    title_font: Optional[DrawText] = None,
) -> None:
    colors = [
        (226, 232, 240, 255), (94, 234, 212, 255), (96, 165, 250, 255),
        (129, 140, 248, 255), (244, 114, 182, 255), (251, 146, 60, 255),
        (250, 204, 21, 255), (74, 222, 128, 255), (45, 212, 191, 255),
        (56, 189, 248, 255), (168, 85, 247, 255), (236, 72, 153, 255),
        (248, 113, 113, 255), (132, 204, 22, 255),
    ]
    cx, cy = center
    total = sum(max(0, int(value or 0)) for _, value in data_pair)
    (title_font or tb).draw(cx, cy - radius - 45, 25, title, (44, 52, 64, 255), "mm")
    box = (cx - radius, cy - radius, cx + radius, cy + radius)
    if total <= 0:
        dr.ellipse(box, fill=(241, 245, 249, 255), outline=(148, 163, 184, 255), width=2)
        tb.draw(cx, cy, 24, "No Data", (100, 116, 139, 255), "mm")
        return

    start = -90.0
    for index, (_label, value) in enumerate(data_pair):
        numeric = max(0, int(value or 0))
        if numeric <= 0:
            continue
        end = start + 360.0 * numeric / total
        dr.pieslice(box, start, end, fill=colors[index % len(colors)])
        start = end
    dr.ellipse(box, outline=(255, 255, 255, 255), width=3)
    dr.ellipse((cx - 58, cy - 58, cx + 58, cy + 58), fill=(255, 255, 255, 255), outline=(226, 232, 240, 255), width=2)
    tb.draw(cx, cy - 8, 20, str(total), (15, 23, 42, 255), "mm")
    tb.draw(cx, cy + 18, 13, "Total", (100, 116, 139, 255), "mm")

    legend_x = cx - radius - 25
    legend_y = cy + radius + 42
    line_height = 28
    columns = 2 if len(data_pair) > 7 else 1
    column_width = 170
    for index, (label, value) in enumerate(data_pair):
        numeric = max(0, int(value or 0))
        column = index % columns
        row = index // columns
        x = legend_x + column * column_width
        y = legend_y + row * line_height
        dr.rounded_rectangle((x, y - 10, x + 18, y + 8), radius=4, fill=colors[index % len(colors)])
        percent = numeric / total * 100 if total else 0
        tb.draw(x + 26, y, 13, f"{label} {numeric} ({percent:.1f}%)", (44, 52, 64, 255), "lm")


class DrawScore(ScoreBaseImage):
    
    def __init__(self, image: Image.Image = None) -> None:
        super().__init__(image)
        self._im.alpha_composite(self.aurora_bg)
        self._im.alpha_composite(self.shines_bg, (34, 0))
        self._im.alpha_composite(self.rainbow_bg, (319, self._im.size[1] - 643))
        self._im.alpha_composite(self.rainbow_bottom_bg, (100, self._im.size[1] - 343))
        for h in range((self._im.size[1] // 358) + 1):
            self._im.alpha_composite(self.pattern_bg, (0, (358 + 7) * h))

    def whilepic(self, data: List[RaMusic], y: int = 200):
        """
        循环绘制谱面
        
        Params:
            `data`: `谱面数据`
            `y`: `Y轴偏移`
        """
        dy = 65
        x = 0
        for n, v in enumerate(data):
            if n % 20 == 0:
                x = 55
                y += dy if n != 0 else 0
            else:
                x += 65
            try:
                cover = Image.open(music_picture(v.id)).resize((55, 55))
            except (ValueError, TypeError):
                cover = Image.open(coverdir / '11000.png').resize((55, 55))
            self._im.alpha_composite(cover, (x, y))
            self._im.alpha_composite(self.id_diff[int(v.lv)], (x, y + 45))
            display_id = _display_song_id(v.id)
            if coloumWidth(display_id) > 12:
                display_id = changeColumnWidth(display_id, 11) + '...'
            self._tb.draw(x + 27, y + 50, 10, display_id, self.t_color[int(v.lv)], 'mm')
    
    def whilerisepic(self, data: List[RiseScore], low_score: int, isdx: bool):
        """
        循环绘制上分推荐数据
        
        Params:
            `data`: `上分数据`
            `low_score`: `最低分`
            `isdx`: `是否DX版本`
        """
        y = 120
        for index, _d in enumerate(data):
            x = 200 if isdx else 700
            y += 140 if index != 0 else 0
            
            rate = Image.open(maimaidir / f'UI_TTR_Rank_{_d.rate}.png').resize((63, 28))
            
            self._im.alpha_composite(self._rise[_d.level_index], (x + 30, y))
            self._im.alpha_composite(Image.open(music_picture(_d.song_id)).resize((80, 80)), (x + 55, y + 40))
            self._im.alpha_composite(Image.open(maimaidir / f'{_d.type.upper()}.png').resize((60, 22)), (x + 240, y + 114))
            if _d.oldrate:
                oldrate = Image.open(maimaidir / f'UI_TTR_Rank_{_d.oldrate}.png').resize((63, 28))
                self._im.alpha_composite(oldrate, (x + 145, y + 82))
            self._im.alpha_composite(rate, (x + 305, y + 82))
            
            title = _d.title
            if coloumWidth(title) > 26:
                title = changeColumnWidth(title, 25) + '...'
            self._sy.draw(x + 142, y + 44, 17, title, self.t_color[_d.level_index], 'lm')
            self._tb.draw(x + 145, y + 124, 18, f'ID: {_display_song_id(_d.song_id)}', self.id_color[_d.level_index], 'lm')
            self._tb.draw(x + 210, y + 71, 25, f'{_d.oldachievements:.4f}%', self.t_color[_d.level_index], anchor='mm')
            self._tb.draw(x + 245, y + 96, 17, f'Ra: {_d.oldra}', self.t_color[_d.level_index], anchor='mm')
            self._tb.draw(x + 370, y + 71, 25, f'{_d.achievements:.4f}%', self.t_color[_d.level_index], anchor='mm')
            self._tb.draw(x + 415, y + 96, 17, f'Ra: {_d.ra}', self.t_color[_d.level_index], anchor='mm')
            self._tb.draw(x + 315, y + 124, 18, f'ds:{_d.ds}', self.id_color[_d.level_index], anchor='lm')
            if _d.oldra > low_score:
                new_ra = _d.ra - _d.oldra
            else:
                new_ra = _d.ra - low_score
            self._tb.draw(x + 390, y + 124, 18, f'Ra +{new_ra}', self.id_color[_d.level_index], 'lm')
         
    def draw_rise(self, sd: List[RiseScore], sd_score: int, dx: List[RiseScore], dx_score: int) -> Image.Image:
        """
        绘制上分数据表
        
        Params:
            `sd`: `旧版本谱面`
            `sd_score`: `旧版本最低分`
            `sd`: `新版本谱面`
            `dx_score`: `新版本最低分`
        Returns:
            `Image.Image`
        """
        title_bg = self.title_bg.copy().resize((273, 80))
        self._im.alpha_composite(title_bg, (314, 30))
        self._sy.draw(450, 68, 18, '旧版本谱面推荐', self.text_color, 'mm')
        self.whilerisepic(sd, sd_score, True)
        self._im.alpha_composite(title_bg, (814, 30))
        self._sy.draw(950, 68, 18, '新版本谱面推荐', self.text_color, 'mm')
        self.whilerisepic(dx, dx_score, False)
        
        height = self._im.size[1]
        self._im.alpha_composite(self.design_bg.resize((800, 72)), (300, height - 110))
        self._sy.draw(700, height - 76, 18, CREDIT_TEXT, self.text_color, 'mm')
        return self._im

    def draw_plan(
        self,
        completed: Union[List[PlayInfoDefault], List[PlayInfoDev]],
        completed_y: int,
        unfinished: Union[List[PlayInfoDefault], List[PlayInfoDev]],
        unfinished_y: int,
        notstarted: List[RaMusic],
        plan: str,
        completed_len: int
    ) -> Image.Image:
        """
        绘制进度表
        
        Params:
            `completed`: `已完成谱面`
            `completed_y`: `已完成谱面高度`
            `unfinished`: `未完成谱面`
            `unfinished_y`: `未完成谱面高度`
            `notstarted`: `未游玩谱面`
            `plan`: `目标`
            `completed_len`: `已完成谱面数量`
        Returns:
            `Image.Image`
        """
        max = len(completed + unfinished + notstarted)

        self._im.alpha_composite(self.title_lengthen_bg, (475, 30))
        self._im.alpha_composite(self.title_lengthen_bg, (475, 30 + completed_y))
        self._im.alpha_composite(self.title_lengthen_bg, (475, 30 + completed_y + unfinished_y))
        
        self._sy.draw(700, 77, 22, f'已完成谱面「{len(completed)}」个', self.text_color, 'mm')
        self._sy.draw(700, 77 + completed_y, 22, f'未完成谱面「{len(unfinished)}」个', self.text_color, 'mm')
        self._sy.draw(700, 77 + completed_y + unfinished_y, 22, f'未游玩谱面「{len(notstarted)}」个', self.text_color, 'mm')
        
        self.whiledraw(completed[:completed_len], True, 140)
        self.whiledraw(unfinished[:30], True, 140 + completed_y)
        self.whilepic(notstarted[:100], 140 + completed_y + unfinished_y)

        self._im.alpha_composite(self.design_bg, (200, self._im.size[1] - 113))
        pagemsg = f'共计「{max}」个谱面，剩余「{len(unfinished + notstarted)}」个谱面未完成「{plan.upper()}」'
        self._sy.draw(700, self._im.size[1] - 70, 25, pagemsg, self.text_color, 'mm')
        return self._im

    def draw_category(
        self, 
        category: str, 
        data: Union[List[PlayInfoDefault], List[PlayInfoDev], List[RaMusic]],
        page: int = 1, 
        end_page: int = 1
    ) -> Image.Image:
        """
        绘制指定进度表
        
        Params:
            `category`: `类别`
            `data`: `数据`
            `page`: `页数`
            `end_page`: `总页数`
        Returns:
            `Image.Image`
        """
        lendata = len(data)
        newdata = data[(page - 1) * 80: page * 80]
        self._im.alpha_composite(self.title_lengthen_bg, (475, 30))
        if category == 'completed' or category == 'unfinished':
            txt = '已完成' if category == 'completed' else '未完成'
            self._sy.draw(700, 77, 28, f'{txt}谱面', self.text_color, 'mm')
            self.whiledraw(newdata, True, 140)
            self._im.alpha_composite(self.design_bg, (200, self._im.size[1] - 113))
            
            pagemsg = f'{txt}谱面共计「{lendata}」个，'
            pagemsg += f'展示第「{(page - 1) * 80 + 1}-{80 * (page - 1) + len(newdata)}」个，'
            pagemsg += f'当前第「{page} / {end_page}」页'
            self._sy.draw(700, self._im.size[1] - 70, 25, pagemsg, self.text_color, 'mm')
        else:
            self._sy.draw(700, 77, 28, '未游玩谱面', self.text_color, 'mm')
            self.whilepic(data)
            self._im.alpha_composite(self.design_bg, (200, self._im.size[1] - 113))
            self._sy.draw(700, self._im.size[1] - 70, 25, f'未游玩谱面共计「{len(data)}」个', self.text_color, 'mm')
        return self._im
    
    def draw_scorelist(
        self, 
        rating: Union[str, float], 
        data: Union[List[PlayInfoDefault], List[PlayInfoDev]], 
        page: int = 1, 
        end_page: int = 1
    ) -> Image.Image:
        """
        绘制分数列表
        
        Params:
            `rating`: `定数`
            `data`: `数据`
            `page`: `页数`
            `end_page`: `总页数`
        Returns:
            `Image.Image`
        """
        lendata = len(data)
        newdata = data[(page - 1) * 80: page * 80]
        r = len(newdata) // 20 + (0 if len(newdata) % 20 == 0 else 1)
        for n in range(r):
            y = (109 * 4 + 140) * n
            self._im.alpha_composite(self.title_lengthen_bg, (475, 30 + y))
            start = (20 * n + 1) + 80 * (page - 1)
            self._sy.draw(700, 77 + y, 28, f'No.{start}- No.{start + len(newdata[n * 20: (n + 1) * 20]) - 1}', self.text_color, 'mm')
            self.whiledraw(newdata[n * 20: (n + 1) * 20], True, 140 + y)
        self._im.alpha_composite(self.design_bg, (200, self._im.size[1] - 113))
        
        pagemsg = f'「{rating}」共计「{lendata}」个成绩，'
        pagemsg += f'展示第「{(page - 1) * 80 + 1}-{80 * (page - 1) + len(newdata)}」个，'
        pagemsg += f'当前第「{page} / {end_page}」页'
        self._sy.draw(700, self._im.size[1] - 70, 25, pagemsg, self.text_color, 'mm')
        return self._im


def get_rise_score_list(
    old_records: DefaultDict[int, Dict[int, float]],
    type: str, 
    info: List[ChartInfo], 
    level: Optional[str] = None, 
    score: Optional[int] = None,
    fallback_info: Optional[List[ChartInfo]] = None,
    all_records: Optional[List[PlayInfoDev]] = None,
    algorithm: str = RISE_SCORE_ALGORITHM_DEFAULT,
) -> Tuple[List[RiseScore], int]:
    """
    获取上分推荐曲目
    
    Params:
        `type`: 版本
        `info`: 游玩成绩列表
        `level`: 等级
        `score`: 分数
        `fallback_info`: 当前分组为空或未满时用于估算候选难度的另一组 B50 成绩
        `all_records`: 全量成绩，用于 expected 算法估算达成概率
        `algorithm`: legacy 使用旧的分桶随机逻辑；expected 使用期望收益排序
    Returns:
        `Tuple[List[RiseScore], int]`
    """
    if algorithm not in RISE_SCORE_ALGORITHMS:
        algorithm = RISE_SCORE_ALGORITHM_DEFAULT
    ignore: set[int] = set()
    for m in info:
        if m.achievements >= 100.5:
            ignore.update(_equivalent_song_ids(m.song_id))
    if level not in (None, ""):
        level = normalize_level_value(level)
    mai._ensure_loaded()
    ra = _rise_score_replacement_floor(type, info)
    candidate_floor = _rise_score_candidate_floor(type, info, fallback_info)
    recommendation_floor = max(ra, candidate_floor)
    effective_candidate_floor = _rise_score_effective_candidate_floor(ra, recommendation_floor)
    ds = _rise_score_candidate_ds_range(candidate_floor, score)
    version = _rise_score_candidate_versions(type, info)
    musiclist = mai.total_list.filter(level=level, ds=ds, version=version)
    legacy_candidates: List[Tuple[int, RiseScore]] = []
    expected_candidates: List[Tuple[float, int, int, RiseScore]] = []
    profile_records = _rise_score_profile_records(all_records, info, fallback_info)
    ability_by_target = {
        target: _rise_score_target_ability_ds(profile_records, target, recommendation_floor)
        for target in RISE_SCORE_TARGETS
    }
    for _m in musiclist:
        try:
            song_id = int(_m.id)
        except (TypeError, ValueError):
            continue
        if _equivalent_song_ids(song_id) & ignore:
            continue
        if song_id >= 100000:
            continue
        for index in _m.diff:
            fit_delta = _rise_score_fit_delta(_m, index)
            fit_delta_bucket = _rise_score_fit_delta_bucket(fit_delta)
            if fit_delta_bucket is None:
                fit_delta_bucket = 4
            best_expected: Optional[Tuple[float, int, int, RiseScore]] = None
            for r in RISE_SCORE_TARGETS:
                basera, rate = computeRa(_m.ds[index], r, israte=True)
                candidate_gate_floor = (
                    recommendation_floor
                    if algorithm == RISE_SCORE_ALGORITHM_LEGACY
                    else effective_candidate_floor
                )
                if basera <= candidate_gate_floor:
                    continue
                if score and basera - int(score) < recommendation_floor:
                    continue
                old_achievement = _old_record_achievement(old_records, song_id, index)
                if algorithm != RISE_SCORE_ALGORITHM_LEGACY:
                    if not _rise_score_target_allowed_by_history(r, old_achievement):
                        continue
                    ability_ds = ability_by_target.get(r, _floor_ds_for_achievement(recommendation_floor, r))
                    if float(_m.ds[index]) > ability_ds + _rise_score_target_margin_limit(r, old_achievement):
                        continue
                if old_achievement is not None:
                    oldra, oldrate = computeRa(_m.ds[index], old_achievement, israte=True)
                    if oldra >= basera:
                        continue
                    ss = RiseScore(
                        song_id=song_id,
                        title=_m.title,
                        type=_m.type,
                        level_index=index,
                        ds=_m.ds[index],
                        ra=basera,
                        rate=rate,
                        achievements=r,
                        oldra=oldra,
                        oldrate=oldrate,
                        oldachievements=old_achievement
                    )
                else:
                    oldra = 0
                    old_achievement = 0
                    ss = RiseScore(
                        song_id=song_id,
                        title=_m.title,
                        type=_m.type,
                        level_index=index,
                        ds=_m.ds[index],
                        ra=basera,
                        rate=rate,
                        achievements=r
                    )
                legacy_candidates.append((fit_delta_bucket, ss))
                if algorithm == RISE_SCORE_ALGORITHM_LEGACY:
                    break
                old_floor = max(oldra, ra)
                actual_gain = basera - old_floor
                over_floor = basera - effective_candidate_floor
                if actual_gain <= 0 or over_floor <= 0:
                    continue
                probability = _rise_score_target_probability(
                    float(_m.ds[index]),
                    r,
                    ability_ds,
                    old_achievement,
                )
                expected_score = _rise_score_expected_candidate_score(
                    ss,
                    actual_gain=actual_gain,
                    over_floor=over_floor,
                    replacement_floor=ra,
                    recommendation_floor=recommendation_floor,
                    ability_ds=ability_ds,
                    probability=probability,
                    fit_delta_bucket=fit_delta_bucket,
                )
                expected_item = (expected_score, actual_gain, -fit_delta_bucket, ss)
                if best_expected is None or expected_item[:3] > best_expected[:3]:
                    best_expected = expected_item
            if best_expected is not None:
                expected_candidates.append(best_expected)
    if algorithm == RISE_SCORE_ALGORITHM_LEGACY:
        return _select_rise_score_candidates(legacy_candidates), ra
    return _weighted_sample_expected_rise_scores(expected_candidates), ra


async def rise_score_data(
    qqid: int, 
    username: Optional[str] = None, 
    level: Optional[str] = None, 
    score: Optional[int] = None,
    algorithm: str = RISE_SCORE_ALGORITHM_DEFAULT,
) -> Union[MessageSegment, str]:
    """
    上分数据
    
    Params:
        `qqid`: 用户QQ
        `username`: 查分器用户名
        `level`: 定数
        `score`: 分数
        `algorithm`: legacy 或 expected
    Returns:
        `Union[Image.Image, str]`
    """
    try:
        if level not in (None, ""):
            level = normalize_level_value(level)
        user = await maiApi.query_user_b50(
            qqid=qqid,
            username=username,
            include_chart_metadata=False,
        )
        records = await maiApi.query_user_plate(qqid=qqid, username=username)
        old_records: DefaultDict[int, Dict[int, float]] = defaultdict(dict)
        for m in records:
            old_records[m.song_id][m.level_index] = m.achievements
        
        sd_info = user.charts.sd or []
        dx_info = user.charts.dx or []
        sd, sd_low_score = get_rise_score_list(
            old_records,
            'SD',
            sd_info,
            level,
            score,
            fallback_info=dx_info,
            all_records=records,
            algorithm=algorithm,
        )
        dx, dx_low_score = get_rise_score_list(
            old_records,
            'DX',
            dx_info,
            level,
            score,
            fallback_info=sd_info,
            all_records=records,
            algorithm=algorithm,
        )
        
        if not sd and not dx:
            return '没有推荐的铺面'
        
        lensd, lendx = len(sd), len(dx)
        
        h = max(lensd, lendx)
        height = h * 140 + 110 + 150
        image = tricolor_gradient(1400, height)
        
        ds = DrawScore(image)
        im = ds.draw_rise(sd, sd_low_score, dx, dx_low_score).crop((200, 0, 1200, height))
        
        msg = MessageSegment.image(image_to_base64(im))
    except (UserNotFoundError, UserNotExistsError, UserDisabledQueryError) as e:
        msg = str(e)
    except Exception as e:
        log.error(traceback.format_exc())
        msg = f'未知错误：{type(e)}\n请联系Bot管理员'
        
    return msg


def plate_message(
    result: str, 
    plan: str, 
    music_list: List[PlayInfoDefault], 
    played: List[Tuple[object, int]]
) -> Union[MessageSegment, str]:
    """
    Params:
        `result`: 结果
        `plan`: 目标
        `music_list`: 谱面列表
        `played`: 已游玩谱面
    Returns:
        `Union[MessageSegment, str]`
    """
    played_keys = {(_plate_song_key(song_id), level_index) for song_id, level_index in played}
    for n, m in enumerate(music_list):
        self_record = ''
        if (_plate_song_key(m.song_id), m.level_index) in played_keys:
            if plan in ['将', '者']:
                self_record = f'{m.achievements}%'
            if plan in ['極', '极', '神']:
                self_record = m.fc
            if plan in '舞舞':
                self_record = m.fs
        result += f'No.{n + 1:02d} {f"「{_display_song_id(m.song_id)}」":>7} {f"「{diffs[m.level_index]}」":>11} 「{m.ds}」 {m.title}  {self_record}\n'
    if len(music_list) > 10:
        result = MessageSegment.image(image_to_base64(text_to_image(result.strip())))
    return result


def _plate_progress_result(
    username: Optional[str],
    version: str,
    plan: str,
    unfinished_model_list: Filter,
    played: List[Tuple[object, int]],
    include_remaster: bool = False,
) -> Union[MessageSegment, str]:
    basic, advanced, expert, master, re_master = unfinished_model_list

    ramain = basic + advanced + expert + master + re_master
    ramain.sort(key=lambda x: x.ds, reverse=True)
    difficult = [_m for _m in ramain if _m.ds > 13.6]

    appellation = username if username else '您'
    result = dedent(f'''\
        {appellation}的「{version}{plan}」剩余进度如下：
        Basic剩余「{len(basic)}」首
        Advanced剩余「{len(advanced)}」首
        Expert剩余「{len(expert)}」首
        Master剩余「{len(master)}」首
    ''')
    if include_remaster:
        result += f'Re:Master剩余「{len(re_master)}」首\n'

    if len(difficult) > 0:
        if len(difficult) < 60:
            result += '剩余定数大于13.6的曲目：\n'
            result = plate_message(result, plan, difficult, played)
        else:
            result += f'还有{len(difficult)}首大于13.6定数的曲目，加油推分捏！\n'
    elif len(ramain) > 0:
        if len(ramain) < 60:
            result += '剩余曲目：\n'
            result = plate_message(result, plan, ramain, played)
        else:
            result += '已经没有定数大于13.6的曲目了，加油清谱捏！\n'
    else:
        result = f'已经没有剩余的的曲目了，恭喜{appellation}完成「{version}{plan}」！'
    return result


def _plate_play_info(song: dict, level_index: int, record: Optional[PlayInfoDefault] = None) -> PlayInfoDefault:
    render_id = str(song.get("render_id") or song.get("song_id") or song.get("title") or "")
    level_values = list(song.get("level_values") or [])
    ds_values = list(song.get("ds_values") or [])

    if record is not None:
        info = record.model_copy()
    else:
        numeric_id = _positive_song_id(render_id) or 1
        info = PlayInfoDefault(
            id=numeric_id,
            achievements=0,
            level='',
            level_index=level_index,
            title=str(song.get("title") or ""),
            type=str(song.get("type") or "DX"),
        )
    info.song_id = render_id
    info.level_index = level_index
    info.title = str(song.get("title") or info.title)
    info.type = str(song.get("type") or info.type)
    if level_index < len(level_values):
        info.level = str(level_values[level_index])
    if level_index < len(ds_values):
        try:
            info.ds = float(ds_values[level_index] or 0)
        except (TypeError, ValueError):
            info.ds = 0
    return info


async def _jp_player_plate_data(
    qqid: int,
    username: Optional[str],
    version: str,
    plan: str,
    records: Optional[List[PlayInfoDefault]] = None,
) -> Union[MessageSegment, str]:
    _ = qqid, username, version, plan, records
    return "当前分支不支持日服/dxdata 曲目数据。"


async def _custom_player_plate_data(
    qqid: int,
    username: Optional[str],
    version: str,
    plan: str,
    records: Optional[List[PlayInfoDefault]] = None,
) -> Union[MessageSegment, str]:
    from maimai_mcp.search import normalize_text
    from ..shim.mai_music import get_custom_plate_songs

    custom_songs = get_custom_plate_songs(version)
    if not custom_songs:
        return f'自定义牌子「{version}」未找到对应歌曲'

    custom_by_song_id: dict[int, dict] = {}
    custom_by_title_type: dict[tuple[str, str], dict] = {}
    for song in custom_songs:
        song_id = _positive_song_id(song.get("song_id"))
        if song_id:
            custom_by_song_id[song_id] = song
        title_key = normalize_text(song.get("title", ""))
        song_type = str(song.get("type", "")).upper()
        if title_key and song_type:
            custom_by_title_type[(title_key, song_type)] = song

    try:
        records = [record.model_copy() for record in records] if records is not None else await maiApi.query_user_plate(qqid=qqid, username=username)
    except (UserNotFoundError, UserNotExistsError, UserDisabledQueryError) as e:
        return str(e)

    callable_ = _plate_plan_condition(plan)
    unfinished_model_list: Filter = ([], [], [], [], [])
    unfinished: List[Tuple[object, int]] = []
    played: List[Tuple[object, int]] = []
    record_by_key: dict[tuple[str, int], PlayInfoDefault] = {}

    for record in records:
        record_song_id = _positive_song_id(record.song_id)
        matched = custom_by_song_id.get(record_song_id) if record_song_id else None
        if not matched:
            matched = custom_by_title_type.get((normalize_text(record.title), str(record.type).upper()))
        if not matched or record.level_index >= 4:
            continue

        info = _plate_play_info(matched, record.level_index, record)
        key = (_plate_song_key(info.song_id), info.level_index)
        if callable_(info):
            unfinished.append(key)
        played.append(key)
        record_by_key[key] = info

    for song in custom_songs:
        render_id = _plate_song_key(song.get("render_id") or song.get("song_id") or song.get("title"))
        level_values = list(song.get("level_values") or [])
        ds_values = list(song.get("ds_values") or [])
        for level_index in range(min(4, len(level_values), len(ds_values))):
            if not level_values[level_index]:
                continue
            key = (render_id, level_index)
            if key not in played or key in unfinished:
                info = record_by_key.get(key) or _plate_play_info(song, level_index)
                unfinished_model_list[level_index].append(info)

    return _plate_progress_result(username, version, plan, unfinished_model_list, played)


async def player_plate_data(
    qqid: int, 
    username: str, 
    version: str, 
    plan: str,
    server: str = "cn",
    records: Optional[List[PlayInfoDefault]] = None,
) -> Union[MessageSegment, str]:
    """
    查看牌子进度
    
    Params:
        `qqid`: 用户QQ
        `username`: 查分器用户名
        `version`: 版本
        `plan`: 目标
    Returns:
        `Union[MessageSegment, str]`
    """
    if version in platecn:
        version = platecn[version]
    mai._ensure_loaded()
    from ..shim.mai_music import custom_plate_exists

    if server == "jp":
        return "当前分支不支持日服/dxdata 曲目数据。"
    if server == "custom":
        return await _custom_player_plate_data(qqid, username, version, plan, records=records)
    if _cn_plate_supported(version):
        pass
    elif custom_plate_exists(version):
        return await _custom_player_plate_data(qqid, username, version, plan, records=records)

    ver, _ver = version_map.get(version, ([plate_to_dx_version.get(version)], version))
    
    try:
        verlist = [record.model_copy() for record in records] if records is not None else await maiApi.query_user_plate(qqid=qqid, username=username, version=ver)
    except (UserNotFoundError, UserNotExistsError, UserDisabledQueryError) as e:
        return str(e)
    
    callable_ = _plate_plan_condition(plan)
    
    unfinished_model_list: Filter = ([], [], [], [], [])
    unfinished: List[Tuple[int, int]] = []
    played: List[Tuple[int, int]] = []
    remaster: List[int] = []
    
    # 已游玩未完成曲目
    plate_id_list = mai.total_plate_id_list[_ver]
    if version in ['舞', '霸']:
        remaster = mai.total_plate_id_list['舞ReMASTER']
        for music in verlist:
            if music.song_id not in plate_id_list:
                continue
            if music.level_index == 4 and music.song_id not in remaster:
                continue
            if callable_(music):
                unfinished.append((music.song_id, music.level_index))
            played.append((music.song_id, music.level_index))
    else:
        for music in verlist:
            if music.song_id not in plate_id_list:
                continue
            if callable_(music):
                unfinished.append((music.song_id, music.level_index))
            played.append((music.song_id, music.level_index))
    
    # 未游玩未完成曲目
    for music in mai.total_list:
        try:
            music_song_id = int(music.id)
        except (TypeError, ValueError):
            continue
        if music_song_id not in plate_id_list:
            continue
        info = PlayInfoDefault(
            achievements=0,
            level='',
            level_index=0,
            title=music.title,
            type=music.type,
            id=music_song_id
        )
        range_ = range(5 if version in ['舞', '霸'] and music_song_id in remaster else 4)
        for level_index in range_:
            if (m := (info.song_id, level_index)) not in played or m in unfinished:
                _info = info.model_copy()
                _info.level = music.level[level_index]
                _info.ds = music.ds[level_index]
                _info.level_index = level_index
                unfinished_model_list[level_index].append(_info)

    return _plate_progress_result(username, version, plan, unfinished_model_list, played, version in ['舞', '霸'])


def _fill_default_record_from_music(record: PlayInfoDefault) -> str:
    """Refresh local ds/ra metadata when the render song lookup can resolve it.

    The Diving-Fish records already carry level/ds in most cases, so a lookup
    miss must not drop or crash the whole player image.
    """
    mai._ensure_loaded()
    music = mai.total_list.by_id(record.song_id)
    if music and record.level_index < len(music.ds):
        record.ds = music.ds[record.level_index]
        record.level = music.level[record.level_index]
    if record.ds:
        record.ra, record.rate = computeRa(record.ds, record.achievements, israte=True)
    return str(music.id if music else record.song_id)


async def level_process_data(
    qqid: int, 
    username: Optional[str], 
    level: str, 
    plan: str, 
    category: str = 'default', 
    page: int = 1,
    server: str = 'cn'
) -> Union[MessageSegment, str]:
    """
    查看谱面等级进度

    Params:
        `qqid`: 用户QQ
        `username`: 查分器用户名
        `level`: 定数
        `plan`: 评价等级
    Returns:
        `Union[MessageSegment, str]`
    """
    try:
        level = normalize_level_value(level)
        mai._ensure_loaded()
        if maiApi.token:
            devobj = await maiApi.query_user_get_dev(qqid=qqid, username=username)
            obj = devobj.records
        else:
            obj = await maiApi.query_user_plate(qqid=qqid, username=username)
        music = mai.by_plan(level, server=server)

        planlist = [0, 0, 0]
        plannum = 0
        if plan.lower() in scoreRank:
            plannum = 0
            planlist[0] = achievementList[scoreRank.index(plan.lower()) - 1]
        elif plan.lower() in comboRank:
            plannum = 1
            planlist[1] = comboRank.index(plan.lower())
        elif plan.lower() in syncRank:
            plannum = 2
            planlist[2] = syncRank.index(plan.lower())
        else:
            raise
        
        plan_value = planlist[plannum]
        
        def is_completed(plannum: int, _d: Union[PlayInfoDefault, PlayInfoDev]) -> bool:
            if plannum == 0:
                return _d.achievements >= plan_value
            elif plannum == 1:
                return bool(_d.fc and combo_rank.index(_d.fc) >= plan_value)
            elif plannum == 2:
                return bool(_d.fs and (
                    sync_rank.index(_d.fs) >= plan_value 
                    if _d.fs in sync_rank else sync_rank_p.index(_d.fs) >= plan_value
                ))
            return False
        
        for _d in obj:
            song_id = str(_d.song_id)
            if isinstance(_d, PlayInfoDefault):
                song_id = _fill_default_record_from_music(_d)
            if song_id in music and _d.level == level:
                if isinstance(music[song_id], Dict):
                    music[song_id][_d.level_index] = PlanInfo()
                    _p = music[song_id][_d.level_index]
                else:
                    music[song_id] = PlanInfo()
                    _p = music[song_id]
                
                if is_completed(plannum, _d):
                    _p.completed = _d
                else:
                    _p.unfinished = _d

        notplayed: List[RaMusic] = []
        completed: Union[List[PlayInfoDefault], List[PlayInfoDev]] = []
        unfinished: Union[List[PlayInfoDefault], List[PlayInfoDev]] = []
        for m in music:
            play = music[m]
            if isinstance(play, Dict):
                for index, p in play.items():
                    if isinstance(p, RaMusic):
                        notplayed.append(p)
                    elif p.completed:
                        completed.append(p.completed)
                    elif p.unfinished:
                        unfinished.append(p.unfinished)
            elif isinstance(play, PlanInfo):
                if play.completed:
                    completed.append(play.completed)
                if play.unfinished:
                    unfinished.append(play.unfinished)
            else:
                notplayed.append(play)
        completed.sort(key=lambda x: x.achievements if plannum == 0 else x.fc if plannum == 1 else x.fs, reverse=True)
        unfinished.sort(key=lambda x: x.achievements if plannum == 0 else x.fc if plannum == 1 else x.fs, reverse=True)
        notplayed.sort(key=lambda x: x.ds, reverse=True)

        if category == 'default':
            completed_len = 60 if len(unfinished) == 0 and len(notplayed) == 0 else 30
            clen = len(completed[:completed_len])
            completed_y = (clen // 5 + (0 if clen % 5 == 0 else 1)) * 109 + 140
            ulen = len(unfinished[:30])
            unfinished_y = (ulen // 5 + (0 if ulen % 5 == 0 else 1)) * 109 + 140
            nlen = len(notplayed[:100])
            notstarted_y = (nlen // 20 + (0 if nlen % 20 == 0 else 1)) * 65 + 140
            image = tricolor_gradient(1400, 150 + completed_y + unfinished_y + notstarted_y)
            dp = DrawScore(image)
            im = dp.draw_plan(completed, completed_y, unfinished, unfinished_y, notplayed, plan, completed_len)
        elif category == 'completed' or category == 'unfinished':
            data = completed if category == 'completed' else unfinished
            lendata = len(data)
            end_page_num = lendata // 80 + 1
            if page > end_page_num:
                return f'超出页数，您的成绩共计「{end_page_num}」页，请重新输入'
            topage = len(data[(page - 1) * 80: page * 80])
            plc = (topage // 5 + (0 if topage % 5 == 0 else 1)) * 109
            image = tricolor_gradient(1400, 240 + plc + 120)
            dp = DrawScore(image)
            im = dp.draw_category(category, data, page, end_page_num)
        else:
            lennotstarted = len(notplayed)
            pln = (lennotstarted // 20 + (0 if lennotstarted % 20 == 0 else 1)) * 65
            image = tricolor_gradient(1400, 240 + pln + 120)
            dp = DrawScore(image)
            im = dp.draw_category(category, notplayed)
        
        msg = MessageSegment.image(image_to_base64(im))
    except (UserNotFoundError, UserNotExistsError, UserDisabledQueryError) as e:
        msg = str(e)
    except Exception as e:
        log.error(traceback.format_exc())
        msg = f'未知错误：{type(e)}\n请联系Bot管理员'
    return msg


async def level_achievement_list_data(
    qqid: int, 
    username: Optional[str], 
    rating: Union[str, float], 
    page: int = 1
) -> Union[MessageSegment, str]:
    """
    查看分数列表

    Params:
        `qqid` : 用户QQ
        `username` : 查分器用户名
        `rating` : 定数
        `page` : 页数
        `nickname` : 用户昵称
    Returns:
        `Union[MessageSegment, str]
    """
    try:
        data: Union[List[PlayInfoDefault], List[PlayInfoDev]] = []
        if maiApi.token:
            obj = await maiApi.query_user_get_dev(qqid=qqid, username=username)
            data = obj.records
        else:
            obj = await maiApi.query_user_plate(qqid=qqid, username=username)
            for _d in obj:
                _fill_default_record_from_music(_d)
            data = obj

        if isinstance(rating, str):
            newdata = sorted(list(filter(lambda x: x.level == rating, data)), key=lambda z: z.achievements, reverse=True)
        else:
            newdata = sorted(list(filter(lambda x: x.ds == rating, data)), key=lambda z: z.achievements, reverse=True)
        
        lendata = len(newdata)
        end_page_num = lendata // 80 + 1
        if page > end_page_num:
            return f'超出页数，您的成绩共计「{end_page_num}」页，请重新输入'
        
        topage = len(newdata[(page - 1) * 80: page * 80])
        line = topage // 5 + (0 if topage % 5 == 0 else 1)
        if page < end_page_num:
            plc = line * 109 + 140 * 4
        elif topage <= 20:
            plc = 4 * 109 + 140
        elif topage <= 40:
            plc = line * 109 + 140 * 2
        elif topage <= 60:
            plc = line * 109 + 140 * 3
        else:
            plc = line * 109 + 140 * 4
        
        image = tricolor_gradient(1400, 150 + plc)

        sc = DrawScore(image)
        im = sc.draw_scorelist(rating, newdata, page, end_page_num)
        msg = MessageSegment.image(image_to_base64(im))
    except (UserNotFoundError, UserNotExistsError, UserDisabledQueryError) as e:
        msg = str(e)
    except Exception as e:
        log.error(traceback.format_exc())
        msg = f'未知错误：{type(e)}\n请联系Bot管理员'
    return msg


async def rating_ranking_data(name: str, page: int) -> Union[MessageSegment, str]:
    """
    查看查分器排行榜
    
    Params:
        `name`: 指定用户名
        `page`: 页数
    Returns:
        `Union[MessageSegment, str]`
    """
    try:
        rank_data = await maiApi.rating_ranking()

        _time = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime())
        if name != '':
            if name in [r.username.lower() for r in rank_data]:
                rank_index = [r.username.lower() for r in rank_data].index(name) + 1
                nickname = rank_data[rank_index - 1].username
                data = f'截止至 {_time}\n玩家 {nickname} 在查分器已注册用户ra排行第{rank_index}'
            else:
                data = '未找到该玩家'
        else:
            user_num = len(rank_data)
            msg = f'截止至 {_time}，查分器已注册用户ra排行：\n'
            if page * 50 > user_num:
                page = user_num // 50 + 1
            end = page * 50 if page * 50 < user_num else user_num
            for i, ranker in enumerate(rank_data[(page - 1) * 50:end]):
                msg += f'No.{i + 1 + (page - 1) * 50:02d}.「{ranker.ra}」 {ranker.username} \n'
            msg += f'第「{page}」页，共「{user_num // 50 + 1}」页'
            data = MessageSegment.image(image_to_base64(text_to_image(msg.strip())))
    except Exception as e:
        log.error(traceback.format_exc())
        data = f'未知错误：{type(e)}\n请联系Bot管理员'
    return data
