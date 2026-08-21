use serde::Deserialize;

use super::super::dto::Scalar;

#[derive(Default, Deserialize)]
pub(super) struct RatingRankingArgs {
    pub qq: Option<Scalar>,
    pub username: Option<Scalar>,
    pub name: Option<Scalar>,
    #[serde(rename = "startRank")]
    pub start_rank: Option<Scalar>,
    #[serde(rename = "endRank")]
    pub end_rank: Option<Scalar>,
    pub page: Option<Scalar>,
}
