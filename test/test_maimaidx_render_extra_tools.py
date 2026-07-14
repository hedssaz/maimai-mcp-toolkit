from __future__ import annotations

import asyncio
from collections import defaultdict
import json
import os
import tempfile
import unittest
import warnings
from types import SimpleNamespace
from unittest.mock import patch

from PIL import Image

import maimaidx_render_mcp.server as server
from maimaidx_render_mcp.maimaidx import category
from maimaidx_render_mcp.maimaidx import maimaidx_music_info as music_info
from maimaidx_render_mcp.maimaidx import maimaidx_player_score as player_score
from maimaidx_render_mcp.maimaidx import tool as render_tool
from maimaidx_render_mcp.maimaidx.image import image_to_base64
from maimaidx_render_mcp.maimaidx.maimaidx_model import BasicInfo, ChartInfo, Music, Stats
from maimaidx_render_mcp.shim import mai_api as shim_mai_api
from maimaidx_render_mcp.shim import mai_music as shim_music
from maimaidx_render_mcp.shim.mai_music import MusicList


def _rise_score_floor_record(song_id: int = 999, ra: int = 250) -> ChartInfo:
    return ChartInfo(
        song_id=song_id,
        title=f"floor {song_id}",
        type="DX",
        level="12",
        level_label="Master",
        level_index=0,
        ds=12.0,
        achievements=99.0,
        dxScore=0,
        ra=ra,
        rate="ss",
    )


def _rise_score_music(
    music_id: str,
    ds: float,
    *,
    level: str = "14",
    fit_diff: float | None = None,
) -> Music:
    return Music(
        id=music_id,
        title=f"song {music_id}",
        type="DX",
        ds=[ds],
        level=[level],
        cids=[],
        charts=[],
        basic_info=BasicInfo(
            title=f"song {music_id}",
            artist="artist",
            genre="maimai",
            bpm=190,
            is_new=True,
            **{"from": "maimai でらっくす PRiSM"},
        ),
        stats=[] if fit_diff is None else [Stats(fit_diff=fit_diff)],
    )


class MaimaidxRenderExtraToolsTests(unittest.TestCase):
    def test_extra_render_tools_are_exposed(self) -> None:
        names = {tool["name"] for tool in server.TOOLS}

        self.assertIn("render_maimai_music_score", names)
        self.assertIn("render_maimai_music_info_batch", names)
        self.assertIn("render_maimai_plate_batch", names)
        self.assertIn("render_maimai_plate_progress_batch", names)
        self.assertIn("render_maimai_music_global_stats", names)
        self.assertIn("render_maimai_rise_score", names)
        self.assertIn("render_maimai_score_list", names)
        self.assertIn("render_maimai_rating_ranking", names)

    def test_plate_wuwu_progress_condition_matches_completion_table(self) -> None:
        condition = player_score._plate_plan_condition("舞舞")

        for marker in ("fsd", "fdx", "fsdp", "fdxp"):
            with self.subTest(marker=marker):
                self.assertFalse(condition(SimpleNamespace(fs=marker)))
        for marker in ("", None, "fs", "fsp"):
            with self.subTest(marker=marker):
                self.assertTrue(condition(SimpleNamespace(fs=marker)))

    def test_ok_image_treats_plain_string_as_error(self) -> None:
        result = server._ok_image("您未游玩该曲目")

        self.assertTrue(result.get("isError"))
        self.assertEqual(result["content"][0]["text"], "您未游玩该曲目")

    def test_ok_image_treats_empty_string_as_missing_render_dependency(self) -> None:
        result = server._ok_image("")

        self.assertTrue(result.get("isError"))
        self.assertIn("渲染未生成图片", result["content"][0]["text"])

    def test_standardized_music_categories_resolve_to_info_assets(self) -> None:
        for genre in ["anime", "maimai", "niconico", "touhou", "game", "ongeki"]:
            self.assertIn(genre, category)
            self.assertEqual(category[genre], genre)

    def test_music_global_stats_pil_fallback_returns_image(self) -> None:
        stats = Stats(
            dist=[0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377],
            fc_dist=[610, 55, 34, 21, 13],
        )
        music = Music(
            id="11475",
            title="SUPER AMBULANCE",
            type="DX",
            ds=[7.0, 10.0, 13.0, 14.0],
            level=["7", "10", "13", "14"],
            cids=[],
            charts=[],
            basic_info=BasicInfo(
                title="SUPER AMBULANCE",
                artist="曲師",
                genre="ongeki",
                bpm=190,
                is_new=False,
                **{"from": "舞"},
            ),
            stats=[stats, stats, stats, stats],
        )

        old_output_dir = os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            try:
                os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = tmpdir
                result = player_score._draw_music_global_data_fallback(music, 3)
                saved = server._ok_image(result)
            finally:
                if old_output_dir is None:
                    os.environ.pop("MAIMAIDX_RENDER_OUTPUT_DIR", None)
                else:
                    os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = old_output_dir

        self.assertFalse(saved.get("isError"))
        payload = json.loads(saved["content"][0]["text"])
        self.assertEqual(payload["width"], 1000)
        self.assertEqual(payload["height"], 800)

    def test_music_global_stats_browser_html_loads_bundled_cjk_font(self) -> None:
        old_pie_html_file = player_score.pie_html_file
        with tempfile.TemporaryDirectory() as tmpdir:
            try:
                html_path = os.path.join(tmpdir, "temp_pie.html")
                with open(html_path, "w", encoding="utf-8") as fp:
                    fp.write("<html><head><script>ResourceHanRoundedCN</script></head><body></body></html>")
                player_score.pie_html_file = render_tool.Path(html_path)

                player_score._patch_global_stats_html_font()

                html = player_score.pie_html_file.read_text(encoding="utf-8")
            finally:
                player_score.pie_html_file = old_pie_html_file

        self.assertIn("@font-face", html)
        self.assertIn("ResourceHanRoundedCN", html)
        self.assertIn("ResourceHanRoundedCN-Bold.ttf", html)

    def test_chromium_executable_path_uses_env_override(self) -> None:
        old_value = os.environ.get("MAIMAIDX_CHROMIUM_EXECUTABLE")
        old_playwright_value = os.environ.get("PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH")
        with tempfile.TemporaryDirectory() as tmpdir:
            fake_chrome = os.path.join(tmpdir, "chromium")
            with open(fake_chrome, "w", encoding="utf-8") as fp:
                fp.write("#!/bin/sh\n")
            try:
                os.environ["MAIMAIDX_CHROMIUM_EXECUTABLE"] = fake_chrome
                os.environ.pop("PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH", None)

                self.assertEqual(render_tool._chromium_executable_path(), fake_chrome)
            finally:
                if old_value is None:
                    os.environ.pop("MAIMAIDX_CHROMIUM_EXECUTABLE", None)
                else:
                    os.environ["MAIMAIDX_CHROMIUM_EXECUTABLE"] = old_value
                if old_playwright_value is None:
                    os.environ.pop("PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH", None)
                else:
                    os.environ["PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH"] = old_playwright_value

    def test_score_music_by_id_falls_back_to_local_search(self) -> None:
        music_info.mai._ensure_loaded()
        old_total_list = music_info.mai.total_list
        old_loaded = music_info.mai._loaded
        try:
            music_info.mai.total_list = MusicList()
            music_info.mai._loaded = True

            music = music_info._score_music_by_id("348")
        finally:
            music_info.mai.total_list = old_total_list
            music_info.mai._loaded = old_loaded

        self.assertIsNotNone(music)
        self.assertEqual(music.id, "348")
        self.assertEqual(music.title, "Axeria")

    def test_render_music_list_augments_from_merged_search_database(self) -> None:
        music_info.mai._ensure_loaded()
        old_total_list = music_info.mai.total_list
        old_loaded = music_info.mai._loaded
        try:
            music_info.mai.total_list = MusicList()
            music_info.mai._augment_from_search_database()
            numeric_music = music_info.mai.total_list.by_id("348")
        finally:
            music_info.mai.total_list = old_total_list
            music_info.mai._loaded = old_loaded

        self.assertIsNotNone(numeric_music)
        self.assertEqual(numeric_music.title, "Axeria")

    def test_rating_level_data_preserves_image_name(self) -> None:
        music = Music(
            id="1180",
            title="local image-name song",
            type="DX",
            ds=[15.0],
            level=["15"],
            cids=[],
            charts=[],
            basic_info=BasicInfo(
                title="local image-name song",
                artist="xi",
                genre="maimai",
                bpm=180,
                is_new=False,
                **{"from": "maimai でらっくす PRiSM PLUS"},
            ),
            stats=[],
            image_name="local-cover-name",
        )
        old_total_list = music_info.mai.total_list
        old_level_data = music_info.mai.total_level_data
        try:
            music_info.mai.total_list = MusicList([music])
            music_info.mai._build_level_data()
            entry = music_info.mai.total_level_data["15"]["15.0"][0]
        finally:
            music_info.mai.total_list = old_total_list
            music_info.mai.total_level_data = old_level_data

        self.assertEqual(entry.image_name, "local-cover-name")

    def test_music_list_by_plan_accepts_string_ids(self) -> None:
        music = Music(
            id="Xaleid◆scopiX",
            title="Xaleid◆scopiX",
            type="DX",
            ds=[7.6, 11.0, 13.7, 14.9, 15.0],
            level=["7+", "11", "13+", "14+", "15"],
            cids=[],
            charts=[],
            basic_info=BasicInfo(
                title="Xaleid◆scopiX",
                artist="xi",
                genre="maimai",
                bpm=180,
                is_new=False,
                **{"from": "maimai でらっくす PRiSM"},
            ),
            stats=[],
        )

        result = MusicList([music]).by_plan("14+")

        self.assertIn("Xaleid◆scopiX", result)

    def test_render_music_by_plan_rejects_non_cn_server(self) -> None:
        music_info.mai._loaded = False
        music_info.mai._ensure_loaded()

        cn = music_info.mai.by_plan("15", server="cn")
        jp = music_info.mai.by_plan("15", server="jp")

        self.assertIsInstance(cn, dict)
        self.assertEqual(jp, {})

    def test_render_music_list_dedupes_dx_offset_ids(self) -> None:
        music_info.mai._loaded = False
        music_info.mai._ensure_loaded()
        matches = [music for music in music_info.mai.total_list if music.title == "系ぎて"]

        self.assertEqual([music.id for music in matches], ["11663"])
        self.assertEqual(music_info.mai.total_list.by_id("1663").id, "11663")

    def test_render_music_list_keeps_standard_dx_pair_with_chart_specific_ids(self) -> None:
        music_info.mai._loaded = False
        music_info.mai._ensure_loaded()
        matches = [music for music in music_info.mai.total_list if music.title == "Selector"]

        self.assertEqual(
            [(music.id, music.type) for music in matches],
            [("574", "SD"), ("10574", "DX")],
        )
        self.assertEqual(music_info.mai.total_list.by_id("574").type, "SD")
        self.assertEqual(music_info.mai.total_list.by_id("10574").type, "DX")

    def test_render_music_prefers_divingfish_version_metadata(self) -> None:
        music = shim_music.music_from_search_song({
            "id": "1790",
            "title": "HYP3RTRIBE",
            "artist": "sky_delta vs KO3 vs Tanchiky",
            "genre": "ゲームバラエティ",
            "bpm": 190,
            "version": 25009,
            "is_new": False,
            "source_fields": {
                "divingfish": {
                    "version": "maimai でらっくす PRiSM",
                    "is_new": True,
                }
            },
            "matched_charts": [
                {
                    "chart_type": "dx",
                    "difficulty_index": 3,
                    "level": "14",
                    "ds": 14.2,
                    "charter": "Luxizhel",
                    "notes": {"tap": 673, "hold": 73, "slide": 76, "touch": 54, "break": 60},
                }
            ],
        })

        self.assertIsNotNone(music)
        assert music is not None
        self.assertEqual(music.basic_info.version, "maimai でらっくす PRiSM")
        self.assertTrue(music.basic_info.is_new)

    def test_rise_score_versions_use_cn_current_set(self) -> None:
        def make_music(music_id: str, version: str, is_new: bool) -> Music:
            return Music(
                id=music_id,
                title=f"song {music_id}",
                type="DX",
                ds=[14.0],
                level=["14"],
                cids=[],
                charts=[],
                basic_info=BasicInfo(
                    title=f"song {music_id}",
                    artist="artist",
                    genre="maimai",
                    bpm=190,
                    is_new=is_new,
                    **{"from": version},
                ),
                stats=[],
            )

        old_loaded = player_score.mai._loaded
        old_total_list = player_score.mai.total_list
        old_regions = player_score.mai.music_regions_by_id
        try:
            player_score.mai._loaded = True
            player_score.mai.total_list = MusicList([
                make_music("11790", "maimai でらっくす PRiSM", True),
                make_music("11469", "maimai でらっくす FESTiVAL", False),
                make_music("non-cn-song", "CiRCLE", True),
            ])
            player_score.mai.music_regions_by_id = {
                "11790": {"cn": True},
                "11469": {"cn": True},
                "non-cn-song": {"cn": False},
            }

            current = player_score._rise_score_candidate_versions("DX", [])
            legacy = player_score._rise_score_candidate_versions("SD", [])
        finally:
            player_score.mai._loaded = old_loaded
            player_score.mai.total_list = old_total_list
            player_score.mai.music_regions_by_id = old_regions

        self.assertEqual(current, ["maimai でらっくす PRiSM"])
        self.assertEqual(legacy, ["maimai でらっくす FESTiVAL"])

    def test_rise_score_candidates_prefer_overrated_fit_delta_buckets(self) -> None:
        def make_music(music_id: str, fit_diff: float | None) -> Music:
            stats = [Stats(fit_diff=fit_diff)] if fit_diff is not None else []
            return Music(
                id=music_id,
                title=f"song {music_id}",
                type="DX",
                ds=[12.0],
                level=["12"],
                cids=[],
                charts=[],
                basic_info=BasicInfo(
                    title=f"song {music_id}",
                    artist="artist",
                    genre="maimai",
                    bpm=190,
                    is_new=True,
                    **{"from": "maimai でらっくす PRiSM"},
                ),
                stats=stats,
            )

        old_loaded = player_score.mai._loaded
        old_total_list = player_score.mai.total_list
        old_regions = player_score.mai.music_regions_by_id
        try:
            player_score.mai._loaded = True
            player_score.mai.total_list = MusicList([
                make_music("101", 11.70),
                make_music("102", 11.75),
                make_music("103", 11.85),
                make_music("104", 12.00),
                make_music("105", 12.10),
                make_music("106", 12.30),
                make_music("107", None),
            ])
            player_score.mai.music_regions_by_id = {
                music.id: {"cn": True}
                for music in player_score.mai.total_list
            }

            result, low_score = player_score.get_rise_score_list(
                defaultdict(dict),
                "DX",
                [_rise_score_floor_record()],
                level="12",
                algorithm="legacy",
            )
        finally:
            player_score.mai._loaded = old_loaded
            player_score.mai.total_list = old_total_list
            player_score.mai.music_regions_by_id = old_regions

        self.assertEqual(low_score, 0)
        self.assertEqual([item.song_id for item in result], [105, 104, 103, 102, 101])

    def test_rise_score_candidates_use_underfit_bucket_when_needed(self) -> None:
        music = Music(
            id="201",
            title="song 201",
            type="DX",
            ds=[12.0],
            level=["12"],
            cids=[],
            charts=[],
            basic_info=BasicInfo(
                title="song 201",
                artist="artist",
                genre="maimai",
                bpm=190,
                is_new=True,
                **{"from": "maimai でらっくす PRiSM"},
            ),
            stats=[Stats(fit_diff=12.3)],
        )

        old_loaded = player_score.mai._loaded
        old_total_list = player_score.mai.total_list
        old_regions = player_score.mai.music_regions_by_id
        try:
            player_score.mai._loaded = True
            player_score.mai.total_list = MusicList([music])
            player_score.mai.music_regions_by_id = {"201": {"cn": True}}

            result, _low_score = player_score.get_rise_score_list(
                defaultdict(dict),
                "DX",
                [_rise_score_floor_record()],
                level="12",
            )
        finally:
            player_score.mai._loaded = old_loaded
            player_score.mai.total_list = old_total_list
            player_score.mai.music_regions_by_id = old_regions

        self.assertEqual([item.song_id for item in result], [201])

    def test_rise_score_candidates_fall_back_to_missing_fit_stats(self) -> None:
        music = Music(
            id="301",
            title="song 301",
            type="DX",
            ds=[12.0],
            level=["12"],
            cids=[],
            charts=[],
            basic_info=BasicInfo(
                title="song 301",
                artist="artist",
                genre="maimai",
                bpm=190,
                is_new=True,
                **{"from": "maimai でらっくす PRiSM"},
            ),
            stats=[],
        )

        old_loaded = player_score.mai._loaded
        old_total_list = player_score.mai.total_list
        old_regions = player_score.mai.music_regions_by_id
        try:
            player_score.mai._loaded = True
            player_score.mai.total_list = MusicList([music])
            player_score.mai.music_regions_by_id = {"301": {"cn": True}}

            result, low_score = player_score.get_rise_score_list(
                defaultdict(dict),
                "DX",
                [_rise_score_floor_record()],
                level="12",
            )
        finally:
            player_score.mai._loaded = old_loaded
            player_score.mai.total_list = old_total_list
            player_score.mai.music_regions_by_id = old_regions

        self.assertEqual(low_score, 0)
        self.assertEqual([item.song_id for item in result], [301])
        self.assertGreater(result[0].ra, low_score)

    def test_rise_score_empty_current_b15_uses_fallback_ability_with_zero_floor(self) -> None:
        low_music = Music(
            id="400",
            title="song 400",
            type="DX",
            ds=[10.0],
            level=["10"],
            cids=[],
            charts=[],
            basic_info=BasicInfo(
                title="song 400",
                artist="artist",
                genre="maimai",
                bpm=190,
                is_new=True,
                **{"from": "maimai でらっくす PRiSM"},
            ),
            stats=[Stats(fit_diff=10.0)],
        )
        high_music = Music(
            id="401",
            title="song 401",
            type="DX",
            ds=[12.0],
            level=["12"],
            cids=[],
            charts=[],
            basic_info=BasicInfo(
                title="song 401",
                artist="artist",
                genre="maimai",
                bpm=190,
                is_new=True,
                **{"from": "maimai でらっくす PRiSM"},
            ),
            stats=[Stats(fit_diff=12.0)],
        )

        old_loaded = player_score.mai._loaded
        old_total_list = player_score.mai.total_list
        old_regions = player_score.mai.music_regions_by_id
        try:
            player_score.mai._loaded = True
            player_score.mai.total_list = MusicList([low_music, high_music])
            player_score.mai.music_regions_by_id = {
                "400": {"cn": True},
                "401": {"cn": True},
            }

            result, low_score = player_score.get_rise_score_list(
                defaultdict(dict),
                "DX",
                [],
                fallback_info=[_rise_score_floor_record()],
            )
        finally:
            player_score.mai._loaded = old_loaded
            player_score.mai.total_list = old_total_list
            player_score.mai.music_regions_by_id = old_regions

        self.assertEqual(low_score, 0)
        self.assertEqual([item.song_id for item in result], [401])
        self.assertGreater(result[0].ra, 0)

    def test_rise_score_partial_current_b15_uses_higher_fallback_ability_floor(self) -> None:
        low_music = Music(
            id="403",
            title="song 403",
            type="DX",
            ds=[13.0],
            level=["13"],
            cids=[],
            charts=[],
            basic_info=BasicInfo(
                title="song 403",
                artist="artist",
                genre="maimai",
                bpm=190,
                is_new=True,
                **{"from": "maimai でらっくす PRiSM"},
            ),
            stats=[Stats(fit_diff=13.0)],
        )
        high_music = Music(
            id="404",
            title="song 404",
            type="DX",
            ds=[14.4],
            level=["14"],
            cids=[],
            charts=[],
            basic_info=BasicInfo(
                title="song 404",
                artist="artist",
                genre="maimai",
                bpm=190,
                is_new=True,
                **{"from": "maimai でらっくす PRiSM"},
            ),
            stats=[Stats(fit_diff=14.4)],
        )

        old_loaded = player_score.mai._loaded
        old_total_list = player_score.mai.total_list
        old_regions = player_score.mai.music_regions_by_id
        try:
            player_score.mai._loaded = True
            player_score.mai.total_list = MusicList([low_music, high_music])
            player_score.mai.music_regions_by_id = {
                "403": {"cn": True},
                "404": {"cn": True},
            }

            partial_b15 = [_rise_score_floor_record(9000 + index, 280) for index in range(6)]
            result, low_score = player_score.get_rise_score_list(
                defaultdict(dict),
                "DX",
                partial_b15,
                fallback_info=[_rise_score_floor_record(8000, 308)],
            )
        finally:
            player_score.mai._loaded = old_loaded
            player_score.mai.total_list = old_total_list
            player_score.mai.music_regions_by_id = old_regions

        self.assertEqual(low_score, 0)
        self.assertEqual([item.song_id for item in result], [404])

    def test_rise_score_expected_expands_window_to_fill_recommendations(self) -> None:
        music_items = [
            _rise_score_music("410", 13.8, fit_diff=13.8),
            _rise_score_music("411", 13.9, fit_diff=13.9),
            _rise_score_music("412", 14.0, fit_diff=14.0),
            _rise_score_music("413", 14.1, fit_diff=14.1),
            _rise_score_music("414", 14.2, fit_diff=14.2),
        ]

        old_loaded = player_score.mai._loaded
        old_total_list = player_score.mai.total_list
        old_regions = player_score.mai.music_regions_by_id
        try:
            player_score.mai._loaded = True
            player_score.mai.total_list = MusicList(music_items)
            player_score.mai.music_regions_by_id = {
                music.id: {"cn": True}
                for music in music_items
            }

            partial_b15 = [_rise_score_floor_record(9100 + index, 280) for index in range(6)]
            result, low_score = player_score.get_rise_score_list(
                defaultdict(dict),
                "DX",
                partial_b15,
                fallback_info=[_rise_score_floor_record(8100, 308)],
                algorithm="expected",
            )
        finally:
            player_score.mai._loaded = old_loaded
            player_score.mai.total_list = old_total_list
            player_score.mai.music_regions_by_id = old_regions

        self.assertEqual(low_score, 0)
        self.assertEqual(len(result), 5)
        self.assertEqual({item.song_id for item in result}, {410, 411, 412, 413, 414})

    def test_rise_score_expected_selection_uses_weighted_random_sampling(self) -> None:
        candidates = [
            (
                float(score),
                score,
                0,
                player_score.RiseScore(
                    song_id=song_id,
                    title=f"song {song_id}",
                    type="DX",
                    level_index=3,
                    ds=14.0,
                    ra=300 + score,
                    rate="SSS",
                    achievements=100.0,
                ),
            )
            for song_id, score in enumerate(range(10, 16), start=100)
        ]

        def choose_last(population, weights, k):
            return [population[-1]]

        with patch.object(player_score.random, "choices", side_effect=choose_last) as choices:
            result = player_score._weighted_sample_expected_rise_scores(candidates, limit=3)

        self.assertTrue(choices.called)
        self.assertEqual([item.song_id for item in result], [102, 101, 100])

    def test_rise_score_expected_selection_allows_same_song_different_difficulties(self) -> None:
        candidates = [
            (
                float(score),
                score,
                0,
                player_score.RiseScore(
                    song_id=song_id,
                    title=f"song {song_id}",
                    type="DX",
                    level_index=level_index,
                    ds=14.0,
                    ra=300 + score,
                    rate="SSS",
                    achievements=100.0,
                ),
            )
            for song_id, level_index, score in (
                (500, 4, 30),
                (500, 3, 20),
                (501, 3, 10),
                (502, 3, 5),
            )
        ]

        result = player_score._weighted_sample_expected_rise_scores(candidates, limit=5)

        self.assertEqual(
            [(item.song_id, item.level_index) for item in result],
            [(502, 3), (501, 3), (500, 4), (500, 3)],
        )

    def test_rise_score_expected_skips_unrealistic_high_targets(self) -> None:
        hard_candidate = _rise_score_music("510", 14.7, fit_diff=14.7)
        playable_candidate = _rise_score_music("511", 14.4, fit_diff=14.4)

        old_loaded = player_score.mai._loaded
        old_total_list = player_score.mai.total_list
        old_regions = player_score.mai.music_regions_by_id
        try:
            player_score.mai._loaded = True
            player_score.mai.total_list = MusicList([hard_candidate, playable_candidate])
            player_score.mai.music_regions_by_id = {
                "510": {"cn": True},
                "511": {"cn": True},
            }
            high_sssp = _rise_score_floor_record(8200, 308)
            high_sssp.ds = 14.0
            high_sssp.achievements = 100.5
            high_sss = _rise_score_floor_record(8201, 306)
            high_sss.ds = 14.1
            high_sss.achievements = 100.0
            high_ssp = _rise_score_floor_record(8202, 304)
            high_ssp.ds = 14.2
            high_ssp.achievements = 99.5

            result, low_score = player_score.get_rise_score_list(
                defaultdict(dict),
                "DX",
                [_rise_score_floor_record(9100 + index, 280) for index in range(6)],
                fallback_info=[_rise_score_floor_record(8100, 308)],
                all_records=[high_sssp, high_sss, high_ssp],
                algorithm="expected",
            )
        finally:
            player_score.mai._loaded = old_loaded
            player_score.mai.total_list = old_total_list
            player_score.mai.music_regions_by_id = old_regions

        self.assertEqual(low_score, 0)
        self.assertEqual(
            [(item.song_id, item.achievements) for item in result],
            [(511, 100.0), (510, 99.0)],
        )

    def test_rise_score_partial_b15_uses_zero_floor_until_section_is_full(self) -> None:
        music = Music(
            id="402",
            title="song 402",
            type="DX",
            ds=[12.0],
            level=["12"],
            cids=[],
            charts=[],
            basic_info=BasicInfo(
                title="song 402",
                artist="artist",
                genre="maimai",
                bpm=190,
                is_new=True,
                **{"from": "maimai でらっくす PRiSM"},
            ),
            stats=[Stats(fit_diff=12.0)],
        )

        old_loaded = player_score.mai._loaded
        old_total_list = player_score.mai.total_list
        old_regions = player_score.mai.music_regions_by_id
        try:
            player_score.mai._loaded = True
            player_score.mai.total_list = MusicList([music])
            player_score.mai.music_regions_by_id = {"402": {"cn": True}}

            for size, expected_floor in ((1, 0), (3, 0), (14, 0), (15, 250)):
                with self.subTest(size=size):
                    info = [_rise_score_floor_record(9000 + index, 250) for index in range(size)]
                    result, low_score = player_score.get_rise_score_list(
                        defaultdict(dict),
                        "DX",
                        info,
                        level="12",
                        algorithm="legacy",
                    )
                    self.assertEqual(low_score, expected_floor)
                    self.assertEqual([item.song_id for item in result], [402])
        finally:
            player_score.mai._loaded = old_loaded
            player_score.mai.total_list = old_total_list
            player_score.mai.music_regions_by_id = old_regions

    def test_render_b50_yuzu_treats_none_bonus_markers_and_dx_offset_ids(self) -> None:
        from unittest.mock import patch

        chart = {
            "songId": 8,
            "title": "True Love Song",
            "type": "SD",
            "level": "12",
            "levelLabel": "Master",
            "levelIndex": 3,
            "ds": 12.4,
            "achievements": 100.0,
            "dxScore": 100,
            "fc": None,
            "fs": "None",
            "ra": 250,
            "rate": "sss",
        }
        dx_offset_chart = dict(chart)
        dx_offset_chart.update({"songId": 11475, "title": "SUPER AMBULANCE"})
        b50 = {
            "player": {"nickname": "Tester", "rating": 15000, "additionalRating": 0},
            "charts": {"sd": [chart, dx_offset_chart], "dx": []},
        }
        query_calls: list[dict[str, object]] = []

        def fake_query_b50(arguments):
            query_calls.append(arguments)
            return b50

        old_output_dir = os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR")
        with tempfile.TemporaryDirectory() as tmpdir:
            try:
                os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = tmpdir
                with patch("diving_fish_b50_mcp.server.query_b50", side_effect=fake_query_b50):
                    with patch("maimaidx_render_mcp.server._schedule_b50_cache_enrichment") as schedule_enrich:
                        with warnings.catch_warnings():
                            warnings.simplefilter("ignore", ResourceWarning)
                            result = server._render_b50({"qq": "123456"})
                schedule_enrich.assert_called_once()
                self.assertEqual(schedule_enrich.call_args.args[0], "123456")
                self.assertIs(schedule_enrich.call_args.args[1], b50)
                self.assertEqual(schedule_enrich.call_args.kwargs, {"timeout_ms": 30000})
                self.assertFalse(result.get("isError"), result["content"][0]["text"])
                payload = json.loads(result["content"][0]["text"])
                self.assertTrue(os.path.exists(payload["imagePath"]))
                self.assertEqual(payload["mimeType"], "image/png")
                self.assertEqual(
                    query_calls,
                    [{"qq": "123456", "includeChartMetadata": False, "timeoutMs": 30000}],
                )
            finally:
                if old_output_dir is None:
                    os.environ.pop("MAIMAIDX_RENDER_OUTPUT_DIR", None)
                else:
                    os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = old_output_dir

    def test_render_music_score_resolves_music_and_saves_image(self) -> None:
        calls: list[tuple[int | None, str, str | None]] = []
        original_draw = server._draw_music_play_data
        old_output_dir = os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR")

        async def fake_draw(qqid: int | None, music_id: str, username: str | None = None, **_kwargs):
            calls.append((qqid, music_id, username))
            return image_to_base64(Image.new("RGBA", (8, 9), (20, 40, 60, 255)))

        with tempfile.TemporaryDirectory() as tmpdir:
            try:
                server._draw_music_play_data = fake_draw
                os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = tmpdir

                result = server._render_music_score({"qq": "123456", "music_id": "834"})
                self.assertFalse(result.get("isError"))
                payload = json.loads(result["content"][0]["text"])
                self.assertTrue(os.path.exists(payload["imagePath"]))
                self.assertEqual(payload["width"], 8)
                self.assertEqual(payload["height"], 9)
            finally:
                server._draw_music_play_data = original_draw
                if old_output_dir is None:
                    os.environ.pop("MAIMAIDX_RENDER_OUTPUT_DIR", None)
                else:
                    os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = old_output_dir

        self.assertEqual(calls, [(123456, "834", None)])

    def test_render_music_score_preserves_waterfish_dx_id_from_query(self) -> None:
        calls: list[tuple[int | None, str, str | None]] = []
        original_draw = server._draw_music_play_data
        old_output_dir = os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR")

        async def fake_draw(qqid: int | None, music_id: str, username: str | None = None, **_kwargs):
            calls.append((qqid, music_id, username))
            return image_to_base64(Image.new("RGBA", (8, 9), (20, 40, 60, 255)))

        with tempfile.TemporaryDirectory() as tmpdir:
            try:
                server._draw_music_play_data = fake_draw
                os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = tmpdir

                result = server._render_music_score({"qq": "123456", "query": "SUPER AMBULANCE"})
                self.assertFalse(result.get("isError"))
            finally:
                server._draw_music_play_data = original_draw
                if old_output_dir is None:
                    os.environ.pop("MAIMAIDX_RENDER_OUTPUT_DIR", None)
                else:
                    os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = old_output_dir

        self.assertEqual(calls, [(123456, "11475", None)])

    def test_render_music_score_auto_expands_standard_dx_song_and_reuses_records(self) -> None:
        draw_calls: list[tuple[int | None, str, str | None, str | None, list | None]] = []
        record_calls: list[dict] = []
        original_draw = server._draw_music_play_data
        original_query_user_plate = server.maiApi.query_user_plate
        original_load_cover = server._load_cover_for_interactive_render
        old_token = os.environ.get("DIVING_FISH_DEVELOPER_TOKEN")
        old_output_dir = os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR")

        async def fake_query_user_plate(**kwargs):
            record_calls.append(kwargs)
            return []

        async def fake_draw(qqid: int | None, music_id: str, username: str | None = None, **kwargs):
            music = kwargs.get("music")
            draw_calls.append((qqid, music_id, username, getattr(music, "type", None), kwargs.get("records")))
            return image_to_base64(Image.new("RGBA", (8, 9), (20, 40, 60, 255)))

        with tempfile.TemporaryDirectory() as tmpdir:
            try:
                os.environ.pop("DIVING_FISH_DEVELOPER_TOKEN", None)
                server.maiApi.query_user_plate = fake_query_user_plate
                server._draw_music_play_data = fake_draw
                server._load_cover_for_interactive_render = lambda *_args, **_kwargs: None
                os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = tmpdir

                result = server._render_music_score({"qq": "123456", "query": "Selector"})
            finally:
                server._draw_music_play_data = original_draw
                server.maiApi.query_user_plate = original_query_user_plate
                server._load_cover_for_interactive_render = original_load_cover
                if old_token is None:
                    os.environ.pop("DIVING_FISH_DEVELOPER_TOKEN", None)
                else:
                    os.environ["DIVING_FISH_DEVELOPER_TOKEN"] = old_token
                if old_output_dir is None:
                    os.environ.pop("MAIMAIDX_RENDER_OUTPUT_DIR", None)
                else:
                    os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = old_output_dir

        self.assertFalse(result.get("isError"), result["content"][0]["text"])
        payload = json.loads(result["content"][0]["text"])
        self.assertEqual([image["chartType"] for image in payload["images"]], ["ST", "DX"])
        self.assertEqual(len(record_calls), 1)
        self.assertEqual(record_calls[0]["qqid"], 123456)
        self.assertEqual(
            [(call[0], call[1], call[2], call[3], call[4]) for call in draw_calls],
            [(123456, "574", None, "SD", []), (123456, "10574", None, "DX", [])],
        )

    def test_render_music_score_preserves_explicit_waterfish_dx_id(self) -> None:
        calls: list[tuple[int | None, str, str | None]] = []
        original_draw = server._draw_music_play_data
        old_output_dir = os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR")

        async def fake_draw(qqid: int | None, music_id: str, username: str | None = None, **_kwargs):
            calls.append((qqid, music_id, username))
            return image_to_base64(Image.new("RGBA", (8, 9), (20, 40, 60, 255)))

        with tempfile.TemporaryDirectory() as tmpdir:
            try:
                server._draw_music_play_data = fake_draw
                os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = tmpdir

                result = server._render_music_score({"qq": "123456", "music_id": "11475"})
                self.assertFalse(result.get("isError"))
            finally:
                server._draw_music_play_data = original_draw
                if old_output_dir is None:
                    os.environ.pop("MAIMAIDX_RENDER_OUTPUT_DIR", None)
                else:
                    os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = old_output_dir

        self.assertEqual(calls, [(123456, "11475", None)])

    def test_render_music_global_stats_uses_difficulty_alias(self) -> None:
        calls: list[tuple[str, int]] = []
        original_draw = server._music_global_data
        old_output_dir = os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR")

        async def fake_draw(music, level_index: int):
            calls.append((str(music.id), level_index))
            return image_to_base64(Image.new("RGBA", (10, 11), (20, 40, 60, 255)))

        with tempfile.TemporaryDirectory() as tmpdir:
            try:
                server._music_global_data = fake_draw
                os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = tmpdir

                result = server._render_music_global_stats(
                    {"music_id": "834", "difficulty": "紫"}
                )
                self.assertFalse(result.get("isError"))
                payload = json.loads(result["content"][0]["text"])
                self.assertTrue(os.path.exists(payload["imagePath"]))
                self.assertEqual(payload["width"], 10)
                self.assertEqual(payload["height"], 11)
            finally:
                server._music_global_data = original_draw
                if old_output_dir is None:
                    os.environ.pop("MAIMAIDX_RENDER_OUTPUT_DIR", None)
                else:
                    os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = old_output_dir

        self.assertEqual(calls, [("834", 3)])

    def test_render_rating_ranking_converts_text_result_to_image(self) -> None:
        original_draw = server._rating_ranking_data
        old_output_dir = os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR")

        async def fake_draw(name: str, page: int):
            return f"玩家 {name} 在查分器已注册用户ra排行第1"

        with tempfile.TemporaryDirectory() as tmpdir:
            try:
                server._rating_ranking_data = fake_draw
                os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = tmpdir

                result = server._render_rating_ranking({"username": "tester"})
                self.assertFalse(result.get("isError"))
                payload = json.loads(result["content"][0]["text"])
                self.assertTrue(os.path.exists(payload["imagePath"]))
                self.assertGreater(payload["width"], 1)
                self.assertGreater(payload["height"], 1)
            finally:
                server._rating_ranking_data = original_draw
                if old_output_dir is None:
                    os.environ.pop("MAIMAIDX_RENDER_OUTPUT_DIR", None)
                else:
                    os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = old_output_dir

    def test_render_rating_ranking_range_converts_rank_slice_to_image(self) -> None:
        original_api = server.maiApi
        old_output_dir = os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR")

        class FakeRanking:
            def __init__(self, username: str, ra: int):
                self.username = username
                self.ra = ra

        class FakeMaiApi:
            async def rating_ranking(self):
                return [FakeRanking(f"user{i}", 16000 - i) for i in range(1, 61)]

        with tempfile.TemporaryDirectory() as tmpdir:
            try:
                server.maiApi = FakeMaiApi()
                os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = tmpdir

                result = server._render_rating_ranking({"startRank": 31, "endRank": 60})
                self.assertFalse(result.get("isError"))
                payload = json.loads(result["content"][0]["text"])
                self.assertTrue(os.path.exists(payload["imagePath"]))
                self.assertGreater(payload["width"], 1)
                self.assertGreater(payload["height"], 1)
            finally:
                server.maiApi = original_api
                if old_output_dir is None:
                    os.environ.pop("MAIMAIDX_RENDER_OUTPUT_DIR", None)
                else:
                    os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = old_output_dir

    def test_render_rating_ranking_range_rejects_more_than_thirty(self) -> None:
        result = server._render_rating_ranking({"startRank": 1, "endRank": 31})
        self.assertTrue(result.get("isError"))
        self.assertIn("最多输出 30 人", result["content"][0]["text"])

    def test_rating_ranking_shim_sorts_waterfish_public_data_by_ra(self) -> None:
        import diving_fish_b50_mcp.server as b50_server

        original_call_api = b50_server.call_diving_fish_api

        def fake_call_api(arguments):
            self.assertEqual(arguments["operation"], "maimai_rating_ranking_get")
            return {
                "data": [
                    {"username": "middle", "ra": 15000},
                    {"username": "low", "rating": 12000},
                    {"username": "tieB", "ra": 16000},
                    {"name": "tieA", "rating": "16000"},
                    {"username": "bad", "ra": "not-a-number"},
                    {"username": "high", "ra": 17000},
                ]
            }

        try:
            b50_server.call_diving_fish_api = fake_call_api

            result = asyncio.run(shim_mai_api.maiApi.rating_ranking())
        finally:
            b50_server.call_diving_fish_api = original_call_api

        self.assertEqual(
            [(ranker.username, ranker.ra) for ranker in result],
            [
                ("high", 17000),
                ("tieA", 16000),
                ("tieB", 16000),
                ("middle", 15000),
                ("low", 12000),
            ],
        )

    def test_render_plate_accepts_username(self) -> None:
        calls: list[tuple[int | None, str | None, str, str, str]] = []
        original_draw = server._draw_plate_table
        old_output_dir = os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR")

        async def fake_draw(qqid, version, plan, server="cn", username=None, records=None):
            del records
            calls.append((qqid, username, version, plan, server))
            return image_to_base64(Image.new("RGBA", (12, 13), (20, 40, 60, 255)))

        with tempfile.TemporaryDirectory() as tmpdir:
            try:
                server._draw_plate_table = fake_draw
                os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = tmpdir

                result = server._render_plate({"username": "123456", "version": "桃", "plan": "极"})
                self.assertFalse(result.get("isError"))
                payload = json.loads(result["content"][0]["text"])
                self.assertTrue(os.path.exists(payload["imagePath"]))
            finally:
                server._draw_plate_table = original_draw
                if old_output_dir is None:
                    os.environ.pop("MAIMAIDX_RENDER_OUTPUT_DIR", None)
                else:
                    os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = old_output_dir

        self.assertEqual(calls, [(None, "123456", "桃", "极", "cn")])

    def test_render_music_info_accepts_username(self) -> None:
        calls: list[tuple[int | None, str | None]] = []
        original_resolve = server._resolve_music_variants
        original_draw = server._draw_music_info
        old_output_dir = os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR")

        async def fake_draw(music, qqid=None, username=None, user=None, cover_image=None, display_id=None):
            del music, user, cover_image, display_id
            calls.append((qqid, username))
            return image_to_base64(Image.new("RGBA", (12, 13), (20, 40, 60, 255)))

        try:
            server._resolve_music_variants = lambda args: [
                (
                    None,
                    SimpleNamespace(id="296", type="SD", title="Test Song"),
                    {"id": "296", "title": "Test Song"},
                )
            ]
            server._draw_music_info = fake_draw
            with tempfile.TemporaryDirectory() as tmpdir:
                os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = tmpdir

                result = server._render_music_info({"query": "Test Song", "username": "123456"})
                self.assertFalse(result.get("isError"))
                payload = json.loads(result["content"][0]["text"])
                self.assertTrue(os.path.exists(payload["imagePath"]))
        finally:
            server._resolve_music_variants = original_resolve
            server._draw_music_info = original_draw
            if old_output_dir is None:
                os.environ.pop("MAIMAIDX_RENDER_OUTPUT_DIR", None)
            else:
                os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = old_output_dir

        self.assertEqual(calls, [(None, "123456")])

    def test_render_score_list_keeps_integer_level_as_level_string(self) -> None:
        calls: list[tuple[int | None, str | None, str | float, int]] = []
        original_draw = server._level_achievement_list_data
        old_output_dir = os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR")

        async def fake_draw(qqid, username, rating, page=1):
            calls.append((qqid, username, rating, page))
            return image_to_base64(Image.new("RGBA", (12, 13), (20, 40, 60, 255)))

        with tempfile.TemporaryDirectory() as tmpdir:
            try:
                server._level_achievement_list_data = fake_draw
                os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = tmpdir

                result = server._render_score_list({"qq": "123456", "level": "14"})
                self.assertFalse(result.get("isError"))
            finally:
                server._level_achievement_list_data = original_draw
                if old_output_dir is None:
                    os.environ.pop("MAIMAIDX_RENDER_OUTPUT_DIR", None)
                else:
                    os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = old_output_dir

        self.assertEqual(calls, [(123456, None, "14", 1)])

    def test_plate_batch_fetches_records_once(self) -> None:
        calls: list[dict[str, object]] = []
        draw_calls: list[tuple[str, str, str, int]] = []
        original_query_user_plate = server.maiApi.query_user_plate
        original_draw_plate = server._draw_plate_table
        old_output_dir = os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR")

        async def fake_query_user_plate(**kwargs):
            calls.append(kwargs)
            return []

        async def fake_draw_plate(qqid, version, plan, server="cn", records=None):
            draw_calls.append((version, plan, server, len(records or [])))
            return image_to_base64(Image.new("RGBA", (12, 13), (20, 40, 60, 255)))

        with tempfile.TemporaryDirectory() as tmpdir:
            try:
                server.maiApi.query_user_plate = fake_query_user_plate
                server._draw_plate_table = fake_draw_plate
                os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = tmpdir

                result = server._render_plate_batch({
                    "qq": "123456",
                    "items": [
                        {"version": "真", "plan": "极"},
                        {"version": "熊", "plan": "将", "server": "jp"},
                    ],
                })
            finally:
                server.maiApi.query_user_plate = original_query_user_plate
                server._draw_plate_table = original_draw_plate
                if old_output_dir is None:
                    os.environ.pop("MAIMAIDX_RENDER_OUTPUT_DIR", None)
                else:
                    os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = old_output_dir

        self.assertFalse(result.get("isError"))
        payload = json.loads(result["content"][0]["text"])
        self.assertEqual(len(payload["images"]), 1)
        self.assertEqual(len(payload["errors"]), 1)
        self.assertIn("不支持日服/dxdata", payload["errors"][0]["message"])
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0]["qqid"], 123456)
        self.assertEqual([call[:2] for call in draw_calls], [("真", "极")])

    def test_custom_plate_song_list_uses_divingfish_metadata(self) -> None:
        old_custom_path = os.environ.get("MAIMAIDX_CUSTOM_PLATE_PATH")
        with tempfile.TemporaryDirectory() as tmpdir:
            custom_path = os.path.join(tmpdir, "custom_plates.json")
            with open(custom_path, "w", encoding="utf-8") as fp:
                json.dump({"content": {"自定牌": [8]}}, fp, ensure_ascii=False)
            try:
                os.environ["MAIMAIDX_CUSTOM_PLATE_PATH"] = custom_path
                songs = shim_music.get_custom_plate_songs("自定牌")
            finally:
                if old_custom_path is None:
                    os.environ.pop("MAIMAIDX_CUSTOM_PLATE_PATH", None)
                else:
                    os.environ["MAIMAIDX_CUSTOM_PLATE_PATH"] = old_custom_path

        self.assertEqual(len(songs), 1)
        self.assertEqual(songs[0]["song_id"], 8)
        self.assertTrue(songs[0]["title"])
        self.assertGreaterEqual(len(songs[0]["level_values"]), 4)

    def test_plate_batch_auto_falls_back_to_custom_plate(self) -> None:
        calls: list[dict[str, object]] = []
        draw_calls: list[tuple[str, str, str, int]] = []
        original_query_user_plate = server.maiApi.query_user_plate
        original_draw_plate = server._draw_plate_table
        old_output_dir = os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR")
        old_custom_path = os.environ.get("MAIMAIDX_CUSTOM_PLATE_PATH")

        async def fake_query_user_plate(**kwargs):
            calls.append(kwargs)
            return []

        async def fake_draw_plate(qqid, version, plan, server="cn", records=None):
            draw_calls.append((version, plan, server, len(records or [])))
            return image_to_base64(Image.new("RGBA", (12, 13), (20, 40, 60, 255)))

        with tempfile.TemporaryDirectory() as tmpdir:
            custom_path = os.path.join(tmpdir, "custom_plates.json")
            with open(custom_path, "w", encoding="utf-8") as fp:
                json.dump({"content": {"自定牌": [8]}}, fp, ensure_ascii=False)
            try:
                server.maiApi.query_user_plate = fake_query_user_plate
                server._draw_plate_table = fake_draw_plate
                os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = tmpdir
                os.environ["MAIMAIDX_CUSTOM_PLATE_PATH"] = custom_path

                result = server._render_plate_batch({
                    "qq": "123456",
                    "items": [
                        {"version": "自定牌", "plan": "极"},
                        {"version": "另一牌", "plan": "将", "server": "自定义"},
                    ],
                })
            finally:
                server.maiApi.query_user_plate = original_query_user_plate
                server._draw_plate_table = original_draw_plate
                if old_output_dir is None:
                    os.environ.pop("MAIMAIDX_RENDER_OUTPUT_DIR", None)
                else:
                    os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = old_output_dir
                if old_custom_path is None:
                    os.environ.pop("MAIMAIDX_CUSTOM_PLATE_PATH", None)
                else:
                    os.environ["MAIMAIDX_CUSTOM_PLATE_PATH"] = old_custom_path

        self.assertFalse(result.get("isError"))
        payload = json.loads(result["content"][0]["text"])
        self.assertEqual(len(payload["images"]), 2)
        self.assertEqual(len(calls), 1)
        self.assertEqual(draw_calls, [
            ("自定牌", "极", "custom", 0),
            ("另一牌", "将", "custom", 0),
        ])

    def test_music_info_batch_fetches_b50_once(self) -> None:
        b50_calls: list[tuple[int, bool]] = []
        draw_users: list[object] = []
        original_query_user_b50 = server.maiApi.query_user_b50
        original_resolve_music_variants = server._resolve_music_variants
        original_draw_music_info = server._draw_music_info
        old_output_dir = os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR")
        fake_user = object()

        class FakeMusic:
            def __init__(self, music_id: str, title: str):
                self.id = music_id
                self.title = title
                self.type = "DX"

        async def fake_query_user_b50(qqid, username=None, include_chart_metadata=True):
            del username
            b50_calls.append((qqid, include_chart_metadata))
            return fake_user

        def fake_resolve_music_variants(args):
            query = args.get("query")
            music_id = "1001" if query == "Axeria" else "1002"
            return [(None, FakeMusic(music_id, str(query)), {"id": music_id, "title": str(query)})]

        async def fake_draw_music_info(music, qqid=None, username=None, user=None, cover_image=None, display_id=None):
            del username
            draw_users.append(user)
            return image_to_base64(Image.new("RGBA", (12, 13), (20, 40, 60, 255)))

        with tempfile.TemporaryDirectory() as tmpdir:
            try:
                server.maiApi.query_user_b50 = fake_query_user_b50
                server._resolve_music_variants = fake_resolve_music_variants
                server._draw_music_info = fake_draw_music_info
                os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = tmpdir

                result = server._render_music_info_batch({
                    "qq": "123456",
                    "queries": ["Axeria", "Xaleid"],
                })
            finally:
                server.maiApi.query_user_b50 = original_query_user_b50
                server._resolve_music_variants = original_resolve_music_variants
                server._draw_music_info = original_draw_music_info
                if old_output_dir is None:
                    os.environ.pop("MAIMAIDX_RENDER_OUTPUT_DIR", None)
                else:
                    os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = old_output_dir

        self.assertFalse(result.get("isError"))
        payload = json.loads(result["content"][0]["text"])
        self.assertEqual(len(payload["images"]), 2)
        self.assertEqual(b50_calls, [(123456, False)])
        self.assertEqual(draw_users, [fake_user, fake_user])


if __name__ == "__main__":
    unittest.main()
