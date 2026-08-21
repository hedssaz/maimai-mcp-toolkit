mod convert;
mod dto;
mod error;
mod format;
mod handler;

pub use handler::{ScoreListDispatcher, TOOL_NAME, score_list_server};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScoreListSurface {
    Main,
    Public,
}

#[cfg(test)]
mod tests;
