//! The controller layer between the Rust download engine and a UI: turns
//! engine snapshots into small ordered patches, owns settings, and decides
//! when a notification is due. No clock and no UI framework in here.

pub mod app;
pub mod error;
pub mod host;
pub mod notify;
pub mod protocol;
pub mod settings;
pub mod theme;

mod convert;
mod differ;
mod url_input;

pub use app::{App, Dependencies};
pub use error::AppError;
pub use theme::theme_css;
