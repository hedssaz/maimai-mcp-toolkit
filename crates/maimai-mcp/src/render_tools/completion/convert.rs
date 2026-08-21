use maimai_app::{
    completion::{
        AchievementTarget, CompletionIdentity, CompletionTarget, LevelProgressRequest, PlateSpec,
        ProgressCategory, RatingTableRequest,
    },
    scores::Lookup,
};
use maimai_catalog::{PlateName, PlateServer};
use maimai_core::{PlayerUsername, QqId, ScoreSource};
use maimai_render::{FullComboStatus, FullSyncStatus};

use super::{
    CompletionSurface, CompletionToolError,
    dto::{IdentityDto, PlateArgs, PlateBatchArgs, PlateItemDto, ProgressArgs, RatingArgs, Scalar},
};
use crate::render_tools::score_source;

pub(super) fn plate(
    args: PlateArgs,
    surface: CompletionSurface,
) -> Result<(CompletionIdentity, PlateSpec), CompletionToolError> {
    let identity = identity(&args.identity, surface)?;
    let spec = plate_spec(
        args.version.as_ref().or(args.plate.as_ref()),
        args.plan.as_deref(),
        args.server.as_deref(),
        surface,
    )?;
    Ok((identity, spec))
}

pub(super) fn plate_batch(
    args: PlateBatchArgs,
    surface: CompletionSurface,
) -> Result<(CompletionIdentity, Vec<PlateSpec>), CompletionToolError> {
    let identity = identity(&args.identity, surface)?;
    let mut specs = Vec::new();
    if !args.items.is_empty() {
        for (index, item) in args.items.into_iter().enumerate() {
            let spec = match item {
                PlateItemDto::Object(item) => plate_spec(
                    item.version.as_ref().or(item.plate.as_ref()),
                    item.plan.as_deref().or(args.plan.as_deref()),
                    item.server.as_deref().or(args.server.as_deref()),
                    surface,
                )?,
                PlateItemDto::Version(version) => plate_spec(
                    Some(&version),
                    args.plan.as_deref(),
                    args.server.as_deref(),
                    surface,
                )?,
                PlateItemDto::Invalid(_) => {
                    return Err(CompletionToolError::invalid(format!(
                        "items[{}] 必须是版本字符串、数字或牌子对象",
                        index + 1
                    )));
                }
            };
            specs.push(spec);
        }
    } else {
        let versions = args
            .versions
            .map(|values| values.into_vec())
            .unwrap_or_else(|| args.version.into_iter().collect::<Vec<_>>());
        let plans = args
            .plans
            .map(|values| values.into_vec())
            .unwrap_or_default();
        for (index, version) in versions.iter().enumerate() {
            let plan = if plans.len() == versions.len() {
                plans.get(index).map(String::as_str)
            } else {
                plans.first().map(String::as_str)
            }
            .or(args.plan.as_deref());
            specs.push(plate_spec(
                Some(version),
                plan,
                args.server.as_deref(),
                surface,
            )?);
        }
    }
    Ok((identity, specs))
}

pub(super) fn progress(
    args: ProgressArgs,
    surface: CompletionSurface,
) -> Result<LevelProgressRequest, CompletionToolError> {
    let identity = identity(&args.identity, surface)?;
    let level = args
        .level
        .as_ref()
        .map_or_else(|| "14".to_owned(), Scalar::level_text);
    let target = progress_target(args.plan.as_deref().unwrap_or("sss"))?;
    let server = parse_server(args.server.as_deref(), surface, false)?.unwrap_or(PlateServer::Cn);
    let category = match args
        .category
        .as_deref()
        .unwrap_or("default")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "default" => ProgressCategory::Overview,
        "completed" => ProgressCategory::Completed,
        "unfinished" => ProgressCategory::Unfinished,
        "notstarted" | "not_started" => ProgressCategory::NotStarted,
        _ => {
            return Err(CompletionToolError::invalid(
                "category 必须是 default/completed/unfinished/notstarted",
            ));
        }
    };
    let page = args.page.unwrap_or(1);
    let page = usize::try_from(page)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| CompletionToolError::invalid("page 必须大于 0"))?;
    Ok(LevelProgressRequest {
        identity,
        level,
        target,
        server,
        category,
        page,
    })
}

pub(super) fn rating(
    args: RatingArgs,
    surface: CompletionSurface,
) -> Result<RatingTableRequest, CompletionToolError> {
    let request = RatingTableRequest {
        identity: identity(&args.identity, surface)?,
        level: args
            .rating
            .as_ref()
            .map_or_else(|| "14".to_owned(), Scalar::level_text),
        mode: if args.isfc {
            maimai_render::RatingTableMode::FullCombo
        } else {
            maimai_render::RatingTableMode::Achievement
        },
    };
    request.validate().map_err(CompletionToolError::from)?;
    Ok(request)
}

fn plate_spec(
    version: Option<&Scalar>,
    plan: Option<&str>,
    server: Option<&str>,
    surface: CompletionSurface,
) -> Result<PlateSpec, CompletionToolError> {
    let version = version.map_or_else(|| "真".to_owned(), Scalar::text);
    let version =
        PlateName::new(version).map_err(|_| CompletionToolError::invalid("version 格式不正确"))?;
    Ok(PlateSpec {
        version,
        target: plate_target(plan.unwrap_or("极"))?,
        server: parse_server(server, surface, true)?,
    })
}

fn identity(
    args: &IdentityDto,
    surface: CompletionSurface,
) -> Result<CompletionIdentity, CompletionToolError> {
    let qq = args
        .qq
        .as_ref()
        .map(Scalar::text)
        .filter(|value| !value.trim().is_empty());
    let username = args
        .username
        .as_ref()
        .map(Scalar::text)
        .filter(|value| !value.trim().is_empty());
    let lookup = match (qq, username) {
        (Some(_), Some(_)) => {
            return Err(CompletionToolError::invalid("qq 和 username 只能提供一个"));
        }
        (Some(value), None) => QqId::new(value.trim())
            .map(Lookup::Qq)
            .map_err(|_| CompletionToolError::invalid("qq 必须是数字字符串"))?,
        (None, Some(value)) => PlayerUsername::new(value.trim())
            .map(Lookup::Username)
            .map_err(|_| CompletionToolError::invalid("username 格式不正确"))?,
        (None, None) => return Err(CompletionToolError::invalid("需要提供 qq 或 username")),
    };
    let source_value = args
        .score_source_camel
        .as_deref()
        .or(args.score_source.as_deref())
        .or(args.data_source_camel.as_deref())
        .or(args.data_source.as_deref())
        .or(args.source.as_deref());
    if surface == CompletionSurface::Public && source_value.is_some() {
        return Err(CompletionToolError::invalid(
            "public surface 不支持 source 参数",
        ));
    }
    let source = if matches!(lookup, Lookup::Username(_)) {
        None
    } else {
        source_value.map(parse_source).transpose()?
    };
    Ok(CompletionIdentity { lookup, source })
}

fn parse_server(
    value: Option<&str>,
    surface: CompletionSurface,
    custom: bool,
) -> Result<Option<PlateServer>, CompletionToolError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let server = match value.to_ascii_lowercase().as_str() {
        "cn" | "china" | "中国" | "国服" => PlateServer::Cn,
        "jp" | "japan" | "日本" | "日服" => PlateServer::Jp,
        "custom" | "自定义" | "自定義" if custom => PlateServer::Custom,
        _ => {
            return Err(CompletionToolError::invalid(if custom {
                "server 必须是 cn、jp 或 custom"
            } else {
                "server 必须是 cn 或 jp"
            }));
        }
    };
    if surface == CompletionSurface::Public && server == PlateServer::Jp {
        return Err(CompletionToolError::invalid(
            "当前分支不支持日服/dxdata 曲目数据。",
        ));
    }
    Ok(Some(server))
}

fn plate_target(value: &str) -> Result<CompletionTarget, CompletionToolError> {
    match value.trim() {
        "极" | "極" | "级" => Ok(CompletionTarget::PlateExtreme),
        "将" => Ok(CompletionTarget::PlateGeneral),
        "者" => Ok(CompletionTarget::Achievement(AchievementTarget::Eighty)),
        "神" => Ok(CompletionTarget::PlateGod),
        "舞舞" => Ok(CompletionTarget::PlateDance),
        _ => Err(CompletionToolError::invalid(
            "plan 必须是 极、将、神、舞舞 或兼容目标 者",
        )),
    }
}

fn progress_target(value: &str) -> Result<CompletionTarget, CompletionToolError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "s" => Ok(CompletionTarget::Achievement(AchievementTarget::S)),
        "s+" | "sp" => Ok(CompletionTarget::Achievement(AchievementTarget::SPlus)),
        "ss" => Ok(CompletionTarget::Achievement(AchievementTarget::Ss)),
        "ss+" | "ssp" => Ok(CompletionTarget::Achievement(AchievementTarget::SsPlus)),
        "sss" => Ok(CompletionTarget::Achievement(AchievementTarget::Sss)),
        "sss+" | "sssp" => Ok(CompletionTarget::Achievement(AchievementTarget::SssPlus)),
        "fc" => Ok(CompletionTarget::FullCombo(FullComboStatus::FullCombo)),
        "fc+" | "fcp" => Ok(CompletionTarget::FullCombo(FullComboStatus::FullComboPlus)),
        "ap" => Ok(CompletionTarget::FullCombo(FullComboStatus::AllPerfect)),
        "ap+" | "app" => Ok(CompletionTarget::FullCombo(FullComboStatus::AllPerfectPlus)),
        "fs" => Ok(CompletionTarget::FullSync(FullSyncStatus::FullSync)),
        "fs+" | "fsp" => Ok(CompletionTarget::FullSync(FullSyncStatus::FullSyncPlus)),
        "fsd" | "fdx" => Ok(CompletionTarget::FullSync(FullSyncStatus::FullSyncDeluxe)),
        "fsd+" | "fdx+" | "fsdp" | "fdxp" => Ok(CompletionTarget::FullSync(
            FullSyncStatus::FullSyncDeluxePlus,
        )),
        _ => Err(CompletionToolError::invalid(
            "plan 必须是 s/s+/ss/ss+/sss/sss+/fc/ap/fsd",
        )),
    }
}

fn parse_source(value: &str) -> Result<ScoreSource, CompletionToolError> {
    score_source::parse(value)
        .ok_or_else(|| CompletionToolError::invalid("source 必须是 local、sy 或 lxns"))
}

#[cfg(test)]
mod tests {
    use maimai_render::FullSyncStatus;

    use super::progress_target;
    use maimai_app::completion::CompletionTarget;

    #[test]
    fn advertised_fsd_is_not_rejected_or_guessed() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            progress_target("fsd")?,
            CompletionTarget::FullSync(FullSyncStatus::FullSyncDeluxe)
        );
        Ok(())
    }
}
