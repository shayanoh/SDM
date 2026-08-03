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
