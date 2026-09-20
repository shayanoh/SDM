import Foundation
import SDMCore

public struct RangeRequest: Sendable {
    public let url: URL
    /// Half-open range to request, or `nil` for the whole resource.
    public let range: ByteRange?

    public init(url: URL, range: ByteRange? = nil) {
        self.url = url
        self.range = range
    }
}

public struct RangeResponse: Sendable {
    public let statusCode: Int
    /// Total size of the whole resource, when the origin discloses it.
    public let totalSize: Int64?
    public let acceptsRanges: Bool
    /// `ETag`, or `Last-Modified` when no `ETag` is offered.
    public let validator: String?
    /// The URL the response actually came from, after redirects. A
    /// redirecting origin (e.g. a mirror-network frontend) can send
    /// different requests to different backends, so this is what a caller
    /// should reuse for subsequent requests against the *same* resource
    /// instead of re-resolving the original URL each time.
    public let resolvedURL: URL
    public let body: AsyncThrowingStream<Data, any Error>

    public init(
        statusCode: Int,
        totalSize: Int64?,
        acceptsRanges: Bool,
        validator: String?,
        resolvedURL: URL,
        body: AsyncThrowingStream<Data, any Error>
    ) {
        self.statusCode = statusCode
        self.totalSize = totalSize
        self.acceptsRanges = acceptsRanges
        self.validator = validator
        self.resolvedURL = resolvedURL
        self.body = body
    }
}

public enum TransportError: Error, Equatable {
    case connectionDropped
    case http(status: Int)
    case malformedResponse
}

/// The engine's only route to the network. Injected so tests never touch it.
public protocol HTTPTransport: Sendable {
    func fetch(_ request: RangeRequest) async throws -> RangeResponse
}
