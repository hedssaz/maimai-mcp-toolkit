use serde::Deserialize;

use super::super::dto::Scalar;

#[derive(Default, Deserialize)]
pub(super) struct ScoreListArgs {
    pub qq: Option<Scalar>,
    pub username: Option<Scalar>,
    pub rating: Option<Scalar>,
    pub level: Option<Scalar>,
    pub ds: Option<Scalar>,
    pub page: Option<Scalar>,
    pub source: Option<Scalar>,
    #[serde(rename = "scoreSource")]
    pub score_source_camel: Option<Scalar>,
    pub score_source: Option<Scalar>,
    #[serde(rename = "dataSource")]
    pub data_source_camel: Option<Scalar>,
    pub data_source: Option<Scalar>,
}
