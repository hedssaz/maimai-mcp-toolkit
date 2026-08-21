mod convert;
mod dto;
mod error;
mod format;
mod handler;

pub use error::CompletionToolError;
pub use handler::CompletionDispatcher;

pub const TOOL_NAMES: [&str; 6] = [
    "render_maimai_plate",
    "render_maimai_plate_batch",
    "render_maimai_rating",
    "render_maimai_progress",
    "render_maimai_plate_progress",
    "render_maimai_plate_progress_batch",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionSurface {
    Main,
    Public,
}

#[cfg(test)]
mod tests;
