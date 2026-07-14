from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path

from PIL import Image

from maimaidx_render_mcp.output import image_output_context, image_path_payload, next_image_path


class RenderOutputPathTests(unittest.TestCase):
    def test_next_image_path_uses_unique_paths_for_rapid_outputs(self) -> None:
        previous_output_dir = os.environ.get("MAIMAIDX_RENDER_OUTPUT_DIR")
        with tempfile.TemporaryDirectory() as temp_dir:
            os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = temp_dir
            try:
                first = next_image_path("plate_jp")
                second = next_image_path("plate_jp")
            finally:
                if previous_output_dir is None:
                    os.environ.pop("MAIMAIDX_RENDER_OUTPUT_DIR", None)
                else:
                    os.environ["MAIMAIDX_RENDER_OUTPUT_DIR"] = previous_output_dir

        self.assertNotEqual(first, second)
        self.assertEqual(first.parent, second.parent)
        self.assertTrue(first.name.startswith("plate_jp_"))
        self.assertTrue(second.name.startswith("plate_jp_"))
        self.assertEqual(first.suffix, ".png")
        self.assertEqual(second.suffix, ".png")

    def test_output_context_uses_stem_counter_and_continues_existing_files(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            with image_output_context("sample_player", output_dir=temp_dir):
                first = next_image_path("music_info")
                second = next_image_path("music_info")
                first.touch()
                second.touch()

            with image_output_context("sample_player", output_dir=temp_dir):
                third = next_image_path("music_score")

        self.assertEqual(first.name, "sample_player_000.png")
        self.assertEqual(second.name, "sample_player_001.png")
        self.assertEqual(third.name, "sample_player_002.png")

    def test_image_path_payload_includes_original_dimensions(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = os.path.join(temp_dir, "plate.png")
            Image.new("RGBA", (1400, 2800), (255, 255, 255, 255)).save(path)

            payload = image_path_payload(next_image_path("unused", output_dir=temp_dir), image=Image.new("RGBA", (1, 2)))
            file_payload = image_path_payload(Path(path))

        self.assertEqual(payload["mimeType"], "image/png")
        self.assertEqual(payload["width"], 1)
        self.assertEqual(payload["height"], 2)
        self.assertEqual(file_payload["width"], 1400)
        self.assertEqual(file_payload["height"], 2800)


if __name__ == "__main__":
    unittest.main()
