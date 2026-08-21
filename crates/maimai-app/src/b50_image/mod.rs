mod atomic_file;
mod data;
mod error;
mod output;
mod service;
mod style;

use time::{OffsetDateTime, UtcOffset};

pub use data::{
    B50ImageData, B50ImageDataError, B50ImageDataRequest, B50ImageDataService, B50ImageLookup,
};
pub use error::B50ImageError;
pub use output::{OutputPolicy, OutputStore, SavedImage};
pub use service::{B50ImageService, B50RenderResult, RenderOptions, ResourceDirectories};
pub use style::{B50ImageStyle, StyleSelection, StyleStore, StyleUpdate};

pub fn utc_timestamp(value: OffsetDateTime) -> String {
    let value = value.to_offset(UtcOffset::UTC);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}+00:00",
        value.year(),
        u8::from(value.month()),
        value.day(),
        value.hour(),
        value.minute(),
        value.second(),
        value.nanosecond() / 1_000
    )
}

#[cfg(test)]
mod tests;
