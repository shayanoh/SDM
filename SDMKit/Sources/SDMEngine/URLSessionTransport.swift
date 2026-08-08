import Foundation
import SDMCore

/// The production transport.
///
/// Body bytes arrive as whole `Data` buffers from `URLSessionDataDelegate` and
/// are yielded straight through. The previous implementation iterated
/// `URLSession.AsyncBytes`, whose element is a single `UInt8` — one
/// async-sequence iteration and one `Data.append` per byte, i.e. tens of
/// millions of suspension-point checks per 100 MB. That capped throughput at a
/// few MB/s no matter how fast the link was, which defeats the entire point of
/// a segmented multi-connection downloader.
///
/// The session is `.ephemeral` with `reloadIgnoringLocalCacheData`, so a
/// multi-gigabyte response never lands in `URLCache`.
///
/// Not covered by the package suite: the constraint is that no test touches
/// the network. Everything that can be decided without a socket — `Range`
/// header construction, `Accept-Ranges` / status interpretation, validator
/// selection and the `Content-Range`-over-`Content-Length` total-size
/// preference — is factored into the `static` helpers below and unit-tested
/// against synthesized `HTTPURLResponse` values.
public final class URLSessionTransport: HTTPTransport, @unchecked Sendable {
    private let session: URLSession
    private let streamDelegate: StreamingDelegate

    public init(configuration: URLSessionConfiguration = URLSessionTransport.defaultConfiguration) {
        let delegate = StreamingDelegate()
        let queue = OperationQueue()
        queue.maxConcurrentOperationCount = 1
        queue.name = "com.sdm.transport.delegate"
        self.streamDelegate = delegate
        self.session = URLSession(
            configuration: configuration,
            delegate: delegate,
            delegateQueue: queue
        )
    }

    /// Ephemeral so nothing is written to a disk cache, and
    /// `reloadIgnoringLocalCacheData` so a large body is never even considered
    /// for `URLCache`.
    public static var defaultConfiguration: URLSessionConfiguration {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.urlCache = nil
        configuration.httpShouldUsePipelining = false
        // Set it to maximum possible so NSURLSession doesn't throttle the number of concurrent
        // connections to a single host. The default is 6, which is too low for segmented downloads.
        // We already manage total number of connections ourselves.
        configuration.httpMaximumConnectionsPerHost = 256
        return configuration
    }

    deinit {
        session.finishTasksAndInvalidate()
    }

    public func fetch(_ request: RangeRequest) async throws -> RangeResponse {
        let task = session.dataTask(with: Self.makeURLRequest(for: request))

        // The body stream is registered *before* `resume()`, so a delegate
        // callback can never arrive for an unregistered task identifier.
        var bodyContinuation: AsyncThrowingStream<Data, any Error>.Continuation!
        let body = AsyncThrowingStream<Data, any Error>(
            bufferingPolicy: .unbounded
        ) { bodyContinuation = $0 }
        streamDelegate.register(body: bodyContinuation, for: task.taskIdentifier)
        bodyContinuation.onTermination = { _ in task.cancel() }

        let http: HTTPURLResponse
        do {
            http = try await withTaskCancellationHandler {
                try await withCheckedThrowingContinuation { continuation in
                    streamDelegate.setResponseContinuation(
                        continuation,
                        for: task.taskIdentifier
                    )
                    task.resume()
                }
            } onCancel: {
                task.cancel()
            }
        } catch {
            // `didCompleteWithError` finishes the body stream itself, but a
            // cancellation thrown before the task ever reported completion
            // would otherwise leave the stream open forever.
            streamDelegate.finish(taskIdentifier: task.taskIdentifier, error: error)
            throw error
        }

        return RangeResponse(
            statusCode: http.statusCode,
            totalSize: Self.totalSize(from: http),
            acceptsRanges: Self.acceptsRanges(from: http),
            validator: Self.validator(from: http),
            body: body
        )
    }

    // MARK: - Pure boundary logic (unit-tested without a network)

    /// HTTP `Range` is inclusive on both ends; `ByteRange` is half-open. This
    /// is the only place in the codebase that conversion happens.
    static func rangeHeaderValue(for range: ByteRange?) -> String? {
        guard let range, range.length > 0 else { return nil }
        return "bytes=\(range.start)-\(range.end - 1)"
    }

    static func makeURLRequest(for request: RangeRequest) -> URLRequest {
        var urlRequest = URLRequest(url: request.url)
        urlRequest.cachePolicy = .reloadIgnoringLocalCacheData
        if let value = rangeHeaderValue(for: request.range) {
            urlRequest.setValue(value, forHTTPHeaderField: "Range")
        }
        return urlRequest
    }

    static func acceptsRanges(from response: HTTPURLResponse) -> Bool {
        if response.statusCode == 206 { return true }
        return response.value(forHTTPHeaderField: "Accept-Ranges")?.lowercased() == "bytes"
    }

    static func validator(from response: HTTPURLResponse) -> String? {
        response.value(forHTTPHeaderField: "ETag")
            ?? response.value(forHTTPHeaderField: "Last-Modified")
    }

    /// Prefers the total from `Content-Range` (correct for partial responses)
    /// over `Content-Length` (which reports only the slice).
    static func totalSize(from response: HTTPURLResponse) -> Int64? {
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

/// Fans `URLSession` delegate callbacks out to the per-request continuations
/// `fetch` is waiting on.
///
/// `@unchecked Sendable` because the entry table is guarded by an `NSLock`.
/// Every continuation is resumed exactly once: `entries.removeValue` under the
/// lock is what claims the right to resume, so a response and a completion
/// racing each other cannot both fire it.
private final class StreamingDelegate: NSObject, URLSessionDataDelegate, @unchecked Sendable {
    private struct Entry {
        var responseContinuation: CheckedContinuation<HTTPURLResponse, any Error>?
        var body: AsyncThrowingStream<Data, any Error>.Continuation?
    }

    private let lock = NSLock()
    private var entries: [Int: Entry] = [:]

    func register(
        body: AsyncThrowingStream<Data, any Error>.Continuation,
        for taskIdentifier: Int
    ) {
        lock.lock()
        defer { lock.unlock() }
        entries[taskIdentifier, default: Entry()].body = body
    }

    func setResponseContinuation(
        _ continuation: CheckedContinuation<HTTPURLResponse, any Error>,
        for taskIdentifier: Int
    ) {
        lock.lock()
        defer { lock.unlock() }
        entries[taskIdentifier, default: Entry()].responseContinuation = continuation
    }

    /// Ends a request from the caller's side (cancellation before the task
    /// ever completed). Idempotent.
    func finish(taskIdentifier: Int, error: (any Error)?) {
        lock.lock()
        let entry = entries.removeValue(forKey: taskIdentifier)
        lock.unlock()
        guard let entry else { return }
        entry.responseContinuation?.resume(throwing: error ?? TransportError.connectionDropped)
        if let error {
            entry.body?.finish(throwing: error)
        } else {
            entry.body?.finish()
        }
    }

    // MARK: URLSessionDataDelegate

    func urlSession(
        _ session: URLSession,
        dataTask: URLSessionDataTask,
        didReceive response: URLResponse,
        completionHandler: @escaping (URLSession.ResponseDisposition) -> Void
    ) {
        let identifier = dataTask.taskIdentifier
        guard let http = response as? HTTPURLResponse else {
            finish(taskIdentifier: identifier, error: TransportError.malformedResponse)
            completionHandler(.cancel)
            return
        }

        lock.lock()
        let continuation = entries[identifier]?.responseContinuation
        entries[identifier]?.responseContinuation = nil
        lock.unlock()

        continuation?.resume(returning: http)
        completionHandler(.allow)
    }

    func urlSession(
        _ session: URLSession,
        dataTask: URLSessionDataTask,
        didReceive data: Data
    ) {
        lock.lock()
        let body = entries[dataTask.taskIdentifier]?.body
        lock.unlock()
        // Yielded whole; no per-byte iteration anywhere on this path.
        body?.yield(data)
    }

    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        didCompleteWithError error: (any Error)?
    ) {
        lock.lock()
        let entry = entries.removeValue(forKey: task.taskIdentifier)
        lock.unlock()
        guard let entry else { return }

        if let error {
            // The response continuation is only still present when the request
            // failed before any headers arrived.
            entry.responseContinuation?.resume(throwing: error)
            entry.body?.finish(throwing: error)
        } else {
            entry.responseContinuation?.resume(throwing: TransportError.malformedResponse)
            entry.body?.finish()
        }
    }
}
