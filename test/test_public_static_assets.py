from __future__ import annotations

import unittest
from pathlib import Path


STATIC_ROOT = Path(__file__).resolve().parents[1] / "maimaidx_render_mcp" / "static"

SNOWPEAK_REQUIRED_IMAGES = {
    "mai/pic/UI_CHR_PlayBonus_AP.png",
    "mai/pic/UI_CHR_PlayBonus_APp.png",
    "mai/pic/UI_CHR_PlayBonus_FC.png",
    "mai/pic/UI_CHR_PlayBonus_FCp.png",
    "mai/pic/UI_CHR_PlayBonus_FSD.png",
    "mai/pic/UI_CHR_PlayBonus_FSDp.png",
    "mai/pic/UI_TTR_Rank_SSS.png",
    "mai/pic/UI_TTR_Rank_SSSp.png",
    "mai/pic/complete_bg_2.png",
    "mai/pic/plate_num.png",
    "mai/pic/t-0.png",
    "mai/pic/t-1.png",
    "mai/pic/t-2.png",
    "mai/pic/t-3.png",
    "mai/pic/unfinished_bg_2.png",
    "mai/plate/custom_雪峰.png",
    "mai/plate/雪峰将.png",
    "mai/plate/雪峰極.png",
    "mai/plate/雪峰神.png",
    "mai/plate/雪峰舞舞.png",
}


class PublicStaticAssetsTests(unittest.TestCase):
    def test_snowpeak_plate_runtime_images_are_present(self) -> None:
        missing = sorted(
            relative_path
            for relative_path in SNOWPEAK_REQUIRED_IMAGES
            if not (STATIC_ROOT / relative_path).is_file()
        )
        self.assertEqual(missing, [])


if __name__ == "__main__":
    unittest.main()
