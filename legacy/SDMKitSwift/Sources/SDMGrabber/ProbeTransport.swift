import Foundation
import SDMCore

public struct ProbeRequest: Sendable {
    public enum Method: Sendable, Equatable {
        case head
        case get
    }

    public let url: URL
    public let method: Method
    /// Only meaningful for `.get`: requests a slice via `Range`, or the
    /// whole (bounded) body when `nil`.
    public let range: ByteRange?

    public init(url: URL, method: Method, range: ByteRange? = nil) {
        self.url = url
        self.method = method
        self.range = range
    }
}

public struct ProbeResponse: Sendable {
    public let statusCode: Int
    /// URL after following redirects.
    public let finalURL: URL
    /// Header names lowercased for case-insensitive lookup.
    public let headers: [String: String]
    /// Empty for `.head`; the requested slice (or whole body) for `.get`.
    public let body: Data

    public init(statusCode: Int, finalURL: URL, headers: [String: String], body: Data) {
        self.statusCode = statusCode
        self.finalURL = finalURL
        self.headers = headers
        self.body = body
    }
}

public enum ProbeError: Error, Equatable {
    case timedOut
    case dnsFailure
    case malformedResponse
}

/// The grabber's only route to the network. Injected so tests never touch
/// it. Deliberately separate from `SDMEngine.HTTPTransport` — `SDMGrabber`
/// depends only on `SDMCore` per the spec's module table.
public protocol ProbeTransport: Sendable {
    func send(_ request: ProbeRequest) async throws -> ProbeResponse
}
