use std::{collections::HashMap, sync::Arc};

use rmcp::{
    ErrorData, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, Implementation, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
    },
    service::{QuitReason, RequestContext, RoleServer, ServerInitializeError},
};
use serde_json::json;
use thiserror::Error;

use crate::contract::SurfaceContract;

use super::{
    DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolFailure, ToolOutput,
    tools_from_contract,
};

/// 由冻结的 surface 契约驱动的 rmcp server。
#[derive(Debug)]
pub struct ContractServer<D> {
    dispatcher: D,
    info: ServerInfo,
    tools: Arc<[Tool]>,
    tool_indexes: HashMap<String, usize>,
}

impl<D> ContractServer<D> {
    pub fn new(contract: SurfaceContract, dispatcher: D) -> Self {
        let tools = tools_from_contract(&contract);
        let tool_indexes = tools
            .iter()
            .enumerate()
            .map(|(index, tool)| (tool.name.to_string(), index))
            .collect();
        let info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                contract.server_info().name(),
                contract.server_info().version(),
            ));

        Self {
            dispatcher,
            info,
            tools: tools.into(),
            tool_indexes,
        }
    }

    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    fn tool(&self, name: &str) -> Option<&Tool> {
        self.tool_indexes
            .get(name)
            .and_then(|index| self.tools.get(*index))
    }
}

impl<D> ServerHandler for ContractServer<D>
where
    D: ToolDispatcher,
{
    fn get_info(&self) -> ServerInfo {
        self.info.clone()
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tool(name).cloned()
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(
            self.tools.iter().cloned().collect(),
        )))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.into_owned();
        if self.tool(&name).is_none() {
            return Err(ErrorData::invalid_params(
                format!("Unknown tool: {name}"),
                Some(json!({"tool": name})),
            ));
        }

        let call = ToolCall::new(name, request.arguments.unwrap_or_default());
        match self.dispatcher.dispatch(call).await {
            Ok(output) => Ok(success_result(output).into()),
            Err(DispatchError::Tool(failure)) => Ok(tool_error_result(failure).into()),
            Err(DispatchError::Protocol(error)) => Err(protocol_error(error)),
        }
    }
}

fn success_result(output: ToolOutput) -> CallToolResult {
    let (content, structured_content) = output.into_parts();
    let mut result = CallToolResult::success(content);
    result.structured_content = structured_content;
    result
}

fn tool_error_result(failure: ToolFailure) -> CallToolResult {
    let (content, structured_content) = failure.into_parts();
    let mut result = CallToolResult::error(content);
    result.structured_content = structured_content;
    result
}

fn protocol_error(error: ProtocolError) -> ErrorData {
    match error {
        ProtocolError::InvalidParams { message, data } => ErrorData::invalid_params(message, data),
        ProtocolError::Internal { message, data } => ErrorData::internal_error(message, data),
    }
}

#[derive(Debug, Error)]
pub enum StdioServerError {
    #[error(transparent)]
    Initialize(Box<ServerInitializeError>),

    #[error("MCP 服务任务异常：{0}")]
    Runtime(#[from] tokio::task::JoinError),
}

impl From<ServerInitializeError> for StdioServerError {
    fn from(value: ServerInitializeError) -> Self {
        Self::Initialize(Box::new(value))
    }
}

/// 在 stdin/stdout 上运行 MCP；本函数本身不向 stdout 写任何非协议内容。
pub async fn serve_stdio<D>(server: ContractServer<D>) -> Result<QuitReason, StdioServerError>
where
    D: ToolDispatcher,
{
    let running = rmcp::serve_server(server, rmcp::transport::stdio()).await?;
    match running.waiting().await? {
        QuitReason::JoinError(error) => Err(StdioServerError::Runtime(error)),
        reason => Ok(reason),
    }
}

#[cfg(test)]
mod tests {
    use std::{future::ready, io};

    use rmcp::model::ErrorCode;
    use serde_json::{Map, Value, json};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};

    use super::{ContractServer, QuitReason};
    use crate::{
        contract::SurfaceContract,
        protocol::{
            DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolFailure, ToolOutput,
        },
    };

    #[derive(Debug)]
    struct FixtureDispatcher;

    impl ToolDispatcher for FixtureDispatcher {
        fn dispatch(
            &self,
            call: ToolCall,
        ) -> impl Future<Output = Result<ToolOutput, DispatchError>> + Send {
            let result = match call.name() {
                "echo" => {
                    let payload = Value::Object(call.arguments().clone());
                    Ok(ToolOutput::text(payload.to_string()).with_structured_content(payload))
                }
                "fail" => Err(ToolFailure::text("fixture tool failure")
                    .with_structured_content(json!({"error": {"code": "FIXTURE"}}))
                    .into()),
                "protocol" => Err(ProtocolError::invalid_params(
                    "fixture protocol failure",
                    Some(json!({"field": "value"})),
                )
                .into()),
                _ => Err(ProtocolError::internal("unreachable fixture route", None).into()),
            };
            ready(result)
        }
    }

    fn fixture_contract() -> Result<SurfaceContract, crate::contract::ContractError> {
        SurfaceContract::parse(
            r#"{
                "serverInfo": {"name": "fixture-server", "version": "1.2.3"},
                "tools": [
                    {
                        "name": "echo",
                        "description": "Echo arguments.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {"value": {"type": "string"}},
                            "required": ["value"],
                            "additionalProperties": false
                        }
                    },
                    {
                        "name": "fail",
                        "description": "Return a tool error.",
                        "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
                    },
                    {
                        "name": "protocol",
                        "description": "Return a protocol error.",
                        "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
                    }
                ]
            }"#,
        )
    }

    async fn write_message<W>(writer: &mut W, message: &Value) -> Result<(), io::Error>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        let mut encoded = serde_json::to_vec(message).map_err(io::Error::other)?;
        encoded.push(b'\n');
        writer.write_all(&encoded).await?;
        writer.flush().await
    }

    async fn read_message<R>(lines: &mut Lines<BufReader<R>>) -> Result<Value, io::Error>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let line = lines
            .next_line()
            .await?
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "MCP response missing"))?;
        serde_json::from_str(&line).map_err(io::Error::other)
    }

    #[tokio::test]
    async fn initialize_list_and_call_follow_rmcp_wire_protocol()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = ContractServer::new(fixture_contract()?, FixtureDispatcher);
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let server_task = tokio::spawn(async move {
            let running = rmcp::serve_server(server, server_io)
                .await
                .map_err(|error| io::Error::other(error.to_string()))?;
            match running
                .waiting()
                .await
                .map_err(|error| io::Error::other(error.to_string()))?
            {
                QuitReason::JoinError(error) => Err(io::Error::other(error.to_string())),
                _ => Ok(()),
            }
        });
        let (client_read, mut client_write) = tokio::io::split(client_io);
        let mut response_lines = BufReader::new(client_read).lines();

        write_message(
            &mut client_write,
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "fixture-client", "version": "1.0.0"}
                }
            }),
        )
        .await?;
        let initialized = read_message(&mut response_lines).await?;
        assert_eq!(initialized["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(
            initialized["result"]["serverInfo"]["name"],
            "fixture-server"
        );
        assert_eq!(initialized["result"]["serverInfo"]["version"], "1.2.3");
        assert!(initialized["result"]["capabilities"]["tools"].is_object());

        write_message(
            &mut client_write,
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        )
        .await?;
        write_message(
            &mut client_write,
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
        )
        .await?;
        let listed = read_message(&mut response_lines).await?;
        assert_eq!(listed["result"]["tools"].as_array().map(Vec::len), Some(3));
        assert_eq!(
            listed["result"]["tools"][0]["inputSchema"],
            json!({
                "type": "object",
                "properties": {"value": {"type": "string"}},
                "required": ["value"],
                "additionalProperties": false
            })
        );

        write_message(
            &mut client_write,
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {"name": "echo", "arguments": {"value": "hello"}}
            }),
        )
        .await?;
        let called = read_message(&mut response_lines).await?;
        assert_eq!(called["result"]["isError"], false);
        assert_eq!(
            called["result"]["structuredContent"],
            json!({"value": "hello"})
        );

        write_message(
            &mut client_write,
            &json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {"name": "fail", "arguments": {}}
            }),
        )
        .await?;
        let tool_error = read_message(&mut response_lines).await?;
        assert_eq!(tool_error["result"]["isError"], true);
        assert_eq!(
            tool_error["result"]["structuredContent"]["error"]["code"],
            "FIXTURE"
        );

        write_message(
            &mut client_write,
            &json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": {"name": "protocol", "arguments": {}}
            }),
        )
        .await?;
        let protocol_error = read_message(&mut response_lines).await?;
        assert_eq!(protocol_error["error"]["code"], ErrorCode::INVALID_PARAMS.0);

        write_message(
            &mut client_write,
            &json!({
                "jsonrpc": "2.0",
                "id": 6,
                "method": "tools/call",
                "params": {"name": "missing", "arguments": {}}
            }),
        )
        .await?;
        let missing = read_message(&mut response_lines).await?;
        assert_eq!(missing["error"]["code"], ErrorCode::INVALID_PARAMS.0);

        client_write.shutdown().await?;
        drop(client_write);
        server_task.await??;
        Ok(())
    }

    #[test]
    fn server_exposes_contract_tools_in_frozen_order() -> Result<(), Box<dyn std::error::Error>> {
        let server = ContractServer::new(fixture_contract()?, FixtureDispatcher);
        let names = server
            .tools()
            .iter()
            .map(|tool| tool.name.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(names, ["echo", "fail", "protocol"]);
        assert_eq!(
            server.tools()[0].input_schema.get("additionalProperties"),
            Some(&Value::Bool(false))
        );
        assert_eq!(
            server.tools()[0].input_schema.get("properties"),
            Some(&Value::Object(Map::from_iter([(
                "value".to_owned(),
                json!({"type": "string"}),
            )])))
        );
        Ok(())
    }
}
