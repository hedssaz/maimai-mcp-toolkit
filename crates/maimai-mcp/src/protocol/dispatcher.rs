use std::future::Future;

use rmcp::model::ContentBlock;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use thiserror::Error;

/// 已完成路由检查、可交给业务层执行的一次工具调用。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    name: String,
    arguments: Map<String, Value>,
}

impl ToolCall {
    pub(crate) fn new(name: String, arguments: Map<String, Value>) -> Self {
        Self { name, arguments }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn arguments(&self) -> &Map<String, Value> {
        &self.arguments
    }

    pub fn into_arguments(self) -> Map<String, Value> {
        self.arguments
    }

    /// 在进入业务层前把动态 MCP 参数收束为工具专用 Rust DTO。
    ///
    /// 错误只报告位置，不回显原值，避免 OAuth code、token 等敏感参数进入日志或响应。
    pub fn deserialize<T: DeserializeOwned>(&self) -> Result<T, ProtocolError> {
        serde_json::from_value(Value::Object(self.arguments.clone())).map_err(|error| {
            ProtocolError::invalid_params(
                format!("Invalid arguments for tool {}", self.name),
                Some(serde_json::json!({
                    "tool": self.name,
                    "line": error.line(),
                    "column": error.column(),
                })),
            )
        })
    }
}

/// 一次成功的工具执行结果。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutput {
    content: Vec<ContentBlock>,
    structured_content: Option<Value>,
}

impl ToolOutput {
    pub fn new(content: Vec<ContentBlock>) -> Self {
        Self {
            content,
            structured_content: None,
        }
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self::new(vec![ContentBlock::text(text)])
    }

    pub fn with_structured_content(mut self, structured_content: Value) -> Self {
        self.structured_content = Some(structured_content);
        self
    }

    pub(crate) fn into_parts(self) -> (Vec<ContentBlock>, Option<Value>) {
        (self.content, self.structured_content)
    }
}

/// 已正确路由、但业务执行失败时返回给调用者的工具级错误。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolFailure {
    content: Vec<ContentBlock>,
    structured_content: Option<Value>,
}

impl ToolFailure {
    pub fn new(content: Vec<ContentBlock>) -> Self {
        Self {
            content,
            structured_content: None,
        }
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self::new(vec![ContentBlock::text(text)])
    }

    pub fn with_structured_content(mut self, structured_content: Value) -> Self {
        self.structured_content = Some(structured_content);
        self
    }

    pub(crate) fn into_parts(self) -> (Vec<ContentBlock>, Option<Value>) {
        (self.content, self.structured_content)
    }
}

/// 不能作为正常工具结果返回的 JSON-RPC 协议错误。
#[derive(Debug, Clone, Error, PartialEq)]
pub enum ProtocolError {
    #[error("{message}")]
    InvalidParams {
        message: String,
        data: Option<Value>,
    },

    #[error("{message}")]
    Internal {
        message: String,
        data: Option<Value>,
    },
}

impl ProtocolError {
    pub fn invalid_params(message: impl Into<String>, data: Option<Value>) -> Self {
        Self::InvalidParams {
            message: message.into(),
            data,
        }
    }

    pub fn internal(message: impl Into<String>, data: Option<Value>) -> Self {
        Self::Internal {
            message: message.into(),
            data,
        }
    }
}

/// 显式区分工具级失败与协议级失败，避免调用方看到错误的 MCP 语义。
#[derive(Debug, Clone, Error, PartialEq)]
pub enum DispatchError {
    #[error("工具执行失败")]
    Tool(ToolFailure),

    #[error(transparent)]
    Protocol(ProtocolError),
}

impl From<ToolFailure> for DispatchError {
    fn from(value: ToolFailure) -> Self {
        Self::Tool(value)
    }
}

impl From<ProtocolError> for DispatchError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

/// 业务层实现此 trait；协议层负责工具存在性与 MCP 错误封装。
pub trait ToolDispatcher: Send + Sync + 'static {
    fn dispatch(
        &self,
        call: ToolCall,
    ) -> impl Future<Output = Result<ToolOutput, DispatchError>> + Send;
}
