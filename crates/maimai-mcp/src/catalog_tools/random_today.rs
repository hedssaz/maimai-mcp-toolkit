use maimai_catalog::CatalogSnapshot;
use maimai_core::{TodayDate, TodaySong, format_today_maimai, today_maimai};
use rand::{SeedableRng, prelude::SliceRandom, rngs::StdRng};
use serde_json::{Value, json};
use time::{OffsetDateTime, UtcOffset};

use super::{
    dto::{RandomArgs, Scalar, TodayArgs},
    error::CatalogToolError,
    search_ops::{execute_query, search_result},
};

pub(super) fn random(
    snapshot: &CatalogSnapshot,
    arguments: &RandomArgs,
) -> Result<Value, CatalogToolError> {
    if arguments.count == 0 || arguments.count > 100 {
        return Err(CatalogToolError::input(
            "count/limit must be between 1 and 100",
        ));
    }
    let (_, mut hits) = execute_query(snapshot, &arguments.search)?;
    let total_candidates = hits.len();
    if hits.is_empty() {
        return Err(CatalogToolError::input(
            "No songs matched the random criteria.",
        ));
    }
    if let Some(seed) = &arguments.seed {
        let mut rng = StdRng::seed_from_u64(seed_value(seed));
        hits.shuffle(&mut rng);
    } else {
        hits.shuffle(&mut rand::rng());
    }
    hits.truncate(arguments.count.min(hits.len()));
    let mut result = search_result(snapshot, &arguments.search, &hits, total_candidates, false)?;
    let Some(output) = result.as_object_mut() else {
        return Err(CatalogToolError::input("invalid random result"));
    };
    output.remove("total_matches");
    output.insert("total_candidates".to_owned(), json!(total_candidates));
    if let Some(criteria) = output.get_mut("criteria").and_then(Value::as_object_mut) {
        criteria.insert("count".to_owned(), json!(arguments.count));
        criteria.insert("seed".to_owned(), json!(arguments.seed));
        criteria.insert(
            "random_mode".to_owned(),
            json!(if has_chart_filter(&arguments.search) {
                "chart_filtered"
            } else {
                "song_id"
            }),
        );
    }
    Ok(result)
}

pub(super) fn today(
    snapshot: &CatalogSnapshot,
    arguments: &TodayArgs,
) -> Result<Value, CatalogToolError> {
    let qq = arguments
        .qq
        .text()
        .parse::<u64>()
        .map_err(|_| CatalogToolError::input("qq must be an integer"))?;
    let offset = arguments
        .offset
        .as_ref()
        .map(|value| value.text().parse::<i64>())
        .transpose()
        .map_err(|_| CatalogToolError::input("offset must be an integer"))?
        .map_or(0, |value| value);
    let offset_time =
        UtcOffset::from_hms(8, 0, 0).map_err(|error| CatalogToolError::input(error.to_string()))?;
    let now = OffsetDateTime::now_utc().to_offset(offset_time);
    let date = TodayDate::new(u8::from(now.month()), now.day())?;
    let songs = today_songs(snapshot);
    if songs.is_empty() {
        return Err(CatalogToolError::input("本地曲库为空，无法生成今日舞萌。"));
    }
    let result = today_maimai(qq, &songs, date, offset)?;
    let text = format_today_maimai(&arguments.bot_name, qq, &songs, date, offset)?;
    Ok(json!({
        "qq": arguments.qq.text(),
        "rp": result.rp,
        "good": result.good,
        "bad": result.bad,
        "offset": result.offset,
        "total_candidates": songs.len(),
        "song": {
            "id": result.song.id,
            "title": result.song.title,
            "ds": result.song.chart_constants,
        },
        "text": text,
    }))
}

fn today_songs(snapshot: &CatalogSnapshot) -> Vec<TodaySong> {
    snapshot
        .songs()
        .iter()
        .enumerate()
        .filter_map(|(index, song)| {
            let id = snapshot
                .song_metadata(index)?
                .canonical_numeric_ids
                .iter()
                .next()?
                .to_string();
            let constants = song.charts.iter().filter_map(|chart| {
                chart.constant.map(|constant| {
                    let mut value = constant.value().normalize().to_string();
                    if !value.contains('.') {
                        value.push_str(".0");
                    }
                    value
                })
            });
            Some(TodaySong::new(id, &song.title, constants))
        })
        .collect()
}

fn seed_value(seed: &Scalar) -> u64 {
    seed.text()
        .bytes()
        .fold(14_695_981_039_346_656_037, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(1_099_511_628_211)
        })
}

fn has_chart_filter(arguments: &super::dto::SearchArgs) -> bool {
    arguments.level.is_some()
        || arguments.ds.is_some()
        || arguments.ds_min.is_some()
        || arguments.ds_max.is_some()
        || arguments.fit_diff.is_some()
        || arguments.fit_delta.is_some()
        || arguments.fit_label.is_some()
        || arguments.difficulty.is_some()
        || arguments.song_type.is_some()
        || arguments.charter.is_some()
        || arguments.tag.is_some()
        || arguments.tag_exclude.is_some()
        || arguments.released_after.is_some()
        || arguments.released_before.is_some()
}
