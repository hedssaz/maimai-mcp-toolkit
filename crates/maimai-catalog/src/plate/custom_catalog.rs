use std::collections::{BTreeSet, HashMap};

use maimai_core::{ChartConstant, ChartGeneration, Difficulty, SongIdValue};

use crate::{CatalogError, TextNormalizer};

use super::{
    PlateMember, PlateMemberIdentity,
    catalog::MemberSet,
    custom::{CustomGeneration, CustomPlateDocument, CustomSong, CustomSongDefinition},
    member::PlateChart,
    source_index::{
        SourceIndex, member_from_projection, projection_generation, with_image, with_remaster,
    },
};

pub(super) fn build_custom(
    document: CustomPlateDocument,
    sources: &SourceIndex<'_>,
    normalizer: &TextNormalizer,
) -> Result<HashMap<String, MemberSet>, CatalogError> {
    document
        .plates
        .into_iter()
        .map(|(name, values)| {
            let mut members = Vec::new();
            let mut seen = BTreeSet::new();
            for value in values {
                let member = custom_member(&name, value, sources, normalizer)?;
                let key = (member.identity().display_id().clone(), member.generation());
                if seen.insert(key) {
                    members.push(member);
                }
            }
            Ok((
                name,
                MemberSet {
                    declared: members.len(),
                    members,
                },
            ))
        })
        .collect()
}

fn custom_member(
    plate: &str,
    value: CustomSong,
    sources: &SourceIndex<'_>,
    normalizer: &TextNormalizer,
) -> Result<PlateMember, CatalogError> {
    match value {
        CustomSong::Reference(value) => resolve_reference(
            plate,
            &value.query,
            value.generation,
            value.image_name,
            sources,
            normalizer,
        ),
        CustomSong::Defined(value) => {
            if let Some(id) = value.id.as_deref()
                && let Some(member) = resolve_reference_optional(
                    plate,
                    id,
                    Some(value.generation),
                    value.image_name.clone(),
                    sources,
                    normalizer,
                )?
            {
                return Ok(member);
            }
            external_member(plate, value)
        }
    }
}

fn resolve_reference(
    plate: &str,
    query: &str,
    generation: Option<CustomGeneration>,
    image_name: Option<String>,
    sources: &SourceIndex<'_>,
    normalizer: &TextNormalizer,
) -> Result<PlateMember, CatalogError> {
    resolve_reference_optional(plate, query, generation, image_name, sources, normalizer)?
        .ok_or_else(|| CatalogError::UnsupportedSourceValue {
            source_name: "自定义牌子",
            song_id: plate.to_owned(),
            field: "songs",
            value: format!("未找到歌曲：{query}"),
        })
}

fn resolve_reference_optional(
    plate: &str,
    query: &str,
    generation: Option<CustomGeneration>,
    image_name: Option<String>,
    sources: &SourceIndex<'_>,
    normalizer: &TextNormalizer,
) -> Result<Option<PlateMember>, CatalogError> {
    if let Ok(id) = query.trim().parse::<u32>() {
        let Some((index, projection)) = sources.df(id)? else {
            return Ok(None);
        };
        if generation.is_some_and(|expected| {
            Some(expected.chart_generation()) != projection_generation(projection)
        }) {
            return Ok(None);
        }
        return member_from_projection(
            sources.song(index),
            projection,
            Some(projection),
            None,
            None,
        )
        .map(|member| Some(with_image(member, image_name)));
    }
    let normalized = normalizer.normalize(query);
    let mut matches = Vec::new();
    for index in sources.search_candidates(&normalized) {
        for projection in sources.projections(index) {
            if generation.is_some_and(|expected| {
                Some(expected.chart_generation()) != projection_generation(projection)
            }) {
                continue;
            }
            let diving_fish = match projection_generation(projection) {
                Some(generation) => sources.df_for_song_generation(index, generation)?,
                None => None,
            };
            matches.push(member_from_projection(
                sources.song(index),
                projection,
                diving_fish,
                None,
                None,
            )?);
        }
    }
    matches.sort_by(|left, right| {
        left.identity()
            .display_id()
            .cmp(right.identity().display_id())
            .then_with(|| left.generation().cmp(&right.generation()))
    });
    matches.dedup_by(|left, right| {
        left.identity().display_id() == right.identity().display_id()
            && left.generation() == right.generation()
    });
    match matches.as_slice() {
        [] => Ok(None),
        [only] => Ok(Some(with_image(only.clone(), image_name))),
        _ => Err(CatalogError::AmbiguousSongIdentity {
            source_name: "自定义牌子",
            song_id: plate.to_owned(),
            title: query.to_owned(),
            candidates: (0..matches.len()).collect(),
        }),
    }
}

fn external_member(plate: &str, value: CustomSongDefinition) -> Result<PlateMember, CatalogError> {
    let display_id = match value.id {
        Some(value) => value.parse::<u32>().map_or_else(
            |_| SongIdValue::text(value),
            |value| Ok(SongIdValue::Numeric(value)),
        ),
        None => SongIdValue::text(format!(
            "custom:{plate}:{}:{}",
            value.title,
            value.generation.label()
        )),
    }
    .map_err(|_| CatalogError::InvalidSourceSongId {
        source_name: "自定义牌子",
        value: value.title.clone(),
    })?;
    let generation = value.generation.chart_generation();
    let charts = value
        .levels
        .into_iter()
        .zip(value.constants)
        .enumerate()
        .map(|(index, (level, constant))| external_chart(&value.title, index, level, constant))
        .collect::<Result<_, _>>()?;
    Ok(with_remaster(
        PlateMember::new(
            PlateMemberIdentity::External { display_id },
            value.title,
            generation,
            value.image_name,
            charts,
        ),
        false,
    ))
}

fn external_chart(
    title: &str,
    index: usize,
    level: String,
    constant: serde_json::Number,
) -> Result<PlateChart, CatalogError> {
    let difficulty = [
        Difficulty::Basic,
        Difficulty::Advanced,
        Difficulty::Expert,
        Difficulty::Master,
        Difficulty::ReMaster,
    ]
    .get(index)
    .copied()
    .ok_or(CatalogError::InvalidDifficulty {
        source_name: "自定义牌子",
        song_id: title.to_owned(),
        difficulty: index,
    })?;
    let value = constant.to_string();
    let constant =
        ChartConstant::from_decimal_str(&value).map_err(|_| CatalogError::InvalidNumber {
            source_name: "自定义牌子",
            song_id: title.to_owned(),
            field: "ds",
            value,
        })?;
    Ok(PlateChart::new(None, difficulty, level, Some(constant)))
}

impl CustomGeneration {
    pub(super) const fn chart_generation(self) -> ChartGeneration {
        match self {
            Self::Standard => ChartGeneration::Standard,
            Self::Deluxe => ChartGeneration::Deluxe,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Standard => "SD",
            Self::Deluxe => "DX",
        }
    }
}
