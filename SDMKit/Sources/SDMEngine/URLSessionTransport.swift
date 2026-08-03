import Foundation
import SDMCore

/// The production transport. Never exercised by the test suite.
public struct URLSessionTransport: HTTPTransport {
    private let session: URLSession

    public init(session: URLSession = .shared) {
        self.session = session
    }

    public func fetch(_ request: RangeRequest) async throws -> RangeResponse {
        var urlRequest = URLRequest(url: request.url)
        if let range = request.range {
            // HTTP ranges are inclusive on both ends; ByteRange is half-open.
            urlRequest.setValue(
                "bytes=\(range.start)-\(range.end - 1)",
                forHTTPHeaderField: "Range"
            )
        }

        let (bytes, response) = try await session.bytes(for: urlRequest)
        guard let http = response as? HTTPURLResponse else {
            throw TransportError.malformedResponse
        }

        let acceptsRanges =
            http.statusCode == 206
            || (http.value(forHTTPHeaderField: "Accept-Ranges")?.lowercased() == "bytes")
        let validator =
            http.value(forHTTPHeaderField: "ETag")
            ?? http.value(forHTTPHeaderField: "Last-Modified")

        let body = AsyncThrowingStream<Data, any Error> { continuation in
            let task = Task {
                do {
                    var buffer = Data()
                    buffer.reserveCapacity(64 * 1024)
                    for try await byte in bytes {
                        buffer.append(byte)
                        if buffer.count >= 64 * 1024 {
                            continuation.yield(buffer)
                            buffer.removeAll(keepingCapacity: true)
                        }
                    }
                    if !buffer.isEmpty { continuation.yield(buffer) }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
            continuation.onTermination = { _ in task.cancel() }
        }

        return RangeResponse(
            statusCode: http.statusCode,
            totalSize: Self.totalSize(from: http),
            acceptsRanges: acceptsRanges,
            validator: validator,
            body: body
        )
    }

    /// Prefers the total from `Content-Range` (correct for partial responses)
    /// over `Content-Length` (which reports only the slice).
    private static func totalSize(from response: HTTPURLResponse) -> Int64? {
        if let contentRange = response.value(forHTTPHeaderField: "Content-Range"),
            let slash = contentRange.lastIndex(of: "/")
        {
            let total = contentRange[contentRange.index(after: slash)...]
            if total != "*", let value = Int64(total) { return value }
        }
        let length = response.expectedContentLength
        return length == NSURLSessionTransferSizeUnknown ? nil : length
    }
}
