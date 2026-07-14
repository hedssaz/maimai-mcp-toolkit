from __future__ import annotations

import json
import os
import tempfile
import unittest
import asyncio

from PIL import Image

import maimaidx_render_mcp.server as server
from maimaidx_render_mcp.maimaidx import maimaidx_music_info as music_info
from maimaidx_render_mcp.maimaidx.image import image_to_base64
from maimaidx_render_mcp.maimaidx.maimaidx_model import BasicInfo, Music, PlayInfoDefault
from maimaidx_render_mcp.shim.mai_music import music_from_search_song


class MaimaidxRenderMusicInfoTests(unittest.TestCase):
    def test_music_info_level_ds_text_keeps_decimal_constants(self) -> None:
        self.assertEqual(music_info._level_ds_text("5", 5.0), "5(5.0)")
        self.assertEqual(music_info._level_ds_text("8", 8), "8(8.0)")
        self.assertEqual(music_info._level_ds_text("12", 12.4), "12(12.4)")
        self.assertEqual(music_info._level_ds_text("", 14.0), "14.0")

    def test_search_result_preserves_known_total_when_note_parts_are_unknown(self) -> None:
        music = music_from_search_song({
            "id": "1820",
            "title": "Xaleid◆scopiX",
            "artist": "xi",
            "genre": "舞萌",
            "bpm": 180,
            "version": "PRiSM PLUS",
            "matched_charts": [
                {
                    "chart_type": "dx",
                    "difficulty_index": 0,
                    "level": "7+",
                    "ds": 7.9,
                    "charter": "",
                    "notes": {
                        "tap": None,
                        "hold": None,
                        "slide": None,
                        "touch": None,
                        "break": None,
                        "total": 600,
                    },
                }
            ],
        })

        self.assertIsNotNone(music)
        assert music is not None
        self.assertEqual(music.charts[0].total_notes, 600)
        self.assertEqual(list(music.charts[0].notes), [None, None, None, None])
        self.assertEqual(music_info._note_display_values(music.charts[0]), ("600", ["", "", "", ""]))

    def test_render_music_prefers_detailed_notes_from_secondary_cn_source(self) -> None:
        song = {
            "id": "1809",
            "title": "Antinomie",
            "artist": "test",
            "matched_charts": [
                {
                    "source": "cn",
                    "chart_type": "dx",
                    "difficulty_index": 0,
                    "difficulty": "Master",
                    "level": "14+",
                    "ds": 14.6,
                    "notes": {
                        "tap": None,
                        "hold": None,
                        "slide": None,
                        "touch": None,
                        "break": None,
                        "total": 1039,
                    },
                    "charter": "SAFARi☆CAT",
                },
                {
                    "source": "divingfish",
                    "chart_type": "dx",
                    "difficulty_index": 0,
                    "difficulty": "Master",
                    "level": "14+",
                    "ds": 14.5,
                    "notes": {"tap": 631, "hold": 98, "slide": 114, "touch": 72, "break": 124, "total": 1039},
                    "charter": "SAFARI☆CAT",
                },
            ],
        }

        music = music_from_search_song(song)

        self.assertIsNotNone(music)
        assert music is not None
        self.assertEqual(music.ds[0], 14.6)
        self.assertEqual(music.charts[0].total_notes, 1039)
        self.assertEqual(list(music.charts[0].notes), [631, 98, 114, 72, 124])

    def test_partial_music_info_draws_with_only_cover_and_id(self) -> None:
        music = Music(
            id="999",
            title="",
            type="",
            ds=[],
            level=[],
            cids=[],
            charts=[],
            basic_info=BasicInfo.model_validate({
                "title": "",
                "artist": "",
                "genre": "",
                "bpm": 0,
                "from": "",
                "is_new": False,
            }),
            stats=[],
        )

        image = asyncio.run(
            music_info.draw_music_info(
                music,
                cover_image=Image.new("RGBA", (300, 300), (20, 40, 60, 255)),
                display_id="999",
            )
        )

        self.assertIsInstance(image, str)
        self.assertGreater(len(image), 100)

    def test_music_info_respects_song_type_for_merged_standard_dx_song(self) -> None:
        standard, _ = server._resolve_music({"query": "Selector", "songType": "standard"})
        dx, _ = server._resolve_music({"query": "Selector", "songType": "dx"})

        self.assertEqual(standard.title, "Selector")
        self.assertEqual(standard.type, "SD")
        self.assertEqual(standard.level[3], "13+")
        self.assertEqual(standard.ds[3], 13.8)
        self.assertEqual(dx.title, "Selector")
        self.assertEqual(dx.type, "DX")
        self.assertEqual(dx.level[3], "13+")
        self.assertEqual(dx.ds[3], 13.9)

    def test_music_info_exact_match_wins_before_query_type_inference(self) -> None:
        for query in ("MEGATON BLAST", "百万吨爆炸"):
            with self.subTest(query=query):
                music, search_song = server._resolve_music({"query": query})

                self.assertEqual(music.title, "MEGATON BLAST")
                self.assertEqual(music.type, "DX")
                self.assertEqual(search_song["id"], "1248")

    def test_music_info_explicit_song_type_remains_strong_for_exact_query(self) -> None:
        music, _ = server._resolve_music({"query": "Believe the Rainbow", "songType": "standard"})

        self.assertEqual(music.title, "Believe the Rainbow")
        self.assertEqual(music.type, "SD")

    def test_music_info_infers_dx_song_type_from_query_alias(self) -> None:
        music, _ = server._resolve_music({"query": "dxSelector"})

        self.assertEqual(music.title, "Selector")
        self.assertEqual(music.type, "DX")
        self.assertEqual(music.ds[3], 13.9)

    def test_music_info_auto_expands_standard_dx_song_without_song_type(self) -> None:
        variants = server._resolve_music_variants({"query": "Selector"})

        self.assertEqual([label for label, _music, _song in variants], ["standard", "dx"])
        self.assertEqual([music.type for _label, music, _song in variants], ["SD", "DX"])

    def test_dual_chart_record_matching_keeps_standard_and_dx_scores_separate(self) -> None:
        standard_args = {"query": "Selector", "songType": "standard"}
        standard_music, standard_song = server._resolve_music(standard_args)
        dx_args = {"query": "Selector", "songType": "dx"}
        dx_music, dx_song = server._resolve_music(dx_args)
        standard_lookup_ids = music_info._score_lookup_ids(
            server._music_score_query_id(standard_args, standard_music, standard_song),
            standard_music,
        )
        dx_lookup_ids = music_info._score_lookup_ids(
            server._music_score_query_id(dx_args, dx_music, dx_song),
            dx_music,
        )
        standard_record = PlayInfoDefault(
            id=574,
            title="Selector",
            type="SD",
            level="13+",
            level_index=3,
            ds=13.8,
            achievements=97.1234,
            dxScore=111,
            fc="",
            fs="",
            ra=1,
            rate="s",
        )
        dx_record = PlayInfoDefault(
            id=10574,
            title="Selector",
            type="DX",
            level="13+",
            level_index=3,
            ds=13.9,
            achievements=100.9876,
            dxScore=222,
            fc="",
            fs="",
            ra=2,
            rate="sss",
        )

        self.assertTrue(music_info._record_matches_music(standard_record, standard_music, standard_lookup_ids))
        self.assertFalse(music_info._record_matches_music(dx_record, standard_music, standard_lookup_ids))
        self.assertFalse(music_info._record_matches_music(standard_record, dx_music, dx_lookup_ids))
        self.assertTrue(music_info._record_matches_music(dx_record, dx_music, dx_lookup_ids))

    def test_render_music_info_auto_expands_standard_dx_song(self) -> None:
        b50_calls: list[tuple[int, bool]] = []
        draw_calls: list[tuple[str, object | None, str | None]] = []
        original_query_user_b50 = server.maiApi.query_user_b50
        original_draw = server._draw_music_info
        original_load_cover = server._load_cover_for_interactive_render
        old_output_dir = os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR")
        fake_user = object()

        async def fake_query_user_b50(qqid, username=None, include_chart_metadata=True):
            del username
            b50_calls.append((qqid, include_chart_metadata))
            return fake_user

        async def fake_draw(music, qqid=None, username=None, user=None, cover_image=None, display_id=None):
            del qqid, username, cover_image
            draw_calls.append((music.type, user, display_id))
            return image_to_base64(Image.new("RGBA", (8, 9), (20, 40, 60, 255)))

        with tempfile.TemporaryDirectory() as tmpdir:
            try:
                server.maiApi.query_user_b50 = fake_query_user_b50
                server._draw_music_info = fake_draw
                server._load_cover_for_interactive_render = lambda *_args, **_kwargs: None
                os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = tmpdir

                result = server._render_music_info({"qq": "123456", "query": "Selector"})
            finally:
                server.maiApi.query_user_b50 = original_query_user_b50
                server._draw_music_info = original_draw
                server._load_cover_for_interactive_render = original_load_cover
                if old_output_dir is None:
                    os.environ.pop("MAIMAIDX_RENDER_OUTPUT_DIR", None)
                else:
                    os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = old_output_dir

        self.assertFalse(result.get("isError"))
        payload = json.loads(result["content"][0]["text"])
        self.assertEqual([image["chartType"] for image in payload["images"]], ["ST", "DX"])
        self.assertEqual(b50_calls, [(123456, False)])
        self.assertEqual(draw_calls, [("SD", fake_user, "574"), ("DX", fake_user, "10574")])

    def test_music_score_query_id_offsets_low_numeric_dx_ids(self) -> None:
        args = {"query": "Selector", "songType": "dx"}
        music, search_song = server._resolve_music(args)

        self.assertEqual(server._music_score_query_id(args, music, search_song), "10574")

    def test_music_score_query_id_uses_chart_specific_ids_for_dual_song(self) -> None:
        standard_args = {"query": "Selector", "songType": "standard"}
        standard_music, standard_song = server._resolve_music(standard_args)
        dx_args = {"query": "Selector", "songType": "dx"}
        dx_music, dx_song = server._resolve_music(dx_args)

        self.assertEqual(standard_music.type, "SD")
        self.assertEqual(dx_music.type, "DX")
        self.assertEqual(server._music_score_query_id(standard_args, standard_music, standard_song), "574")
        self.assertEqual(server._music_score_query_id(dx_args, dx_music, dx_song), "10574")

    def test_explicit_offset_id_can_resolve_dx_chart_when_search_has_only_base_id(self) -> None:
        music, search_song = server._resolve_music({"music_id": "10574"})

        self.assertEqual(music.title, "Selector")
        self.assertEqual(music.type, "DX")
        self.assertEqual(server._music_score_query_id({"music_id": "10574"}, music, search_song), "10574")

    def test_render_display_id_uses_numeric_score_id_only(self) -> None:
        dx_args = {"query": "Selector", "songType": "dx"}
        dx_music, dx_search_song = server._resolve_music(dx_args)
        string_music = Music(
            id="string-only",
            title="String Only",
            type="DX",
            ds=[],
            level=[],
            cids=[],
            charts=[],
            basic_info=BasicInfo.model_validate({
                "title": "String Only",
                "artist": "",
                "genre": "",
                "bpm": 0,
                "from": "",
                "is_new": False,
            }),
            stats=[],
        )

        self.assertEqual(server._music_display_id(dx_args, dx_music, dx_search_song), "10574")
        self.assertEqual(server._music_display_id({}, string_music, {"id": "string-only"}), "")

    def test_music_info_displays_internal_genre_keys_as_names(self) -> None:
        music, _search_song = server._resolve_music({"music_id": "203"})

        self.assertEqual(music.basic_info.genre, "touhou")
        self.assertEqual(music_info._display_genre(music.basic_info.genre), "東方Project")
        self.assertEqual(music_info._display_genre("game"), "ゲームバラエティ")
        self.assertEqual(music_info._display_genre("舞萌"), "舞萌")

    def test_multi_candidate_text_uses_dx_chart_id(self) -> None:
        standard_song = {
            "id": "203",
            "title": "Help me, ERINNNNNN!!（Band ver.）",
            "available_chart_types": ["standard"],
        }
        dx_song = {
            "id": "1853",
            "title": "Help me, ERINNNNNN!!",
            "available_chart_types": ["dx"],
            "matched_charts": [{"chart_type": "dx", "internal_id": 11853}],
        }
        dual_song = {
            "id": "835",
            "title": "Believe the Rainbow",
            "available_chart_types": ["standard", "dx"],
            "matched_charts": [
                {"chart_type": "standard", "internal_id": 835},
                {"chart_type": "dx", "internal_id": 10835},
            ],
        }

        self.assertEqual(server._format_search_song_id(standard_song), "203")
        self.assertEqual(server._format_search_song_id(dx_song), "11853")
        self.assertEqual(server._format_search_song_id(dual_song), "ST#835 / DX#10835")
        self.assertTrue(server._is_exact_song_query(dx_song, "11853"))

    def test_interactive_cover_loader_does_not_download_remote_covers(self) -> None:
        old_timeout = os.environ.get("MAIMAIDX_INTERACTIVE_COVER_TIMEOUT")
        old_attempts = os.environ.get("MAIMAIDX_INTERACTIVE_COVER_ATTEMPTS")

        try:
            os.environ["MAIMAIDX_INTERACTIVE_COVER_TIMEOUT"] = "1.5"
            os.environ["MAIMAIDX_INTERACTIVE_COVER_ATTEMPTS"] = "1"

            self.assertIsNone(server._load_cover_for_interactive_render("song", "image"))
        finally:
            if old_timeout is None:
                os.environ.pop("MAIMAIDX_INTERACTIVE_COVER_TIMEOUT", None)
            else:
                os.environ["MAIMAIDX_INTERACTIVE_COVER_TIMEOUT"] = old_timeout
            if old_attempts is None:
                os.environ.pop("MAIMAIDX_INTERACTIVE_COVER_ATTEMPTS", None)
            else:
                os.environ["MAIMAIDX_INTERACTIVE_COVER_ATTEMPTS"] = old_attempts


if __name__ == "__main__":
    unittest.main()
