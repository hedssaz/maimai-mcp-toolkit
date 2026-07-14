from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from deploy.astrbot.plugins.astrbot_plugin_maimai_auto_send_images.image_paths import (
    collect_paths_from_mapping,
    collect_paths_from_text,
    is_maimai_subagent_tool_name,
    is_render_tool_name,
    parse_prefix_mappings,
    strip_paths_from_text,
    unique_existing_paths,
)


class AstrBotAutoSendImageHelpersTest(unittest.TestCase):
    def test_render_tool_matching(self) -> None:
        self.assertTrue(is_render_tool_name("render_maimai_b50"))
        self.assertTrue(is_render_tool_name("render_b50_image"))
        self.assertFalse(is_render_tool_name("query_b50"))
        self.assertTrue(is_render_tool_name("custom_draw", ["custom_.*"]))

    def test_maimai_subagent_tool_matching(self) -> None:
        self.assertTrue(is_maimai_subagent_tool_name("transfer_to_maimai"))
        self.assertTrue(is_maimai_subagent_tool_name("handoff_to_maimai_worker"))
        self.assertFalse(is_maimai_subagent_tool_name("transfer_to_other"))

    def test_collect_paths_from_structured_content(self) -> None:
        payload = {
            "imagePath": "/AstrBot/data/maimai-images/a.png",
            "images": [{"imagePath": "/AstrBot/data/maimai-images/b.png"}],
            "other": {"file_path": "/AstrBot/data/maimai-images/c.jpg"},
        }
        self.assertEqual(
            collect_paths_from_mapping(payload),
            [
                "/AstrBot/data/maimai-images/a.png",
                "/AstrBot/data/maimai-images/b.png",
                "/AstrBot/data/maimai-images/c.jpg",
            ],
        )

    def test_collect_paths_from_text_and_json(self) -> None:
        text = '图片: /AstrBot/data/maimai-images/a.png\n{"imagePath": "/AstrBot/data/b50-images/b.png"}'
        paths = collect_paths_from_text(text)
        self.assertIn("/AstrBot/data/maimai-images/a.png", paths)
        self.assertIn("/AstrBot/data/b50-images/b.png", paths)

    def test_unique_existing_paths_with_mapping(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            image = root / "a.png"
            image.write_bytes(b"\x89PNG\r\n\x1a\n")
            mappings = parse_prefix_mappings("/AstrBot/data=>%s" % tmp)
            self.assertEqual(
                unique_existing_paths(["/AstrBot/data/a.png", str(image)], mappings),
                [str(image)],
            )

    def test_strip_sent_paths_from_text(self) -> None:
        path = "/AstrBot/data/maimai-images/a.png"
        text = f"B50 图片已生成。\n图片: {path}\n请查看上面的图。"
        self.assertEqual(strip_paths_from_text(text, [path]), "B50 图片已生成。\n请查看上面的图。")


if __name__ == "__main__":
    unittest.main()
