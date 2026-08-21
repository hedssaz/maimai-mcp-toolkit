use maimai_app::identity::IdentityError;
use maimai_storage::{IdentityJobError, IdentityJobErrorCode};
use serde_json::json;

use crate::{DispatchError, ToolFailure};

#[derive(Debug)]
pub struct IdentityToolError {
    pub error: IdentityJobError,
}

impl IdentityToolError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            error: IdentityJobError {
                code: IdentityJobErrorCode::InvalidInput,
                message: message.into(),
                status: None,
                body: None,
            },
        }
    }

    pub fn internal() -> Self {
        Self {
            error: IdentityJobError {
                code: IdentityJobErrorCode::Unknown,
                message: "QQ 身份工具内部错误。".to_owned(),
                status: None,
                body: None,
            },
        }
    }
}

impl From<IdentityError> for IdentityToolError {
    fn from(value: IdentityError) -> Self {
        Self {
            error: value.safe_job_error(),
        }
    }
}

impl From<IdentityError> for DispatchError {
    fn from(value: IdentityError) -> Self {
        IdentityToolError::from(value).into()
    }
}

impl From<IdentityToolError> for DispatchError {
    fn from(value: IdentityToolError) -> Self {
        let message = value.error.message.clone();
        let structured = json!({
            "code": value.error.code.as_str(),
            "message": value.error.message,
            "status": value.error.status,
            "body": value.error.body,
        });
        ToolFailure::text(message)
            .with_structured_content(json!({"error": structured}))
            .into()
    }
}
