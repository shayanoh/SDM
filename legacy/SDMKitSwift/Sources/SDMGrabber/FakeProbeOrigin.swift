import Foundation
import SDMCore

/// An in-process, per-URL programmable origin for grabber tests. See spec
/// §11.1 for the sibling `FakeOrigin` this mirrors in `SDMEngine`.
public actor FakeProbeOrigin: ProbeTransport {
    public struct Behavior: Sendable {
        public var statusCode: Int = 200
        public var finalURL: URL?
        /// Expected already-lowercased, matching what a real
        /// `ProbeTransport` hands back.
        public var headers: [String: String] = [:]
        public var body: Data = Data()
        /// A `.head` request throws `.malformedResponse` — the trigger for
        /// stage 1's `Range: bytes=0-0` GET fallback.
        public var rejectsHead = false
        public var error: ProbeError?
        /// Holds the request until `release(_:)` is called for this URL, so
        /// concurrency-budget tests can observe an in-flight request.
        public var holdsUntilReleased = false

        public init() {}
    }

    private var behaviors: [URL: Behavior] = [:]
    private let defaultBehavior: Behavior = {
        var behavior = Behavior()
        behavior.statusCode = 404
        return behavior
    }()
    public private(set) var requestLog: [ProbeRequest] = []
    private var released: Set<URL> = []
    private var releaseWaiters: [URL: [CheckedContinuation<Void, Never>]] = [:]

    public init() {}

    public func setBehavior(_ behavior: Behavior, for url: URL) {
        behaviors[url] = behavior
    }

    public func release(_ url: URL) {
        released.insert(url)
        for waiter in releaseWaiters.removeValue(forKey: url) ?? [] { waiter.resume() }
    }

    public func send(_ request: ProbeRequest) async throws -> ProbeResponse {
        requestLog.append(request)
        let behavior = behaviors[request.url] ?? defaultBehavior

        if behavior.holdsUntilReleased, !released.contains(request.url) {
            await withCheckedContinuation { continuation in
                releaseWaiters[request.url, default: []].append(continuation)
            }
        }

        if let error = behavior.error { throw error }
        if request.method == .head, behavior.rejectsHead { throw ProbeError.malformedResponse }

        let body: Data
        switch (request.method, request.range) {
        case (.head, _):
            body = Data()
        case (.get, let range?):
            let count = Int64(behavior.body.count)
            let lower = Int(min(range.start, count))
            let upper = Int(min(range.end, count))
            body = behavior.body.subdata(in: lower..<upper)
        case (.get, nil):
            body = behavior.body
        }

        return ProbeResponse(
            statusCode: behavior.statusCode,
            finalURL: behavior.finalURL ?? request.url,
            headers: behavior.headers,
            body: body
        )
    }
}
