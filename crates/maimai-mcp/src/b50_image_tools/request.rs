use std::{path::PathBuf, time::Duration};

use maimai_app::b50_image::{B50ImageDataRequest, B50ImageLookup};
use maimai_core::{GroupId, PlayerUsername, QqId};
use time::OffsetDateTime;

use super::{dto::RenderArgs, error::B50ToolError};

pub(super) enum RenderInput {
    Provided,
    Query(B50ImageDataRequest),
}

pub(super) struct PreparedInput {
    pub input: RenderInput,
    pub timeout: Duration,
}

pub(super) fn input(args: &RenderArgs, now: OffsetDateTime) -> Result<PreparedInput, B50ToolError> {
    let timeout = timeout(args.timeout_ms)?;
    let qq = identifier(args.qq.as_deref())?;
    let username = identifier(args.username.as_deref())?;
    let target = identifier(args.target.as_deref())?;
    let group_id = identifier(args.group_id.as_deref())?
        .map(GroupId::new)
        .transpose()
        .map_err(|_| B50ToolError::invalid("groupId 格式不正确。"))?;
    let provided = usize::from(qq.is_some())
        + usize::from(username.is_some())
        + usize::from(target.is_some())
        + usize::from(args.b50_data.is_some());
    if provided != 1 {
        return Err(B50ToolError::invalid(
            "必须且只能提供 qq、username、target、b50Data 其中一个。",
        ));
    }
    if args.b50_data.is_some() {
        return Ok(PreparedInput {
            input: RenderInput::Provided,
            timeout,
        });
    }
    let lookup = if let Some(qq) = qq {
        B50ImageLookup::Qq(
            QqId::new(qq).map_err(|_| B50ToolError::invalid("qq 必须是数字字符串。"))?,
        )
    } else if let Some(username) = username {
        B50ImageLookup::Username(
            PlayerUsername::new(username)
                .map_err(|_| B50ToolError::invalid("username 格式不正确。"))?,
        )
    } else {
        B50ImageLookup::Target(
            maimai_app::identity::IdentityQuery::new(target.unwrap_or_default())
                .map_err(|_| B50ToolError::invalid("target 格式不正确。"))?,
        )
    };
    Ok(PreparedInput {
        input: RenderInput::Query(B50ImageDataRequest {
            lookup,
            group_id,
            source: None,
            title: args.title.clone(),
            timeout,
            now,
        }),
        timeout,
    })
}

pub(super) fn optional_path(
    value: Option<String>,
    field: &str,
) -> Result<Option<PathBuf>, B50ToolError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.chars().any(char::is_control) {
        return Err(B50ToolError::invalid(format!("{field} 不能包含控制字符。")));
    }
    let value = value.trim();
    Ok((!value.is_empty()).then(|| PathBuf::from(value)))
}

fn identifier(value: Option<&str>) -> Result<Option<String>, B50ToolError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.chars().any(char::is_control) {
        return Err(B50ToolError::invalid("查询标识不能包含控制字符。"));
    }
    Ok((!value.trim().is_empty()).then(|| value.trim().to_owned()))
}

fn timeout(value: Option<u64>) -> Result<Duration, B50ToolError> {
    let value = value.unwrap_or(10_000);
    if !(1_000..=30_000).contains(&value) {
        return Err(B50ToolError::invalid(
            "timeoutMs 必须是 1000 到 30000 之间的整数。",
        ));
    }
    Ok(Duration::from_millis(value))
}
