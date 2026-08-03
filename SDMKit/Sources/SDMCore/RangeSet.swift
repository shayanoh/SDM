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

    /// Selects the next range for an idle worker to download.
    ///
    /// Takes the largest gap not already reserved by an active worker. Gaps of
    /// at most `2 * minChunk` are taken whole; larger gaps are halved so other
    /// workers can claim the remainder.
    ///
    /// - Parameter reserved: ranges currently held by active workers.
    /// - Returns: the claimed range, or `nil` when no work remains.
    public func nextClaim(
        total: Int64,
        reserved: [ByteRange],
        minChunk: Int64
    ) -> ByteRange? {
        precondition(minChunk > 0, "minChunk must be positive, got \(minChunk)")

        var blocked = self
        for range in reserved { blocked.insert(range) }

        let free = blocked.gaps(within: total)
        guard
            let target = free.max(by: {
                $0.length == $1.length ? $0.start > $1.start : $0.length < $1.length
            })
        else { return nil }

        if target.length <= minChunk * 2 { return target }
        return ByteRange(start: target.start, end: target.start + target.length / 2)
    }
}
