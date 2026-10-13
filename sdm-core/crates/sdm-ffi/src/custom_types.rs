//! UniFFI custom types: Rust types whose Swift face is a Foundation type.
//!
//! The Swift side of each is configured under `[bindings.swift.custom_types]`
//! in `uniffi.toml`:
//!
//! | Rust | on the wire | Swift |
//! |---|---|---|
//! | `Uuid` | `String` | `UUID` |
//! | `SourceUrl` | `String` | `URL` |
//! | `PathBuf` | `String` | `URL` (a file URL) |
//! | `RangeSet` | `RangeSetData` record | `RangeSetData` (aliased `RangeSet`) |
//! | `usize` | `u64` | `Int` |
//!
//! `SystemTime` (Swift `Date`) and `Duration` (Swift `TimeInterval`) are
//! built into UniFFI and need no declaration. `url::Url` is not declared: no
//! exported signature carries one, because every URL the core passes around
//! is a [`SourceUrl`], which keeps the string it was created from.
//!
//! The conversions are plain functions so they can be tested without a Swift
//! caller. A conversion that fails when Swift hands a value in returns
//! [`CoreError::InvalidArgument`]; UniFFI throws it from the call as that
//! error.

use std::path::PathBuf;

use sdm_core::{ByteRange, RangeSet, SourceUrl};
use uuid::Uuid;

use crate::error::CoreError;

/// Parses the string Swift's `UUID.uuidString` produces (upper case; the
/// parser accepts either case).
pub fn uuid_from_string(text: &str) -> Result<Uuid, CoreError> {
    Uuid::parse_str(text).map_err(|error| CoreError::invalid("uuid", error.to_string()))
}

/// Builds a [`SourceUrl`] from the `absoluteString` of a Swift `URL`,
/// keeping that string byte for byte (never routed through `url::Url`'s
/// normalization). An empty or unparseable string is an error: the core
/// refuses a URL it could not probe or download.
pub fn source_url_from_string(text: &str) -> Result<SourceUrl, CoreError> {
    SourceUrl::new(text).map_err(|error| CoreError::invalid("url", error.to_string()))
}

/// A file path from a Swift file URL's `path`. An empty path is refused; a
/// path need not exist.
pub fn path_from_string(text: &str) -> Result<PathBuf, CoreError> {
    if text.is_empty() {
        return Err(CoreError::invalid("path", "the path is empty"));
    }
    Ok(PathBuf::from(text))
}

/// A path as UTF-8 text for the wire. Paths that are not valid UTF-8 cannot
/// be a Swift `URL`; they are lowered lossily rather than failing the whole
/// snapshot they appear in.
pub fn path_to_string(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A size or count from the wire.
pub fn usize_from_u64(value: u64) -> Result<usize, CoreError> {
    usize::try_from(value).map_err(|_| CoreError::invalid("count", "the value does not fit"))
}

/// The wire form of a [`RangeSet`]: its sorted, merged, non-empty ranges.
/// Swift sees this as `RangeSetData` and its `typealias RangeSet`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
#[uniffi(name = "RangeSetData")]
pub struct RangeSetData {
    pub ranges: Vec<ByteRange>,
}

pub fn range_set_to_data(set: &RangeSet) -> RangeSetData {
    RangeSetData {
        ranges: set.ranges().to_vec(),
    }
}

/// Rebuilds a [`RangeSet`] through `insert`, so whatever Swift sends is
/// sorted and merged again and the set's invariant holds. A range with
/// `start < 0` or `end < start` is refused instead of tripping the
/// `ByteRange` assertion.
pub fn range_set_from_data(data: &RangeSetData) -> Result<RangeSet, CoreError> {
    let mut set = RangeSet::new();
    for range in &data.ranges {
        if range.start < 0 || range.end < range.start {
            return Err(CoreError::invalid(
                "ranges",
                format!("invalid byte range {}..{}", range.start, range.end),
            ));
        }
        set.insert(ByteRange::new(range.start, range.end));
    }
    Ok(set)
}

uniffi::custom_type!(Uuid, String, {
    remote,
    lower: |value| value.to_string(),
    try_lift: |text| Ok(uuid_from_string(&text)?),
});

uniffi::custom_type!(SourceUrl, String, {
    remote,
    lower: |value| value.as_str().to_owned(),
    try_lift: |text| Ok(source_url_from_string(&text)?),
});

uniffi::custom_type!(PathBuf, String, {
    remote,
    lower: |value| path_to_string(&value),
    try_lift: |text| Ok(path_from_string(&text)?),
});

uniffi::custom_type!(RangeSet, RangeSetData, {
    remote,
    lower: |value| range_set_to_data(&value),
    try_lift: |data| Ok(range_set_from_data(&data)?),
});

uniffi::custom_type!(usize, u64, {
    remote,
    lower: |value| value as u64,
    try_lift: |value| Ok(usize_from_u64(value)?),
});
