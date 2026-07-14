import random
from copy import deepcopy
from typing import Dict, List, Optional, Tuple, Union

from . import *
from .maimaidx_model import *

# ============================================================
# Helper functions
# ============================================================

def cross(
    checker: Union[List[str], List[float]],
    elem: Optional[Union[str, float, List[str], List[float], Tuple[float, float]]],
    diff: List[int]
) -> Tuple[bool, List[int]]:
    ret = False
    diff_ret = []
    if not elem or elem is Ellipsis:
        return True, diff
    if isinstance(elem, (str, float)):
        elem = [elem]
    for _j in (range(len(checker)) if diff is Ellipsis else diff):
        if _j >= len(checker):
            continue
        if isinstance(elem, List):
            if checker[_j] in elem:
                diff_ret.append(_j)
                ret = True
        elif isinstance(elem, Tuple):
            if elem[0] <= checker[_j] <= elem[1]:
                diff_ret.append(_j)
                ret = True
    return ret, diff_ret


def in_or_equal(
    checker: Union[str, int, float, List[str], List[int], List[float]],
    elem: Optional[Union[str, float, List[str], List[float], Tuple[float, float]]]
) -> bool:
    if elem is Ellipsis:
        return True
    if isinstance(elem, List):
        return checker in elem
    elif isinstance(elem, Tuple):
        return elem[0] <= checker <= elem[1]
    else:
        return checker == elem


def search_charts(checker: List[Chart], elem: str, diff: List[int]) -> Tuple[bool, List[int]]:
    ret = False
    diff_ret = []
    if not elem or elem is Ellipsis:
        return True, diff
    for _j in (range(len(checker)) if diff is Ellipsis else diff):
        if elem.lower() in checker[_j].charter.lower():
            diff_ret.append(_j)
            ret = True
    return ret, diff_ret


# ============================================================
# MusicList — 曲目列表，带过滤/查询方法
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
        lv: Dict[str, Union[PlanInfo, RaMusic, Dict]] = {}
        for music in self:
            if level not in music.level:
                continue
            if int(music.id) >= 100000:
                continue
            if music.level.count(level) > 1:
                lv[music.id] = {
                    index: RaMusic(
                        id=music.id,
                        ds=music.ds[index],
                        lv=str(index),
                        lvp=music.level[index],
                        type=music.type,
                        image_name=getattr(music, "image_name", None),
                    )
                    for index, _lv in enumerate(music.level)
                    if _lv == level
                }
            else:
                index = music.level.index(level)
                lv[music.id] = RaMusic(
                    id=music.id,
                    ds=music.ds[index],
                    lv=str(index),
                    lvp=music.level[index],
                    type=music.type,
                    image_name=getattr(music, "image_name", None),
                )
        return lv

    def by_level_list(self) -> Dict[str, Dict[str, List[RaMusic]]]:
        from . import levelList

        def level_range(lv: str) -> range:
            if lv == '15':
                return range(1)
            if lv.endswith('+'):
                return range(9, 5, -1)
            return range(9, -1, -1) if int(lv) <= 5 else range(5, -1, -1)

        _level: Dict[str, Dict[str, List[RaMusic]]] = {
            lv: {f"{lv.rstrip('+')}.{i}": [] for i in level_range(lv)} for lv in levelList
        }
        for music in self:
            if int(music.id) >= 100000:
                continue
            for index, ds in enumerate(music.ds):
                if ds < 7:
                    continue
                ra = RaMusic(
                    id=music.id,
                    ds=ds,
                    lv=str(index),
                    lvp=music.level[index],
                    type=music.type,
                    image_name=getattr(music, "image_name", None),
                )
                _level[music.level[index]][str(ds)].append(ra)
        return _level

    def by_id_list(self, music_id_list: List[int]) -> Optional[List[Music]]:
        musicList = []
        for music in self:
            if int(music.id) in music_id_list:
                musicList.append(music)
        return musicList

    def random(self) -> Music:
        return random.choice(self)

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
        diff: List[int] = ...,
        version: Union[str, List[str]] = ...
    ) -> 'MusicList':
        new_list = MusicList()
        for music in self:
            diff2 = diff
            music = deepcopy(music)
            ret, diff2 = cross(music.level, level, diff2)
            if not ret:
                continue
            ret, diff2 = cross(music.ds, ds, diff2)
            if not ret:
                continue
            ret, diff2 = search_charts(music.charts, charter_search, diff2)
            if not ret:
                continue
            if not in_or_equal(music.basic_info.genre, genre):
                continue
            if not in_or_equal(music.type, type):
                continue
            if not in_or_equal(music.basic_info.bpm, bpm):
                continue
            if not in_or_equal(music.basic_info.version, version):
                continue
            if title_search is not Ellipsis and title_search.lower() not in music.title.lower():
                continue
            if artist_search is not Ellipsis and artist_search.lower() not in music.basic_info.artist.lower():
                continue
            music.diff = diff2
            new_list.append(music)
        return new_list
