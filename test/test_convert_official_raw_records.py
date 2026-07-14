from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from scripts import convert_official_raw_records as converter


class ConvertOfficialRawRecordsTests(unittest.TestCase):
    def test_convert_records_prefers_divingfish_title_and_reports_skips(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            divingfish = root / "divingfish.json"
            divingfish.write_text(
                json.dumps(
                    [
                        {"id": "383", "title": "Link(CoF)", "type": "SD"},
                        {"id": "11823", "title": "Zitronectar", "type": "DX"},
                        {"id": "100508", "title": "[協]恋愛裁判", "type": "DX"},
                        {"id": "111355", "title": "[協]ラグトレイン", "type": "DX"},
                        {"id": "111597", "title": "[息]ノンブレス・オブリージュ", "type": "DX"},
                    ],
                    ensure_ascii=False,
                ),
                encoding="utf-8",
            )
            music_index = converter.build_music_index(divingfish_song_list=divingfish)
            raw = {
                "GetUserMusicApi": {
                    "userMusicList": [
                        {
                            "userMusicDetailList": [
                                {
                                    "musicId": 383,
                                    "level": 3,
                                    "achievement": 1012345,
                                    "deluxscoreMax": 2458,
                                    "comboStatus": 2,
                                    "syncStatus": 5,
                                },
                                {
                                    "musicId": 11823,
                                    "level": 4,
                                    "achievement": 1007770,
                                    "deluxscoreMax": "2711",
                                    "comboStatus": 4,
                                    "syncStatus": 4,
                                },
                                {
                                    "musicId": 111597,
                                    "level": 10,
                                    "achievement": 1535756,
                                    "deluxscoreMax": 2295,
                                    "comboStatus": 0,
                                    "syncStatus": 5,
                                },
                                {
                                    "musicId": 100508,
                                    "level": 0,
                                    "achievement": 1000000,
                                    "deluxscoreMax": 200,
                                    "comboStatus": 0,
                                    "syncStatus": 0,
                                },
                                {
                                    "musicId": 111355,
                                    "level": 0,
                                    "achievement": 1000000,
                                    "deluxscoreMax": 200,
                                    "comboStatus": 0,
                                    "syncStatus": 0,
                                },
                                {"musicId": 999999, "level": 3, "achievement": 1000000},
                            ]
                        }
                    ]
                }
            }

            payload, skipped = converter.convert_raw_records(raw, music_index=music_index)

        self.assertEqual(
            payload,
            [
                {
                    "achievements": 101.2345,
                    "dxScore": 2458,
                    "fc": "fcp",
                    "fs": "sync",
                    "level_index": 3,
                    "title": "Link(CoF)",
                    "type": "SD",
                },
                {
                    "achievements": 100.777,
                    "dxScore": 2711,
                    "fc": "app",
                    "fs": "fsdp",
                    "level_index": 4,
                    "title": "Zitronectar",
                    "type": "DX",
                },
                {
                    "achievements": 153.5756,
                    "dxScore": 2295,
                    "fc": "",
                    "fs": "sync",
                    "level_index": 0,
                    "title": "[息]ノンブレス・オブリージュ",
                    "type": "DX",
                },
                {
                    "achievements": 100.0,
                    "dxScore": 200,
                    "fc": "",
                    "fs": "",
                    "level_index": 0,
                    "title": "[協]恋愛裁判",
                    "type": "DX",
                },
                {
                    "achievements": 100.0,
                    "dxScore": 200,
                    "fc": "",
                    "fs": "",
                    "level_index": 0,
                    "title": "[協]ラグトレイン",
                    "type": "DX",
                },
            ],
        )
        self.assertEqual([item["reason"] for item in skipped], ["music_id_not_found"])


if __name__ == "__main__":
    unittest.main()
