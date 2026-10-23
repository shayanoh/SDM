//! The controller layer between the Rust download engine and a UI: turns
//! engine snapshots into small ordered patches, owns settings, and decides
//! when a notification is due. No clock and no UI framework in here.

pub mod error;
pub mod protocol;

mod convert;

pub use error::AppError;
