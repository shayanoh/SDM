use serde::{Deserialize, Serialize};

use crate::byte_range::ByteRange;

/// A set of completed byte ranges, kept sorted, disjoint, and coalesced.
///
/// This is the single source of truth for a download's progress. Segments are
/// ephemeral workers that fill gaps in this set; they are never stored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeSet {
    ranges: Vec<ByteRange>,
}

impl RangeSet {
    pub fn new() -> Self {
        Self { ranges: Vec::new() }
    }

    /// Creates a set from arbitrary ranges, normalizing them.
    pub fn from_ranges(ranges: &[ByteRange]) -> Self {
        let mut set = Self::new();
        for range in ranges {
            set.insert(*range);
        }
        set
    }

    /// The sorted, disjoint, coalesced ranges.
    pub fn ranges(&self) -> &[ByteRange] {
        &self.ranges
    }

    /// Inserts a range, merging it with any overlapping or adjacent ranges.
    pub fn insert(&mut self, range: ByteRange) {
        if range.is_empty() {
            return;
        }

        let mut merged = range;
        let mut result: Vec<ByteRange> = Vec::with_capacity(self.ranges.len() + 1);
        let mut inserted = false;

        for existing in &self.ranges {
            if existing.end < merged.start {
                result.push(*existing);
            } else if existing.start > merged.end {
                if !inserted {
                    result.push(merged);
                    inserted = true;
                }
                result.push(*existing);
            } else {
                merged = ByteRange::new(
                    existing.start.min(merged.start),
                    existing.end.max(merged.end),
                );
            }
        }
        if !inserted {
            result.push(merged);
        }
        self.ranges = result;
    }

    pub fn total_bytes(&self) -> i64 {
        self.ranges.iter().map(|r| r.length()).sum()
    }

    pub fn contains(&self, offset: i64) -> bool {
        self.ranges
            .iter()
            .any(|r| offset >= r.start && offset < r.end)
    }

    /// The complement of this set within `[0, total)`.
    ///
    /// # Panics
    ///
    /// Panics if `total` is negative.
    pub fn gaps_within(&self, total: i64) -> Vec<ByteRange> {
        assert!(total >= 0, "total must be non-negative, got {total}");
        let mut result: Vec<ByteRange> = Vec::new();
        let mut cursor: i64 = 0;
        for range in &self.ranges {
            if range.start > cursor {
                result.push(ByteRange::new(cursor, range.start.min(total)));
            }
            cursor = cursor.max(range.end);
            if cursor >= total {
                break;
            }
        }
        if cursor < total {
            result.push(ByteRange::new(cursor, total));
        }
        result.retain(|r| !r.is_empty());
        result
    }

    pub fn is_complete(&self, total: i64) -> bool {
        self.gaps_within(total).is_empty()
    }

    /// Selects the next free range for an idle worker to download.
    ///
    /// Takes the *entire* largest gap not already reserved by an active
    /// worker — no pre-emptive halving. There should never be free,
    /// unclaimed bytes sitting idle while a worker wants work: everything
    /// incomplete is either being downloaded or immediately claimed whole.
    /// Splitting only happens on demand, when a worker needs work and every
    /// remaining byte is already reserved by someone else — that is
    /// `DownloadTask`'s claim-stealing logic, which needs live per-worker
    /// write progress this pure, `completed`-only function has no way to
    /// see.
    ///
    /// `reserved` is the ranges currently held by active workers. Returns the
    /// claimed range, or `None` when no free gap remains. Among equally large
    /// gaps the one with the lower start wins.
    pub fn next_claim(&self, total: i64, reserved: &[ByteRange]) -> Option<ByteRange> {
        let mut blocked = self.clone();
        for range in reserved {
            blocked.insert(*range);
        }

        let free = blocked.gaps_within(total);
        // Swift: `max(by: { $0.length == $1.length ? $0.start > $1.start : $0.length < $1.length })`.
        // That order is total over distinct gaps (starts differ), so `max_by`'s
        // last-of-equals rule cannot matter here.
        free.into_iter().max_by(|a, b| {
            if a.length() == b.length() {
                b.start.cmp(&a.start)
            } else {
                a.length().cmp(&b.length())
            }
        })
    }
}
