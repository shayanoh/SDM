import Foundation

/// The production transport. Not exercised over the network by the test
/// suite; only the `static` boundary logic below is unit-tested, matching
/// `URLSessionTransport` in `SDMEngine`.
public struct URLSessionProbeTransport: ProbeTransport {
    private let session: URLSession

    public init(session: URLSession = .shared) {
        self.session = session
    }

    public func send(_ request: ProbeRequest) async throws -> ProbeResponse {
        do {
            let (data, response) = try await session.data(for: Self.makeURLRequest(for: request))
            guard let http = response as? HTTPURLResponse else {
                throw ProbeError.malformedResponse
            }
            return ProbeResponse(
                statusCode: http.statusCode,
                finalURL: http.url ?? request.url,
                headers: Self.lowercasedHeaders(from: http),
                body: data
            )
        } catch let error as ProbeError {
            throw error
        } catch let urlError as URLError {
            throw Self.classify(urlError.code) ?? urlError
        }
    }

    // MARK: - Pure boundary logic (unit-tested without a network)

    static func makeURLRequest(for request: ProbeRequest) -> URLRequest {
        var urlRequest = URLRequest(url: request.url)
        urlRequest.httpMethod = request.method == .head ? "HEAD" : "GET"
        urlRequest.cachePolicy = .reloadIgnoringLocalCacheData
        if let range = request.range, range.length > 0 {
            urlRequest.setValue(
                "bytes=\(range.start)-\(range.end - 1)",
                forHTTPHeaderField: "Range"
            )
        }
        return urlRequest
    }

    static func lowercasedHeaders(from response: HTTPURLResponse) -> [String: String] {
        var result: [String: String] = [:]
        for (key, value) in response.allHeaderFields {
            guard let key = key as? String, let value = value as? String else { continue }
            result[key.lowercased()] = value
        }
        return result
    }

    static func classify(_ code: URLError.Code) -> ProbeError? {
        switch code {
        case .timedOut: return .timedOut
        case .cannotFindHost, .dnsLookupFailed: return .dnsFailure
        default: return nil
        }
    }
}
