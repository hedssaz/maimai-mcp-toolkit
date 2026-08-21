use std::fmt;

use maimai_core::{PlayerSelector, PlayerUsername, QqId};
use serde_json::{Map, Value, json};

use crate::{
    DivingFishClient, DivingFishCredentials, DivingFishOperation, DivingFishRequest,
    ProviderEnvelope, RawJsonPayload,
};

use super::{
    DivingFishB50, DivingFishPlayerRecords, DivingFishRatingEntry, DivingFishScoreError,
    PlateVersions, decode,
};

#[derive(Clone)]
pub struct DivingFishScoreClient {
    client: DivingFishClient,
}

impl DivingFishScoreClient {
    pub fn new(client: DivingFishClient) -> Self {
        Self { client }
    }

    pub fn selector(
        qq: Option<QqId>,
        username: Option<PlayerUsername>,
    ) -> Result<PlayerSelector, DivingFishScoreError> {
        match (qq, username) {
            (Some(qq), None) => Ok(PlayerSelector::Qq(qq)),
            (None, Some(username)) => Ok(PlayerSelector::Username(username)),
            (None, None) => Err(DivingFishScoreError::invalid_selector(
                "必须提供 qq 或 username 其中一个",
            )),
            (Some(_), Some(_)) => Err(DivingFishScoreError::invalid_selector(
                "qq 和 username 只能提供其中一个",
            )),
        }
    }

    pub async fn query_b50(
        &self,
        selector: PlayerSelector,
    ) -> Result<DivingFishB50, DivingFishScoreError> {
        let (request, redaction) = b50_request(&selector)?;
        let data = self.execute_data(request, &[redaction]).await?;
        decode::b50(&data, selector)
    }

    pub async fn query_b50_with_raw(
        &self,
        selector: PlayerSelector,
    ) -> Result<ProviderEnvelope<DivingFishB50>, DivingFishScoreError> {
        let (request, redaction) = b50_request(&selector)?;
        let data = self.execute_data(request, &[redaction]).await?;
        let raw = raw_payload(&data)?;
        Ok(ProviderEnvelope::new(decode::b50(&data, selector)?, raw))
    }

    pub async fn query_plate(
        &self,
        selector: PlayerSelector,
        versions: &PlateVersions,
    ) -> Result<DivingFishPlayerRecords, DivingFishScoreError> {
        let (key, value) = selector_field(&selector)?;
        let mut body = Map::new();
        body.insert(key.to_owned(), json!(value));
        body.insert("version".to_owned(), json!(versions.as_slice()));
        let mut redactions = vec![value.to_owned()];
        redactions.extend(versions.as_slice().iter().cloned());
        let request = DivingFishRequest::new(DivingFishOperation::MaimaiQueryPlatePost)
            .with_body(Value::Object(body));
        let data = self.execute_data(request, &redactions).await?;
        decode::records(&data, selector)
    }

    pub async fn query_developer_records(
        &self,
        selector: PlayerSelector,
        credentials: DivingFishCredentials,
    ) -> Result<DivingFishPlayerRecords, DivingFishScoreError> {
        let (request, redaction) = records_request(&selector, credentials)?;
        let data = self.execute_data(request, &[redaction]).await?;
        decode::records(&data, selector)
    }

    pub async fn query_developer_records_with_raw(
        &self,
        selector: PlayerSelector,
        credentials: DivingFishCredentials,
    ) -> Result<ProviderEnvelope<DivingFishPlayerRecords>, DivingFishScoreError> {
        let (request, redaction) = records_request(&selector, credentials)?;
        let data = self.execute_data(request, &[redaction]).await?;
        let raw = raw_payload(&data)?;
        Ok(ProviderEnvelope::new(
            decode::records(&data, selector)?,
            raw,
        ))
    }

    pub async fn rating_ranking(&self) -> Result<Vec<DivingFishRatingEntry>, DivingFishScoreError> {
        let request = DivingFishRequest::new(DivingFishOperation::MaimaiRatingRankingGet);
        let data = self.execute_data(request, &[]).await?;
        decode::ranking(&data)
    }

    async fn execute_data(
        &self,
        request: DivingFishRequest,
        redactions: &[String],
    ) -> Result<Value, DivingFishScoreError> {
        let response = self.client.execute(request).await.map_err(|error| {
            let redactions = redactions.iter().map(String::as_str).collect::<Vec<_>>();
            DivingFishScoreError::provider(error, &redactions)
        })?;
        let data = response.data().cloned().ok_or_else(|| {
            DivingFishScoreError::invalid_response("Diving-Fish 响应缺少 JSON data")
        })?;
        Ok(data)
    }
}

impl fmt::Debug for DivingFishScoreClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DivingFishScoreClient")
            .field("client", &self.client)
            .finish()
    }
}

fn selector_field(selector: &PlayerSelector) -> Result<(&'static str, &str), DivingFishScoreError> {
    match selector {
        PlayerSelector::Qq(value) => Ok(("qq", value.as_str())),
        PlayerSelector::Username(value) => Ok(("username", value.as_str())),
        PlayerSelector::Auto(_) => Err(DivingFishScoreError::invalid_selector(
            "Diving-Fish provider 不解析 auto selector",
        )),
    }
}

fn b50_request(
    selector: &PlayerSelector,
) -> Result<(DivingFishRequest, String), DivingFishScoreError> {
    let (key, value) = selector_field(selector)?;
    let mut body = Map::new();
    body.insert(key.to_owned(), json!(value));
    body.insert("b50".to_owned(), json!("1"));
    Ok((
        DivingFishRequest::new(DivingFishOperation::MaimaiQueryPlayerPost)
            .with_body(Value::Object(body)),
        value.to_owned(),
    ))
}

fn records_request(
    selector: &PlayerSelector,
    credentials: DivingFishCredentials,
) -> Result<(DivingFishRequest, String), DivingFishScoreError> {
    let (key, value) = selector_field(selector)?;
    Ok((
        DivingFishRequest::new(DivingFishOperation::MaimaiDevPlayerRecordsGet)
            .with_query(key, value)
            .with_credentials(credentials),
        value.to_owned(),
    ))
}

fn raw_payload(value: &Value) -> Result<RawJsonPayload, DivingFishScoreError> {
    RawJsonPayload::from_value(value)
        .map_err(|error| DivingFishScoreError::invalid_response(error.to_string()))
}
