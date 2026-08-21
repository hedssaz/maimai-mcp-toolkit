use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DivingFishGame {
    MaimaiDxProber,
    ChunithmProber,
}

impl DivingFishGame {
    pub const fn path_segment(self) -> &'static str {
        match self {
            Self::MaimaiDxProber => "maimaidxprober",
            Self::ChunithmProber => "chunithmprober",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

impl HttpMethod {
    pub(super) const fn as_reqwest(self) -> reqwest::Method {
        match self {
            Self::Get => reqwest::Method::GET,
            Self::Post => reqwest::Method::POST,
            Self::Put => reqwest::Method::PUT,
            Self::Delete => reqwest::Method::DELETE,
        }
    }

    pub(super) const fn accepts_body(self) -> bool {
        !matches!(self, Self::Get)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthRequirement {
    None,
    LoginCredentials,
    Login,
    LoginOrImportToken,
    DeveloperToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationMetadata {
    pub game: DivingFishGame,
    pub method: HttpMethod,
    pub path: &'static str,
    pub auth: AuthRequirement,
    pub mutating: bool,
    pub destructive: bool,
    pub raw_body_allowed: bool,
    pub http: bool,
}

macro_rules! define_operations {
    ($(
        $variant:ident => {
            name: $name:literal,
            game: $game:ident,
            method: $method:ident,
            path: $path:literal,
            auth: $auth:ident,
            mutating: $mutating:literal,
            destructive: $destructive:literal,
            raw_body_allowed: $raw_body_allowed:literal,
            http: $http:literal
        }
    ),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum DivingFishOperation {
            $($variant),+
        }

        impl DivingFishOperation {
            pub const ALL: [Self; define_operations!(@count $($variant),+)] = [
                $(Self::$variant),+
            ];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $name),+
                }
            }

            pub const fn metadata(self) -> OperationMetadata {
                match self {
                    $(Self::$variant => OperationMetadata {
                        game: DivingFishGame::$game,
                        method: HttpMethod::$method,
                        path: $path,
                        auth: AuthRequirement::$auth,
                        mutating: $mutating,
                        destructive: $destructive,
                        raw_body_allowed: $raw_body_allowed,
                        http: $http,
                    }),+
                }
            }
        }

        impl fmt::Display for DivingFishOperation {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for DivingFishOperation {
            type Err = UnknownOperation;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($name => Ok(Self::$variant)),+,
                    _ => Err(UnknownOperation(value.to_owned())),
                }
            }
        }
    };
    (@replace $_value:ident) => { () };
    (@count $($value:ident),+) => { <[()]>::len(&[$(define_operations!(@replace $value)),+]) };
}

define_operations! {
    MaimaiLogin => {
        name: "maimai_login",
        game: MaimaiDxProber,
        method: Post,
        path: "/login",
        auth: LoginCredentials,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    MaimaiPlayerAgreementGet => {
        name: "maimai_player_agreement_get",
        game: MaimaiDxProber,
        method: Get,
        path: "/player/agreement",
        auth: Login,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    MaimaiPlayerAgreementPost => {
        name: "maimai_player_agreement_post",
        game: MaimaiDxProber,
        method: Post,
        path: "/player/agreement",
        auth: Login,
        mutating: true,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    MaimaiPlayerProfileGet => {
        name: "maimai_player_profile_get",
        game: MaimaiDxProber,
        method: Get,
        path: "/player/profile",
        auth: Login,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    MaimaiPlayerProfilePost => {
        name: "maimai_player_profile_post",
        game: MaimaiDxProber,
        method: Post,
        path: "/player/profile",
        auth: Login,
        mutating: true,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    MaimaiPlayerImportTokenPut => {
        name: "maimai_player_import_token_put",
        game: MaimaiDxProber,
        method: Put,
        path: "/player/import_token",
        auth: Login,
        mutating: true,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    MaimaiMusicDataGet => {
        name: "maimai_music_data_get",
        game: MaimaiDxProber,
        method: Get,
        path: "/music_data",
        auth: None,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    MaimaiPlayerRecordsGet => {
        name: "maimai_player_records_get",
        game: MaimaiDxProber,
        method: Get,
        path: "/player/records",
        auth: LoginOrImportToken,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    MaimaiPlayerTestDataGet => {
        name: "maimai_player_test_data_get",
        game: MaimaiDxProber,
        method: Get,
        path: "/player/test_data",
        auth: None,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    MaimaiDevPlayerRecordsGet => {
        name: "maimai_dev_player_records_get",
        game: MaimaiDxProber,
        method: Get,
        path: "/dev/player/records",
        auth: DeveloperToken,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    MaimaiDevPlayerRecordPost => {
        name: "maimai_dev_player_record_post",
        game: MaimaiDxProber,
        method: Post,
        path: "/dev/player/record",
        auth: DeveloperToken,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    MaimaiQueryPlayerPost => {
        name: "maimai_query_player_post",
        game: MaimaiDxProber,
        method: Post,
        path: "/query/player",
        auth: None,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    MaimaiQueryPlatePost => {
        name: "maimai_query_plate_post",
        game: MaimaiDxProber,
        method: Post,
        path: "/query/plate",
        auth: None,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    MaimaiCoverUrl => {
        name: "maimai_cover_url",
        game: MaimaiDxProber,
        method: Get,
        path: "*/covers",
        auth: None,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: false
    },
    MaimaiRatingRankingGet => {
        name: "maimai_rating_ranking_get",
        game: MaimaiDxProber,
        method: Get,
        path: "/rating_ranking",
        auth: None,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    MaimaiPlayerUpdateRecordsPost => {
        name: "maimai_player_update_records_post",
        game: MaimaiDxProber,
        method: Post,
        path: "/player/update_records",
        auth: LoginOrImportToken,
        mutating: true,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    MaimaiPlayerUpdateRecordsHtmlPost => {
        name: "maimai_player_update_records_html_post",
        game: MaimaiDxProber,
        method: Post,
        path: "/player/update_records_html",
        auth: Login,
        mutating: true,
        destructive: false,
        raw_body_allowed: true,
        http: true
    },
    MaimaiPlayerUpdateRecordPost => {
        name: "maimai_player_update_record_post",
        game: MaimaiDxProber,
        method: Post,
        path: "/player/update_record",
        auth: LoginOrImportToken,
        mutating: true,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    MaimaiPlayerDeleteRecordsDelete => {
        name: "maimai_player_delete_records_delete",
        game: MaimaiDxProber,
        method: Delete,
        path: "/player/delete_records",
        auth: LoginOrImportToken,
        mutating: true,
        destructive: true,
        raw_body_allowed: false,
        http: true
    },
    MaimaiChartStatsGet => {
        name: "maimai_chart_stats_get",
        game: MaimaiDxProber,
        method: Get,
        path: "/chart_stats",
        auth: None,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    ChunithmMusicDataGet => {
        name: "chunithm_music_data_get",
        game: ChunithmProber,
        method: Get,
        path: "/music_data",
        auth: None,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    ChunithmLatestVersionGet => {
        name: "chunithm_latest_version_get",
        game: ChunithmProber,
        method: Get,
        path: "/latest_version",
        auth: None,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    ChunithmPlayerRecordsGet => {
        name: "chunithm_player_records_get",
        game: ChunithmProber,
        method: Get,
        path: "/player/records",
        auth: LoginOrImportToken,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    ChunithmPlayerTestDataGet => {
        name: "chunithm_player_test_data_get",
        game: ChunithmProber,
        method: Get,
        path: "/player/test_data",
        auth: None,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    ChunithmDevPlayerRecordsGet => {
        name: "chunithm_dev_player_records_get",
        game: ChunithmProber,
        method: Get,
        path: "/dev/player/records",
        auth: DeveloperToken,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    ChunithmUpdateRecordsHtmlPost => {
        name: "chunithm_update_records_html_post",
        game: ChunithmProber,
        method: Post,
        path: "/player/update_records_html",
        auth: LoginOrImportToken,
        mutating: true,
        destructive: false,
        raw_body_allowed: true,
        http: true
    },
    ChunithmDeleteRecordsDelete => {
        name: "chunithm_delete_records_delete",
        game: ChunithmProber,
        method: Delete,
        path: "/player/delete_records",
        auth: LoginOrImportToken,
        mutating: true,
        destructive: true,
        raw_body_allowed: false,
        http: true
    },
    ChunithmQueryPlayerPost => {
        name: "chunithm_query_player_post",
        game: ChunithmProber,
        method: Post,
        path: "/query/player",
        auth: None,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    PublicCountViewGet => {
        name: "public_count_view_get",
        game: MaimaiDxProber,
        method: Get,
        path: "/count_view",
        auth: None,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    PublicAliveCheckGet => {
        name: "public_alive_check_get",
        game: MaimaiDxProber,
        method: Get,
        path: "/alive_check",
        auth: None,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    PublicMessageGet => {
        name: "public_message_get",
        game: MaimaiDxProber,
        method: Get,
        path: "/message",
        auth: None,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    PublicMessagePost => {
        name: "public_message_post",
        game: MaimaiDxProber,
        method: Post,
        path: "/message",
        auth: Login,
        mutating: true,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
    PublicAdvertisementsGet => {
        name: "public_advertisements_get",
        game: MaimaiDxProber,
        method: Get,
        path: "/advertisements",
        auth: None,
        mutating: false,
        destructive: false,
        raw_body_allowed: false,
        http: true
    },
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error("未知 Diving-Fish operation：{0}")]
pub struct UnknownOperation(String);

impl UnknownOperation {
    pub fn value(&self) -> &str {
        &self.0
    }
}
