//! Domain models, `RangeSet`, themes. No I/O.

uniffi::setup_scaffolding!();

mod app_timing;
mod byte_range;
mod connection_allocator;
mod priority;
mod range_set;

pub use app_timing::AppTiming;
pub use byte_range::ByteRange;
pub use connection_allocator::{ConnectionAllocator, ConnectionBudget, ConnectionDemand};
pub use priority::{ItemState, Priority};
pub use range_set::RangeSet;

/// Temporary skeleton record proving a type defined here is exported from
/// `sdm-ffi` (replaced when the real domain types are ported).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CoreVersion {
    pub version: String,
    pub crates: Vec<String>,
}
