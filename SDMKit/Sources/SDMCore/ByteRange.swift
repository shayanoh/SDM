/// A half-open byte interval `[start, end)`.
///
/// Half-open is used everywhere in the domain model. HTTP `Range` headers are
/// inclusive on both ends; convert only at the HTTP boundary.
public struct ByteRange: Hashable, Sendable, Codable {
    public let start: Int64
    public let end: Int64

    /// - Precondition: `start >= 0` and `end >= start`.
    public init(start: Int64, end: Int64) {
        precondition(start >= 0, "ByteRange.start must be non-negative, got \(start)")
        precondition(end >= start, "ByteRange.end (\(end)) must be >= start (\(start))")
        self.start = start
        self.end = end
    }

    public var length: Int64 { end - start }
    public var isEmpty: Bool { end == start }
}
