/// A set of completed byte ranges, kept sorted, disjoint, and coalesced.
///
/// This is the single source of truth for a download's progress. Segments are
/// ephemeral workers that fill gaps in this set; they are never stored.
public struct RangeSet: Equatable, Sendable, Codable {
    public private(set) var ranges: [ByteRange]

    public init() {
        self.ranges = []
    }

    /// Creates a set from arbitrary ranges, normalizing them.
    public init(_ ranges: [ByteRange]) {
        self.ranges = []
        for range in ranges { insert(range) }
    }

    /// Inserts a range, merging it with any overlapping or adjacent ranges.
    public mutating func insert(_ range: ByteRange) {
        guard !range.isEmpty else { return }

        var merged = range
        var result: [ByteRange] = []
        result.reserveCapacity(ranges.count + 1)
        var inserted = false

        for existing in ranges {
            if existing.end < merged.start {
                result.append(existing)
            } else if existing.start > merged.end {
                if !inserted {
                    result.append(merged)
                    inserted = true
                }
                result.append(existing)
            } else {
                merged = ByteRange(
                    start: Swift.min(existing.start, merged.start),
                    end: Swift.max(existing.end, merged.end)
                )
            }
        }
        if !inserted { result.append(merged) }
        ranges = result
    }

    public var totalBytes: Int64 {
        ranges.reduce(0) { $0 + $1.length }
    }

    public func contains(_ offset: Int64) -> Bool {
        ranges.contains { offset >= $0.start && offset < $0.end }
    }
}

extension RangeSet {
    /// The complement of this set within `[0, total)`.
    public func gaps(within total: Int64) -> [ByteRange] {
        precondition(total >= 0, "total must be non-negative, got \(total)")
        var result: [ByteRange] = []
        var cursor: Int64 = 0
        for range in ranges {
            if range.start > cursor {
                result.append(ByteRange(start: cursor, end: Swift.min(range.start, total)))
            }
            cursor = Swift.max(cursor, range.end)
            if cursor >= total { break }
        }
        if cursor < total {
            result.append(ByteRange(start: cursor, end: total))
        }
        return result.filter { !$0.isEmpty }
    }

    public func isComplete(total: Int64) -> Bool {
        gaps(within: total).isEmpty
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
    /// - Parameter reserved: ranges currently held by active workers.
    /// - Returns: the claimed range, or `nil` when no free gap remains.
    public func nextClaim(
        total: Int64,
        reserved: [ByteRange]
    ) -> ByteRange? {
        var blocked = self
        for range in reserved { blocked.insert(range) }

        let free = blocked.gaps(within: total)
        return free.max(by: {
            $0.length == $1.length ? $0.start > $1.start : $0.length < $1.length
        })
    }
}
