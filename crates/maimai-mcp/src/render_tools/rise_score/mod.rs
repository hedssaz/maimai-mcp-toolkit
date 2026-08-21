mod convert;
mod dto;
mod error;
mod format;
mod handler;

pub use handler::{RiseScoreDispatcher, TOOL_NAME, rise_score_server};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RiseScoreSurface {
    Main,
    Public,
}

#[cfg(test)]
mod tests;
