use maimai_providers::{NapCatError, NapCatErrorCode};
use maimai_storage::{IdentityJobError, IdentityJobErrorCode, StorageError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error(transparent)]
    Provider(#[from] NapCatError),

    #[error(transparent)]
    Storage(#[from] StorageError),

    #[error("invalid identity parameter: {field}")]
    InvalidParameter { field: &'static str },

    #[error("NapCat returned an invalid {field}")]
    InvalidRemoteIdentity { field: &'static str },

    #[error("coalesced identity refresh completed without metadata")]
    MissingRefreshMetadata,

    #[error("identity refresh completion signal closed unexpectedly")]
    RefreshSignalClosed,

    #[error("identity refresh generation overflow")]
    RefreshGenerationOverflow,

    #[error("identity refresh failed: {0}")]
    RefreshFailed(IdentityJobError),

    #[error("identity refresh terminal state could not be persisted")]
    TerminalPersistence,

    #[error("identity refresh task terminated unexpectedly")]
    RefreshTaskJoin,
}

impl IdentityError {
    pub fn safe_job_error(&self) -> IdentityJobError {
        match self {
            Self::Provider(error) => provider_job_error(error),
            Self::Storage(_) => IdentityJobError {
                code: IdentityJobErrorCode::Storage,
                message: "QQ 身份缓存存储失败。".to_owned(),
                status: None,
                body: None,
            },
            Self::InvalidParameter { .. } | Self::InvalidRemoteIdentity { .. } => {
                IdentityJobError {
                    code: IdentityJobErrorCode::InvalidInput,
                    message: "QQ 身份刷新参数或远端身份格式不正确。".to_owned(),
                    status: None,
                    body: None,
                }
            }
            Self::MissingRefreshMetadata
            | Self::RefreshSignalClosed
            | Self::RefreshGenerationOverflow
            | Self::RefreshTaskJoin => IdentityJobError {
                code: IdentityJobErrorCode::Unknown,
                message: "QQ 身份缓存刷新异常结束。".to_owned(),
                status: None,
                body: None,
            },
            Self::RefreshFailed(error) => error.clone(),
            Self::TerminalPersistence => IdentityJobError {
                code: IdentityJobErrorCode::Storage,
                message: "QQ 身份刷新终态暂未写入存储。".to_owned(),
                status: None,
                body: None,
            },
        }
    }
}

fn provider_job_error(error: &NapCatError) -> IdentityJobError {
    let provider_code = error.code();
    let (code, message) = match provider_code {
        NapCatErrorCode::InvalidConfiguration | NapCatErrorCode::InvalidRequest => (
            IdentityJobErrorCode::InvalidInput,
            "NapCat 配置或请求参数不正确。",
        ),
        NapCatErrorCode::Timeout => (IdentityJobErrorCode::Timeout, "NapCat 请求超时。"),
        NapCatErrorCode::Network => (IdentityJobErrorCode::Network, "NapCat 请求失败。"),
        NapCatErrorCode::Http => (IdentityJobErrorCode::Http, "NapCat HTTP 请求失败。"),
        NapCatErrorCode::OneBot => (IdentityJobErrorCode::NapCat, "NapCat OneBot 请求被拒绝。"),
        NapCatErrorCode::InvalidResponse => (
            IdentityJobErrorCode::InvalidResponse,
            "NapCat 返回内容格式不正确。",
        ),
    };
    IdentityJobError {
        code,
        message: message.to_owned(),
        status: error.http_status(),
        body: matches!(
            provider_code,
            NapCatErrorCode::Http | NapCatErrorCode::OneBot | NapCatErrorCode::InvalidResponse
        )
        .then(|| error.body().map(str::to_owned))
        .flatten(),
    }
}
