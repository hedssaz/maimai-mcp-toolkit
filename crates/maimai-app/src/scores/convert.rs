use maimai_catalog::CatalogSnapshot;
use maimai_core::{
    AchievementRate, ChartGeneration, Difficulty, FullComboStatus, FullSyncStatus, PlayAchievement,
    ScoreSource, SongIdNamespace, SourceSongId, achievement_rank, single_song_rating,
};
use maimai_providers::{
    DivingFishB50, DivingFishChartGeneration, DivingFishPlayer, DivingFishPlayerRecords,
    DivingFishScore, LxnsChartType, LxnsDifficulty, LxnsPlayer, LxnsPlayerBests, LxnsPlayerScores,
    LxnsScore,
};
use maimai_storage::{PlayerProfile, PlayerRecord};

use super::{
    B50Chart, B50Result, FitLabel, Lookup, PlayerScoreProfile, PlayerScores, ScoreError,
    ScoreErrorCode,
    catalog::{GenerationMatch, ResolvedChart, ScoreCatalog},
    compute::{UpstreamTotals, b50_from_sections},
    marker::{full_combo, full_sync},
};

pub fn from_diving_fish_b50(
    input: DivingFishB50,
    snapshot: &CatalogSnapshot,
) -> Result<B50Result, ScoreError> {
    let upstream = input.rating_breakdown;
    let catalog = ScoreCatalog::new(snapshot)?;
    let lookup = Lookup::try_from(input.lookup)?;
    let player = diving_fish_player(input.player);
    let b35 = input
        .sd
        .into_iter()
        .map(|score| diving_fish_chart(score, &catalog))
        .collect::<Result<Vec<_>, _>>()?;
    let b15 = input
        .dx
        .into_iter()
        .map(|score| diving_fish_chart(score, &catalog))
        .collect::<Result<Vec<_>, _>>()?;
    b50_from_sections(
        lookup,
        ScoreSource::DivingFish,
        player,
        b35,
        b15,
        UpstreamTotals {
            b35: Some(upstream.b35),
            b15: Some(upstream.b15),
            total: Some(upstream.total),
        },
    )
}

pub fn from_diving_fish_records(
    input: DivingFishPlayerRecords,
    snapshot: &CatalogSnapshot,
) -> Result<PlayerScores, ScoreError> {
    let catalog = ScoreCatalog::new(snapshot)?;
    Ok(PlayerScores {
        lookup: Lookup::try_from(input.lookup)?,
        source: ScoreSource::DivingFish,
        player: diving_fish_player(input.player),
        records: input
            .records
            .into_iter()
            .map(|score| diving_fish_chart(score, &catalog))
            .collect::<Result<_, _>>()?,
    })
}

pub fn from_lxns_bests(
    lookup: Lookup,
    input: LxnsPlayerBests,
    snapshot: &CatalogSnapshot,
) -> Result<B50Result, ScoreError> {
    require_qq(&lookup, ScoreSource::Lxns)?;
    let catalog = ScoreCatalog::new(snapshot)?;
    let standard_total = input.standard_total;
    let deluxe_total = input.deluxe_total;
    let player = lxns_player(input.player.as_ref());
    let b35 = input
        .standard
        .into_iter()
        .map(|score| lxns_chart(score, &catalog))
        .collect::<Result<Vec<_>, _>>()?;
    let b15 = input
        .deluxe
        .into_iter()
        .map(|score| lxns_chart(score, &catalog))
        .collect::<Result<Vec<_>, _>>()?;
    b50_from_sections(
        lookup,
        ScoreSource::Lxns,
        player,
        b35,
        b15,
        UpstreamTotals {
            b35: standard_total,
            b15: deluxe_total,
            total: None,
        },
    )
}

pub fn from_lxns_scores(
    lookup: Lookup,
    input: LxnsPlayerScores,
    snapshot: &CatalogSnapshot,
) -> Result<PlayerScores, ScoreError> {
    require_qq(&lookup, ScoreSource::Lxns)?;
    let catalog = ScoreCatalog::new(snapshot)?;
    Ok(PlayerScores {
        lookup,
        source: ScoreSource::Lxns,
        player: lxns_player(input.player.as_ref()),
        records: input
            .scores
            .into_iter()
            .map(|score| lxns_chart(score, &catalog))
            .collect::<Result<_, _>>()?,
    })
}

pub fn from_local_records(
    lookup: Lookup,
    records: &[PlayerRecord],
    profile: Option<&PlayerProfile>,
    snapshot: &CatalogSnapshot,
) -> Result<PlayerScores, ScoreError> {
    from_stored_records(lookup, records, profile, snapshot, ScoreSource::Local)
}

pub(crate) fn from_stored_records(
    lookup: Lookup,
    records: &[PlayerRecord],
    profile: Option<&PlayerProfile>,
    snapshot: &CatalogSnapshot,
    source: ScoreSource,
) -> Result<PlayerScores, ScoreError> {
    let expected_qq = match &lookup {
        Lookup::Qq(qq) => qq,
        Lookup::Username(_) => return Err(unsupported_username(source)),
    };
    if profile.is_some_and(|profile| &profile.qq != expected_qq)
        || records.iter().any(|record| &record.qq != expected_qq)
    {
        return Err(ScoreError::invalid("缓存成绩或玩家资料不属于请求的 QQ"));
    }
    let catalog = ScoreCatalog::new(snapshot)?;
    Ok(PlayerScores {
        lookup,
        source,
        player: local_player(profile)?,
        records: records
            .iter()
            .map(|record| local_chart(record, &catalog))
            .collect::<Result<_, _>>()?,
    })
}

fn diving_fish_chart(
    score: DivingFishScore,
    catalog: &ScoreCatalog,
) -> Result<B50Chart, ScoreError> {
    let generation = match score.generation {
        DivingFishChartGeneration::Standard => GenerationMatch::Exact(ChartGeneration::Standard),
        DivingFishChartGeneration::Deluxe => GenerationMatch::Exact(ChartGeneration::Deluxe),
        DivingFishChartGeneration::Utage => GenerationMatch::Utage,
    };
    let resolved = catalog.resolve_chart(&score.song_id, generation, score.difficulty)?;
    let constant = score.constant.or(resolved.constant);
    let rating = rating(
        constant,
        score.achievements.and_then(PlayAchievement::ranked),
        score.rating,
    )?;
    let version = preferred_version(score.version, &resolved.version);
    Ok(chart(
        resolved,
        score.song_id,
        constant,
        score.achievements,
        score.dx_score,
        rating,
        score.grade,
        score.full_combo,
        score.full_sync,
        version,
    ))
}

fn lxns_chart(score: LxnsScore, catalog: &ScoreCatalog) -> Result<B50Chart, ScoreError> {
    let source_song_id = SourceSongId::numeric(SongIdNamespace::Lxns, score.id.get());
    let (generation, difficulty) = match score.chart_type {
        LxnsChartType::Standard => (
            GenerationMatch::Exact(ChartGeneration::Standard),
            lxns_difficulty(score.level_index),
        ),
        LxnsChartType::Deluxe => (
            GenerationMatch::Exact(ChartGeneration::Deluxe),
            lxns_difficulty(score.level_index),
        ),
        LxnsChartType::Utage => (GenerationMatch::Utage, Difficulty::Utage),
    };
    let resolved = catalog.resolve_chart(&source_song_id, generation, difficulty)?;
    let constant = score.ds.or(resolved.constant);
    let ranked = score.achievements.ranked();
    let rating = rating(constant, ranked, score.dx_rating)?;
    let grade = score
        .rate
        .or_else(|| ranked.map(|value| achievement_rank(value).legacy_code().to_owned()));
    let version = resolved.version.clone();
    Ok(chart(
        resolved,
        source_song_id,
        constant,
        Some(score.achievements),
        Some(score.dx_score),
        rating,
        grade,
        score.fc.map(full_combo),
        score.fs.map(full_sync),
        version,
    ))
}

fn local_chart(record: &PlayerRecord, catalog: &ScoreCatalog) -> Result<B50Chart, ScoreError> {
    let resolved = catalog.resolve_chart(
        record.chart.song(),
        GenerationMatch::Exact(record.chart.generation()),
        record.chart.difficulty(),
    )?;
    let constant = record.ds.or(resolved.constant);
    let provider_rating = record
        .ra
        .map(|value| nonnegative_u32(value, "ra"))
        .transpose()?;
    let rating = rating(
        constant,
        record.achievements.and_then(PlayAchievement::ranked),
        provider_rating,
    )?;
    let version = preferred_version(record.version.clone(), &resolved.version);
    Ok(chart(
        resolved,
        record.chart.song().clone(),
        constant,
        record.achievements,
        record
            .dx_score
            .map(|value| nonnegative_u32(value, "dxScore"))
            .transpose()?,
        rating,
        record.rate.clone(),
        record.fc,
        record.fs,
        version,
    ))
}

#[allow(clippy::too_many_arguments)]
fn chart(
    resolved: ResolvedChart,
    source_song_id: SourceSongId,
    constant: Option<maimai_core::ChartConstant>,
    achievements: Option<PlayAchievement>,
    dx_score: Option<u32>,
    rating: Option<u32>,
    grade: Option<String>,
    full_combo: Option<FullComboStatus>,
    full_sync: Option<FullSyncStatus>,
    version: String,
) -> B50Chart {
    let fit_label =
        constant
            .zip(resolved.fit_constant)
            .map(|(actual, fit)| match actual.cmp(&fit) {
                std::cmp::Ordering::Greater => FitLabel::Inflated,
                std::cmp::Ordering::Less => FitLabel::Deflated,
                std::cmp::Ordering::Equal => FitLabel::Equal,
            });
    B50Chart {
        key: resolved.key,
        source_song_id,
        title: resolved.title,
        level: resolved.level,
        constant,
        achievements,
        dx_score,
        rating,
        original_rating: None,
        grade,
        full_combo,
        full_sync,
        version,
        is_current: resolved.is_current,
        fit_constant: resolved.fit_constant,
        fit_label,
    }
}

fn preferred_version(value: Option<String>, fallback: &str) -> String {
    value.unwrap_or_else(|| fallback.to_owned())
}

fn rating(
    constant: Option<maimai_core::ChartConstant>,
    achievements: Option<AchievementRate>,
    provider_rating: Option<u32>,
) -> Result<Option<u32>, ScoreError> {
    constant
        .zip(achievements)
        .map(|(constant, achievements)| single_song_rating(constant, achievements))
        .transpose()
        .map(|calculated| calculated.or(provider_rating))
        .map_err(ScoreError::from)
}

fn diving_fish_player(player: DivingFishPlayer) -> PlayerScoreProfile {
    PlayerScoreProfile {
        nickname: player.nickname,
        username: player.username.map(|value| value.to_string()),
        rating: player.rating,
        actual_rating: None,
        additional_rating: player.additional_rating,
        plate: player.plate,
    }
}

fn lxns_player(player: Option<&LxnsPlayer>) -> PlayerScoreProfile {
    PlayerScoreProfile {
        nickname: player.and_then(|value| value.name.clone()),
        username: player
            .and_then(|value| value.friend_code.as_ref())
            .map(ToString::to_string),
        rating: player.and_then(|value| value.rating),
        actual_rating: None,
        additional_rating: None,
        plate: None,
    }
}

fn local_player(profile: Option<&PlayerProfile>) -> Result<PlayerScoreProfile, ScoreError> {
    Ok(PlayerScoreProfile {
        nickname: profile.and_then(|value| value.nickname.clone()),
        username: None,
        rating: profile
            .and_then(|value| value.player_rating)
            .map(|value| nonnegative_u32(value, "playerRating"))
            .transpose()?,
        actual_rating: None,
        additional_rating: None,
        plate: None,
    })
}

fn lxns_difficulty(value: LxnsDifficulty) -> Difficulty {
    match value {
        LxnsDifficulty::Basic => Difficulty::Basic,
        LxnsDifficulty::Advanced => Difficulty::Advanced,
        LxnsDifficulty::Expert => Difficulty::Expert,
        LxnsDifficulty::Master => Difficulty::Master,
        LxnsDifficulty::ReMaster => Difficulty::ReMaster,
    }
}

fn nonnegative_u32(value: i64, field: &'static str) -> Result<u32, ScoreError> {
    u32::try_from(value).map_err(|_| ScoreError::invalid(format!("{field} 必须是 u32 范围整数")))
}

fn require_qq(lookup: &Lookup, source: ScoreSource) -> Result<(), ScoreError> {
    if matches!(lookup, Lookup::Qq(_)) {
        Ok(())
    } else {
        Err(unsupported_username(source))
    }
}

fn unsupported_username(source: ScoreSource) -> ScoreError {
    ScoreError::new(
        ScoreErrorCode::UnsupportedSource,
        format!("username 查询固定使用 Diving-Fish，不能使用 {source:?}"),
    )
}
