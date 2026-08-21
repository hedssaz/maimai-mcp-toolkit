mod atomic_file;
mod error;
mod store;

pub use error::ImageOutputError;
pub use store::{ImageOutputPolicy, ImageOutputStore, SavedImage};

pub(crate) use atomic_file::{atomic_write, reject_non_regular, secure_absolute};
