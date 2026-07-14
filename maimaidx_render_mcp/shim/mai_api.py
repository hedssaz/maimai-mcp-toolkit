"""maimaiDX 兼容层：替换 maimaidx_api_data.MaimaiAPI

数据源统一为 diving_fish_b50_mcp：
  - query_user_b50    → diving_fish_b50_mcp.query_b50
  - query_user_plate  → diving_fish_b50_mcp.query_maimai_player_records
  - query_user_get_dev → 同上（version=None 即全量）
"""

import asyncio
import os
from io import BytesIO
from typing import List, Optional

from ..maimaidx.maimaidx_error import (
    TokenNotFoundError,
    TokenError,
    UserNotFoundError,
    UserNotExistsError,
    UserDisabledQueryError,
    UnknownError,
)
from ..maimaidx.maimaidx_model import (
    ChartInfo,
    PlayInfoDefault,
    PlayInfoDev,
    UserInfo,
    UserInfoDev,
    Data,
    UserRanking,
)


def _render_query_timeout_ms(timeout_ms: Optional[int] = None) -> int:
    raw: object = timeout_ms
    if raw is None:
        raw = os.environ.get("MAIMAIDX_RENDER_QUERY_TIMEOUT_MS", "30000")
    try:
        value = int(raw)
    except (TypeError, ValueError):
        value = 30000
    return max(1000, min(30000, value))


def _user_ranking_sort_key(ranker: UserRanking) -> tuple[int, str]:
    return (-ranker.ra, ranker.username.casefold())


class _MaiAPI:
    """替换 maimaiDX 的 MaimaiAPI"""

    class _Config:
        saveinmem: bool = False

    config = _Config()

    @property
    def token(self):
        return bool(os.environ.get("DIVING_FISH_DEVELOPER_TOKEN", ""))

    @staticmethod
    def _clean_marker(value: object) -> str:
        if value in (None, ""):
            return ""
        text = str(value).strip()
        if text.lower() in {"none", "null", "nan"}:
            return ""
        return text

    @classmethod
    def _clean_rate(cls, value: object) -> str:
        return cls._clean_marker(value) or "d"

    def _user_info_from_b50_result(self, result: dict, *, rating_override: Optional[int] = None) -> UserInfo:
        player = result.get("player", {})
        charts = result.get("charts", {})
        sd = charts.get("sd", [])
        dx = charts.get("dx", [])
        is_computed_b50 = result.get("source") == "diving-fish-records"

        def _number(value: object, default: float = 0) -> float:
            try:
                return float(value or default)
            except (TypeError, ValueError):
                return default

        def _int_number(value: object, default: int = 0) -> int:
            try:
                return int(value or default)
            except (TypeError, ValueError):
                return default

        def _chart_is_fit_based(c: dict) -> bool:
            return is_computed_b50 or c.get("ratingBase") == "fitDiff"

        def _display_ds(c: dict) -> float:
            if _chart_is_fit_based(c) and c.get("fitDiff") not in (None, ""):
                return round(_number(c.get("fitDiff")), 2)
            return _number(c.get("ds"))

        def _display_ra(c: dict) -> int:
            if _chart_is_fit_based(c) and c.get("fittedRa") not in (None, ""):
                return _int_number(c.get("fittedRa"))
            return _int_number(c.get("ra"))

        def _make_chart_info(c):
            return ChartInfo(
                song_id=c.get("songId", 0),
                title=c.get("title", ""),
                type=c.get("type", ""),
                level=c.get("level", ""),
                level_label=c.get("levelLabel", ""),
                level_index=c.get("levelIndex", 0),
                ds=_display_ds(c),
                achievements=_number(c.get("achievements")),
                dxScore=_int_number(c.get("dxScore")),
                fc=self._clean_marker(c.get("fc")),
                fs=self._clean_marker(c.get("fs")),
                ra=_display_ra(c),
                rate=self._clean_rate(c.get("rate")),
            )

        if is_computed_b50:
            sort_key = lambda c: (
                _display_ra(c),
                _number(c.get("achievements")),
                _display_ds(c),
            )
            sd = sorted(sd, key=sort_key, reverse=True)
            dx = sorted(dx, key=sort_key, reverse=True)

        return UserInfo(
            nickname=player.get("nickname"),
            username=player.get("username"),
            plate=player.get("plate"),
            rating=rating_override if rating_override is not None else player.get("rating") or 0,
            additional_rating=player.get("additionalRating") or 0,
            charts=Data(
                sd=[_make_chart_info(c) for c in sd],
                dx=[_make_chart_info(c) for c in dx],
            ),
        )

    async def query_user_b50(
        self,
        *,
        qqid: Optional[int] = None,
        username: Optional[str] = None,
        include_chart_metadata: bool = True,
        timeout_ms: Optional[int] = None,
    ) -> UserInfo:
        from diving_fish_b50_mcp.server import query_b50

        args: dict = {}
        if qqid:
            args["qq"] = str(qqid)
        if username:
            args["username"] = username
        if not args:
            args["qq"] = ""
        args["includeChartMetadata"] = include_chart_metadata
        args["timeoutMs"] = _render_query_timeout_ms(timeout_ms)

        result = query_b50(args)
        return self._user_info_from_b50_result(result)

    async def query_user_computed_b50(
        self,
        *,
        qqid: Optional[int] = None,
        username: Optional[str] = None,
        include_chart_metadata: bool = False,
        timeout_ms: Optional[int] = None,
    ) -> UserInfo:
        from diving_fish_b50_mcp.server import query_computed_b50

        args: dict = {}
        if qqid:
            args["qq"] = str(qqid)
        if username:
            args["username"] = username
        if not args:
            args["qq"] = ""
        args["includeChartMetadata"] = include_chart_metadata
        args["timeoutMs"] = _render_query_timeout_ms(timeout_ms)

        result = query_computed_b50(args)
        rating_breakdown = result.get("ratingBreakdown") if isinstance(result.get("ratingBreakdown"), dict) else {}
        computed_rating = rating_breakdown.get("total")
        rating_override = int(computed_rating) if isinstance(computed_rating, (int, float)) else None
        return self._user_info_from_b50_result(result, rating_override=rating_override)

    async def query_user_plate(
        self,
        *,
        qqid: Optional[int] = None,
        username: Optional[str] = None,
        version: Optional[List[str]] = None,
        timeout_ms: Optional[int] = None,
    ) -> List[PlayInfoDefault]:
        """调 query_maimai_player_records（底层 /dev/player/records）。"""
        from diving_fish_b50_mcp.server import query_maimai_player_records, DivingFishError

        args: dict = {}
        if qqid:
            args["qq"] = str(qqid)
        if username:
            args["username"] = username
        if version:
            args["version"] = version
        args["timeoutMs"] = _render_query_timeout_ms(timeout_ms)

        try:
            result = query_maimai_player_records(args)
        except DivingFishError as exc:
            if exc.code == "AUTH_REQUIRED":
                raise TokenNotFoundError() from exc
            if exc.status == 403:
                raise UserDisabledQueryError() from exc
            if exc.status in (400, 404):
                raise UserNotExistsError() from exc
            if exc.status == 401:
                raise TokenError() from exc
            raise UnknownError(f"获取玩家完整成绩失败：{exc}") from exc
        except Exception as exc:
            raise UnknownError(f"获取玩家完整成绩失败：{exc}") from exc

        records = result.get("records", [])
        return [
            PlayInfoDefault(
                song_id=r.get("songId", 0),
                id=r.get("songId", 0),
                title=r.get("title", ""),
                type=r.get("type", ""),
                level=r.get("level", ""),
                level_index=r.get("levelIndex", 0),
                ds=float(r.get("ds", 0) or 0),
                achievements=float(r.get("achievements", 0) or 0),
                dxScore=int(r.get("dxScore", 0) or 0),
                fc=self._clean_marker(r.get("fc")),
                fs=self._clean_marker(r.get("fs")),
                ra=int(r.get("ra", 0) or 0),
                rate=self._clean_rate(r.get("rate")),
            )
            for r in records
        ]

    async def query_user_get_dev(
        self,
        *,
        qqid: Optional[int] = None,
        username: Optional[str] = None,
        timeout_ms: Optional[int] = None,
    ) -> UserInfoDev:
        """全量成绩 → PlayInfoDev 列表（复用 query_user_plate，不传 version）"""
        records = await self.query_user_plate(
            qqid=qqid,
            username=username,
            version=None,
            timeout_ms=timeout_ms,
        )
        return UserInfoDev(
            records=[
                PlayInfoDev(
                    song_id=r.song_id,
                    title=r.title,
                    type=r.type,
                    level=r.level,
                    level_label="",
                    level_index=r.level_index,
                    ds=r.ds,
                    achievements=r.achievements,
                    dxScore=r.dxScore,
                    fc=r.fc,
                    fs=r.fs,
                    ra=r.ra,
                    rate=r.rate,
                )
                for r in records
            ]
        )

    async def query_user_post_dev(
        self,
        *,
        qqid: Optional[int] = None,
        username: Optional[str] = None,
        music_id: str | int,
    ) -> List[PlayInfoDev]:
        """指定曲目成绩 → PlayInfoDev 列表（复用 Diving-Fish 单曲成绩工具）"""
        from diving_fish_b50_mcp.server import query_maimai_song_score, DivingFishError

        args: dict = {"musicId": music_id}
        if qqid:
            args["qq"] = str(qqid)
        if username:
            args["username"] = username

        try:
            result = query_maimai_song_score(args)
        except DivingFishError as exc:
            if exc.code == "AUTH_REQUIRED":
                raise TokenNotFoundError() from exc
            if exc.status == 403:
                raise UserDisabledQueryError() from exc
            if exc.status in (400, 404):
                raise UserNotExistsError() from exc
            if exc.status == 401:
                raise TokenError() from exc
            raise UnknownError(f"获取玩家单曲成绩失败：{exc}") from exc
        except Exception as exc:
            raise UnknownError(f"获取玩家单曲成绩失败：{exc}") from exc

        records = result.get("records", [])
        return [
            PlayInfoDev(
                song_id=r.get("songId") or r.get("musicId") or music_id,
                title=r.get("title", ""),
                type=r.get("type", ""),
                level=r.get("level", ""),
                level_label=r.get("levelLabel", ""),
                level_index=int(r.get("levelIndex", 0) or 0),
                ds=float(r.get("ds", 0) or 0),
                achievements=float(r.get("achievements", 0) or 0),
                dxScore=int(r.get("dxScore", 0) or 0),
                fc=self._clean_marker(r.get("fc")),
                fs=self._clean_marker(r.get("fs")),
                ra=int(r.get("ra", 0) or 0),
                rate=self._clean_rate(r.get("rate")),
            )
            for r in records
        ]

    async def rating_ranking(self) -> List[UserRanking]:
        """公开 rating 排行榜。"""
        from diving_fish_b50_mcp.server import call_diving_fish_api

        result = call_diving_fish_api({"operation": "maimai_rating_ranking_get"})
        data = result.get("data", [])
        if not isinstance(data, list):
            data = []

        ranking: List[UserRanking] = []
        for item in data:
            if not isinstance(item, dict):
                continue
            username = item.get("username") or item.get("name")
            ra = item.get("ra", item.get("rating"))
            if not username:
                continue
            try:
                ranking.append(UserRanking(username=str(username), ra=int(ra or 0)))
            except (TypeError, ValueError):
                continue
        return sorted(ranking, key=_user_ranking_sort_key)

    async def qqlogo(self, *, qqid: int) -> bytes:
        import urllib.request
        url = f"https://q1.qlogo.cn/g?b=qq&nk={qqid}&s=640"
        try:
            with urllib.request.urlopen(url, timeout=5) as resp:
                return resp.read()
        except Exception:
            return b""


maiApi = _MaiAPI()
