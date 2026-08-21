use std::error::Error;

use serde_json::{Value, json};

use super::{FormatOptions, render};

#[test]
fn legacy_dual_chart_text_and_compact_match_goldens() -> Result<(), Box<dyn Error>> {
    let value = json!({
        "count": 1,
        "total_matches": 1,
        "criteria": {"query": "相信彩虹"},
        "songs": [dual_chart_song()],
    });
    assert_eq!(
        render("search_maimai_songs", &value, options(Some("compact")))?,
        include_str!("format_golden/legacy_dual_compact.txt").trim_end()
    );
    assert_eq!(
        render("search_maimai_songs", &value, options(Some("text")))?,
        include_str!("format_golden/legacy_dual_text.txt").trim_end()
    );
    Ok(())
}

#[test]
fn dx_only_uses_five_digit_display_id() -> Result<(), Box<dyn Error>> {
    let value = json!({
        "count": 1,
        "total_matches": 1,
        "criteria": {},
        "songs": [{
            "title": "Help me, ERINNNNNN!!",
            "id": "1853",
            "artist": "ビートまりお",
            "bpm": 183,
            "version": "PRiSM PLUS",
            "available_chart_types": ["dx"],
            "matched_charts": [{
                "chart_type": "dx",
                "difficulty": "Master",
                "difficulty_index": 3,
                "level": "13",
                "ds": 13.1,
            }],
        }],
    });
    assert_eq!(
        render("search_maimai_songs", &value, options(Some("compact")))?,
        include_str!("format_golden/legacy_dx_only.txt").trim_end()
    );
    Ok(())
}

#[test]
fn complete_song_value_preserves_all_user_visible_metadata() -> Result<(), Box<dyn Error>> {
    let value = json!({
        "count": 1,
        "total_matches": 4,
        "truncated": true,
        "criteria": {"query": "彩虹", "region_has": ["jp", "cn"]},
        "songs": [complete_song()],
    });
    assert_eq!(
        render("search_maimai_songs", &value, options(None))?,
        include_str!("format_golden/complete_song.txt").trim_end()
    );
    Ok(())
}

#[test]
fn versions_and_history_match_exact_legacy_text() -> Result<(), Box<dyn Error>> {
    let versions = json!({
        "count": 2,
        "total_matches": 3,
        "truncated": true,
        "criteria": {"query": "PR", "limit": 2},
        "latest_cn_versions": ["2025", "2024"],
        "latest_cn_years": ["25", "24"],
        "versions": [
            {"version": "PRiSM", "song_count": 10, "chart_count": 40, "sources": ["LXNS", "dxdata"]},
            {"version": null, "song_count": null, "chart_count": 0, "sources": []},
        ],
    });
    let history = json!({
        "songs": [{
            "title": "Alpha",
            "id": "10001",
            "artist": "Alice",
            "charts": [{
                "chart_type": "dx",
                "difficulty": "Master",
                "level": "13+",
                "current_ds": 13.0,
                "history": [
                    {"versions": ["BUDDiES", "BUDDiES PLUS"], "ds": 12.9},
                    {"versions": ["PRiSM"], "ds": 13.0},
                ],
            }],
        }],
    });
    let actual = format!(
        "{}\n---\n{}",
        render("list_maimai_versions", &versions, options(None))?,
        render("query_chart_history", &history, options(None))?
    );
    assert_eq!(
        actual,
        include_str!("format_golden/versions_history.txt").trim_end()
    );
    Ok(())
}

#[test]
fn song_and_batch_caps_are_rendered() -> Result<(), Box<dyn Error>> {
    let charts = (0..13)
        .map(|index| {
            json!({
                "chart_type": "dx",
                "difficulty": "Master",
                "difficulty_index": index,
                "level": "13",
                "ds": 13.0,
            })
        })
        .collect::<Vec<_>>();
    let songs = json!({
        "count": 1,
        "total_matches": 1,
        "criteria": {},
        "songs": [{"id": "1", "matched_charts": charts}],
    });
    let song_text = render("search_maimai_songs", &songs, options(None))?;
    assert!(song_text.ends_with("  ... 还有 1 张谱面未展开"));

    let items = (0..51)
        .map(|index| {
            json!({
                "index": index,
                "key": null,
                "ok": true,
                "result": {"count": 1, "total_matches": 2},
            })
        })
        .collect::<Vec<_>>();
    let batch = json!({
        "counts": {"requested": 51, "success": 51, "failure": 0},
        "items": items,
    });
    let batch_text = render("batch_search_maimai_songs", &batch, options(None))?;
    assert_eq!(batch_text.lines().count(), 52);
    assert!(batch_text.contains("- 49: 返回 1 / 2"));
    assert!(batch_text.ends_with("... 还有 1 项未展开"));
    Ok(())
}

#[test]
fn absent_values_use_legacy_dash_fallback() -> Result<(), Box<dyn Error>> {
    let value = json!({
        "count": 1,
        "total_matches": 1,
        "criteria": {"query": null, "levels": [], "filter": {}},
        "songs": [{"matched_charts": [{}]}],
    });
    let text = render("search_maimai_songs", &value, options(None))?;
    assert!(text.starts_with("搜索结果: 返回 1 / 1\n条件: -\n"));
    assert!(text.contains("1. - | 编号 - | - | 来源  | 地区 - | 版本 - | BPM -"));
    assert!(text.ends_with("- - - 等级 - 定数 - | 拟合 -, 差值 - | - | 谱师 -"));
    Ok(())
}

#[test]
fn random_list_and_today_keep_their_legacy_routes() -> Result<(), Box<dyn Error>> {
    let random = json!({
        "count": 0,
        "total_candidates": 0,
        "criteria": {},
        "songs": [],
    });
    assert_eq!(
        render("random_maimai_songs", &random, options(Some("compact")))?,
        "随机结果: 返回 0 / 0\n条件: -\n没有匹配歌曲。"
    );
    let list = json!({
        "count": 0,
        "total_matches": 0,
        "criteria": {},
        "songs": [],
    });
    assert_eq!(
        render("list_maimai_songs_by_id", &list, options(None))?,
        "ID 列表: 返回 0 / 0\n条件: -\n没有匹配歌曲。"
    );
    assert_eq!(
        render("today_maimai", &json!({"text": "今日舞萌"}), options(None))?,
        "今日舞萌"
    );
    Ok(())
}

fn dual_chart_song() -> Value {
    json!({
        "title": "Believe the Rainbow",
        "id": "835",
        "artist": "Shoichiro Hirata feat.Sana",
        "bpm": 170,
        "version": "maimai",
        "available_chart_types": ["standard", "dx"],
        "matched_charts": [
            {"chart_type": "standard", "difficulty": "Basic", "difficulty_index": 0, "level": "4", "ds": 4.0},
            {"chart_type": "standard", "difficulty": "Master", "difficulty_index": 3, "level": "13", "ds": 13.4},
            {"chart_type": "dx", "difficulty": "Basic", "difficulty_index": 0, "level": "2", "ds": 2.0},
            {"chart_type": "dx", "difficulty": "Master", "difficulty_index": 3, "level": "13", "ds": 13.0},
        ],
        "aliases": ["相信彩虹"],
        "match": {"field": "alias", "mode": "exact", "value": "相信彩虹"},
    })
}

fn complete_song() -> Value {
    json!({
        "title": "完整歌曲",
        "id": "835",
        "artist": "Artist",
        "source": "lxns+official+dxdata+cndivingfish",
        "regions": {"jp": true, "intl": true, "usa": false, "cn": true},
        "version": "PRiSM",
        "bpm": 180,
        "release_date": "2025-01-01",
        "is_new": true,
        "is_locked": true,
        "genre": "maimai",
        "available_chart_types": ["dx", "standard"],
        "match": {"field": "source_id", "mode": "contains", "value": "835"},
        "aliases": ["A", "B", "A", "C", "D", "E", "F", "G", "H", "I"],
        "source_fields": {
            "cn": {"version": "2025", "genre": "maimai", "release_date": "2025-01-01", "is_new": true, "ds": [13.0, 14.2]},
            "official": {"version": "2024", "genre": "maimai", "release_date": "2024-01-01", "is_new": false, "ds": [13.1, 14.2]},
            "jp": {"version": "2025", "genre": "POPS", "release_date": "2025-02-01", "is_new": true, "ds": [13.0, 14.3]},
        },
        "matched_charts": [
            {
                "source": "jp",
                "chart_type": "standard",
                "chart_id": 835.0,
                "difficulty": "Basic",
                "difficulty_index": 0,
                "level": "4",
                "ds": 4.0,
                "fit_diff": null,
                "fit_delta": null,
                "notes": {},
                "charter": null,
            },
            {
                "source": "cn",
                "chart_type": "dx",
                "chart_id": 10835.0,
                "difficulty": "Master",
                "difficulty_index": 3,
                "level": "14+",
                "ds": 14.0,
                "fit_diff": 13.8756,
                "fit_delta": 0.12444,
                "fit_label": "虚高",
                "notes": {
                    "left": {"tap": 1, "hold": 2, "slide": 3, "touch": 4, "break": 5},
                    "right": {"total": 6.0, "tap": 6.0},
                },
                "charter": "Alice",
                "version": "PRiSM",
                "is_buddy": true,
                "kanji": "宙",
                "description": "1234567890123456789012345678901234567890EXTRA",
            },
        ],
    })
}

const fn options(format: Option<&str>) -> FormatOptions<'_> {
    FormatOptions {
        format,
        include_raw: false,
        debug: false,
    }
}
