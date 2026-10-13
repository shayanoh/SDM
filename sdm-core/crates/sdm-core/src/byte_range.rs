use serde::{Deserialize, Serialize};

/// A half-open byte interval `[start, end)`.
///
/// Half-open is used everywhere in the domain model. HTTP `Range` headers are
/// inclusive on both ends; convert only at the HTTP boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: i64,
    pub end: i64,
}

impl ByteRange {
    /// # Panics
    ///
    /// Panics unless `start >= 0` and `end >= start`.
    pub fn new(start: i64, end: i64) -> Self {
        assert!(
            start >= 0,
            "ByteRange.start must be non-negative, got {start}"
        );
        assert!(
            end >= start,
            "ByteRange.end ({end}) must be >= start ({start})"
        );
        Self { start, end }
    }

    pub fn length(&self) -> i64 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.end == self.start
    }
}
