import copy
from typing import Any, Optional

from .image import rounded_corners, tricolor_gradient
from .maimai_best_50 import *
from .maimaidx_model import Music
from . import mai, maiApi, normalize_level_value


GENRE_DISPLAY_NAMES = {
    'anime': 'POPSアニメ',
    'niconico': 'niconicoボーカロイド',
    'touhou': '東方Project',
    'game': 'ゲームバラエティ',
    'ongeki': 'オンゲキCHUNITHM',
    'maimai': 'maimai',
    '宴会场': '宴会場',
}


def _score_music_search_ids(music_id: str) -> list[str]:
    ids = [str(music_id)]
    try:
        numeric_id = int(str(music_id))
    except (TypeError, ValueError):
        return ids
    if numeric_id > 10000:
        ids.append(str(numeric_id - 10000))
    elif 1000 < numeric_id < 10000:
        ids.append(str(numeric_id + 10000))
    return list(dict.fromkeys(ids))


def _score_music_by_id(music_id: str) -> Optional[Music]:
    mai._ensure_loaded()
    music = mai.total_list.by_id(music_id)
    if music is not None:
        return music

    try:
        from maimai_mcp.search import search_songs
        from maimaidx_render_mcp.shim.mai_music import music_from_search_song
    except Exception:
        return None

    lookup_ids = _score_music_search_ids(music_id)
    for lookup_id in lookup_ids:
        try:
            if str(lookup_id).isdigit():
                result = search_songs(id=lookup_id, limit=5)
            else:
                result = search_songs(query=lookup_id, limit=5)
        except Exception:
            continue
        for song in result.get("songs") or []:
            if not isinstance(song, dict):
                continue
            music = music_from_search_song(song)
            if music is not None:
                return music
    return None


def _score_lookup_ids(music_id: str, music: Music) -> list[str]:
    ids: list[str] = []
    for value in (music_id, music.id):
        if value in (None, ""):
            continue
        ids.extend(_score_music_search_ids(str(value)))
    return list(dict.fromkeys(ids))


def _normalize_match_text(value: object) -> str:
    try:
        from maimai_mcp.search import normalize_text
    except Exception:
        return str(value or "").strip().casefold()
    return normalize_text(str(value or ""))


def _score_chart_type(value: object) -> str | None:
    text = str(value or "").strip().upper()
    if text in {"SD", "ST", "STD", "STANDARD"}:
        return "standard"
    if text == "DX":
        return "dx"
    if text in {"UTAGE", "宴", "宴会场"}:
        return "utage"
    return None


def _record_matches_music(record: PlayInfoDefault | PlayInfoDev, music: Music, lookup_ids: list[str]) -> bool:
    record_type = _score_chart_type(getattr(record, "type", ""))
    music_type = _score_chart_type(music.type)
    if record_type and music_type and record_type != music_type:
        return False

    record_id = getattr(record, "song_id", None)
    if record_id not in (None, ""):
        if str(record_id) in lookup_ids:
            return True
        for alt_id in _score_music_search_ids(str(record_id)):
            if alt_id in lookup_ids:
                return True

    record_title = _normalize_match_text(getattr(record, "title", ""))
    music_title = _normalize_match_text(music.title)
    if not record_title or record_title != music_title:
        return False
    return not music_type or not record_type or record_type == music_type


def _positive_song_id(value) -> Optional[int]:
    try:
        song_id = int(value)
    except (TypeError, ValueError):
        return None
    return song_id if song_id > 0 else None


def _plate_sort_ds(music: Music) -> float:
    if len(music.ds) > 3:
        return music.ds[3]
    if music.ds:
        return music.ds[-1]
    return 0


def _plate_plan_title(plan: str) -> str:
    return "極" if plan in ("极", "極") else str(plan)


def _draw_plate_title_fallback(version: str, plan: str) -> Image.Image:
    title = f"{version}{_plate_plan_title(plan)}"
    image = Image.new("RGBA", (1000, 161), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    text = DrawText(draw, SIYUAN)

    draw.rounded_rectangle(
        (18, 17, 982, 144),
        radius=24,
        fill=(255, 255, 255, 218),
        outline=(124, 130, 255, 255),
        width=6,
    )
    draw.rounded_rectangle(
        (34, 33, 966, 128),
        radius=18,
        outline=(193, 247, 225, 255),
        width=3,
    )

    size = 82
    while size > 42 and text.get_box(title, size)[2] - text.get_box(title, size)[0] > 850:
        size -= 4
    text.draw(500, 80, size, title, (124, 130, 255, 255), "mm", 5, (255, 255, 255, 255))
    return image


def _load_plate_title_image(version: str, plan: str) -> Image.Image:
    title_path = platedir / f"{version}{_plate_plan_title(plan)}.png"
    if title_path.exists():
        return Image.open(title_path).convert("RGBA").resize((1000, 161))
    return _draw_plate_title_fallback(version, plan)


def _plate_song_cover_key(song: dict[str, Any]) -> str:
    return str(song.get("render_id") or song.get("song_id") or song.get("title") or "")


def _build_plate_background(
    music: list[Music],
    *,
    image_name_by_id: dict[str, str] | None = None,
) -> Image.Image:
    """Build a plate background when a pre-rendered static template is absent."""
    from .maimaidx_update_table import _draw_song_id_label, _load_cover

    image_name_by_id = image_name_by_id or {}
    id_bg = Image.new('RGBA', (100, 20), (124, 129, 255, 255))
    rlv: Dict[str, List[Music]] = {level: [] for level in reversed(levelList)}
    for item in music:
        if len(item.level) > 3 and item.level[3]:
            rlv.setdefault(item.level[3], []).append(item)

    lines = 0
    interval = 0
    for level in rlv:
        musicnum = len(rlv[level])
        if musicnum == 0:
            continue
        interval += 1
        remainder = musicnum % 10
        lines += (musicnum // 10) + (1 if remainder else 0)

    linesheight = 115 * lines + max(0, interval - 1) * 15
    width, height = 1400, 150 + linesheight + 360
    im = tricolor_gradient(width, height)
    sbi = ScoreBaseImage if maiApi.config.saveinmem else ScoreBaseImage()
    im.alpha_composite(sbi.aurora_bg)
    im.alpha_composite(sbi.shines_bg, (34, 0))
    im.alpha_composite(sbi.rainbow_bg, (319, height - 643))
    im.alpha_composite(sbi.rainbow_bottom_bg, (100, height - 343))
    for h in range((height // 358) + 1):
        im.alpha_composite(sbi.pattern_bg, (0, (358 + 7) * h))

    dr = ImageDraw.Draw(im)
    ts = DrawText(dr, TBFONT)
    sy = DrawText(dr, SIYUAN)
    im.alpha_composite(Image.open(maimaidir / 'design.png'), (200, height - 113))
    sy.draw(700, height - 70, 22, CREDIT_TEXT, sbi.text_color, 'mm')

    y = 245
    for level in rlv:
        if rlv[level]:
            rlv[level].sort(key=_plate_sort_ds, reverse=True)
            y += 15
            im.alpha_composite(Image.open(maimaidir / 'UI_CMN_Chara_Level_S_01.png'), (65, y + 115))
            ts.draw(113, y + 164, 35, level, anchor='mm')
        x = 200
        for num, item in enumerate(rlv[level]):
            if num % 10 == 0:
                x = 200
                y += 115
            else:
                x += 115
            cover = _load_cover(item.id, image_name_by_id.get(str(item.id)))
            if cover:
                im.alpha_composite(cover.resize((100, 100)), (x, y))
            im.alpha_composite(id_bg, (x, y + 80))
            _draw_song_id_label(
                ts,
                sy,
                x + 50,
                y + 88,
                item.id,
                (255, 255, 255, 255),
                numeric_size=20,
                cjk_size=12,
                max_width=96,
            )
    return im


def _cn_plate_data_key(version: str) -> str | None:
    if version in version_map:
        return version_map[version][1]
    if version in plate_to_dx_version:
        return version
    return None


def _cn_plate_supported(version: str) -> bool:
    data_key = _cn_plate_data_key(version)
    return bool(data_key and data_key in getattr(mai, "total_plate_id_list", {}))


def newbestscore(song_id: str, lv: int, value: int, bestlist: List[ChartInfo]) -> int:
    for v in bestlist:
        if song_id == str(v.song_id) and lv == v.level_index:
            if value >= v.ra:
                return value - v.ra
            else:
                return 0
    return value - bestlist[-1].ra


def _display_song_id(value) -> str:
    try:
        song_id = int(str(value))
    except (TypeError, ValueError):
        return ""
    return str(song_id) if song_id > 0 else ""


def _known_positive_number(value) -> Optional[float]:
    try:
        parsed = float(value)
    except (TypeError, ValueError):
        return None
    return parsed if parsed > 0 else None


def _known_text(value) -> str:
    text = str(value or "").strip()
    return "" if text in {"-", "None", "null"} else text


def _display_genre(value) -> str:
    genre = _known_text(value)
    return GENRE_DISPLAY_NAMES.get(genre, genre)


def _format_ds_text(ds_value: float) -> str:
    text = f"{ds_value:.4f}".rstrip("0").rstrip(".")
    return f"{text}.0" if ds_value.is_integer() else text


def _level_ds_text(level, ds) -> str:
    level_text = _known_text(level)
    ds_value = _known_positive_number(ds)
    if level_text and ds_value is not None:
        ds_text = _format_ds_text(ds_value)
        return f"{level_text}({ds_text})"
    if level_text:
        return level_text
    if ds_value is not None:
        return _format_ds_text(ds_value)
    return ""


def _note_display_values(chart) -> tuple[str, list[str]]:
    notes = list(chart.notes)
    total_notes = getattr(chart, "total_notes", None)
    if total_notes in (None, ""):
        if any(value is not None for value in notes):
            total_notes = sum(int(value or 0) for value in notes)
    total_text = str(int(total_notes)) if isinstance(total_notes, (int, float)) and total_notes > 0 else ""
    component_texts: list[str] = []
    for value in notes:
        if value is None:
            component_texts.append("")
        elif value == 0 and not total_text:
            component_texts.append("")
        else:
            component_texts.append(str(int(value)))
    return total_text, component_texts


async def draw_music_info(
    music: Music, 
    qqid: Optional[int] = None, 
    username: Optional[str] = None,
    user: Optional[UserInfo] = None,
    cover_image: Optional[Image.Image] = None,
    display_id: Optional[str] = None,
) -> MessageSegment:
    """
    查看谱面
    
    Params:
        `music`: 曲目模型
        `qqid`: QQID
        `username`: 查分器用户名
        `user`: 用户模型
    Returns:
        `MessageSegment`
    """
    calc = True
    isfull = True
    bestlist: List[ChartInfo] = []
    try:
        if qqid or username:
            if user is None:
                player = await maiApi.query_user_b50(
                    qqid=qqid,
                    username=username,
                    include_chart_metadata=False,
                )
            else:
                player = user
            if music.basic_info.version == list(plate_to_dx_version.values())[-1]:
                bestlist = player.charts.dx
                isfull = bool(len(bestlist) == 15)
            else:
                bestlist = player.charts.sd
                isfull = bool(len(bestlist) == 35)
        else:
            calc = False
    except (UserNotFoundError, UserNotExistsError, UserDisabledQueryError):
        calc = False
    except Exception:
        calc = False

    im = Image.open(maimaidir / 'song_bg.png').convert('RGBA')
    dr = ImageDraw.Draw(im)
    mr = DrawText(dr, SIYUAN)
    tb = DrawText(dr, TBFONT)

    default_color = (124, 130, 255, 255)

    im.alpha_composite(Image.open(maimaidir / 'logo.png').resize((249, 120)), (65, 25))
    if music.basic_info.is_new:
        im.alpha_composite(Image.open(maimaidir / 'UI_CMN_TabTitle_NewSong.png').resize((249, 120)), (940, 100))
    render_id = str(display_id) if display_id is not None else str(music.id)
    cover_id = render_id or str(music.id)
    shown_id = _display_song_id(render_id)
    if cover_image is not None:
        songbg = cover_image.convert('RGBA').resize((280, 280))
    else:
        try:
            songbg = Image.open(music_picture(cover_id)).resize((280, 280))
        except (ValueError, TypeError):
            songbg = Image.open(coverdir / '11000.png').resize((280, 280))
    im.alpha_composite(rounded_corners(songbg, 17, (True, False, False, True)), (110, 180))
    version_icon = maimaidir / f'{music.basic_info.version}.png'
    if music.basic_info.version and version_icon.exists():
        im.alpha_composite(Image.open(version_icon).resize((182, 90)), (800, 370))
    type_icon = maimaidir / f'{music.type}.png'
    if music.type and type_icon.exists():
        im.alpha_composite(Image.open(type_icon).resize((80, 30)), (410, 375))

    title = _known_text(music.title)
    if title:
        if coloumWidth(title) > 40:
            title = changeColumnWidth(title, 39) + '...'
        mr.draw(405, 220, 28, title, default_color, 'lm')
    artist = _known_text(music.basic_info.artist)
    if artist:
        if coloumWidth(artist) > 50:
            artist = changeColumnWidth(artist, 49) + '...'
        mr.draw(407, 265, 20, artist, default_color, 'lm')
    if _known_positive_number(music.basic_info.bpm) is not None:
        tb.draw(460, 330, 30, music.basic_info.bpm, default_color, 'lm')
    if shown_id:
        tb.draw(405, 435, 28, f'ID {shown_id}', default_color, 'lm')
    genre = _display_genre(music.basic_info.genre)
    if genre:
        mr.draw(665, 435, 24, genre, default_color, 'mm')

    for num, _ in enumerate(music.level):
        if num == 4:
            color = (255, 255, 255, 255)
        else:
            color = (255, 255, 255, 255)
        stat = music.stats[num] if music.stats and num < len(music.stats) else None
        fit_diff = stat.fit_diff if stat and stat.fit_diff is not None else None
        level_text = _level_ds_text(music.level[num], music.ds[num])
        if level_text:
            tb.draw(181, 610 + 73 * num, 30, level_text, color, 'mm')
        if fit_diff is not None:
            tb.draw(
                315, 600 + 73 * num, 30,
                f'{round(fit_diff, 2):.2f}',
                default_color, 'mm'
            )
        total_notes, notes = _note_display_values(music.charts[num])
        if total_notes:
            tb.draw(437, 600 + 73 * num, 30, total_notes, default_color, 'mm')
        if len(notes) == 4:
            notes.insert(3, '')
        for n, c in enumerate(notes):
            if c != "":
                tb.draw(556 + 119 * n, 600 + 73 * num, 30, c, default_color, 'mm')
        if num > 1:
            charter = _known_text(music.charts[num].charter)
            if charter:
                if coloumWidth(charter) > 19:
                    charter = changeColumnWidth(charter, 18) + '...'
                mr.draw(372, 1030 + 47 * (num - 2), 18, charter, default_color, 'mm')
            if _known_positive_number(music.ds[num]) is not None:
                ra = sorted([computeRa(music.ds[num], r) for r in achievementList[-6:]], reverse=True)
                for _n, value in enumerate(ra):
                    size = 25
                    if not calc:
                        rating = value
                    elif not isfull:
                        size = 20
                        rating = f'{value}(+{value})'
                    elif shown_id and value > bestlist[-1].ra:
                        new = newbestscore(shown_id, num, value, bestlist)
                        if new == 0:
                            rating = value
                        else:
                            size = 20
                            rating = f'{value}(+{new})'
                    else:
                        rating = value
                    tb.draw(536 + 101 * _n, 1030 + 47 * (num - 2), size, rating, default_color, 'mm')
    mr.draw(600, 1212, 22, CREDIT_TEXT, default_color, 'mm')
    return MessageSegment.image(image_to_base64(im))


async def draw_music_play_data(
    qqid: Optional[int],
    music_id: str,
    username: Optional[str] = None,
    music: Optional[Music] = None,
    cover_image: Optional[Image.Image] = None,
    records: Optional[List[Union[PlayInfoDev, PlayInfoDefault]]] = None,
) -> Union[str, MessageSegment]:
    """
    谱面游玩
    
    Params:
        `qqid`: QQID
        `music_id`: 曲目ID
        `username`: 查分器用户名
    Returns:
        `Union[str, MessageSegment]`
    """
    try:
        if music is None:
            music = _score_music_by_id(music_id)
        if music is None:
            return f"未找到曲目元数据: {music_id}"

        diff: List[Union[None, PlayInfoDev, PlayInfoDefault]] = [None for _ in music.ds]
        dev_flags = [False for _ in music.ds]
        lookup_ids = _score_lookup_ids(music_id, music)
        numeric_lookup_id = next((lookup_id for lookup_id in lookup_ids if str(lookup_id).isdigit()), None)

        if maiApi.token and numeric_lookup_id:
            data = await maiApi.query_user_post_dev(qqid=qqid, username=username, music_id=numeric_lookup_id)
            for _d in data or []:
                if 0 <= _d.level_index < len(diff):
                    diff[_d.level_index] = _d
                    dev_flags[_d.level_index] = True
        else:
            data = [record.model_copy() for record in records] if records is not None else await maiApi.query_user_plate(qqid=qqid, username=username)
            for _d in data or []:
                if _record_matches_music(_d, music, lookup_ids) and 0 <= _d.level_index < len(diff):
                    diff[_d.level_index] = _d

        im = Image.open(maimaidir / 'info_bg.png').convert('RGBA')
    
        dr = ImageDraw.Draw(im)
        tb = DrawText(dr, TBFONT)
        mr = DrawText(dr, SIYUAN)

        im.alpha_composite(Image.open(maimaidir / 'logo.png').resize((249, 120)), (0, 34))
        if cover_image is not None:
            cover = cover_image.convert('RGBA')
        else:
            try:
                cover = Image.open(music_picture(music_id)).convert('RGBA')
            except (ValueError, TypeError):
                cover = Image.open(coverdir / '11000.png').convert('RGBA')
        im.alpha_composite(cover.resize((300, 300)), (100, 260))
        category_name = category.get(music.basic_info.genre, 'maimai')
        category_path = maimaidir / f'info-{category_name}.png'
        if not category_path.exists():
            category_path = maimaidir / 'info-maimai.png'
        im.alpha_composite(Image.open(category_path), (100, 260))
        version_icon = maimaidir / f'{music.basic_info.version}.png'
        if version_icon.exists():
            im.alpha_composite(Image.open(version_icon).resize((183, 90)), (295, 205))
        im.alpha_composite(Image.open(maimaidir / f'{music.type}.png').resize((55, 20)), (350, 560))
        
        color = (124, 129, 255, 255)
        artist = music.basic_info.artist
        if coloumWidth(artist) > 58:
            artist = changeColumnWidth(artist, 57) + '...'
        mr.draw(255, 595, 12, artist, color, 'mm')
        title = music.title
        if coloumWidth(title) > 38:
            title = changeColumnWidth(title, 37) + '...'
        mr.draw(255, 622, 18, title, color, 'mm')
        display_id = _display_song_id(music_id)
        if coloumWidth(display_id) > 20:
            display_id = changeColumnWidth(display_id, 19) + '...'
        tb.draw(160, 720, 22, display_id, color, 'mm')
        tb.draw(380, 720, 22, music.basic_info.bpm, color, 'mm')

        y = 100
        for num, info in enumerate(diff):
            im.alpha_composite(Image.open(maimaidir / f'd-{num}.png'), (650, 235 + y * num))
            if info:
                im.alpha_composite(Image.open(maimaidir / 'ra-dx.png'), (850, 272 + y * num))
                if dev_flags[num]:
                    dxscore = info.dxScore
                    _dxscore = sum(music.charts[num].notes) * 3
                    dxnum = dxScore(dxscore / _dxscore * 100)
                    rating, rate = info.ra, score_Rank_l[info.rate]
                    if dxnum != 0:
                        im.alpha_composite(
                            Image.open(maimaidir / f'UI_GAM_Gauge_DXScoreIcon_0{dxnum}.png').resize((32, 19)), 
                            (851, 296 + y * num)
                        )
                    tb.draw(916, 304 + y * num, 13, f'{dxscore}/{_dxscore}', color, 'mm')
                else:
                    rating, rate = computeRa(music.ds[num], info.achievements, israte=True)
                    
                im.alpha_composite(Image.open(maimaidir / 'fcfs.png'), (965, 265 + y * num))
                if info.fc:
                    im.alpha_composite(
                        Image.open(maimaidir / f'UI_CHR_PlayBonus_{fcl[info.fc]}.png').resize((65, 65)), 
                        (960, 261 + y * num)
                    )
                if info.fs:
                    im.alpha_composite(
                        Image.open(maimaidir / f'UI_CHR_PlayBonus_{fsl[info.fs]}.png').resize((65, 65)), 
                        (1025, 261 + y * num)
                    )
                im.alpha_composite(Image.open(maimaidir / 'ra.png'), (1350, 405 + y * num))
                im.alpha_composite(
                    Image.open(maimaidir / f'UI_TTR_Rank_{rate}.png').resize((100, 45)), 
                    (737, 272 + y * num)
                )

                tb.draw(510, 292 + y * num, 42, f'{info.achievements:.4f}%', color, 'lm')
                tb.draw(685, 248 + y * num, 25, music.ds[num], anchor='mm')
                tb.draw(915, 283 + y * num, 18, rating, color, 'mm')
            else:
                tb.draw(685, 248 + y * num, 25, music.ds[num], anchor='mm')
                mr.draw(800, 302 + y * num, 30, '未游玩', color, 'mm')
        if len(diff) == 4:
            mr.draw(800, 302 + y * 4, 30, '没有该难度', color, 'mm')

        mr.draw(600, 827, 22, CREDIT_TEXT, color, 'mm')
        msg = MessageSegment.image(image_to_base64(im))
        
    except (UserNotFoundError, UserNotExistsError, UserDisabledQueryError, MusicNotPlayError) as e:
        msg = str(e)
    except Exception as e:
        log.error(traceback.format_exc())
        msg = f'未知错误：{type(e)}\n请联系Bot管理员'
    return msg


def calc_achievements_fc(scorelist: Union[List[float], List[str]], lvlist_num: int, isfc: bool = False) -> int:
    r = -1
    obj = range(4) if isfc else achievementList[-6:]
    for __f in obj:
        if len(list(filter(lambda x: x >= __f, scorelist))) == lvlist_num:
            r += 1
        else:
            break
    return r


def draw_rating(rating: str, path: Path) -> MessageSegment:
    """
    绘制指定定数表文字
    
    Params:
        `rating`: 定数
        `path`: 路径
    Returns:
        `MessageSegment`
    """
    im = Image.open(path)
    dr = ImageDraw.Draw(im)
    sy = DrawText(dr, SIYUAN)
    sy.draw(700, 100, 65, f'Level.{rating}   定数表', (124, 129, 255, 255), 'mm', 5, (255, 255, 255, 255))
    return MessageSegment.image(image_to_base64(im))


async def draw_rating_table(
    qqid: Optional[int],
    rating: str,
    isfc: bool = False,
    username: Optional[str] = None,
) -> Union[MessageSegment, str]:
    """绘制定数表"""
    try:
        rating = normalize_level_value(rating)
        mai._ensure_loaded()
        obj = await maiApi.query_user_plate(qqid=qqid, username=username)
        
        statistics = {
            'clear': 0,
            'sync':  0,
            's':     0,
            'sp':    0,
            'ss':    0,
            'ssp':   0,
            'sss':   0,
            'sssp':  0,
            'fc':    0,
            'fcp':   0,
            'ap':    0,
            'app':   0,
            'fs':    0,
            'fsp':   0,
            'fsd':   0,
            'fsdp':  0,
        }
        fromid = {}
        
        sp = score_Rank[-6:]
        for _d in obj:
            if _d.level != rating:
                continue
            if (id := str(_d.song_id)) not in fromid:
                fromid[id] = {}
            fromid[id][str(_d.level_index)] = {
                'achievements': _d.achievements,
                'fc': _d.fc,
                'level': _d.level
            }
            rate = computeRa(_d.ds, _d.achievements, onlyrate=True).lower()
            if _d.achievements >= 80:
                statistics['clear'] += 1
            if rate in sp:
                r_index = sp.index(rate)
                for _r in range(r_index + 1):
                    statistics[sp[_r]] += 1
            if _d.fc:
                fc_index = combo_rank.index(_d.fc)
                for _f in range(fc_index + 1):
                    statistics[combo_rank[_f]] += 1
            if _d.fs:
                if _d.fs == 'sync':
                    statistics[_d.fs] += 1
                else:
                    fs_index = sync_rank.index(_d.fs)
                    for _s in range(fs_index + 1):
                        statistics[sync_rank[_s]] += 1

        achievements_fc_list: List[Union[float, List[float]]] = []
        lvlist = mai.total_level_data[rating]
        lvnum = sum([len(v) for v in lvlist.values()])
        
        rating_bg = Image.open(maimaidir / 'rating_bg.png')
        unfinished_bg = Image.open(maimaidir / 'unfinished_bg.png')
        complete_bg = Image.open(maimaidir / 'complete_bg.png')
        
        bg = ratingdir / f'{rating}.png'
        
        im = Image.open(bg).convert('RGBA')
        dr = ImageDraw.Draw(im)
        sy = DrawText(dr, SIYUAN)
        tb = DrawText(dr, TBFONT)
        
        im.alpha_composite(rating_bg, (600, 25))
        sy.draw(305, 60, 65, f'Level.{rating}', (124, 129, 255, 255), 'mm', 5, (255, 255, 255, 255))
        sy.draw(305, 130, 65, '定数表', (124, 129, 255, 255), 'mm', 5, (255, 255, 255, 255))
        tb.draw(700, 127, 45, lvnum, (124, 129, 255, 255), 'mm', 5, (255, 255, 255, 255))
        
        y = 22
        for n, v in enumerate(statistics):
            if n % 8 == 0:
                x = 824
                y += 56
            else:
                x += 64
            tb.draw(x, y, 20, statistics[v], (124, 129, 255, 255), 'mm', 2, (255, 255, 255, 255))
        
        y = 118
        for ra in lvlist:
            x = 158
            y += 20
            for num, music in enumerate(lvlist[ra]):
                if num % 14 == 0:
                    x = 158
                    y += 85
                else:
                    x += 85
                if music.id in fromid and music.lv in fromid[music.id]:
                    if not isfc:
                        score = fromid[music.id][music.lv]['achievements']
                        achievements_fc_list.append(score)
                        rate = computeRa(music.ds, score, onlyrate=True)
                        rank = Image.open(maimaidir / f'UI_TTR_Rank_{rate}.png').resize((78, 35))
                        if score >= 100:
                            im.alpha_composite(complete_bg, (x + 2, y - 18))
                        else:
                            im.alpha_composite(unfinished_bg, (x + 2, y - 18))
                        im.alpha_composite(rank, (x, y - 5))
                        continue
                    if _fc := fromid[music.id][music.lv]['fc']:
                        achievements_fc_list.append(combo_rank.index(_fc))
                        fc = Image.open(maimaidir / f'UI_MSS_MBase_Icon_{fcl[_fc]}.png').resize((50, 50))
                        im.alpha_composite(complete_bg, (x + 2, y - 18))
                        im.alpha_composite(fc, (x + 15, y - 12))

        if len(achievements_fc_list) == lvnum:
            r = calc_achievements_fc(achievements_fc_list, lvnum, isfc)
            if r != -1:
                pic = fcl[combo_rank[r]] if isfc else score_Rank_l[score_Rank[-6:][r]]
                im.alpha_composite(Image.open(maimaidir / f'UI_MSS_Allclear_Icon_{pic}.png'), (40, 40))
        
        msg = MessageSegment.image(image_to_base64(im))
    except (UserNotFoundError, UserNotExistsError, UserDisabledQueryError, TokenNotFoundError, TokenError, UnknownError) as e:
        msg = str(e)
    except Exception as e:
        log.error(traceback.format_exc())
        msg = f'未知错误：{type(e)}\n请联系Bot管理员'
    return msg


async def draw_plate_table(
    qqid: Optional[int],
    version: str,
    plan: str,
    server: str = "cn",
    username: Optional[str] = None,
    records: Optional[List[PlayInfoDefault]] = None,
) -> Union[MessageSegment, str]:
    """
    绘制完成表
    
    Params:
        `qqid`: QQID
        `username`: 查分器用户名
        `version`: 版本（牌子名，如 真/超/檄）或自定义牌子名
        `plan`: 计划（极/将/神/舞舞）
        `server`: cn（国服）或 custom（自定义）
    """
    try:
        mai._ensure_loaded()
        if version in platecn:
            version = platecn[version]
        from ..shim.mai_music import custom_plate_exists
        if server == "jp":
            return "当前分支不支持日服/dxdata 曲目数据。"
        elif server == "custom":
            server = "custom"
        elif _cn_plate_supported(version):
            server = "cn"
        elif custom_plate_exists(version):
            server = "custom"

        image_name_by_id: dict[str, str] = {}
        if server == "custom":
            from maimai_mcp.search import normalize_text
            from ..shim.mai_music import build_custom_plate_music, get_custom_plate_songs

            plate_songs = get_custom_plate_songs(version)
            if not plate_songs:
                return f'自定义牌子「{version}」未找到对应歌曲'

            plate_total_num = len(plate_songs)
            # 水鱼成绩优先按 song_id 匹配，title+type 只作为无 id 时的兜底。
            plate_by_song_id: dict[int, dict] = {}
            plate_by_title_type: dict[tuple[str, str], dict] = {}
            image_name_by_id = {
                _plate_song_cover_key(song): song.get("image_name") or song.get("imageName")
                for song in plate_songs
                if song.get("image_name") or song.get("imageName")
            }
            for s in plate_songs:
                song_id = _positive_song_id(s.get("song_id"))
                if song_id:
                    plate_by_song_id[song_id] = s
                plate_by_title_type[(normalize_text(s["title"]), s["type"])] = s

            # 拉取玩家成绩（全量）；批量渲染时复用外层已查好的 records。
            obj = (
                [record.model_copy() for record in records]
                if records is not None
                else await maiApi.query_user_plate(qqid=qqid, username=username)
            )

            playerdata: List[PlayInfoDefault] = []
            for _d in obj:
                record_song_id = _positive_song_id(_d.song_id)
                matched = plate_by_song_id.get(record_song_id) if record_song_id else None
                if not matched:
                    matched = plate_by_title_type.get((normalize_text(_d.title), _d.type))
                if not matched:
                    continue
                ds_values = matched.get("ds_values") or []
                level_values = matched.get("level_values") or []
                if _d.level_index < len(ds_values):
                    _d.ds = ds_values[_d.level_index]
                if len(level_values) > 3:
                    _d.table_level = level_values
                if not _positive_song_id(matched.get("song_id")) and matched.get("render_id"):
                    _d.song_id = matched["render_id"]
                playerdata.append(_d)

            music = build_custom_plate_music(plate_songs)

            ver = []

        else:
            # 国服：走 maimaidxplate.json 白名单（ID 为水鱼 DivingFish 体系）
            ver, _ver = version_map.get(version, ([plate_to_dx_version[version]], version))

            music_id_list = mai.total_plate_id_list[_ver]
            df_to_music = mai.get_songs_by_df_ids(music_id_list)
            music = [df_to_music[df_id] for df_id in music_id_list if df_id in df_to_music]
            plate_total_num = len(music_id_list)
            playerdata: List[PlayInfoDefault] = []

            # 拉全量成绩，本地按 song_id 筛选；批量渲染时复用外层已查好的 records。
            obj = (
                [record.model_copy() for record in records]
                if records is not None
                else await maiApi.query_user_plate(qqid=qqid, username=username)
            )
            for _d in obj:
                if _d.song_id not in music_id_list:
                    continue
                _music = df_to_music.get(_d.song_id)
                if _music and _d.level_index < len(_music.ds):
                    _d.table_level = _music.level
                    _d.ds = _music.ds[_d.level_index]
                playerdata.append(_d)

        ra: Dict[str, Dict[str, List[Optional[PlayInfoDefault]]]] = {}
        """
        {
            "14+": {
                "365": [None, None, None, PlayInfoDefault, None],
                ...
            },
            "14": {
                ...
            }
        }
        """
        music.sort(key=_plate_sort_ds, reverse=True)
        number = 4 if version not in ['霸', '舞'] else 5
        for _m in music:
            if len(_m.level) <= 3 or not _m.level[3]:
                continue
            if _m.level[3] not in ra:
                ra[_m.level[3]] = {}
            ra[_m.level[3]][_m.id] = [None for _ in range(number)]
        for _d in playerdata:
            if number == 4 and _d.level_index == 4:
                continue
            if len(_d.table_level) <= 3:
                continue
            level_key = _d.table_level[3]
            song_key = str(_d.song_id)
            if level_key in ra and song_key in ra[level_key]:
                ra[level_key][song_key][_d.level_index] = _d
        
        finished_bg = [Image.open(maimaidir / f't-{_}.png') for _ in range(4)]
        unfinished_bg = Image.open(maimaidir / 'unfinished_bg_2.png')
        complete_bg = Image.open(maimaidir / 'complete_bg_2.png')

        if server == "custom":
            bg_path = platedir / f'custom_{version}.png'
        else:
            bg_path = platedir / f'{version}.png'
        im = Image.open(bg_path) if bg_path.exists() else _build_plate_background(music, image_name_by_id=image_name_by_id)
        draw = ImageDraw.Draw(im)
        tr = DrawText(draw, TBFONT)
        mr = DrawText(draw, SIYUAN)
        
        im.alpha_composite(Image.open(maimaidir / 'plate_num.png'), (185, 20))
        im.alpha_composite(
            _load_plate_title_image(version, plan),
            (200, 35)
        )
        lv: List[set[int]] = [set() for _ in range(number)]
        y = 245
        # if plan == '者':
        #     for level in ra:
        #         x = 200
        #         y += 15
        #         for num, _id in enumerate(ra[level]):
        #             if num % 10 == 0:
        #                 x = 200
        #                 y += 115
        #             else:
        #                 x += 115
        #             f: List[int] = []
        #             for num, play in enumerate(ra[level][_id]):
        #                 if play.achievements or not play.achievements >= 80: continue
        #                 fc = Image.open(maimaidir / f'UI_MSS_MBase_Icon_{fcl[play.fc]}.png')
        #                 im.alpha_composite(fc, (x, y))
        #                 f.append(n)
        #             for n in f:
        #                 im.alpha_composite(finished_bg[n], (x + 5 + 25 * n, y + 67))
        if plan == '极' or plan == '極':
            for level in ra:
                x = 200
                y += 15
                for num, _id in enumerate(ra[level]):
                    if num % 10 == 0:
                        x = 200
                        y += 115
                    else:
                        x += 115
                    f: List[int] = []
                    for n, play in enumerate(ra[level][_id]):
                        if play is None or not play.fc: continue
                        if n == 3:
                            im.alpha_composite(complete_bg, (x, y))
                            fc = Image.open(maimaidir / f'UI_CHR_PlayBonus_{fcl[play.fc]}.png').resize((75, 75))
                            im.alpha_composite(fc, (x + 13, y + 3))
                        lv[n].add(play.song_id)
                        f.append(n)
                    for n in f:
                        im.alpha_composite(finished_bg[n], (x + 5 + 25 * n, y + 67))
        if plan == '将':
            for level in ra:
                x = 200
                y += 15
                for num, _id in enumerate(ra[level]):
                    if num % 10 == 0:
                        x = 200
                        y += 115
                    else:
                        x += 115
                    f: List[int] = []
                    for n, play in enumerate(ra[level][_id]):
                        if play is None or play.achievements < 100: continue
                        if n == 3:
                            im.alpha_composite(complete_bg if play.achievements >= 100 else unfinished_bg, (x, y))
                            rate = computeRa(play.ds, play.achievements, onlyrate=True)
                            rank = Image.open(maimaidir / f'UI_TTR_Rank_{rate}.png').resize((102, 46))
                            im.alpha_composite(rank, (x - 1, y + 15))
                        lv[n].add(play.song_id)
                        f.append(n)
                    for n in f:
                        im.alpha_composite(finished_bg[n], (x + 5 + 25 * n, y + 67))
        if plan == '神':
            _fc = ['ap', 'app']
            for level in ra:
                x = 200
                y += 15
                for num, _id in enumerate(ra[level]):
                    if num % 10 == 0:
                        x = 200
                        y += 115
                    else:
                        x += 115
                    f: List[int] = []
                    for n, play in enumerate(ra[level][_id]):
                        if play is None or play.fc not in _fc: continue
                        if n == 3:
                            im.alpha_composite(complete_bg, (x, y))
                            ap = Image.open(maimaidir / f'UI_CHR_PlayBonus_{fcl[play.fc]}.png').resize((75, 75))
                            im.alpha_composite(ap, (x + 13, y + 3))
                        lv[n].add(play.song_id)
                        f.append(n)
                    for n in f:
                        im.alpha_composite(finished_bg[n], (x + 5 + 25 * n, y + 67))
        if plan == '舞舞':
            fs = ['fsd', 'fdx', 'fsdp', 'fdxp']
            for level in ra:
                x = 200
                y += 15
                for num, _id in enumerate(ra[level]):
                    if num % 10 == 0:
                        x = 200
                        y += 115
                    else:
                        x += 115
                    f: List[int] = []
                    for n, play in enumerate(ra[level][_id]):
                        if play is None or play.fs not in fs:
                            continue
                        if n == 3:
                            im.alpha_composite(complete_bg, (x, y))
                            fsd = Image.open(maimaidir / f'UI_CHR_PlayBonus_{fsl[play.fs]}.png').resize((75, 75))
                            im.alpha_composite(fsd, (x + 13, y + 3))
                        lv[n].add(play.song_id)
                        f.append(n)
                    for n in f:
                        im.alpha_composite(finished_bg[n], (x + 5 + 25 * n, y + 67))
        
        color = ScoreBaseImage.id_color.copy()
        color.insert(0, (124, 129, 255, 255))
        for num in range(len(lv) + 1):
            if num == 0:
                v = set.intersection(*lv)
                _v = f'{len(v)}/{plate_total_num}'
            else:
                _v = len(lv[num - 1])
            if _v == plate_total_num:
                mr.draw(390 + 200 * num, 270, 35, '完成', color[num], 'rm', 4, (255, 255, 255, 255))
            else:
                tr.draw(390 + 200 * num, 270, 40, _v, color[num], 'rm', 4, (255, 255, 255, 255))
        
        msg = MessageSegment.image(image_to_base64(im))
    except (UserNotFoundError, UserNotExistsError, UserDisabledQueryError, TokenNotFoundError, TokenError, UnknownError) as e:
        msg = str(e)
    except Exception as e:
        log.error(traceback.format_exc())
        msg = f'未知错误：{type(e)}\n请联系Bot管理员'
    return msg
