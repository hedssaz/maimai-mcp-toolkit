use serde::Deserialize;

use super::super::dto::Scalar;

#[derive(Default, Deserialize)]
pub(super) struct RiseScoreArgs {
    pub qq: Option<Scalar>,
    pub username: Option<Scalar>,
    pub level: Option<Scalar>,
    pub score: Option<Scalar>,
    pub algorithm: Option<Scalar>,
    pub source: Option<Scalar>,
    #[serde(rename = "scoreSource")]
    pub score_source_camel: Option<Scalar>,
    pub score_source: Option<Scalar>,
    #[serde(rename = "dataSource")]
    pub data_source_camel: Option<Scalar>,
    pub data_source: Option<Scalar>,
}
