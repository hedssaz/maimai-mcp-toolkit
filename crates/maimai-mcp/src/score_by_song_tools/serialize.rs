use maimai_app::{
    score_by_song::{
        MusicIdScores, PlayerLookupRequest, ScoreBySongResult, SongCandidate, SongSelection,
    },
    scores::{PlayerScores, SelectionReason},
};
use maimai_core::{ChartGeneration, ScoreSource, SongIdValue};
use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::error::ScoreBySongToolError;
use crate::score_queries::serialize::{self, OutputContext};

pub fn result(mut result: ScoreBySongResult) -> Result<Value, ScoreBySongToolError> {
    let requested_at = timestamp(result.requested_at)?;
    let raw = result
        .raw
        .take()
        .map(|raw| {
            raw.into_value()
                .map_err(|_| ScoreBySongToolError::internal())
        })
        .transpose()?;
    let lookup = requested_lookup(&result.requested_player);
    let identity = result.identity.clone();
    let selection = result.source_selection;
    let scores = result
        .scores
        .iter()
        .map(|item| score_item(item, &result, &requested_at, identity.clone(), selection))
        .collect::<Result<Vec<_>, _>>()?;
    let requested = scores.len();
    let mut output = json!({
        "source": "maimai-score-query",
        "requestedAt": requested_at,
        "lookup": lookup,
        "songQuery": result.song_query,
        "selectedSong": candidate(&result.selected_song.candidate),
        "selection": selection_value(&result.selection),
        "musicIds": result.selected_song.music_ids,
        "counts": {"requested": requested, "success": requested, "failure": 0},
        "scores": scores,
    });
    if let Some(raw) = raw {
        output["raw"] = raw;
    }
    Ok(output)
}

fn score_item(
    item: &MusicIdScores,
    result: &ScoreBySongResult,
    requested_at: &str,
    identity: Option<maimai_storage::IdentityRecord>,
    selection: maimai_app::scores::SourceSelection,
) -> Result<Value, ScoreBySongToolError> {
    let player_scores = PlayerScores {
        lookup: result.lookup.clone(),
        source: result.source,
        player: result.player.clone(),
        records: item.records.clone(),
    };
    let mut structured = serialize::song_scores(
        &player_scores,
        json!(item.music_id),
        OutputContext {
            requested_at: requested_at.to_owned(),
            identity,
            selection,
            include_chart_metadata: true,
            raw: None,
        },
    )
    .map_err(|_| ScoreBySongToolError::internal())?;
    structured["operation"] = Value::String(operation(result.source).to_owned());
    structured["sourcePreference"] = json!({
        "qq": match &result.lookup { maimai_app::scores::Lookup::Qq(qq) => Some(qq.as_str()), _ => None },
        "preferredSource": source_key(selection.preferred_source),
        "preferredSourceLabel": source_label(selection.preferred_source),
        "usedSource": source_key(selection.source),
        "usedSourceLabel": source_label(selection.source),
        "explicitSource": selection.reason == SelectionReason::ExplicitOverride,
    });
    Ok(json!({
        "musicId": item.music_id,
        "ok": true,
        "result": structured,
        "error": null,
        "text": null,
    }))
}

fn requested_lookup(value: &PlayerLookupRequest) -> Value {
    match value {
        PlayerLookupRequest::Qq(value) => json!({"qq":value.as_str()}),
        PlayerLookupRequest::Username(value) => json!({"username":value.as_str()}),
        PlayerLookupRequest::Auto(value) => json!({"target":value.as_str()}),
    }
}

fn candidate(value: &SongCandidate) -> Value {
    json!({
        "id": song_id(value.id.value()),
        "sourceId": song_id(value.id.value()),
        "title": value.title,
        "artist": value.artist,
        "source": value.source,
        "availableChartTypes": available_chart_types(value),
        "aliases": value.aliases,
    })
}

fn available_chart_types(value: &SongCandidate) -> Vec<&'static str> {
    let mut output = Vec::new();
    if value
        .available_generations
        .contains(&ChartGeneration::Standard)
    {
        output.push("standard");
    }
    if value
        .available_generations
        .contains(&ChartGeneration::Deluxe)
    {
        output.push("dx");
    }
    if value.available_generations.iter().any(|generation| {
        matches!(
            generation,
            ChartGeneration::UtageOnePlayer | ChartGeneration::UtageTwoPlayer
        )
    }) {
        output.push("utage");
    }
    output
}

fn selection_value(value: &SongSelection) -> Value {
    json!({
        "autoSelected": value.auto_selected,
        "selectedRank": value.selected_rank,
        "totalMatches": value.total_matches,
        "truncated": value.truncated,
        "candidates": value.candidates.iter().map(candidate).collect::<Vec<_>>(),
    })
}

fn song_id(value: &SongIdValue) -> Value {
    match value {
        SongIdValue::Numeric(value) => json!(value),
        SongIdValue::Text(value) => json!(value),
    }
}

fn timestamp(value: OffsetDateTime) -> Result<String, ScoreBySongToolError> {
    value
        .format(&Rfc3339)
        .map_err(|_| ScoreBySongToolError::internal())
}

const fn operation(source: ScoreSource) -> &'static str {
    match source {
        ScoreSource::DivingFish => "maimai_dev_player_records_get",
        ScoreSource::Lxns => "lxns_player_scores_filter",
        ScoreSource::Local => "local_maimai_player_records_filter",
        ScoreSource::OfficialCn => "official_cn_player_records_filter",
    }
}

const fn source_key(source: ScoreSource) -> &'static str {
    match source {
        ScoreSource::DivingFish => "sy",
        ScoreSource::Lxns => "lxns",
        ScoreSource::Local => "local",
        ScoreSource::OfficialCn => "official_cn",
    }
}

const fn source_label(source: ScoreSource) -> &'static str {
    match source {
        ScoreSource::DivingFish => "水鱼",
        ScoreSource::Lxns => "落雪",
        ScoreSource::Local => "本地缓存",
        ScoreSource::OfficialCn => "国服",
    }
}
