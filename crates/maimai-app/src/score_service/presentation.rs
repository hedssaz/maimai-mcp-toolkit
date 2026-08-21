use maimai_core::QqId;
use maimai_providers::LxnsPlayer;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::scores::Lookup;

use super::{
    PlayerPresentationProfile, PlayerPresentationTrophyColor, PlayerScoreService,
    PlayerScoreServiceError, PlayerScoreServiceErrorCode, helpers::oauth_subject, lxns,
};

impl PlayerScoreService {
    pub async fn lxns_player_profile(
        &self,
        qq: &QqId,
        now: i64,
    ) -> Result<PlayerPresentationProfile, PlayerScoreServiceError> {
        let access = self.lxns()?;
        let subject = oauth_subject(&Lookup::Qq(qq.clone()))?;
        let player = lxns::player(&access.oauth, &access.endpoint, subject, now).await?;
        presentation(player)
    }
}

fn presentation(player: LxnsPlayer) -> Result<PlayerPresentationProfile, PlayerScoreServiceError> {
    let upload_time = player
        .upload_time
        .as_deref()
        .map(|value| OffsetDateTime::parse(value, &Rfc3339))
        .transpose()
        .map_err(|_| {
            PlayerScoreServiceError::new(
                PlayerScoreServiceErrorCode::Provider,
                "LXNS 玩家资料时间格式不正确",
            )
        })?;
    Ok(PlayerPresentationProfile {
        nickname: player.name,
        rating: player.rating,
        course_rank: player.course_rank,
        class_rank: player.class_rank,
        star: player.star,
        trophy_id: player.trophy.as_ref().map(|value| value.get()),
        trophy_name: player
            .trophy
            .as_ref()
            .and_then(|value| value.name().map(str::to_owned)),
        trophy_color: player
            .trophy
            .as_ref()
            .and_then(|value| trophy_color(value.color())),
        icon_id: player.icon.as_ref().map(|value| value.get()),
        plate_id: player.name_plate.as_ref().map(|value| value.get()),
        frame_id: player.frame.as_ref().map(|value| value.get()),
        upload_time,
    })
}

fn trophy_color(value: Option<&str>) -> Option<PlayerPresentationTrophyColor> {
    match value?.trim().to_ascii_lowercase().as_str() {
        "normal" => Some(PlayerPresentationTrophyColor::Normal),
        "bronze" => Some(PlayerPresentationTrophyColor::Bronze),
        "silver" => Some(PlayerPresentationTrophyColor::Silver),
        "gold" => Some(PlayerPresentationTrophyColor::Gold),
        "rainbow" => Some(PlayerPresentationTrophyColor::Rainbow),
        _ => None,
    }
}
