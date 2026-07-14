import copy
import time
from io import BytesIO
from typing import Any

from .image import tricolor_gradient
from .maimai_best_50 import *
from .maimaidx_model import Music
from . import mai, maiApi


def _positive_int(value: Any) -> int | None:
    try:
        number = int(value)
    except (TypeError, ValueError):
        return None
    return number if number > 0 else None


def _cover_candidates(song_id: str) -> list[Path]:
    nid = _positive_int(song_id)
    if not nid:
        return []
    candidates = [coverdir / f"{nid}.png"]
    if nid > 100000:
        candidates.append(coverdir / f"{nid - 100000}.png")
    if 1000 < nid < 10000 or 10000 < nid <= 11000:
        candidates.extend([coverdir / f"{nid + 10000}.png", coverdir / f"{nid - 10000}.png"])
    if nid >= 10000:
        candidates.append(coverdir / f"{nid % 10000}.png")
    seen: set[Path] = set()
    return [path for path in candidates if not (path in seen or seen.add(path))]


def _load_exact_cover(song_id: str):
    for cover in _cover_candidates(song_id):
        if cover.exists():
            return Image.open(cover)
    return None


def _load_cover(
    song_id: str,
    image_name: str | None = None,
    *,
    allow_download: bool = True,
    timeout_seconds: float = 15,
    attempts: int = 3,
):
    """加载本地封面。旧参数保留用于兼容调用方。"""
    _ = image_name, allow_download, timeout_seconds, attempts
    cover = _load_exact_cover(song_id)
    if cover:
        return cover
    try:
        cover = music_picture(song_id)
    except (ValueError, TypeError):
        return None
    if cover.exists():
        return Image.open(cover)
    try:
        nid = int(song_id)
        if nid >= 10000:
            alt = music_picture(str(nid % 10000))
            if alt.exists():
                return Image.open(alt)
    except (ValueError, TypeError):
        pass
    return None


def _display_song_id(song_id: str) -> str:
    try:
        return str(int(song_id))
    except (ValueError, TypeError):
        return ""


def _song_id_label(song_id: str) -> tuple[str, bool]:
    display_id = _display_song_id(song_id)
    if display_id:
        return display_id, True
    return str(song_id or "").strip(), False


def _text_width(draw_text: DrawText, text: str, size: int) -> int:
    left, _top, right, _bottom = draw_text.get_box(text, size)
    return int(right - left)


def _truncate_label(draw_text: DrawText, text: str, size: int, max_width: int) -> str:
    if _text_width(draw_text, text, size) <= max_width:
        return text
    suffix = "..."
    kept = ""
    for char in text:
        candidate = f"{kept}{char}{suffix}"
        if _text_width(draw_text, candidate, size) > max_width:
            break
        kept += char
    return f"{kept}{suffix}" if kept else ""


def _draw_song_id_label(
    numeric_font: DrawText,
    cjk_font: DrawText,
    x: int,
    y: int,
    song_id: str,
    color: tuple[int, int, int, int],
    *,
    numeric_size: int,
    cjk_size: int,
    max_width: int,
) -> None:
    label, is_numeric = _song_id_label(song_id)
    if not label:
        return
    if is_numeric:
        numeric_font.draw(x, y, numeric_size, label, color, "mm")
        return
    label = _truncate_label(cjk_font, label, cjk_size, max_width)
    if label:
        cjk_font.draw(x, y, cjk_size, label, color, "mm")


async def update_rating_table() -> str:
    """更新定数表"""
    try:
        mai._ensure_loaded()
        dx = Image.open(maimaidir / 'DX.png').convert('RGBA').resize((44, 16))
        diff = [Image.new('RGBA', (75, 16), color) for color in ScoreBaseImage.bg_color]
        sbi = ScoreBaseImage if maiApi.config.saveinmem else ScoreBaseImage()
        atime = 0
        for lv in levelList[6:]:
            _otime = time.time()
            picname = ratingdir / f'{lv}.png'
            lvlist = mai.total_level_data[lv]
            lines = 0
            for _lv in lvlist:
                musicnum = len(lvlist[_lv])
                if musicnum == 0:
                    r = 1
                else:
                    remainder = musicnum % 14
                    r = (musicnum // 14) + (1 if remainder else 0)
                lines += r

            if '+' in lv:
                f = 4
            elif lv == '6':
                f = 10
            else:
                f = 8

            linesheight = 85 * lines
            """
            `85` 为曲绘高度 `80` + 间隔 `5`
            `lines` 为行数
            """
            
            width, height = 1400, 325 + f * 20 + linesheight
            """
            `325` 为顶部文字和底部图片高度 + 上下间隔高度
            `f * 20` 为等级数量 `f` * 等级间隔 `20`
            `linesheight` 为各等级曲绘和间隔总和高度
            """
            
            im = tricolor_gradient(width, height)
            
            im.alpha_composite(sbi.aurora_bg)
            im.alpha_composite(sbi.shines_bg, (34, 0))
            im.alpha_composite(sbi.rainbow_bg, (319, height - 643))
            im.alpha_composite(sbi.rainbow_bottom_bg, (100, height - 343))
            for h in range((height // 358) + 1):
                im.alpha_composite(sbi.pattern_bg, (0, (358 + 7) * h))

            dr = ImageDraw.Draw(im)
            sy = DrawText(dr, SIYUAN)
            ts = DrawText(dr, TBFONT)
            im.alpha_composite(Image.open(maimaidir / 'design.png'), (200, height - 113))
            sy.draw(
                700, 
                height - 70, 
                22, 
                CREDIT_TEXT,
                sbi.text_color, 
                'mm'
            )
            y = 100
            for _lv in lvlist: 
                x = 160
                y += 20
                im.alpha_composite(
                    Image.open(maimaidir / 'UI_CMN_Chara_Level_S_01.png').resize((80, 80)), (50, y + 80)
                )
                ts.draw(88, y + 120, 35, _lv, anchor='mm')
                for num, music in enumerate(lvlist[_lv]):
                    if num % 14 == 0:
                        x = 160
                        y += 85
                    else:
                        x += 85
                    cover_source = _load_cover(
                        music.id,
                        music.image_name,
                        attempts=1,
                        timeout_seconds=5,
                    )
                    if cover_source is None:
                        cover_source = Image.open(music_picture(music.id))
                    cover = cover_source.resize((75, 75))
                    im.alpha_composite(cover, (x, y))
                    if music.type == 'DX':
                        im.alpha_composite(dx, (x + 31, y))
                    im.alpha_composite(diff[int(music.lv)], (x, y + 59))
                    _draw_song_id_label(
                        ts,
                        sy,
                        x + 37,
                        y + 67,
                        music.id,
                        sbi.t_color[int(music.lv)],
                        numeric_size=13,
                        cjk_size=9,
                        max_width=70,
                    )
                if not lvlist[_lv]:
                    y += 85

            by = BytesIO()
            im.save(by, 'PNG')
            with open(picname, 'wb') as f:
                f.write(by.getbuffer())
            _ntime = int(time.time() - _otime)
            atime += _ntime
            log.info(f'lv.{lv} 定数表更新完成，耗时：{_ntime}s')
        log.info(f'定数表更新完成，共耗时{atime}s')
        return f'定数表更新完成，共耗时{atime}s'
    except Exception as e:
        log.error(traceback.format_exc())
        return f'定数表更新失败，Error: {e}'


async def update_plate_table() -> str:
    """更新完成表"""
    try:
        mai._ensure_loaded()
        version = list(_ for _ in plate_to_dx_version.keys())[1:]
        # version.append('霸')
        # version.append('舞')
        id_bg = Image.new('RGBA', (100, 20), (124, 129, 255, 255))
        rlv: Dict[str, List[Music]] = {}
        for _ in list(reversed(levelList)):
            rlv[_] = []
        sbi = ScoreBaseImage if maiApi.config.saveinmem else ScoreBaseImage()
        for _v in version:
            if _v in platecn:
                _v = platecn[_v]
            ver, _ver = version_map.get(_v, ([plate_to_dx_version.get(_v)], _v))
            
            music_id_list = mai.total_plate_id_list[_ver]
            df_to_music = mai.get_songs_by_df_ids(music_id_list)
            music = [df_to_music[df_id] for df_id in music_id_list if df_id in df_to_music]
            ralv = copy.deepcopy(rlv)

            for m in music:
                ralv[m.level[3]].append(m)

            lines = 0
            interval = 0
            for _ in ralv:
                musicnum = len(ralv[_])
                if musicnum == 0:
                    continue
                interval += 1
                remainder = musicnum % 10
                lines += (musicnum // 10) + (1 if remainder else 0)
            
            linesheight = 115 * lines + (interval - 1) * 15
            """
            `linesheight`: 各等级曲绘和间隔总和高度
            
                - `115` 为曲绘高度 `100` + 间隔 `15`
                - `lines` 为行数
                - `interval` 为各等级间隔行数
                - `(interval - 1) * 15` 为各等级间隔高度，各等级之间间隔为 `30`，所以只加 `15`
            """
            width, height = 1400, 150 + linesheight + 360
            """
            `150` 为底部图片 `design` 高度 + 上下间隔高度
            `linesheight` 为各等级曲绘和间隔总和高度
            `360` 为顶部图片 `` 高度 + 上下间隔高度
            """

            im = tricolor_gradient(width, height)
            
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
            sy.draw(
                700, 
                height - 70, 
                22, 
                CREDIT_TEXT,
                sbi.text_color, 
                'mm'
            )
            y = 245
            for r in ralv:
                if _v in ['霸', '舞']:
                    ralv[r].sort(key=lambda x: x.ds[-1], reverse=True)
                else:
                    ralv[r].sort(key=lambda x: x.ds[3], reverse=True)
                if ralv[r]:
                    y += 15
                    im.alpha_composite(
                        Image.open(maimaidir / 'UI_CMN_Chara_Level_S_01.png'), (65, y + 115)
                    )
                    ts.draw(113, y + 164, 35, r, anchor='mm')
                x = 200
                for num, music in enumerate(ralv[r]):
                    if num % 10 == 0:
                        x = 200
                        y += 115
                    else:
                        x += 115
                    cover = _load_cover(music.id)
                    if cover:
                        im.alpha_composite(cover.resize((100, 100)), (x, y))
                    im.alpha_composite(id_bg, (x, y + 80))
                    _draw_song_id_label(
                        ts,
                        sy,
                        x + 50,
                        y + 88,
                        music.id,
                        (255, 255, 255, 255),
                        numeric_size=20,
                        cjk_size=12,
                        max_width=96,
                    )

            by = BytesIO()
            im.save(by, 'PNG')
            with open(platedir / f'{_v}.png', 'wb') as f:
                f.write(by.getbuffer())
            log.info(f'{_v}代牌子更新完成')

        return f'完成表更新完成'
    except Exception as e:
        log.error(traceback.format_exc())
        return f'完成表更新失败，Error: {e}'
