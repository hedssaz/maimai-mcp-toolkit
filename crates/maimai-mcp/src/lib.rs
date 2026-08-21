//! MCP 传输适配器和公开工具契约。

pub mod b50_image_tools;
pub mod catalog_tools;
pub mod contract;
pub mod identity_tools;
pub mod oauth_tools;
pub mod protocol;
pub mod ranking_tools;
pub mod render_tools;
pub mod score_by_song_tools;
pub mod score_queries;
pub mod score_settings;
mod scoring;

pub use protocol::{
    ContractServer, DispatchError, PairDispatcher, ProtocolError, StdioServerError, ToolCall,
    ToolDispatcher, ToolFailure, ToolOutput, serve_stdio,
};
