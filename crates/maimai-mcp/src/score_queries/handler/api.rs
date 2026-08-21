use std::collections::BTreeMap;

use maimai_providers::{
    AuthRequirement, DivingFishGame, DivingFishOperation, DivingFishResponse, HttpMethod,
};
use secrecy::ExposeSecret;
use serde_json::{Value, json};

use crate::{DispatchError, ToolOutput};

use super::{ScoreQueryHandler, within};
use crate::score_queries::{
    convert,
    dto::{DivingFishApiArgs, ListApiArgs},
    error::ScoreQueryToolError,
    format,
};

impl ScoreQueryHandler {
    pub(super) fn list_apis(&self, args: ListApiArgs) -> Result<ToolOutput, DispatchError> {
        let game = args.game.as_deref().map(parse_game).transpose()?;
        let auth = args.auth.as_deref().map(parse_auth).transpose()?;
        let apis = DivingFishOperation::ALL
            .into_iter()
            .filter(|operation| game.is_none_or(|value| operation.metadata().game == value))
            .filter(|operation| auth.is_none_or(|value| operation.metadata().auth == value))
            .filter(|operation| args.include_mutating || !operation.metadata().mutating)
            .map(|operation| (operation.as_str(), metadata(operation)))
            .collect::<BTreeMap<_, _>>();
        let structured = json!({"apis":apis});
        let text = format::pretty_json(&structured["apis"])?;
        Ok(ToolOutput::text(text).with_structured_content(structured))
    }

    pub(super) async fn diving_fish_api(
        &self,
        args: DivingFishApiArgs,
    ) -> Result<ToolOutput, DispatchError> {
        let include_headers = args.include_headers;
        let timeout = convert::timeout(args.timeout_ms)?;
        let bound = self
            .settings
            .developer_token()
            .await?
            .map(|token| token.token().clone());
        let request = convert::generic_request(args, bound)?;
        let response = within(timeout, async {
            self.diving_fish
                .execute(request)
                .await
                .map_err(ScoreQueryToolError::provider)
        })
        .await?;
        let structured = response_value(&response, include_headers);
        let text = api_text(&structured);
        Ok(ToolOutput::text(text).with_structured_content(structured))
    }
}

fn parse_game(value: &str) -> Result<DivingFishGame, ScoreQueryToolError> {
    match value {
        "maimaidxprober" => Ok(DivingFishGame::MaimaiDxProber),
        "chunithmprober" => Ok(DivingFishGame::ChunithmProber),
        _ => Err(ScoreQueryToolError::invalid("game 格式不正确。")),
    }
}

fn parse_auth(value: &str) -> Result<AuthRequirement, ScoreQueryToolError> {
    match value {
        "none" => Ok(AuthRequirement::None),
        "login_credentials" => Ok(AuthRequirement::LoginCredentials),
        "login" => Ok(AuthRequirement::Login),
        "login_or_import_token" => Ok(AuthRequirement::LoginOrImportToken),
        "developer_token" => Ok(AuthRequirement::DeveloperToken),
        _ => Err(ScoreQueryToolError::invalid("auth 格式不正确。")),
    }
}

fn metadata(operation: DivingFishOperation) -> Value {
    let value = operation.metadata();
    json!({
        "operation":operation.as_str(),
        "game":game(value.game),
        "method":method(value.method),
        "path":value.path,
        "url":value.http.then(|| format!("https://www.diving-fish.com/api/{}{}", value.game.path_segment(), value.path)),
        "auth":auth(value.auth),
        "keyRequired":key_required(value.auth),
        "mutating":value.mutating,
        "destructive":value.destructive,
        "requiresConfirmation":value.mutating,
        "description":description(operation),
        "queryHint":query_hint(operation),
        "bodyHint":body_hint(operation),
    })
}

fn response_value(response: &DivingFishResponse, include_headers: bool) -> Value {
    let headers = include_headers.then(|| {
        response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_owned(), Value::from(value)))
            })
            .collect::<serde_json::Map<_, _>>()
    });
    json!({
        "operation":response.operation().as_str(),
        "endpoint":metadata(response.operation()),
        "status":response.status(),
        "url":response.url().as_str(),
        "headers":headers,
        "data":response.data(),
        "text":response.text(),
        "jwtToken":response.jwt_token().map(|token|token.expose_secret()),
    })
}

fn api_text(value: &Value) -> String {
    let mut lines = vec![format!(
        "{} -> HTTP {}",
        value["operation"].as_str().unwrap_or("-"),
        value["status"].as_u64().unwrap_or(0)
    )];
    if let Some(data) = value.get("data").filter(|value| !value.is_null()) {
        if let Ok(rendered) = serde_json::to_string_pretty(data) {
            lines.push(rendered);
        }
    } else if let Some(text) = value["text"].as_str() {
        lines.push(text.to_owned());
    }
    if value["jwtToken"].is_string() {
        lines.push("jwtToken 已返回，请妥善保管。".to_owned());
    }
    lines.join("\n")
}

const fn game(value: DivingFishGame) -> &'static str {
    match value {
        DivingFishGame::MaimaiDxProber => "maimaidxprober",
        DivingFishGame::ChunithmProber => "chunithmprober",
    }
}

const fn method(value: HttpMethod) -> &'static str {
    match value {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
        HttpMethod::Put => "PUT",
        HttpMethod::Delete => "DELETE",
    }
}

const fn auth(value: AuthRequirement) -> &'static str {
    match value {
        AuthRequirement::None => "none",
        AuthRequirement::LoginCredentials => "login_credentials",
        AuthRequirement::Login => "login",
        AuthRequirement::LoginOrImportToken => "login_or_import_token",
        AuthRequirement::DeveloperToken => "developer_token",
    }
}

fn key_required(value: AuthRequirement) -> Value {
    match value {
        AuthRequirement::DeveloperToken => Value::from("Developer-Token"),
        AuthRequirement::LoginOrImportToken => Value::from("Import-Token 或登录 jwt_token"),
        _ => Value::Bool(false),
    }
}

fn query_hint(operation: DivingFishOperation) -> Value {
    match operation {
        DivingFishOperation::MaimaiDevPlayerRecordsGet
        | DivingFishOperation::ChunithmDevPlayerRecordsGet => json!({"qq":"123456789"}),
        DivingFishOperation::MaimaiCoverUrl => json!({"song_id":38}),
        DivingFishOperation::ChunithmUpdateRecordsHtmlPost => json!({"recent":0}),
        _ => Value::Null,
    }
}

fn body_hint(operation: DivingFishOperation) -> Value {
    match operation {
        DivingFishOperation::MaimaiLogin => {
            json!({"username":"your_username","password":"your_password"})
        }
        DivingFishOperation::MaimaiPlayerAgreementPost => json!({"accept_agreement":true}),
        DivingFishOperation::MaimaiPlayerProfilePost => {
            json!({"nickname":"new_nickname","privacy":false})
        }
        DivingFishOperation::MaimaiDevPlayerRecordPost => {
            json!({"qq":"123456789","music_id":[11466]})
        }
        DivingFishOperation::MaimaiQueryPlayerPost => json!({"qq":"123456789","b50":"1"}),
        DivingFishOperation::MaimaiQueryPlatePost => {
            json!({"qq":"123456789","version":["maimai でらっくす FESTiVAL PLUS"]})
        }
        DivingFishOperation::ChunithmQueryPlayerPost => json!({"qq":"123456789"}),
        DivingFishOperation::PublicMessagePost => json!({"text":"早","nickname":""}),
        _ => Value::Null,
    }
}

fn description(operation: DivingFishOperation) -> &'static str {
    match operation {
        DivingFishOperation::MaimaiLogin => {
            "使用 Diving-Fish 用户名和密码登录，返回 jwt_token cookie。"
        }
        DivingFishOperation::MaimaiPlayerAgreementGet => "获取当前登录用户是否同意用户协议。",
        DivingFishOperation::MaimaiPlayerAgreementPost => "更新当前登录用户是否同意用户协议。",
        DivingFishOperation::MaimaiPlayerProfileGet => "获取当前登录用户资料。",
        DivingFishOperation::MaimaiPlayerProfilePost => "更新当前登录用户资料。",
        DivingFishOperation::MaimaiPlayerImportTokenPut => "生成新的 Import-Token 并覆盖旧 token。",
        DivingFishOperation::MaimaiMusicDataGet => {
            "获取 maimai DX 歌曲数据。支持 If-None-Match 缓存校验。"
        }
        DivingFishOperation::MaimaiPlayerRecordsGet => "获取当前用户 maimai 完整成绩。",
        DivingFishOperation::MaimaiPlayerTestDataGet => "获取 maimai 完整成绩测试数据。",
        DivingFishOperation::MaimaiDevPlayerRecordsGet => {
            "通过 Developer-Token 获取指定用户 maimai 完整成绩。"
        }
        DivingFishOperation::MaimaiDevPlayerRecordPost => {
            "通过 Developer-Token 获取指定用户指定歌曲的 maimai 单曲成绩。"
        }
        DivingFishOperation::MaimaiQueryPlayerPost => {
            "无需验证查询用户 maimai 简略成绩。B50 需要 body 带 b50。"
        }
        DivingFishOperation::MaimaiQueryPlatePost => {
            "按版本获取用户 maimai 成绩，取决于用户隐私设置。"
        }
        DivingFishOperation::MaimaiCoverUrl => "按歌曲 ID 生成封面 URL。",
        DivingFishOperation::MaimaiRatingRankingGet => "获取公开用户 username-rating 数据。",
        DivingFishOperation::MaimaiPlayerUpdateRecordsPost => "批量更新当前用户 maimai 成绩。",
        DivingFishOperation::MaimaiPlayerUpdateRecordsHtmlPost => {
            "通过 HTML 源码导入 maimai 成绩。"
        }
        DivingFishOperation::MaimaiPlayerUpdateRecordPost => "更新当前用户 maimai 单曲成绩。",
        DivingFishOperation::MaimaiPlayerDeleteRecordsDelete => "删除当前用户全部 maimai 成绩。",
        DivingFishOperation::MaimaiChartStatsGet => "获取 maimai 谱面拟合难度和分布统计。",
        DivingFishOperation::ChunithmMusicDataGet => {
            "获取 CHUNITHM 歌曲数据。支持 If-None-Match 缓存校验。"
        }
        DivingFishOperation::ChunithmLatestVersionGet => "获取 CHUNITHM 当前新曲版本标识。",
        DivingFishOperation::ChunithmPlayerRecordsGet => "获取当前用户 CHUNITHM 完整成绩。",
        DivingFishOperation::ChunithmPlayerTestDataGet => "获取 CHUNITHM 测试成绩数据。",
        DivingFishOperation::ChunithmDevPlayerRecordsGet => {
            "通过 Developer-Token 获取指定用户 CHUNITHM 完整成绩。"
        }
        DivingFishOperation::ChunithmUpdateRecordsHtmlPost => "通过 HTML 源码导入 CHUNITHM 成绩。",
        DivingFishOperation::ChunithmDeleteRecordsDelete => "删除当前用户全部 CHUNITHM 成绩。",
        DivingFishOperation::ChunithmQueryPlayerPost => {
            "无需验证查询用户 CHUNITHM 简略成绩（b30+n20）。"
        }
        DivingFishOperation::PublicCountViewGet => "获取查分器主页 views 次数。",
        DivingFishOperation::PublicAliveCheckGet => "验证服务器状态。",
        DivingFishOperation::PublicMessageGet => "获取查分器主页今日留言。",
        DivingFishOperation::PublicMessagePost => "提交查分器主页今日留言。",
        DivingFishOperation::PublicAdvertisementsGet => "获取查分器主页广告。",
    }
}
