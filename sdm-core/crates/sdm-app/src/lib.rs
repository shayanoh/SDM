//! The controller layer between the Rust download engine and a UI: turns
//! engine snapshots into small ordered patches, owns settings, and decides
//! when a notification is due. No clock and no UI framework in here.

// Nothing outside the tests calls the converter and differ until `App` (Task 10);
// remove this allow there.
#![cfg_attr(not(test), allow(dead_code))]

pub mod error;
pub mod protocol;

mod convert;
mod differ;

pub use error::AppError;
