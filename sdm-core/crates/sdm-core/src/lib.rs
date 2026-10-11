//! Domain models, `RangeSet`, themes. No I/O.

uniffi::setup_scaffolding!();

/// Temporary skeleton record proving a type defined here is exported from
/// `sdm-ffi` (replaced when the real domain types are ported).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CoreVersion {
    pub version: String,
    pub crates: Vec<String>,
}
