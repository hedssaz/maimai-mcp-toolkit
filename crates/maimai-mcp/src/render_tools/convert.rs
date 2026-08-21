use std::collections::BTreeSet;

use maimai_app::{
    music_info::{KnownMusicMetadata, MusicInfoChartType, MusicInfoRequest, ResolvedSongHint},
    scores::Lookup,
};
use maimai_core::{ChartGeneration, PlayerUsername, QqId, SongIdValue};

use super::{
    RenderToolError,
    dto::{BatchItemDto, MusicInfoArgs, MusicInfoBatchArgs, ResolvedSongDto, Scalar},
};

pub(super) fn single(
    args: MusicInfoArgs,
) -> Result<(MusicInfoRequest, Option<Lookup>), RenderToolError> {
    let player = player(args.qq.as_ref(), args.username.as_ref())?;
    Ok((request(args, None)?, player))
}

pub(super) fn batch(
    args: MusicInfoBatchArgs,
) -> Result<(Vec<MusicInfoRequest>, Option<Lookup>), RenderToolError> {
    let player = player(args.qq.as_ref(), args.username.as_ref())?;
    let inherited_type = parse_chart_type(
        args.song_type_camel
            .as_deref()
            .or(args.song_type.as_deref()),
    )?;
    let mut requests = Vec::new();
    if !args.items.is_empty() {
        for (index, item) in args.items.into_iter().enumerate() {
            match item {
                BatchItemDto::Object(args) => requests.push(request(*args, inherited_type)?),
                BatchItemDto::Query(query) => requests.push(request(
                    MusicInfoArgs {
                        query: Some(query),
                        ..MusicInfoArgs::default()
                    },
                    inherited_type,
                )?),
                BatchItemDto::Ignored(_) => {
                    return Err(RenderToolError::invalid(format!(
                        "items[{}] 必须是字符串、数字或曲目对象",
                        index + 1
                    )));
                }
            }
        }
    } else {
        let queries = if args.queries.is_empty() {
            args.query.into_iter().collect()
        } else {
            args.queries
        };
        for query in queries {
            requests.push(request(
                MusicInfoArgs {
                    query: Some(query),
                    ..MusicInfoArgs::default()
                },
                inherited_type,
            )?);
        }
    }
    Ok((requests, player))
}

pub(super) fn request(
    args: MusicInfoArgs,
    inherited_type: Option<MusicInfoChartType>,
) -> Result<MusicInfoRequest, RenderToolError> {
    let music_id = args
        .music_id
        .as_ref()
        .or(args.music_id_camel.as_ref())
        .or(args.id.as_ref())
        .map(song_id)
        .transpose()?;
    let query = args
        .query
        .as_ref()
        .or(args.song_query_camel.as_ref())
        .or(args.song_query.as_ref())
        .or(args.title.as_ref())
        .map(Scalar::text);
    let chart_type = parse_chart_type(
        args.song_type_camel
            .as_deref()
            .or(args.song_type.as_deref())
            .or(args.chart_type_camel.as_deref())
            .or(args.chart_type.as_deref())
            .or(args.type_name.as_deref()),
    )?
    .or(inherited_type);
    let image_name = args.image_name.or(args.image_name_camel);
    let known_title = args
        .known_title_camel
        .or(args.known_title)
        .or(args.name)
        .or_else(|| args.title.as_ref().map(Scalar::text))
        .unwrap_or_default();
    let known = KnownMusicMetadata::new(
        known_title,
        args.artist.unwrap_or_default(),
        args.genre.or(args.category).unwrap_or_default(),
        args.version.or(args.from).unwrap_or_default(),
        args.bpm.as_ref().map(bpm).transpose()?,
        args.is_new.or(args.is_new_camel).unwrap_or(false),
    )?;
    let resolved = args
        .resolved_song_camel
        .or(args.resolved_song)
        .map(resolved_hint)
        .transpose()?;
    MusicInfoRequest::new(music_id, query, chart_type, image_name, known, resolved)
        .map_err(Into::into)
}

fn resolved_hint(value: ResolvedSongDto) -> Result<ResolvedSongHint, RenderToolError> {
    let id = value
        .id
        .as_ref()
        .or(value.source_id.as_ref())
        .map(song_id)
        .transpose()?;
    let mut chart_types = BTreeSet::new();
    for value in value.available_chart_types.into_iter().chain(
        value
            .matched_charts
            .into_iter()
            .filter_map(|chart| chart.chart_type),
    ) {
        if let Some(generation) = generation_hint(&value) {
            chart_types.insert(generation);
        }
    }
    Ok(ResolvedSongHint::new(
        id,
        value.title,
        chart_types,
        value.image_name.or(value.image_name_camel),
    )?)
}

fn player(
    qq: Option<&Scalar>,
    username: Option<&Scalar>,
) -> Result<Option<Lookup>, RenderToolError> {
    if let Some(qq) = qq
        .map(Scalar::text)
        .filter(|value| !value.trim().is_empty())
    {
        return QqId::new(qq.trim())
            .map(Lookup::Qq)
            .map(Some)
            .map_err(|_| RenderToolError::invalid("qq 必须是数字字符串"));
    }
    username
        .map(Scalar::text)
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            PlayerUsername::new(value.trim())
                .map(Lookup::Username)
                .map_err(|_| RenderToolError::invalid("username 格式不正确"))
        })
        .transpose()
}

fn song_id(value: &Scalar) -> Result<SongIdValue, RenderToolError> {
    let text = value.text();
    let text = text.trim();
    if let Ok(value) = text.parse::<u32>() {
        if value == 0 {
            return Err(RenderToolError::invalid(
                "music_id 必须是正整数或非空字符串",
            ));
        }
        return Ok(SongIdValue::Numeric(value));
    }
    SongIdValue::text(text)
        .map_err(|_| RenderToolError::invalid("music_id 必须是正整数或非空字符串"))
}

fn bpm(value: &Scalar) -> Result<u32, RenderToolError> {
    let value = value
        .text()
        .parse::<f64>()
        .map_err(|_| RenderToolError::invalid("bpm 必须是非负数字"))?;
    if !value.is_finite() || value < 0.0 || value > f64::from(u32::MAX) {
        return Err(RenderToolError::invalid("bpm 必须是非负数字"));
    }
    Ok(value.trunc() as u32)
}

fn parse_chart_type(value: Option<&str>) -> Result<Option<MusicInfoChartType>, RenderToolError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let value = value.to_ascii_lowercase().replace(' ', "");
    match value.as_str() {
        "dx" | "でらっくす" => Ok(Some(MusicInfoChartType::Deluxe)),
        "sd" | "st" | "std" | "standard" | "标准" | "标" | "标准谱面" => {
            Ok(Some(MusicInfoChartType::Standard))
        }
        _ => Err(RenderToolError::invalid("songType 必须是 standard 或 dx")),
    }
}

fn generation_hint(value: &str) -> Option<ChartGeneration> {
    match value.trim().to_ascii_lowercase().as_str() {
        "standard" | "sd" | "st" | "std" => Some(ChartGeneration::Standard),
        "dx" => Some(ChartGeneration::Deluxe),
        "utage" | "utage1p" => Some(ChartGeneration::UtageOnePlayer),
        "utage2p" => Some(ChartGeneration::UtageTwoPlayer),
        _ => None,
    }
}
