//! The controller layer between the Rust download engine and a UI: turns
//! engine snapshots into small ordered patches, owns settings, and decides
//! when a notification is due. No clock and no UI framework in here.

// Nothing outside the tests calls the converter and differ until `App` (Task 10);
// remove this allow there.
#![cfg_attr(not(test), allow(dead_code))]

pub mod error;
pub mod host;
pub mod notify;
pub mod protocol;
pub mod settings;
pub mod theme;

mod convert;
mod differ;
mod url_input;

pub use error::AppError;
pub use theme::theme_css;
