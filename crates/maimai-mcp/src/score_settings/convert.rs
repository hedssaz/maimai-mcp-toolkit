use maimai_app::score_settings::{DeveloperTokenStatus, ScoreSourceSetting};
use maimai_core::{QqId, ScoreSource};
use time::OffsetDateTime;

use super::{
    dto::{SwitchSourceDto, TokenStatusDto},
    error::ScoreSettingsToolError,
    format,
};

pub fn qq(value: Option<String>) -> Result<QqId, ScoreSettingsToolError> {
    QqId::new(value.unwrap_or_default())
        .map_err(|_| ScoreSettingsToolError::invalid("必须提供 qq。"))
}

pub fn score_source(value: Option<String>) -> Result<ScoreSource, ScoreSettingsToolError> {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
        return Ok(ScoreSource::DivingFish);
    };
    let normalized = value.trim().to_lowercase();
    match normalized.as_str() {
        "sy" | "水鱼" | "diving-fish" | "divingfish" | "waterfish" => Ok(ScoreSource::DivingFish),
        "local" | "本地" | "cache" | "缓存" => Ok(ScoreSource::Local),
        "lxns" | "落雪" | "luoxue" => Ok(ScoreSource::Lxns),
        _ => Err(ScoreSettingsToolError::invalid(
            "source 必须是 local、sy 或 lxns。",
        )),
    }
}

pub fn token_status_dto(status: DeveloperTokenStatus) -> TokenStatusDto {
    TokenStatusDto {
        bound: status.bound,
        updated_at: status.updated_at.map(timestamp),
        security_notice: status.security_notice,
    }
}

pub fn source_dto(setting: ScoreSourceSetting, lxns_allowed: bool) -> SwitchSourceDto {
    let preferred_source = source_name(setting.source);
    let source_label = source_label(setting.source);
    SwitchSourceDto {
        qq: setting.qq.as_str().to_owned(),
        preferred_source,
        source_label,
        text: format::source_switched(source_label, lxns_allowed),
    }
}

pub const fn source_name(source: ScoreSource) -> &'static str {
    match source {
        ScoreSource::DivingFish => "sy",
        ScoreSource::Lxns => "lxns",
        ScoreSource::Local => "local",
        ScoreSource::OfficialCn => "official_cn",
    }
}

pub const fn source_label(source: ScoreSource) -> &'static str {
    match source {
        ScoreSource::DivingFish => "水鱼",
        ScoreSource::Lxns => "落雪",
        ScoreSource::Local => "本地缓存",
        ScoreSource::OfficialCn => "国服",
    }
}

fn timestamp(value: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+00:00",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second(),
    )
}
