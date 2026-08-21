//! 基于冻结契约的 MCP 协议适配层。

mod dispatcher;
mod router;
mod server;
mod surface;

pub use dispatcher::{
    DispatchError, ProtocolError, ToolCall, ToolDispatcher, ToolFailure, ToolOutput,
};
pub use router::PairDispatcher;
pub use server::{ContractServer, StdioServerError, serve_stdio};
pub use surface::tools_from_contract;
