import Foundation
import SDMCore

/// An in-process origin server for tests, programmable to misbehave the way
/// real servers do. See spec §11.1.
public actor FakeOrigin: HTTPTransport {
    public struct Behavior: Sendable {
        /// Serve the whole body regardless of the requested range.
        public var ignoresRanges = false
        /// Report this size instead of the true payload size.
        public var reportedSizeOverride: Int64?
        /// Throw `.connectionDropped` after emitting this many bytes.
        public var dropAfterBytes: Int?
        /// Return this status instead of 200/206.
        public var statusOverride: Int?
        /// Value reported as the `ETag`.
        public var validator: String?
        /// Bytes emitted per chunk of the response stream.
        public var chunkSize = 64

        public init() {}
    }

    private let payload: Data
    private var behavior: Behavior
    public private(set) var requestedRanges: [ByteRange] = []

    public init(payload: Data, behavior: Behavior = Behavior()) {
        self.payload = payload
        self.behavior = behavior
    }

    public func setValidator(_ validator: String?) {
        behavior.validator = validator
    }

    public func setBehavior(_ behavior: Behavior) {
        self.behavior = behavior
    }

    public func fetch(_ request: RangeRequest) async throws -> RangeResponse {
        let total = Int64(payload.count)
        if let range = request.range {
            requestedRanges.append(range)
        }

        if let status = behavior.statusOverride {
            return RangeResponse(
                statusCode: status,
                totalSize: behavior.reportedSizeOverride ?? total,
                acceptsRanges: !behavior.ignoresRanges,
                validator: behavior.validator,
                body: AsyncThrowingStream { $0.finish() }
            )
        }

        let slice: Data
        let status: Int
        if let range = request.range, !behavior.ignoresRanges {
            let lower = Int(Swift.min(range.start, total))
            let upper = Int(Swift.min(range.end, total))
            slice = payload.subdata(in: lower..<upper)
            status = 206
        } else {
            slice = payload
            status = 200
        }

        let chunkSize = behavior.chunkSize
        let dropAfter = behavior.dropAfterBytes
        let body = AsyncThrowingStream<Data, any Error> { continuation in
            var emitted = 0
            var offset = 0
            while offset < slice.count {
                let end = Swift.min(offset + chunkSize, slice.count)
                if let limit = dropAfter, emitted + (end - offset) > limit {
                    let allowed = limit - emitted
                    if allowed > 0 {
                        continuation.yield(slice.subdata(in: offset..<(offset + allowed)))
                    }
                    continuation.finish(throwing: TransportError.connectionDropped)
                    return
                }
                continuation.yield(slice.subdata(in: offset..<end))
                emitted += end - offset
                offset = end
            }
            continuation.finish()
        }

        return RangeResponse(
            statusCode: status,
            totalSize: behavior.reportedSizeOverride ?? total,
            acceptsRanges: !behavior.ignoresRanges,
            validator: behavior.validator,
            body: body
        )
    }
}
