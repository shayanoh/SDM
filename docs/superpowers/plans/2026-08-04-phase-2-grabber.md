# SDM Phase 2 — Linkgrabber Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status: IMPLEMENTED — all five phases of the project are complete and merged to `main` (Phase 5 finished 2026-09-02).** Historical record only. Any "deferred to later phases" items here were picked up downstream; anything still open lives in `todo.md` at the repo root.

**Goal:** Build a tested `SDMGrabber` module — URL extraction, two-stage link probing, verdict rules, and package clustering — plus the app-side clipboard watcher and Linkgrabber UI that drive it.

**Architecture:** `SDMGrabber` depends only on `SDMCore` (per the spec's module table), so it cannot see `SDMEngine`'s `HTTPTransport`, `DownloadEngine`, or `DownloadItem`. It defines its own narrow `ProbeTransport` protocol for the HEAD/ranged-GET traffic link probing needs, and its own `FakeProbeOrigin` test double, mirroring the shape of `SDMEngine`'s `HTTPTransport`/`FakeOrigin` without depending on them. Probing, verdict evaluation, and package clustering are pure functions or protocol-injected actors — table-driven and fixture-tested, per spec §7.3–§7.4. The clipboard watcher and all `NSPasteboard`/drag-and-drop code live in the `SDM` app target, per the spec's module table (`SDMApp` owns "clipboard watcher"), and reach `SDMGrabber` only through `GrabberSession`.

**Tech Stack:** Swift 6.2, Swift Testing, SwiftUI, Foundation `URLSession`, `NSDataDetector`, `NSPasteboard`. No third-party dependencies.

**Spec:** [docs/superpowers/specs/2026-08-03-sdm-design.md](../specs/2026-08-03-sdm-design.md). Read §7 and §8's boundary with §7 (the resolver protocol itself is Phase 5 — Phase 2 only grabs and probes generic HTTP links) before starting. Also read the completed [Phase 1 plan](2026-08-03-phase-1-engine.md) for the coding conventions this plan follows: pure-boundary-logic testing (`URLSessionTransport`'s `static` helpers), actor-owned mutable state, and the `EngineController`/`ContentView` pairing this phase's `GrabberController`/`LinkGrabberView` mirror.

## Global Constraints

- **Deployment target stays macOS 15.0**, set in `SDMKit/Package.swift`. One API this phase wants — `NSPasteboard.detectedValues(for:)` — requires macOS 15.4+ per the installed macOS 26 SDK's `AppKit.swiftinterface`; it is used behind `if #available(macOS 15.4, *)` with a full-string-read fallback below that, never a baseline change. Re-verify this availability line against the SDK actually installed at implementation time — Apple could ship it in an earlier 15.x point release later.
- **`SDMGrabber` depends only on `SDMCore`.** It reuses `SDMCore.ByteRange` but defines its own `ProbeTransport` protocol rather than importing `SDMEngine`'s `HTTPTransport`. Do not add an `SDMEngine` dependency to `SDMGrabber` to save code — that inverts the module table in §3.
- **Swift tools version 6.2**, Swift 6 language mode, strict concurrency enabled. All cross-actor types must be `Sendable`.
- **Zero third-party dependencies.** Foundation, AppKit, and Swift standard library only.
- **Swift Testing only** (`@Test` / `#expect`). No XCTest in the package.
- **No `SDMGrabber` test may touch the network.** Every probing test runs against `FakeProbeOrigin`; `URLSessionProbeTransport`'s network call itself is never exercised, only its `static` boundary logic (request construction, header parsing, error classification) against synthesized values — the same pattern `URLSessionTransportTests.swift` already established in Phase 1.
- **All byte offsets are `Int64`.** Reuse `SDMCore.ByteRange` — half-open `[start, end)` — for the deep-sniff `Range` request; do not invent a second range type.
- **Dictionary iteration order is never load-bearing.** `PackageClustering.cluster` groups by dictionary internally but must sort its output deterministically before returning (Task 7) — Swift's per-process hash seed randomization makes raw dictionary iteration order flaky across test runs.
- **Format before every commit:** `swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests`, matching Phase 1.
- **Run tests with:** `swift test --package-path SDMKit`
- `SDMGrabber` lives at `SDMKit/Sources/SDMGrabber` / `SDMKit/Tests/SDMGrabberTests`, alongside the existing `SDMCore` and `SDMEngine` targets. The app target's Xcode project file is edited directly to add the new product dependency (Task 14) — there is no Xcode GUI in this environment.

---

### Task 1: `SDMGrabber` package scaffold

**Files:**
- Modify: `SDMKit/Package.swift`
- Create: `SDMKit/Sources/SDMGrabber/ModuleInfo.swift`
- Create: `SDMKit/Tests/SDMGrabberTests/SmokeTests.swift`

**Interfaces:**
- Consumes: `SDMCore` (already built)
- Produces: a new library target `SDMGrabber`, depending on `SDMCore`, with its own test target `SDMGrabberTests`

- [ ] **Step 1: Add the `SDMGrabber` target to `Package.swift`**

Replace the whole file:

```swift
// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "SDMKit",
    platforms: [.macOS(.v15)],
    products: [
        .library(name: "SDMCore", targets: ["SDMCore"]),
        .library(name: "SDMEngine", targets: ["SDMEngine"]),
        .library(name: "SDMGrabber", targets: ["SDMGrabber"]),
    ],
    targets: [
        .target(name: "SDMCore"),
        .target(name: "SDMEngine", dependencies: ["SDMCore"]),
        .target(name: "SDMGrabber", dependencies: ["SDMCore"]),
        .testTarget(name: "SDMCoreTests", dependencies: ["SDMCore"]),
        .testTarget(name: "SDMEngineTests", dependencies: ["SDMEngine"]),
        .testTarget(name: "SDMGrabberTests", dependencies: ["SDMGrabber"]),
    ]
)
```

- [ ] **Step 2: Create a placeholder source file so the target compiles**

`SDMKit/Sources/SDMGrabber/ModuleInfo.swift`:

```swift
/// Placeholder so the target compiles before real types land in Task 2.
enum SDMGrabberModuleInfo {}
```

- [ ] **Step 3: Create the smoke test**

`SDMKit/Tests/SDMGrabberTests/SmokeTests.swift`:

```swift
import Testing

@Test func grabberTargetBuilds() {
    #expect(Bool(true))
}
```

- [ ] **Step 4: Run the tests**

Run: `swift test --package-path SDMKit`
Expected: PASS, full suite including the new `grabberTargetBuilds` test.

- [ ] **Step 5: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: scaffold SDMGrabber package target"
```

---

### Task 2: URL extraction

**Files:**
- Create: `SDMKit/Sources/SDMGrabber/URLExtractor.swift`
- Create: `SDMKit/Tests/SDMGrabberTests/URLExtractorTests.swift`

**Interfaces:**
- Consumes: nothing
- Produces: `enum URLExtractor` with `static func extractLinks(from text: String) -> [URL]`

Per spec §7.1, extraction uses `NSDataDetector` rather than a regex. Only `http`/`https` schemes are returned — `mailto:`, `ftp:`, and similar are not links this app can grab. Order is preserved and duplicates within one batch are collapsed, since the same paste often repeats a link in surrounding prose.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMGrabberTests/URLExtractorTests.swift`:

```swift
import Testing

@testable import SDMGrabber

@Test func extractsPlainURLFromProse() {
    let text = "Check this out: https://example.com/movie.mp4 nice right"
    #expect(URLExtractor.extractLinks(from: text) == [URL(string: "https://example.com/movie.mp4")!])
}

@Test func extractsMultipleDistinctURLsInOrder() {
    let text = "https://a.example.com/1.zip and https://b.example.com/2.zip"
    #expect(
        URLExtractor.extractLinks(from: text) == [
            URL(string: "https://a.example.com/1.zip")!,
            URL(string: "https://b.example.com/2.zip")!,
        ]
    )
}

@Test func dedupesRepeatedURL() {
    let text = "https://a.example.com/1.zip mirror: https://a.example.com/1.zip"
    #expect(URLExtractor.extractLinks(from: text) == [URL(string: "https://a.example.com/1.zip")!])
}

@Test func ignoresNonHTTPSchemes() {
    let text = "ftp://old.example.com/file.zip or mailto:me@example.com"
    #expect(URLExtractor.extractLinks(from: text).isEmpty)
}

@Test func handlesURLOnItsOwnLineWithinAParagraph() {
    let text = """
        Here's the download link:
        https://cdn.example.com/season1/show.s01e01.mkv
        Let me know if it works.
        """
    #expect(
        URLExtractor.extractLinks(from: text) == [
            URL(string: "https://cdn.example.com/season1/show.s01e01.mkv")!
        ]
    )
}

@Test func returnsEmptyForTextWithNoLinks() {
    #expect(URLExtractor.extractLinks(from: "just some text, nothing to see").isEmpty)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter URLExtractorTests`
Expected: FAIL — `cannot find 'URLExtractor' in scope`.

- [ ] **Step 3: Implement `URLExtractor`**

`SDMKit/Sources/SDMGrabber/URLExtractor.swift`:

```swift
import Foundation

/// Pulls `http`/`https` links out of arbitrary pasted or dropped text.
/// Spec §7.1: `NSDataDetector` rather than a regex, so URLs embedded in
/// prose and wrapped by ordinary line breaks are found correctly.
public enum URLExtractor {
    public static func extractLinks(from text: String) -> [URL] {
        guard
            let detector = try? NSDataDetector(
                types: NSTextCheckingResult.CheckingType.link.rawValue
            )
        else { return [] }

        let range = NSRange(text.startIndex..<text.endIndex, in: text)
        var seen = Set<URL>()
        var result: [URL] = []

        detector.enumerateMatches(in: text, options: [], range: range) { match, _, _ in
            guard let url = match?.url, let scheme = url.scheme?.lowercased(),
                scheme == "http" || scheme == "https"
            else { return }
            if seen.insert(url).inserted { result.append(url) }
        }

        return result
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: add NSDataDetector-based URL extraction"
```

---

### Task 3: `ProbeTransport`, `FakeProbeOrigin`, and `URLSessionProbeTransport`

**Files:**
- Create: `SDMKit/Sources/SDMGrabber/ProbeTransport.swift`
- Create: `SDMKit/Sources/SDMGrabber/FakeProbeOrigin.swift`
- Create: `SDMKit/Sources/SDMGrabber/URLSessionProbeTransport.swift`
- Create: `SDMKit/Tests/SDMGrabberTests/FakeProbeOriginTests.swift`
- Create: `SDMKit/Tests/SDMGrabberTests/URLSessionProbeTransportTests.swift`

**Interfaces:**
- Consumes: `ByteRange` from `SDMCore`
- Produces:
  - `struct ProbeRequest: Sendable` — `url: URL`, `method: Method`, `range: ByteRange?`; `enum Method: Sendable, Equatable { case head, get }`
  - `struct ProbeResponse: Sendable` — `statusCode: Int`, `finalURL: URL`, `headers: [String: String]` (lowercased keys), `body: Data`
  - `enum ProbeError: Error, Equatable` — `timedOut`, `dnsFailure`, `malformedResponse`
  - `protocol ProbeTransport: Sendable` with `func send(_ request: ProbeRequest) async throws -> ProbeResponse`
  - `actor FakeProbeOrigin: ProbeTransport` with a per-URL `Behavior`
  - `struct URLSessionProbeTransport: ProbeTransport`

Unlike `SDMEngine`'s `HTTPTransport`, responses here are always small and bounded — a `HEAD` has no body, and the deep sniff (Task 5) only ever asks for the first 64 KB — so `ProbeResponse.body` is a plain `Data`, not a stream. `FakeProbeOrigin` is keyed by URL, not single-payload, because a grabber test typically probes several distinct links at once.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMGrabberTests/FakeProbeOriginTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMGrabber

private let url = URL(string: "https://example.com/file.zip")!

@Test func headRequestReturnsConfiguredHeaders() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.statusCode = 200
    behavior.headers = ["content-length": "1000", "content-type": "application/zip"]
    await origin.setBehavior(behavior, for: url)

    let response = try await origin.send(ProbeRequest(url: url, method: .head))
    #expect(response.statusCode == 200)
    #expect(response.headers["content-length"] == "1000")
    #expect(response.body.isEmpty)
}

@Test func getRequestWithRangeReturnsSlice() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.body = Data((0..<100).map { UInt8($0) })
    await origin.setBehavior(behavior, for: url)

    let response = try await origin.send(
        ProbeRequest(url: url, method: .get, range: ByteRange(start: 10, end: 20))
    )
    #expect(response.body == Data((10..<20).map { UInt8($0) }))
}

@Test func rejectsHeadThrowsMalformedResponse() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.rejectsHead = true
    await origin.setBehavior(behavior, for: url)

    await #expect(throws: ProbeError.malformedResponse) {
        _ = try await origin.send(ProbeRequest(url: url, method: .head))
    }
}

@Test func defaultBehaviorIs404ForUnconfiguredURL() async throws {
    let origin = FakeProbeOrigin()
    let response = try await origin.send(ProbeRequest(url: url, method: .head))
    #expect(response.statusCode == 404)
}

@Test func requestLogRecordsEachRequestInOrder() async throws {
    let origin = FakeProbeOrigin()
    _ = try await origin.send(ProbeRequest(url: url, method: .head))
    _ = try await origin.send(ProbeRequest(url: url, method: .get, range: ByteRange(start: 0, end: 1)))
    let log = await origin.requestLog
    #expect(log.map(\.method) == [.head, .get])
}

@Test func holdsUntilReleasedBlocksUntilReleaseCalled() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.holdsUntilReleased = true
    await origin.setBehavior(behavior, for: url)

    let task = Task { try await origin.send(ProbeRequest(url: url, method: .head)) }
    while await origin.requestLog.isEmpty { await Task.yield() }
    // Still holding: the send call has registered but not returned.
    await origin.release(url)
    _ = try await task.value
}
```

`SDMKit/Tests/SDMGrabberTests/URLSessionProbeTransportTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMGrabber

private let url = URL(string: "https://example.com/file.zip")!

@Test func requestUsesHeadMethodWithNoRangeHeader() {
    let request = URLSessionProbeTransport.makeURLRequest(
        for: ProbeRequest(url: url, method: .head)
    )
    #expect(request.httpMethod == "HEAD")
    #expect(request.value(forHTTPHeaderField: "Range") == nil)
}

@Test func requestUsesGetMethodWithRangeHeaderWhenProvided() {
    let request = URLSessionProbeTransport.makeURLRequest(
        for: ProbeRequest(url: url, method: .get, range: ByteRange(start: 0, end: 65536))
    )
    #expect(request.httpMethod == "GET")
    #expect(request.value(forHTTPHeaderField: "Range") == "bytes=0-65535")
}

@Test func requestCarriesNoRangeHeaderForNilRange() {
    let request = URLSessionProbeTransport.makeURLRequest(for: ProbeRequest(url: url, method: .get))
    #expect(request.value(forHTTPHeaderField: "Range") == nil)
}

@Test func headersAreLowercasedForCaseInsensitiveLookup() {
    let response = HTTPURLResponse(
        url: url,
        statusCode: 200,
        httpVersion: "HTTP/1.1",
        headerFields: ["Content-Type": "application/zip", "ETag": "\"abc\""]
    )!
    let headers = URLSessionProbeTransport.lowercasedHeaders(from: response)
    #expect(headers["content-type"] == "application/zip")
    #expect(headers["etag"] == "\"abc\"")
}

@Test func classifyMapsTimeoutAndDNSErrorCodes() {
    #expect(URLSessionProbeTransport.classify(.timedOut) == .timedOut)
    #expect(URLSessionProbeTransport.classify(.cannotFindHost) == .dnsFailure)
    #expect(URLSessionProbeTransport.classify(.dnsLookupFailed) == .dnsFailure)
}

@Test func classifyReturnsNilForUnrecognizedErrorCodes() {
    #expect(URLSessionProbeTransport.classify(.notConnectedToInternet) == nil)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter FakeProbeOriginTests`
Expected: FAIL — `cannot find 'FakeProbeOrigin' in scope`.

- [ ] **Step 3: Implement `ProbeTransport`**

`SDMKit/Sources/SDMGrabber/ProbeTransport.swift`:

```swift
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
```

- [ ] **Step 4: Implement `FakeProbeOrigin`**

`SDMKit/Sources/SDMGrabber/FakeProbeOrigin.swift`:

```swift
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
    private let defaultBehavior = Behavior(statusCode: 404)
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
```

- [ ] **Step 5: Implement `URLSessionProbeTransport`**

`SDMKit/Sources/SDMGrabber/URLSessionProbeTransport.swift`:

```swift
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
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: add ProbeTransport protocol, FakeProbeOrigin and URLSessionProbeTransport"
```

---

### Task 4: `ProbedLink` and stage 1 probing

**Files:**
- Create: `SDMKit/Sources/SDMGrabber/ProbedLink.swift`
- Create: `SDMKit/Sources/SDMGrabber/LinkProber.swift`
- Create: `SDMKit/Tests/SDMGrabberTests/LinkProberTests.swift`

**Interfaces:**
- Consumes: `ProbeTransport`, `ProbeRequest`, `ProbeResponse`, `ProbeError` from Task 3
- Produces:
  - `struct ProbedLink: Identifiable, Equatable, Sendable` with `enum Stage: Equatable, Sendable { case queued, probing, sniffing, done }`
  - `struct LinkProber: Sendable` with `func probe(_ url: URL) async -> ProbedLink`

`LinkProber` never computes a `Verdict` itself — that is a separate pure function over the finished `ProbedLink` (Task 6), kept out of the prober so probing and verdict tuning can be tested and revised independently, per spec §7.3's "pure function over the probe result."

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMGrabberTests/LinkProberTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMGrabber

private let url = URL(string: "https://example.com/movie.mp4")!

@Test func probeCapturesHeadResponseFields() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.statusCode = 200
    behavior.headers = [
        "content-length": "5000000",
        "content-type": "video/mp4; charset=binary",
        "etag": "\"abc123\"",
        "content-disposition": "attachment; filename=\"Real Movie.mp4\"",
    ]
    await origin.setBehavior(behavior, for: url)

    let link = await LinkProber(transport: origin, deepSniffEnabled: false).probe(url)
    #expect(link.statusCode == 200)
    #expect(link.contentLength == 5_000_000)
    #expect(link.contentType == "video/mp4")
    #expect(link.validator == "\"abc123\"")
    #expect(link.suggestedFilename == "Real Movie.mp4")
    #expect(link.acceptsRanges == false)
    #expect(link.stage == .done)
    #expect(link.transportFailed == false)
}

@Test func probeFallsBackToRangedGetWhenHeadIsRejected() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.rejectsHead = true
    behavior.statusCode = 206
    await origin.setBehavior(behavior, for: url)

    let link = await LinkProber(transport: origin, deepSniffEnabled: false).probe(url)
    #expect(link.acceptsRanges == true)
    let log = await origin.requestLog
    #expect(log.map(\.method) == [.head, .get])
}

@Test func probeCapturesTotalFromContentRangeOnFallbackGet() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.rejectsHead = true
    behavior.statusCode = 206
    behavior.headers = ["content-range": "bytes 0-0/123456", "content-length": "1"]
    await origin.setBehavior(behavior, for: url)

    let link = await LinkProber(transport: origin, deepSniffEnabled: false).probe(url)
    #expect(link.contentLength == 123456)
}

@Test func probePreservesStatusCodeVerbatim() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.statusCode = 404
    await origin.setBehavior(behavior, for: url)

    let link = await LinkProber(transport: origin, deepSniffEnabled: false).probe(url)
    #expect(link.statusCode == 404)
    #expect(link.stage == .done)
}

@Test func probeMarksTransportFailureWhenBothAttemptsThrow() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.error = .timedOut
    await origin.setBehavior(behavior, for: url)

    let link = await LinkProber(transport: origin, deepSniffEnabled: false).probe(url)
    #expect(link.transportFailed == true)
    #expect(link.statusCode == nil)
    #expect(link.stage == .done)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter LinkProberTests`
Expected: FAIL — `cannot find 'ProbedLink' in scope`.

- [ ] **Step 3: Implement `ProbedLink`**

`SDMKit/Sources/SDMGrabber/ProbedLink.swift`:

```swift
import Foundation

/// One link's progress through the two-stage probing pipeline. Spec §7.5:
/// rows appear immediately as `.queued` and fill in as probes land.
public struct ProbedLink: Identifiable, Equatable, Sendable {
    public enum Stage: Equatable, Sendable {
        case queued
        case probing
        case sniffing
        case done
    }

    public let id: UUID
    public let originalURL: URL
    public var finalURL: URL
    public var stage: Stage
    public var statusCode: Int?
    public var contentLength: Int64?
    public var contentType: String?
    public var suggestedFilename: String?
    /// `ETag`, or `Last-Modified` when no `ETag` is offered.
    public var validator: String?
    /// Whether a `206` came back — spec §7.2's resume-capable flag.
    public var acceptsRanges: Bool
    public var sniffedSignature: FileSignature?
    /// True when both the HEAD and the ranged-GET fallback threw — a DNS or
    /// timeout failure rather than an HTTP-level error status.
    public var transportFailed: Bool
    public var verdict: Verdict?
    public var isDuplicate: Bool

    public init(
        id: UUID = UUID(),
        originalURL: URL,
        finalURL: URL? = nil,
        stage: Stage = .queued,
        statusCode: Int? = nil,
        contentLength: Int64? = nil,
        contentType: String? = nil,
        suggestedFilename: String? = nil,
        validator: String? = nil,
        acceptsRanges: Bool = false,
        sniffedSignature: FileSignature? = nil,
        transportFailed: Bool = false,
        verdict: Verdict? = nil,
        isDuplicate: Bool = false
    ) {
        self.id = id
        self.originalURL = originalURL
        self.finalURL = finalURL ?? originalURL
        self.stage = stage
        self.statusCode = statusCode
        self.contentLength = contentLength
        self.contentType = contentType
        self.suggestedFilename = suggestedFilename
        self.validator = validator
        self.acceptsRanges = acceptsRanges
        self.sniffedSignature = sniffedSignature
        self.transportFailed = transportFailed
        self.verdict = verdict
        self.isDuplicate = isDuplicate
    }

    /// The name used for clustering and display: the server-declared name
    /// when Stage 1 captured one, otherwise the last URL path component.
    public var effectiveFilename: String {
        if let suggestedFilename, !suggestedFilename.isEmpty { return suggestedFilename }
        let last = finalURL.lastPathComponent
        return last.isEmpty ? "download" : last
    }
}
```

Note: this references `Verdict` and `FileSignature`, which do not exist yet — that is expected, they land in Tasks 5 and 6. The package will not compile until then; do not run the full suite between steps of this task, only the filtered `LinkProberTests` target once all three files below exist.

- [ ] **Step 4: Implement `LinkProber`**

`SDMKit/Sources/SDMGrabber/LinkProber.swift`:

```swift
import Foundation
import SDMCore

/// Runs spec §7.2's stage 1 (cheap probe) and, when the link looks alive and
/// deep sniff is enabled, stage 2 (magic-byte sniff, Task 5).
public struct LinkProber: Sendable {
    private let transport: any ProbeTransport
    public let deepSniffEnabled: Bool

    public init(transport: any ProbeTransport, deepSniffEnabled: Bool = true) {
        self.transport = transport
        self.deepSniffEnabled = deepSniffEnabled
    }

    public func probe(_ url: URL) async -> ProbedLink {
        var link = ProbedLink(originalURL: url, stage: .probing)

        do {
            let response = try await stageOneResponse(for: url)
            apply(response, to: &link)
        } catch {
            link.transportFailed = true
            link.stage = .done
            return link
        }

        await sniffIfNeeded(&link)
        link.stage = .done
        return link
    }

    /// HEAD first; falls back to a `Range: bytes=0-0` GET when the origin
    /// rejects or lies about HEAD.
    private func stageOneResponse(for url: URL) async throws -> ProbeResponse {
        do {
            return try await transport.send(ProbeRequest(url: url, method: .head))
        } catch {
            return try await transport.send(
                ProbeRequest(url: url, method: .get, range: ByteRange(start: 0, end: 1))
            )
        }
    }

    private func apply(_ response: ProbeResponse, to link: inout ProbedLink) {
        link.finalURL = response.finalURL
        link.statusCode = response.statusCode
        link.acceptsRanges = response.statusCode == 206
        if let text = response.headers["content-length"], let length = Int64(text) {
            link.contentLength = length
        }
        if let contentRange = response.headers["content-range"],
            let slash = contentRange.lastIndex(of: "/")
        {
            let total = contentRange[contentRange.index(after: slash)...]
            if total != "*", let value = Int64(total) { link.contentLength = value }
        }
        link.contentType = response.headers["content-type"]?
            .split(separator: ";").first.map(String.init)
        link.validator = response.headers["etag"] ?? response.headers["last-modified"]
        link.suggestedFilename = Self.filename(
            fromContentDisposition: response.headers["content-disposition"]
        )
    }

    private static func filename(fromContentDisposition header: String?) -> String? {
        guard let header else { return nil }
        for part in header.split(separator: ";") {
            let trimmed = part.trimmingCharacters(in: .whitespaces)
            guard trimmed.lowercased().hasPrefix("filename=") else { continue }
            let value = trimmed.dropFirst("filename=".count)
                .trimmingCharacters(in: CharacterSet(charactersIn: "\""))
            return value.isEmpty ? nil : value
        }
        return nil
    }
}
```

- [ ] **Step 5: Add a no-op stage 2 stub so `LinkProber` compiles standalone**

Append to `SDMKit/Sources/SDMGrabber/LinkProber.swift` (this stub is replaced wholesale in Task 5):

```swift
extension LinkProber {
    fileprivate func sniffIfNeeded(_ link: inout ProbedLink) async {
        // Stage 2 lands in Task 5.
    }
}
```

- [ ] **Step 6: Add minimal `Verdict` and `FileSignature` stubs so the target compiles**

`SDMKit/Sources/SDMGrabber/ProbedLink.swift` needs `Verdict` and `FileSignature` to exist. Create two placeholder files, replaced wholesale in Tasks 5 and 6:

`SDMKit/Sources/SDMGrabber/FileSignature.swift`:

```swift
/// Placeholder — replaced with real magic-byte detection in Task 5.
public enum FileSignature: Equatable, Sendable {
    case unknown
}
```

`SDMKit/Sources/SDMGrabber/Verdict.swift`:

```swift
/// Placeholder — replaced with the real four-case type in Task 6.
public enum Verdict: Equatable, Sendable {
    case online
}
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: add ProbedLink and stage 1 link probing"
```

---

### Task 5: Stage 2 deep sniff

**Files:**
- Modify: `SDMKit/Sources/SDMGrabber/FileSignature.swift`
- Modify: `SDMKit/Sources/SDMGrabber/LinkProber.swift`
- Create: `SDMKit/Tests/SDMGrabberTests/FileSignatureTests.swift`
- Create: `SDMKit/Tests/SDMGrabberTests/LinkProberStageTwoTests.swift`

**Interfaces:**
- Consumes: `ProbedLink`, `LinkProber` from Task 4
- Produces:
  - `enum FileSignature: Equatable, Sendable` — `zip, rar, gzip, mp4, mkv, pdf, html, unknown`, with `static func detect(in data: Data) -> FileSignature` and `func matches(extension:) -> Bool`
  - `LinkProber.sniffIfNeeded` performs the real stage 2 GET-with-range when the link looks alive and `deepSniffEnabled` is true

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMGrabberTests/FileSignatureTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMGrabber

@Test func detectsZipMagicBytes() {
    #expect(FileSignature.detect(in: Data([0x50, 0x4B, 0x03, 0x04])) == .zip)
}

@Test func detectsRarMagicBytes() {
    #expect(FileSignature.detect(in: Data([0x52, 0x61, 0x72, 0x21])) == .rar)
}

@Test func detectsMp4ByFtypBoxAtOffsetFour() {
    var bytes = Data([0x00, 0x00, 0x00, 0x18])
    bytes.append(contentsOf: "ftypmp42".utf8)
    #expect(FileSignature.detect(in: bytes) == .mp4)
}

@Test func detectsHTMLByDoctype() {
    let data = Data("<!DOCTYPE html><html><body>Not found</body></html>".utf8)
    #expect(FileSignature.detect(in: data) == .html)
}

@Test func unknownForUnrecognizedBytes() {
    #expect(FileSignature.detect(in: Data([0x01, 0x02, 0x03])) == .unknown)
}

@Test func matchesAcceptsExpectedExtensionPairs() {
    #expect(FileSignature.zip.matches(extension: "zip"))
    #expect(FileSignature.mp4.matches(extension: "mp4"))
    #expect(FileSignature.mp4.matches(extension: "m4v"))
}

@Test func matchesRejectsContradictingExtension() {
    #expect(!FileSignature.html.matches(extension: "mp4"))
    #expect(!FileSignature.zip.matches(extension: "pdf"))
}

@Test func unknownSignatureNeverContradictsAnExtension() {
    // Insufficient data to sniff should never itself trigger a "faulty" verdict.
    #expect(FileSignature.unknown.matches(extension: "mp4"))
}
```

`SDMKit/Tests/SDMGrabberTests/LinkProberStageTwoTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMGrabber

private let url = URL(string: "https://example.com/movie.mp4")!

@Test func sniffCapturesMagicBytesForASuccessfulLink() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.statusCode = 200
    behavior.body = Data([0x50, 0x4B, 0x03, 0x04, 0x00, 0x00])
    await origin.setBehavior(behavior, for: url)

    let link = await LinkProber(transport: origin, deepSniffEnabled: true).probe(url)
    #expect(link.sniffedSignature == .zip)
    let log = await origin.requestLog
    #expect(log.map(\.method) == [.head, .get])
}

@Test func sniffSkippedWhenDeepSniffDisabled() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.statusCode = 200
    behavior.body = Data([0x50, 0x4B, 0x03, 0x04])
    await origin.setBehavior(behavior, for: url)

    let link = await LinkProber(transport: origin, deepSniffEnabled: false).probe(url)
    #expect(link.sniffedSignature == nil)
    let log = await origin.requestLog
    #expect(log.map(\.method) == [.head])
}

@Test func sniffSkippedWhenStatusIsNotSuccessful() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.statusCode = 404
    await origin.setBehavior(behavior, for: url)

    let link = await LinkProber(transport: origin, deepSniffEnabled: true).probe(url)
    #expect(link.sniffedSignature == nil)
    let log = await origin.requestLog
    #expect(log.map(\.method) == [.head])
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter FileSignatureTests`
Expected: FAIL — `type 'FileSignature' has no member 'zip'`.

- [ ] **Step 3: Implement the real `FileSignature`**

Replace `SDMKit/Sources/SDMGrabber/FileSignature.swift`:

```swift
import Foundation

/// A magic-byte identification of a small header sample. Spec §7.2 stage 2.
public enum FileSignature: Equatable, Sendable {
    case zip
    case rar
    case gzip
    case mp4
    case mkv
    case pdf
    case html
    case unknown

    public static func detect(in data: Data) -> FileSignature {
        let bytes = [UInt8](data.prefix(16))
        if bytes.starts(with: [0x50, 0x4B, 0x03, 0x04]) || bytes.starts(with: [0x50, 0x4B, 0x05, 0x06]) {
            return .zip
        }
        if bytes.starts(with: [0x52, 0x61, 0x72, 0x21]) { return .rar }
        if bytes.starts(with: [0x1F, 0x8B]) { return .gzip }
        if bytes.count >= 8, bytes[4] == 0x66, bytes[5] == 0x74, bytes[6] == 0x79, bytes[7] == 0x70 {
            return .mp4
        }
        if bytes.starts(with: [0x1A, 0x45, 0xDF, 0xA3]) { return .mkv }
        if bytes.starts(with: [0x25, 0x50, 0x44, 0x46]) { return .pdf }
        if let text = String(data: data.prefix(512), encoding: .utf8)?.lowercased(),
            text.contains("<!doctype html") || text.contains("<html")
        {
            return .html
        }
        return .unknown
    }

    /// Whether this signature is consistent with a claimed file extension.
    /// `.unknown` never contradicts — there was not enough data to sniff, so
    /// it must not itself trigger a "faulty" verdict.
    public func matches(extension ext: String) -> Bool {
        switch (self, ext.lowercased()) {
        case (.unknown, _): return true
        case (.zip, "zip"): return true
        case (.rar, "rar"): return true
        case (.gzip, "gz"), (.gzip, "tgz"): return true
        case (.mp4, "mp4"), (.mp4, "m4v"): return true
        case (.mkv, "mkv"): return true
        case (.pdf, "pdf"): return true
        default: return false
        }
    }
}
```

- [ ] **Step 4: Implement real stage 2 in `LinkProber`**

In `SDMKit/Sources/SDMGrabber/LinkProber.swift`, replace the Task 4 stub extension:

```swift
extension LinkProber {
    fileprivate func sniffIfNeeded(_ link: inout ProbedLink) async {
        guard deepSniffEnabled, let status = link.statusCode, (200...299).contains(status) else {
            return
        }
        link.stage = .sniffing
        guard
            let response = try? await transport.send(
                ProbeRequest(url: link.finalURL, method: .get, range: ByteRange(start: 0, end: 65536))
            )
        else { return }
        link.sniffedSignature = FileSignature.detect(in: response.body)
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: add stage 2 magic-byte sniffing"
```

---

### Task 6: Verdict rules

**Files:**
- Modify: `SDMKit/Sources/SDMGrabber/Verdict.swift`
- Create: `SDMKit/Sources/SDMGrabber/VerdictRules.swift`
- Create: `SDMKit/Tests/SDMGrabberTests/VerdictRulesTests.swift`

**Interfaces:**
- Consumes: `ProbedLink`, `FileSignature` from Tasks 4–5
- Produces:
  - `enum Verdict: Equatable, Sendable` — `online, offline, faulty(reason: String), checkFailed`
  - `enum VerdictRules` with `static func evaluate(_ link: ProbedLink) -> Verdict`

Table-driven per spec §7.3: a fixed rule order, each rule a pure predicate over `ProbedLink`, tuned by editing the table and its fixtures rather than control flow.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMGrabberTests/VerdictRulesTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMGrabber

private func link(
    statusCode: Int? = 200,
    contentLength: Int64? = 5_000_000,
    contentType: String? = "video/mp4",
    filename: String = "movie.mp4",
    signature: FileSignature? = nil,
    transportFailed: Bool = false,
    originalHost: String = "example.com",
    finalHost: String = "example.com",
    finalPath: String = "/movie.mp4"
) -> ProbedLink {
    var probed = ProbedLink(
        originalURL: URL(string: "https://\(originalHost)/movie.mp4")!,
        finalURL: URL(string: "https://\(finalHost)\(finalPath)")!,
        stage: .done,
        statusCode: statusCode,
        contentLength: contentLength,
        contentType: contentType,
        suggestedFilename: filename,
        sniffedSignature: signature,
        transportFailed: transportFailed
    )
    probed.suggestedFilename = filename
    return probed
}

@Test func onlineForAPlausibleSuccessfulResponse() {
    #expect(VerdictRules.evaluate(link()) == .online)
}

@Test func offlineForNon2xxStatus() {
    #expect(VerdictRules.evaluate(link(statusCode: 404)) == .offline)
}

@Test func faultyForHTMLBodyOnAMediaExtension() {
    #expect(
        VerdictRules.evaluate(link(contentType: "text/html")) == .faulty(reason: "html, not mp4")
    )
}

@Test func faultyForImplausiblySmallSize() {
    #expect(
        VerdictRules.evaluate(link(contentLength: 100, filename: "archive.zip"))
            == .faulty(reason: "too small to be a real zip")
    )
}

@Test func faultyForSignatureContradictingExtension() {
    #expect(
        VerdictRules.evaluate(link(signature: .html))
            == .faulty(reason: "file signature does not match .mp4")
    )
}

@Test func checkFailedWhenTheTransportNeverConnected() {
    #expect(VerdictRules.evaluate(link(transportFailed: true)) == .checkFailed)
}

@Test func checkFailedWhenStatusWasNeverCaptured() {
    #expect(VerdictRules.evaluate(link(statusCode: nil)) == .checkFailed)
}

@Test func faultyForRedirectToASuspiciousPathOnADifferentHost() {
    let verdict = VerdictRules.evaluate(
        link(
            contentType: "application/octet-stream",
            finalHost: "sketchy-cdn.example",
            finalPath: "/link-expired"
        )
    )
    #expect(verdict == .faulty(reason: "redirected to sketchy-cdn.example"))
}

@Test func onlineForASmallNonMediaFile() {
    #expect(
        VerdictRules.evaluate(
            link(contentLength: 50, contentType: "text/plain", filename: "notes.txt")
        ) == .online
    )
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter VerdictRulesTests`
Expected: FAIL — `cannot find 'VerdictRules' in scope`.

- [ ] **Step 3: Implement the real `Verdict`**

Replace `SDMKit/Sources/SDMGrabber/Verdict.swift`:

```swift
/// A pure function's output over a probe result. Spec §7.3: four outcomes,
/// because each implies a different user action.
public enum Verdict: Equatable, Sendable {
    case online
    case offline
    case faulty(reason: String)
    case checkFailed
}
```

- [ ] **Step 4: Implement `VerdictRules`**

`SDMKit/Sources/SDMGrabber/VerdictRules.swift`:

```swift
import Foundation

private let mediaAndArchiveExtensions: Set<String> = [
    "mp4", "mkv", "avi", "mov", "m4v", "zip", "rar", "7z", "gz", "tar", "iso", "pdf",
]

private let suspiciousPathTokens = ["login", "error", "expired", "404", "not-found", "denied"]

/// Table-driven pure function over a finished probe. Spec §7.3.
public enum VerdictRules {
    public static func evaluate(_ link: ProbedLink) -> Verdict {
        guard !link.transportFailed else { return .checkFailed }
        guard let status = link.statusCode else { return .checkFailed }
        guard (200...299).contains(status) else { return .offline }

        let ext = link.effectiveFilename.split(separator: ".").last.map { String($0).lowercased() } ?? ""
        let isMediaOrArchive = mediaAndArchiveExtensions.contains(ext)

        if isMediaOrArchive, link.contentType?.lowercased() == "text/html" {
            return .faulty(reason: "html, not \(ext)")
        }

        if isMediaOrArchive, let length = link.contentLength, length < 1024 {
            return .faulty(reason: "too small to be a real \(ext)")
        }

        if let signature = link.sniffedSignature, !ext.isEmpty, !signature.matches(extension: ext) {
            return .faulty(reason: "file signature does not match .\(ext)")
        }

        if link.originalURL.host != link.finalURL.host {
            let path = link.finalURL.path.lowercased()
            if suspiciousPathTokens.contains(where: path.contains) {
                return .faulty(reason: "redirected to \(link.finalURL.host ?? "unknown host")")
            }
        }

        return .online
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: add table-driven verdict rules"
```

---

### Task 7: Package clustering

**Files:**
- Create: `SDMKit/Sources/SDMGrabber/PackageClustering.swift`
- Create: `SDMKit/Tests/SDMGrabberTests/PackageClusteringTests.swift`

**Interfaces:**
- Consumes: nothing beyond `Foundation`
- Produces:
  - `struct ClusterableLink: Identifiable, Equatable, Sendable` — `id: UUID`, `filename: String`, `host: String`, `directoryPath: String`
  - `struct PackageCandidate: Equatable, Sendable` — `name: String`, `linkIDs: [UUID]`, `isArchive: Bool`
  - `enum PackageClustering` with `static func cluster(_ links: [ClusterableLink]) -> [PackageCandidate]`

Spec §7.4's five-step algorithm: archive-part sets lock first (they must never split), then filenames reduce to a template and group by it, singleton templates fall back to host+path grouping, and the name is the cleaned longest common prefix of member stems (falling back to the host). Output is sorted by each candidate's earliest-appearing member so it never depends on `Dictionary`'s randomized iteration order (Global Constraints).

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMGrabberTests/PackageClusteringTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMGrabber

private func link(_ filename: String, host: String = "cdn.example.com", path: String = "/season1")
    -> ClusterableLink
{
    ClusterableLink(id: UUID(), filename: filename, host: host, directoryPath: path)
}

@Test func episodesWithTheSameTemplateClusterTogether() {
    let e1 = link("Show.S01E01.1080p.mkv")
    let e2 = link("Show.S01E02.1080p.mkv")
    let candidates = PackageClustering.cluster([e1, e2])

    #expect(candidates.count == 1)
    #expect(candidates[0].name == "Show.S01E0")
    #expect(Set(candidates[0].linkIDs) == Set([e1.id, e2.id]))
    #expect(candidates[0].isArchive == false)
}

@Test func archivePartsLockTogetherRegardlessOfTemplate() {
    let parts = [
        link("Movie.part01.rar"),
        link("Movie.part02.rar"),
        link("Movie.part03.rar"),
    ]
    let candidates = PackageClustering.cluster(parts)

    #expect(candidates.count == 1)
    #expect(candidates[0].name == "movie")
    #expect(candidates[0].isArchive == true)
    #expect(Set(candidates[0].linkIDs) == Set(parts.map(\.id)))
}

@Test func singletonTemplatesGroupByHostAndPath() {
    let a = link("readme.txt")
    let b = link("changelog.md")
    let candidates = PackageClustering.cluster([a, b])

    #expect(candidates.count == 1)
    #expect(candidates[0].name == "cdn.example.com")
    #expect(Set(candidates[0].linkIDs) == Set([a.id, b.id]))
}

@Test func dissimilarTemplatesOnDifferentHostsStaySeparate() {
    let a = link("readme.txt", host: "one.example.com")
    let b = link("changelog.md", host: "two.example.com")
    let candidates = PackageClustering.cluster([a, b])

    #expect(candidates.count == 2)
    #expect(Set(candidates.flatMap(\.linkIDs)) == Set([a.id, b.id]))
}

@Test func clusteringEmptyInputReturnsNoPackages() {
    #expect(PackageClustering.cluster([]).isEmpty)
}

@Test func outputOrderIsDeterministicAcrossRepeatedCalls() {
    let links = (0..<12).map { link("file\($0).bin", host: "h\($0).example.com") }
    let first = PackageClustering.cluster(links).map(\.name)
    let second = PackageClustering.cluster(links).map(\.name)
    #expect(first == second)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter PackageClusteringTests`
Expected: FAIL — `cannot find 'ClusterableLink' in scope`.

- [ ] **Step 3: Implement `PackageClustering`**

`SDMKit/Sources/SDMGrabber/PackageClustering.swift`:

```swift
import Foundation

public struct ClusterableLink: Identifiable, Equatable, Sendable {
    public let id: UUID
    public let filename: String
    public let host: String
    public let directoryPath: String

    public init(id: UUID, filename: String, host: String, directoryPath: String) {
        self.id = id
        self.filename = filename
        self.host = host
        self.directoryPath = directoryPath
    }
}

public struct PackageCandidate: Equatable, Sendable {
    public var name: String
    public var linkIDs: [UUID]
    public var isArchive: Bool

    public init(name: String, linkIDs: [UUID], isArchive: Bool = false) {
        self.name = name
        self.linkIDs = linkIDs
        self.isArchive = isArchive
    }
}

/// A pure function `[ClusterableLink] -> [PackageCandidate]`. Spec §7.4.
public enum PackageClustering {
    public static func cluster(_ links: [ClusterableLink]) -> [PackageCandidate] {
        guard !links.isEmpty else { return [] }
        let inputOrder = Dictionary(uniqueKeysWithValues: links.enumerated().map { ($1.id, $0) })

        var archiveGroups: [String: [ClusterableLink]] = [:]
        var remaining: [ClusterableLink] = []
        for candidate in links {
            if let base = archiveBaseName(candidate.filename) {
                archiveGroups[base, default: []].append(candidate)
            } else {
                remaining.append(candidate)
            }
        }

        var templateGroups: [String: [ClusterableLink]] = [:]
        for candidate in remaining {
            templateGroups[template(for: candidate.filename), default: []].append(candidate)
        }

        var candidates: [PackageCandidate] = []
        var singletons: [ClusterableLink] = []
        for members in templateGroups.values {
            if members.count > 1 {
                candidates.append(PackageCandidate(name: name(for: members), linkIDs: members.map(\.id)))
            } else {
                singletons.append(contentsOf: members)
            }
        }

        var byHostPath: [String: [ClusterableLink]] = [:]
        for candidate in singletons {
            byHostPath["\(candidate.host)|\(candidate.directoryPath)", default: []].append(candidate)
        }
        for members in byHostPath.values {
            candidates.append(PackageCandidate(name: name(for: members), linkIDs: members.map(\.id)))
        }

        for (base, members) in archiveGroups {
            let cleaned = base.trimmingCharacters(in: CharacterSet(charactersIn: " ._-"))
            candidates.append(
                PackageCandidate(
                    name: cleaned.isEmpty ? name(for: members) : cleaned,
                    linkIDs: members.map(\.id),
                    isArchive: true
                )
            )
        }

        // `Dictionary` iteration order is randomized per process; sort by
        // each candidate's earliest-appearing member so output is
        // deterministic across calls and test runs.
        return candidates.sorted { a, b in
            let aMin = a.linkIDs.compactMap { inputOrder[$0] }.min() ?? .max
            let bMin = b.linkIDs.compactMap { inputOrder[$0] }.min() ?? .max
            return aMin < bMin
        }
    }

    /// Lowercased, extension stripped, separators normalized, digit runs
    /// collapsed to `#` so `Show.S01E01.mkv` and `Show.S01E02.mkv` reduce to
    /// the same template and cluster with no episode-specific regex.
    private static func template(for filename: String) -> String {
        let stem = stripExtension(filename).lowercased()
        var result = ""
        var lastWasDigit = false
        for character in stem {
            if character.isLetter || character.isNumber {
                if character.isNumber {
                    if !lastWasDigit { result.append("#") }
                    lastWasDigit = true
                } else {
                    result.append(character)
                    lastWasDigit = false
                }
            } else {
                result.append(" ")
                lastWasDigit = false
            }
        }
        return result.split(separator: " ").joined(separator: " ")
    }

    /// Detects `.part01.rar`, `.r00`, `.z01`, and `.001`-style archive
    /// parts, returning the shared base name that locks them into one
    /// package regardless of template.
    private static func archiveBaseName(_ filename: String) -> String? {
        let lower = filename.lowercased()
        let patterns = [#"\.part\d+\.rar$"#, #"\.r\d\d$"#, #"\.z\d\d$"#, #"\.\d{3}$"#]
        for pattern in patterns {
            guard let regex = try? NSRegularExpression(pattern: pattern) else { continue }
            let range = NSRange(lower.startIndex..<lower.endIndex, in: lower)
            guard let match = regex.firstMatch(in: lower, range: range),
                let matchRange = Range(match.range, in: lower)
            else { continue }
            return String(lower[lower.startIndex..<matchRange.lowerBound])
        }
        return nil
    }

    private static func stripExtension(_ filename: String) -> String {
        URL(fileURLWithPath: filename).deletingPathExtension().lastPathComponent
    }

    /// Cleaned longest common prefix of member stems, falling back to the host.
    private static func name(for members: [ClusterableLink]) -> String {
        let stems = members.map { stripExtension($0.filename) }
        guard var prefix = stems.first else { return members.first?.host ?? "Package" }
        for stem in stems.dropFirst() {
            prefix = commonPrefix(prefix, stem)
            if prefix.isEmpty { break }
        }
        let cleaned = prefix.trimmingCharacters(in: CharacterSet(charactersIn: " ._-"))
        return cleaned.isEmpty ? (members.first?.host ?? "Package") : cleaned
    }

    private static func commonPrefix(_ a: String, _ b: String) -> String {
        var result = ""
        for (charA, charB) in zip(a, b) {
            guard charA == charB else { break }
            result.append(charA)
        }
        return result
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: add package clustering by template, archive parts, and host+path"
```

---

### Task 8: `GrabberSession` — ingest, budgeted probing, snapshot

**Files:**
- Create: `SDMKit/Sources/SDMGrabber/GrabberSnapshot.swift`
- Create: `SDMKit/Sources/SDMGrabber/GrabberSession.swift`
- Create: `SDMKit/Tests/SDMGrabberTests/GrabberSessionTests.swift`

**Interfaces:**
- Consumes: `LinkProber`, `ProbedLink`, `VerdictRules`, `PackageClustering`, `URLExtractor` from Tasks 2–7
- Produces:
  - `struct GrabberSnapshot: Sendable, Equatable` — `links: [ProbedLink]`, `packages: [PackageCandidate]`, `checkedCount: Int`, `totalCount: Int`, plus `onlineCount`/`offlineCount`/`faultyCount`/`failedCount`
  - `actor GrabberSession` with `struct Budget: Sendable`, `init(prober:budget:)`, `func ingest(text:) async`, `func ingest(urls:) async`, `func snapshot() -> GrabberSnapshot`

Connection budgeting is the grabber's own — a `Budget` of global and per-host caps — not literally shared with `DownloadEngine`'s connection ceiling, because that ceiling is not enforced yet either (deferred to Phase 3 per the Phase 1 plan). Spec §7.2's "same connection budget as the engine" becomes meaningful once Phase 3 wires a real shared limiter; until then each keeps its own budget of the same shape and magnitude (global cap plus a small per-host cap).

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMGrabberTests/GrabberSessionTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMGrabber

@Test func ingestExtractsAndProbesLinks() async throws {
    let origin = FakeProbeOrigin()
    let url = URL(string: "https://a.example.com/movie.mp4")!
    var behavior = FakeProbeOrigin.Behavior()
    behavior.statusCode = 200
    behavior.headers = ["content-type": "video/mp4", "content-length": "5000000"]
    await origin.setBehavior(behavior, for: url)

    let session = GrabberSession(prober: LinkProber(transport: origin, deepSniffEnabled: false))
    await session.ingest(text: "check out https://a.example.com/movie.mp4 now")

    let snapshot = await session.snapshot()
    #expect(snapshot.totalCount == 1)
    #expect(snapshot.checkedCount == 1)
    #expect(snapshot.links.first?.verdict == .online)
}

@Test func ingestDedupesARepeatedURLAcrossCalls() async throws {
    let origin = FakeProbeOrigin()
    let url = URL(string: "https://a.example.com/movie.mp4")!
    let session = GrabberSession(prober: LinkProber(transport: origin, deepSniffEnabled: false))

    await session.ingest(urls: [url])
    await session.ingest(urls: [url])

    let snapshot = await session.snapshot()
    #expect(snapshot.totalCount == 1)
}

@Test func ingestClustersRelatedLinksIntoOnePackage() async throws {
    let origin = FakeProbeOrigin()
    let urls = [
        URL(string: "https://tv.example.com/season1/Show.S01E01.1080p.mkv")!,
        URL(string: "https://tv.example.com/season1/Show.S01E02.1080p.mkv")!,
    ]
    var behavior = FakeProbeOrigin.Behavior()
    behavior.statusCode = 200
    for url in urls { await origin.setBehavior(behavior, for: url) }

    let session = GrabberSession(prober: LinkProber(transport: origin, deepSniffEnabled: false))
    await session.ingest(urls: urls)

    let snapshot = await session.snapshot()
    #expect(snapshot.packages.count == 1)
    #expect(snapshot.packages.first?.linkIDs.count == 2)
}

@Test func probingRespectsTheGlobalConcurrencyBudget() async throws {
    let origin = FakeProbeOrigin()
    let urlA = URL(string: "https://a.example.com/1.bin")!
    let urlB = URL(string: "https://a.example.com/2.bin")!
    var held = FakeProbeOrigin.Behavior()
    held.holdsUntilReleased = true
    await origin.setBehavior(held, for: urlA)
    await origin.setBehavior(held, for: urlB)

    let session = GrabberSession(
        prober: LinkProber(transport: origin, deepSniffEnabled: false),
        budget: GrabberSession.Budget(globalMaxConcurrentProbes: 1, maxConcurrentPerHost: 4)
    )

    let ingestTask = Task { await session.ingest(urls: [urlA, urlB]) }
    while await origin.requestLog.isEmpty { await Task.yield() }
    let midFlight = await session.snapshot()
    #expect(midFlight.links.filter { $0.stage == .probing }.count == 1)
    #expect(midFlight.links.filter { $0.stage == .queued }.count == 1)

    await origin.release(urlA)
    await origin.release(urlB)
    await ingestTask.value

    let final = await session.snapshot()
    #expect(final.links.allSatisfy { $0.stage == .done })
}

@Test func probingRespectsThePerHostBudgetIndependentlyOfTheGlobalBudget() async throws {
    let origin = FakeProbeOrigin()
    let hostAFirst = URL(string: "https://a.example.com/1.bin")!
    let hostASecond = URL(string: "https://a.example.com/2.bin")!
    let hostB = URL(string: "https://b.example.com/1.bin")!
    var held = FakeProbeOrigin.Behavior()
    held.holdsUntilReleased = true
    for url in [hostAFirst, hostASecond, hostB] { await origin.setBehavior(held, for: url) }

    let session = GrabberSession(
        prober: LinkProber(transport: origin, deepSniffEnabled: false),
        budget: GrabberSession.Budget(globalMaxConcurrentProbes: 8, maxConcurrentPerHost: 1)
    )

    let ingestTask = Task { await session.ingest(urls: [hostAFirst, hostASecond, hostB]) }
    while await origin.requestLog.count < 2 { await Task.yield() }
    let midFlight = await session.snapshot()
    #expect(midFlight.links.filter { $0.stage == .probing }.count == 2)
    #expect(midFlight.links.filter { $0.stage == .queued }.count == 1)

    for url in [hostAFirst, hostASecond, hostB] { await origin.release(url) }
    await ingestTask.value
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter GrabberSessionTests`
Expected: FAIL — `cannot find 'GrabberSession' in scope`.

- [ ] **Step 3: Implement `GrabberSnapshot`**

`SDMKit/Sources/SDMGrabber/GrabberSnapshot.swift`:

```swift
/// An immutable view of grabber state, published to the UI. Spec §7.5's
/// determinate `checked / total` header and verdict filter chips read
/// straight off this.
public struct GrabberSnapshot: Sendable, Equatable {
    public let links: [ProbedLink]
    public let packages: [PackageCandidate]
    public let checkedCount: Int
    public let totalCount: Int

    public init(
        links: [ProbedLink],
        packages: [PackageCandidate],
        checkedCount: Int,
        totalCount: Int
    ) {
        self.links = links
        self.packages = packages
        self.checkedCount = checkedCount
        self.totalCount = totalCount
    }

    public var onlineCount: Int { links.filter { $0.verdict == .online }.count }
    public var offlineCount: Int { links.filter { $0.verdict == .offline }.count }
    public var faultyCount: Int {
        links.filter {
            if case .faulty = $0.verdict { return true }
            return false
        }.count
    }
    public var failedCount: Int { links.filter { $0.verdict == .checkFailed }.count }
}
```

- [ ] **Step 4: Implement `GrabberSession`**

`SDMKit/Sources/SDMGrabber/GrabberSession.swift`:

```swift
import Foundation

/// Owns the batch of links being grabbed: extraction, connection-budgeted
/// probing, and package clustering. Spec §7.5.
public actor GrabberSession {
    public struct Budget: Sendable {
        public var globalMaxConcurrentProbes: Int
        public var maxConcurrentPerHost: Int

        public init(globalMaxConcurrentProbes: Int = 16, maxConcurrentPerHost: Int = 4) {
            precondition(globalMaxConcurrentProbes >= 1)
            precondition(maxConcurrentPerHost >= 1)
            self.globalMaxConcurrentProbes = globalMaxConcurrentProbes
            self.maxConcurrentPerHost = maxConcurrentPerHost
        }
    }

    private let prober: LinkProber
    private let budget: Budget
    private var links: [UUID: ProbedLink] = [:]
    private var order: [UUID] = []
    private var seenURLs: Set<URL> = []
    var knownDownloadURLs: Set<URL> = []
    private var packages: [PackageCandidate] = []

    public init(prober: LinkProber, budget: Budget = Budget()) {
        self.prober = prober
        self.budget = budget
    }

    /// Extracts links from pasted or dropped text, dedupes against links
    /// already in this session, then probes the new ones under budget.
    public func ingest(text: String) async {
        await ingest(urls: URLExtractor.extractLinks(from: text))
    }

    public func ingest(urls: [URL]) async {
        var fresh: [UUID] = []
        for url in urls where seenURLs.insert(url).inserted {
            let id = UUID()
            links[id] = ProbedLink(id: id, originalURL: url, isDuplicate: knownDownloadURLs.contains(url))
            order.append(id)
            fresh.append(id)
        }
        guard !fresh.isEmpty else { return }
        await probeBounded(fresh)
        recluster()
    }

    public func snapshot() -> GrabberSnapshot {
        let ordered = order.compactMap { links[$0] }
        return GrabberSnapshot(
            links: ordered,
            packages: packages,
            checkedCount: ordered.filter { $0.stage == .done }.count,
            totalCount: ordered.count
        )
    }

    // MARK: - Probing

    /// Launches probes for `ids` under the global and per-host caps, one
    /// wave of `TaskGroup` children at a time. All mutable state here
    /// (`links`, `hostCounts`, `pending`) is only touched synchronously
    /// between suspension points, so a concurrent `ingest` call started
    /// while this one is still probing sees a consistent, if interleaved,
    /// picture — the same actor-reentrancy shape `DownloadEngine.reconcile`
    /// documents in Phase 1.
    private func probeBounded(_ ids: [UUID]) async {
        var pending = ids
        var hostCounts: [String: Int] = [:]

        await withTaskGroup(of: (UUID, ProbedLink).self) { group in
            var active = 0

            func launchNext() {
                while active < budget.globalMaxConcurrentProbes {
                    guard
                        let index = pending.firstIndex(where: { id in
                            guard let host = links[id]?.originalURL.host else { return true }
                            return (hostCounts[host] ?? 0) < budget.maxConcurrentPerHost
                        })
                    else { return }
                    let id = pending.remove(at: index)
                    guard let url = links[id]?.originalURL else { continue }
                    let host = url.host ?? ""
                    hostCounts[host, default: 0] += 1
                    active += 1
                    links[id]?.stage = .probing
                    group.addTask { [prober] in (id, await prober.probe(url)) }
                }
            }

            launchNext()
            while let (id, probed) = await group.next() {
                active -= 1
                if let host = links[id]?.originalURL.host {
                    hostCounts[host, default: 1] -= 1
                }
                var finished = probed
                finished.isDuplicate = knownDownloadURLs.contains(finished.originalURL)
                finished.verdict = VerdictRules.evaluate(finished)
                links[id] = finished
                launchNext()
            }
        }
    }

    private func recluster() {
        let clusterable = order.compactMap { id -> ClusterableLink? in
            guard let link = links[id] else { return nil }
            let host = link.finalURL.host ?? link.originalURL.host ?? ""
            let directoryPath = link.finalURL.deletingLastPathComponent().path
            return ClusterableLink(id: id, filename: link.effectiveFilename, host: host, directoryPath: directoryPath)
        }
        packages = PackageClustering.cluster(clusterable)
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: add GrabberSession with budgeted probing and clustering"
```

---

### Task 9: `GrabberSession` — duplicates and manual overrides

**Files:**
- Modify: `SDMKit/Sources/SDMGrabber/GrabberSession.swift`
- Create: `SDMKit/Tests/SDMGrabberTests/GrabberSessionOverrideTests.swift`

**Interfaces:**
- Consumes: `GrabberSession` from Task 8
- Produces: `func setKnownDownloadURLs(_ urls: Set<URL>) async`, `func removeLink(_ id: UUID) async`, `func moveLink(_ id: UUID, toPackageNamed name: String) async` on `GrabberSession`

Spec §7.5: links already in the download list are badged as duplicates rather than dropped. Spec §7.4: clustering is "fully overridable" — drag between packages, merge, split.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMGrabberTests/GrabberSessionOverrideTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMGrabber

private func makeSession() -> (GrabberSession, FakeProbeOrigin) {
    let origin = FakeProbeOrigin()
    let session = GrabberSession(prober: LinkProber(transport: origin, deepSniffEnabled: false))
    return (session, origin)
}

@Test func settingKnownDownloadURLsMarksExistingLinksAsDuplicates() async throws {
    let (session, _) = makeSession()
    let url = URL(string: "https://a.example.com/movie.mp4")!
    await session.ingest(urls: [url])
    #expect(await session.snapshot().links.first?.isDuplicate == false)

    await session.setKnownDownloadURLs([url])
    #expect(await session.snapshot().links.first?.isDuplicate == true)
}

@Test func ingestMarksDuplicateImmediatelyWhenAlreadyKnown() async throws {
    let (session, _) = makeSession()
    let url = URL(string: "https://a.example.com/movie.mp4")!
    await session.setKnownDownloadURLs([url])
    await session.ingest(urls: [url])
    #expect(await session.snapshot().links.first?.isDuplicate == true)
}

@Test func removingALinkAllowsItToBeReingested() async throws {
    let (session, _) = makeSession()
    let url = URL(string: "https://a.example.com/movie.mp4")!
    await session.ingest(urls: [url])
    let firstID = try #require(await session.snapshot().links.first?.id)

    await session.removeLink(firstID)
    #expect(await session.snapshot().totalCount == 0)

    await session.ingest(urls: [url])
    #expect(await session.snapshot().totalCount == 1)
    #expect(await session.snapshot().links.first?.id != firstID)
}

@Test func movingALinkOverridesAutomaticClustering() async throws {
    let (session, _) = makeSession()
    let urlA = URL(string: "https://a.example.com/alpha.bin")!
    let urlB = URL(string: "https://b.example.com/beta.bin")!
    await session.ingest(urls: [urlA, urlB])

    let snapshotBefore = await session.snapshot()
    #expect(snapshotBefore.packages.count == 2)
    let packageAName = try #require(snapshotBefore.packages.first?.name)
    let idB = try #require(snapshotBefore.packages.last?.linkIDs.first)

    await session.moveLink(idB, toPackageNamed: packageAName)

    let snapshotAfter = await session.snapshot()
    #expect(snapshotAfter.packages.count == 1)
    #expect(snapshotAfter.packages.first?.linkIDs.count == 2)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter GrabberSessionOverrideTests`
Expected: FAIL — `value of type 'GrabberSession' has no member 'setKnownDownloadURLs'`.

- [ ] **Step 3: Add duplicate tracking and manual overrides**

In `SDMKit/Sources/SDMGrabber/GrabberSession.swift`, add a new stored property alongside the existing ones:

```swift
    private var manualOverrides: [UUID: String] = [:]
```

Add these methods to the `GrabberSession` actor body (after `snapshot()`):

```swift
    /// Refreshes which grabbed links are already in the download list.
    /// Spec §7.5: badged as duplicates, never silently dropped.
    public func setKnownDownloadURLs(_ urls: Set<URL>) {
        knownDownloadURLs = urls
        for id in order {
            guard let url = links[id]?.originalURL else { continue }
            links[id]?.isDuplicate = urls.contains(url)
        }
    }

    public func removeLink(_ id: UUID) {
        guard let link = links.removeValue(forKey: id) else { return }
        seenURLs.remove(link.originalURL)
        order.removeAll { $0 == id }
        manualOverrides[id] = nil
        recluster()
    }

    /// Forces a link into a named package, overriding automatic clustering.
    /// Spec §7.4: "All of it is fully overridable."
    public func moveLink(_ id: UUID, toPackageNamed name: String) {
        guard links[id] != nil else { return }
        manualOverrides[id] = name
        recluster()
    }
```

Replace the `recluster()` method to apply overrides after automatic clustering:

```swift
    private func recluster() {
        let clusterable = order.compactMap { id -> ClusterableLink? in
            guard let link = links[id] else { return nil }
            let host = link.finalURL.host ?? link.originalURL.host ?? ""
            let directoryPath = link.finalURL.deletingLastPathComponent().path
            return ClusterableLink(id: id, filename: link.effectiveFilename, host: host, directoryPath: directoryPath)
        }
        var candidates = PackageClustering.cluster(clusterable)

        guard !manualOverrides.isEmpty else {
            packages = candidates
            return
        }
        for (id, name) in manualOverrides {
            for index in candidates.indices { candidates[index].linkIDs.removeAll { $0 == id } }
            if let index = candidates.firstIndex(where: { $0.name == name }) {
                candidates[index].linkIDs.append(id)
            } else {
                candidates.append(PackageCandidate(name: name, linkIDs: [id]))
            }
        }
        packages = candidates.filter { !$0.linkIDs.isEmpty }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: add duplicate detection and manual package overrides to GrabberSession"
```

---

### Task 10: App — `GrabberSettings` and `GrabberController`

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/EngineSnapshot.swift`
- Modify: `SDMKit/Sources/SDMEngine/DownloadEngine.swift`
- Modify: `SDMKit/Tests/SDMEngineTests/DownloadEngineTests.swift`
- Create: `SDM/GrabberSettings.swift`
- Create: `SDM/GrabberController.swift`

**Interfaces:**
- Consumes: `GrabberSession`, `LinkProber`, `URLSessionProbeTransport`, `GrabberSnapshot` from `SDMGrabber`
- Produces: `ItemSnapshot.url: URL` (new field), `enum GrabberSettings` (UserDefaults-backed toggles), `@MainActor @Observable final class GrabberController`

The duplicate badge (spec §7.5) needs to compare a grabbed URL against every download's source URL, but `ItemSnapshot` — built in Phase 1 before the grabber existed — never carried one. This task adds it; it is a small additive field, not a redesign.

- [ ] **Step 1: Add `url` to `ItemSnapshot`**

In `SDMKit/Sources/SDMEngine/EngineSnapshot.swift`, add a field to `ItemSnapshot`:

```swift
public struct ItemSnapshot: Sendable, Equatable, Identifiable {
    public let id: UUID
    public let url: URL
    public let filename: String
```

Add `url: URL,` as a parameter to `init`, right after `id: UUID,`, and add `self.url = url` in the body, right after `self.id = id`.

- [ ] **Step 2: Update the one construction site**

In `SDMKit/Sources/SDMEngine/DownloadEngine.swift`, in `snapshot()`, add `url: item.url,` to the `ItemSnapshot(...)` call, right after `id: item.id,`.

- [ ] **Step 3: Write a failing test pinning the new field**

In `SDMKit/Tests/SDMEngineTests/DownloadEngineTests.swift`, add:

```swift
@Test func snapshotItemCarriesItsSourceURL() async throws {
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: Data(repeating: 0, count: 100)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1,
            segmentsPerItem: 1,
            globalMaxConnections: 4,
            downloadFolder: FileManager.default.temporaryDirectory
        )
    )
    let url = URL(string: "https://example.com/a.bin")!
    await engine.add(DownloadPackage(name: "Pkg", items: [DownloadItem(url: url, filename: "a.bin")]))

    let snapshot = await engine.snapshot()
    #expect(snapshot.packages.first?.items.first?.url == url)
}
```

- [ ] **Step 4: Run the test to verify it fails, then verify the full suite passes**

Run: `swift test --package-path SDMKit --filter snapshotItemCarriesItsSourceURL`
Expected: FAIL before Steps 1–2, then run again after applying them.

Run: `swift test --package-path SDMKit`
Expected: PASS, full suite.

- [ ] **Step 5: Create `GrabberSettings`**

`SDM/GrabberSettings.swift`:

```swift
import Foundation

/// Backs the Phase 2 toggles from spec §12. There is no dedicated Settings
/// screen yet, so these read and write `UserDefaults` directly rather than
/// through a settings model that does not otherwise exist.
@MainActor
enum GrabberSettings {
    private static let defaults = UserDefaults.standard

    private enum Key {
        static let clipboardWatching = "sdm.clipboardWatchingEnabled"
        static let autoAddAndStart = "sdm.autoAddAndStartOnGrab"
        static let deepSniff = "sdm.deepSniffEnabled"
    }

    static var clipboardWatchingEnabled: Bool {
        get { defaults.object(forKey: Key.clipboardWatching) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Key.clipboardWatching) }
    }

    static var autoAddAndStartOnGrab: Bool {
        get { defaults.object(forKey: Key.autoAddAndStart) as? Bool ?? false }
        set { defaults.set(newValue, forKey: Key.autoAddAndStart) }
    }

    static var deepSniffEnabled: Bool {
        get { defaults.object(forKey: Key.deepSniff) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Key.deepSniff) }
    }
}
```

- [ ] **Step 6: Create `GrabberController`**

`SDM/GrabberController.swift`:

```swift
import Foundation
import Observation
import SDMGrabber

/// Bridges `GrabberSession` to SwiftUI, mirroring `EngineController`'s role
/// for `DownloadEngine`.
@MainActor
@Observable
final class GrabberController {
    private(set) var snapshot = GrabberSnapshot(links: [], packages: [], checkedCount: 0, totalCount: 0)

    private let session: GrabberSession

    init() {
        session = GrabberSession(
            prober: LinkProber(
                transport: URLSessionProbeTransport(),
                deepSniffEnabled: GrabberSettings.deepSniffEnabled
            )
        )
    }

    func ingest(text: String) async {
        await session.ingest(text: text)
        snapshot = await session.snapshot()
    }

    func ingest(urls: [URL]) async {
        await session.ingest(urls: urls)
        snapshot = await session.snapshot()
    }

    func setKnownDownloadURLs(_ urls: Set<URL>) async {
        await session.setKnownDownloadURLs(urls)
        snapshot = await session.snapshot()
    }

    func removeLink(_ id: UUID) async {
        await session.removeLink(id)
        snapshot = await session.snapshot()
    }

    func moveLink(_ id: UUID, toPackageNamed name: String) async {
        await session.moveLink(id, toPackageNamed: name)
        snapshot = await session.snapshot()
    }
}
```

- [ ] **Step 7: Build the app target**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: this will FAIL until Task 14 wires `SDMGrabber` into the Xcode project — that is expected here. Confirm the failure is specifically an unresolved `SDMGrabber` import / module-not-found, not a syntax error in the files just written.

- [ ] **Step 8: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit SDM
git commit -m "feat: add ItemSnapshot.url, GrabberSettings and GrabberController"
```

---

### Task 11: App — `ClipboardWatcher`

**Files:**
- Create: `SDM/ClipboardWatcher.swift`

**Interfaces:**
- Consumes: `URLExtractor` from `SDMGrabber`
- Produces: `@MainActor final class ClipboardWatcher` with `var onLinksDetected: (([URL]) -> Void)?`, `func start()`, `func stop()`, `func ignoreOwnWrite(_ urls: [URL])`

Per spec §7.1, `NSPasteboard` has no change notification, so this polls `changeCount` on a ~0.5 s timer, gated by `GrabberSettings.clipboardWatchingEnabled`. The privacy-preserving `NSPasteboard.detectedValues(for:)` API — verified directly against this machine's macOS 26 SDK (`AppKit.swiftinterface`) — requires macOS 15.4+, one point release above this project's 15.0 baseline, so it is used behind `if #available` with a full-string-read-and-extract fallback below that. No `SDMGrabber` test covers this file; it is AppKit UI glue, verified manually in Task 14 per spec §11.7 ("a launch smoke test and nothing more").

- [ ] **Step 1: Implement `ClipboardWatcher`**

`SDM/ClipboardWatcher.swift`:

```swift
import AppKit
import Foundation
import SDMGrabber

/// Polls `NSPasteboard.changeCount`, since `NSPasteboard` has no change
/// notification. Spec §7.1.
///
/// `NSPasteboard.detectedValues(for:)` — the privacy-preserving read that
/// returns a probable URL without exposing full pasteboard content — was
/// confirmed against the macOS 26 SDK's `AppKit.swiftinterface`:
/// `@available(macOS 15.4, *) func detectedValues(for:) async throws ->
/// NSPasteboard.DetectedValues`, with a non-optional `probableWebURL:
/// String`. That is above this project's macOS 15.0 baseline, so it is used
/// behind an availability check; below 15.4 this falls back to a full
/// `.string(forType: .string)` read passed through `URLExtractor`. Re-check
/// this gap against whatever SDK is installed at implementation time —
/// Apple could backport the API to an earlier 15.x point release.
@MainActor
final class ClipboardWatcher {
    private let pasteboard = NSPasteboard.general
    private var lastChangeCount: Int
    private var lastIgnoredURLs: Set<URL> = []
    private var timer: Timer?

    /// Called with newly detected URLs from content SDM did not itself just
    /// place on the pasteboard.
    var onLinksDetected: (([URL]) -> Void)?

    init() {
        lastChangeCount = pasteboard.changeCount
    }

    func start() {
        guard timer == nil else { return }
        timer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
            guard let self else { return }
            Task { await self.poll() }
        }
    }

    func stop() {
        timer?.invalidate()
        timer = nil
    }

    /// Marks URLs SDM itself just placed on the pasteboard, so copying a
    /// link back out of the app does not re-grab it. Spec §7.1.
    func ignoreOwnWrite(_ urls: [URL]) {
        lastIgnoredURLs = Set(urls)
    }

    private func poll() async {
        guard pasteboard.changeCount != lastChangeCount else { return }
        lastChangeCount = pasteboard.changeCount

        let urls = await detectURLs()
        guard !urls.isEmpty, Set(urls) != lastIgnoredURLs else { return }
        onLinksDetected?(urls)
    }

    private func detectURLs() async -> [URL] {
        if #available(macOS 15.4, *) {
            guard
                let detected = try? await pasteboard.detectedValues(for: [\.probableWebURL]),
                !detected.probableWebURL.isEmpty
            else { return [] }
            return URLExtractor.extractLinks(from: detected.probableWebURL)
        }
        guard let text = pasteboard.string(forType: .string) else { return [] }
        return URLExtractor.extractLinks(from: text)
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add SDM
git commit -m "feat: add ClipboardWatcher polling NSPasteboard for probable URLs"
```

---

### Task 12: App — Linkgrabber list UI

**Files:**
- Create: `SDM/LinkGrabberView.swift`

**Interfaces:**
- Consumes: `GrabberController`, `GrabberSnapshot`, `ProbedLink`, `Verdict`, `PackageCandidate` from Task 10 and `SDMGrabber`
- Produces: `struct LinkGrabberView: View`

Spec §7.5: a determinate `checked / total` header with live verdict counts doubling as filter chips, rows carrying per-link stage, and the faulty reason as badge text rather than a generic warning icon.

- [ ] **Step 1: Implement `LinkGrabberView`**

`SDM/LinkGrabberView.swift`:

```swift
import SDMGrabber
import SwiftUI

struct LinkGrabberView: View {
    @Environment(GrabberController.self) private var controller
    @State private var activeFilter: VerdictFilter = .all
    @State private var isShowingAddSheet = false

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            List {
                ForEach(controller.snapshot.packages, id: \.name) { package in
                    Section(package.name) {
                        ForEach(links(in: package)) { link in
                            LinkRow(link: link, controller: controller)
                        }
                    }
                }
            }
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button("Add links") { isShowingAddSheet = true }
            }
        }
        .sheet(isPresented: $isShowingAddSheet) {
            AddLinksSheet()
        }
        .frame(minWidth: 640, minHeight: 420)
    }

    private var header: some View {
        let snapshot = controller.snapshot
        return VStack(alignment: .leading, spacing: 6) {
            ProgressView(value: Double(snapshot.checkedCount), total: Double(max(snapshot.totalCount, 1)))
            HStack(spacing: 12) {
                Text("\(snapshot.checkedCount) / \(snapshot.totalCount) checked")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
                filterChip(.online, count: snapshot.onlineCount)
                filterChip(.faulty, count: snapshot.faultyCount)
                filterChip(.offline, count: snapshot.offlineCount)
                filterChip(.failed, count: snapshot.failedCount)
            }
        }
        .padding()
    }

    private func filterChip(_ filter: VerdictFilter, count: Int) -> some View {
        Button {
            activeFilter = activeFilter == filter ? .all : filter
        } label: {
            Text("\(filter.label) \(count)")
                .font(.caption)
        }
        .buttonStyle(.bordered)
        .tint(activeFilter == filter ? .accentColor : .secondary)
    }

    private func links(in package: PackageCandidate) -> [ProbedLink] {
        let ids = Set(package.linkIDs)
        return controller.snapshot.links.filter { ids.contains($0.id) && activeFilter.matches($0.verdict) }
    }
}

enum VerdictFilter: Equatable {
    case all, online, faulty, offline, failed

    var label: String {
        switch self {
        case .all: return "All"
        case .online: return "Online"
        case .faulty: return "Faulty"
        case .offline: return "Offline"
        case .failed: return "Check failed"
        }
    }

    func matches(_ verdict: Verdict?) -> Bool {
        switch self {
        case .all: return true
        case .online: return verdict == .online
        case .offline: return verdict == .offline
        case .failed: return verdict == .checkFailed
        case .faulty:
            if case .faulty = verdict { return true }
            return false
        }
    }
}

private struct LinkRow: View {
    let link: ProbedLink
    let controller: GrabberController

    var body: some View {
        HStack {
            Text(link.effectiveFilename).lineLimit(1)
            if link.isDuplicate {
                Text("duplicate")
                    .font(.caption)
                    .foregroundStyle(.orange)
            }
            Spacer()
            verdictBadge
            Button(role: .destructive) {
                let id = link.id
                Task { await controller.removeLink(id) }
            } label: {
                Image(systemName: "trash")
            }
            .buttonStyle(.plain)
        }
        .padding(.vertical, 2)
    }

    @ViewBuilder
    private var verdictBadge: some View {
        switch link.verdict {
        case .online:
            Text("online").font(.caption).foregroundStyle(.green)
        case .offline:
            Text("offline").font(.caption).foregroundStyle(.secondary)
        case .checkFailed:
            Text("check failed").font(.caption).foregroundStyle(.secondary)
        case .faulty(let reason):
            // Spec §7.3: the faulty reason *is* the badge text.
            Text(reason).font(.caption).foregroundStyle(.red)
        case nil:
            // No verdict yet: spec §7.5's queued → probing → sniffing → done
            // per-link state, shown literally rather than a bare spinner.
            HStack(spacing: 4) {
                ProgressView().controlSize(.small)
                Text(stageLabel).font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private var stageLabel: String {
        switch link.stage {
        case .queued: return "queued"
        case .probing: return "probing"
        case .sniffing: return "sniffing"
        case .done: return "done"
        }
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add SDM
git commit -m "feat: add Linkgrabber list UI with verdict filter chips"
```

---

### Task 13: App — Add links sheet, drag-and-drop, and handoff to downloads

**Files:**
- Create: `SDM/AddLinksSheet.swift`
- Modify: `SDM/LinkGrabberView.swift`
- Modify: `SDM/GrabberController.swift`

**Interfaces:**
- Consumes: `EngineController.addDownload`-style flow (a new `addToDownloads` entry point), `GrabberController`
- Produces: `struct AddLinksSheet: View`, drag-and-drop support on `LinkGrabberView`, `GrabberController.confirmedPackageURLs(for:)`, `EngineController.addPackage(name:urls:andStart:)`

Spec §7.5: manual entry via a paste field feeding the identical pipeline, drag-and-drop of text or URLs onto the window, and handoff via "Add to downloads" / "Add and start", with a setting to auto-add-and-start on grab.

- [ ] **Step 1: Add a package-level handoff method to `EngineController`**

In `SDM/EngineController.swift`, add a method alongside `addDownload(urlString:)`:

```swift
    /// Hands a grabbed package off to the download engine. Spec §7.5's "Add
    /// to downloads" / "Add and start".
    func addPackage(name: String, urls: [URL], startImmediately: Bool) async {
        guard !urls.isEmpty else { return }
        let items = urls.map { url in
            DownloadItem(
                url: url,
                filename: url.lastPathComponent.isEmpty ? "download" : url.lastPathComponent,
                isEnabled: startImmediately
            )
        }
        await engine.add(DownloadPackage(name: name, items: items))
        snapshot = await engine.snapshot()
    }
```

- [ ] **Step 2: Add a lookup helper to `GrabberController`**

In `SDM/GrabberController.swift`, add:

```swift
    /// The original URLs of a package's confirmed links, for handoff to the
    /// download engine.
    func urls(inPackageNamed name: String) -> [URL] {
        guard let package = snapshot.packages.first(where: { $0.name == name }) else { return [] }
        let ids = Set(package.linkIDs)
        return snapshot.links.filter { ids.contains($0.id) }.map(\.originalURL)
    }
```

- [ ] **Step 3: Implement `AddLinksSheet`**

`SDM/AddLinksSheet.swift`:

```swift
import SwiftUI

/// Multiline paste field feeding the identical extraction pipeline as
/// clipboard watching. Spec §7.5.
struct AddLinksSheet: View {
    @Environment(GrabberController.self) private var controller
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Add links")
                .font(.headline)
            TextEditor(text: $text)
                .font(.body.monospaced())
                .frame(minWidth: 420, minHeight: 200)
                .overlay(RoundedRectangle(cornerRadius: 6).stroke(.separator))

            HStack {
                Spacer()
                Button("Cancel") { dismiss() }
                Button("Add") {
                    let pasted = text
                    dismiss()
                    Task { await controller.ingest(text: pasted) }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding()
    }
}
```

- [ ] **Step 4: Add drag-and-drop and handoff buttons to `LinkGrabberView`**

In `SDM/LinkGrabberView.swift`, add `@Environment(EngineController.self) private var engineController` alongside the existing `@Environment(GrabberController.self)`, and change the `body`'s outer `VStack` to accept drops:

```swift
        VStack(spacing: 0) {
            header
            Divider()
            List {
                ForEach(controller.snapshot.packages, id: \.name) { package in
                    Section {
                        ForEach(links(in: package)) { link in
                            LinkRow(link: link, controller: controller)
                        }
                    } header: {
                        packageHeader(package)
                    }
                }
            }
        }
        .onDrop(of: [.url, .plainText], isTargeted: nil) { providers in
            for provider in providers {
                _ = provider.loadObject(ofClass: NSString.self) { object, _ in
                    guard let text = object as? String else { return }
                    Task { @MainActor in await controller.ingest(text: text) }
                }
            }
            return true
        }
```

Add the package-level handoff header, replacing the plain `Section(package.name)` call above with a helper:

```swift
    @ViewBuilder
    private func packageHeader(_ package: PackageCandidate) -> some View {
        HStack {
            Text(package.name)
            Spacer()
            Button("Add to downloads") {
                let urls = controller.urls(inPackageNamed: package.name)
                let name = package.name
                Task { await engineController.addPackage(name: name, urls: urls, startImmediately: false) }
            }
            .controlSize(.small)
            Button("Add and start") {
                let urls = controller.urls(inPackageNamed: package.name)
                let name = package.name
                Task { await engineController.addPackage(name: name, urls: urls, startImmediately: true) }
            }
            .controlSize(.small)
        }
    }
```

- [ ] **Step 5: Wire the auto-add-and-start setting into ingest**

In `SDM/GrabberController.swift`, the `ingest(text:)` and `ingest(urls:)` methods need access to `EngineController` to honor spec §7.5's "auto-add-and-start on grab" setting. Rather than giving `GrabberController` a dependency on `EngineController` (which would invert the app's controller layering), this is driven from the view instead — `.onChange` in Task 14's wiring step observes `controller.snapshot` and, when `GrabberSettings.autoAddAndStartOnGrab` is on, hands newly-`.online` packages to `engineController.addPackage` automatically. No change needed here beyond what Task 14 adds; this step documents the decision so Task 14's wiring is not a surprise.

- [ ] **Step 6: Commit**

```bash
git add SDM
git commit -m "feat: add links sheet, drag-and-drop grabbing, and download handoff"
```

---

### Task 14: Wire `SDMGrabber` into the app, navigation, and full verification

**Files:**
- Modify: `SDM.xcodeproj/project.pbxproj`
- Modify: `SDM/SDMApp.swift`
- Modify: `SDM/ContentView.swift`
- Modify: `SDM/EngineController.swift`

**Interfaces:**
- Consumes: everything from Tasks 1–13
- Produces: a building, launchable app with a Downloads/Linkgrabber tab switch

- [ ] **Step 1: Add the `SDMGrabber` product dependency to the Xcode project**

`SDM.xcodeproj/project.pbxproj` has no GUI in this environment; edit it directly, following the exact pattern already used for `SDMCore`/`SDMEngine`.

In the `PBXBuildFile` section, add a new line after the `SDMEngine` entry:

```
		5FAA0006A0B0C0D0E0F00006 /* SDMGrabber in Frameworks */ = {isa = PBXBuildFile; productRef = 5FAA0007A0B0C0D0E0F00007 /* SDMGrabber */; };
```

In the `PBXFrameworksBuildPhase` section, add to the `SDM` target's `files` list (the one already containing `SDMCore in Frameworks` / `SDMEngine in Frameworks`):

```
					5FAA0006A0B0C0D0E0F00006 /* SDMGrabber in Frameworks */,
```

In the `PBXNativeTarget` section, add to the `SDM` target's `packageProductDependencies` list:

```
				5FAA0007A0B0C0D0E0F00007 /* SDMGrabber */,
```

In the `XCSwiftPackageProductDependency` section, add a new entry after `SDMEngine`:

```
		5FAA0007A0B0C0D0E0F00007 /* SDMGrabber */ = {
			isa = XCSwiftPackageProductDependency;
			productName = SDMGrabber;
		};
```

- [ ] **Step 2: Build to confirm the new product resolves**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: still FAILS — `SDM/ContentView.swift` and `SDMApp.swift` do not yet reference `GrabberController` or `LinkGrabberView`, so nothing uses the new dependency yet, but the module itself should now resolve. Confirm there is no `"couldn't load` / `no such module 'SDMGrabber'"` error; any remaining failure should be about unused-symbol warnings at worst, not a missing module.

- [ ] **Step 3: Add `GrabberController` and clipboard lifecycle to `SDMApp`**

Replace `SDM/SDMApp.swift`:

```swift
import SwiftUI

@main
struct SDMApp: App {
    @State private var engineController = EngineController()
    @State private var grabberController = GrabberController()
    @State private var clipboardWatcher = ClipboardWatcher()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(engineController)
                .environment(grabberController)
                .task { await engineController.startHeartbeat() }
                .onAppear {
                    clipboardWatcher.onLinksDetected = { urls in
                        guard GrabberSettings.clipboardWatchingEnabled else { return }
                        Task { await grabberController.ingest(urls: urls) }
                    }
                    if GrabberSettings.clipboardWatchingEnabled { clipboardWatcher.start() }
                }
                .onDisappear { clipboardWatcher.stop() }
                .onChange(of: grabberController.snapshot) { _, newSnapshot in
                    guard GrabberSettings.autoAddAndStartOnGrab else { return }
                    for package in newSnapshot.packages {
                        let ids = Set(package.linkIDs)
                        let links = newSnapshot.links.filter { ids.contains($0.id) }
                        guard !links.isEmpty, links.allSatisfy({ $0.verdict == .online }) else { continue }
                        let name = package.name
                        let urls = links.map(\.originalURL)
                        Task { await engineController.addPackage(name: name, urls: urls, startImmediately: true) }
                    }
                }
        }
    }
}
```

- [ ] **Step 4: Add a Downloads/Linkgrabber switch to `ContentView`**

In `SDM/ContentView.swift`, wrap the existing body in a `TabView`. Replace the `struct ContentView: View { ... }` declaration's `body` property:

```swift
    var body: some View {
        TabView {
            downloadsTab
                .tabItem { Label("Downloads", systemImage: "arrow.down.circle") }
            LinkGrabberView()
                .tabItem { Label("Linkgrabber", systemImage: "link") }
        }
        .frame(minWidth: 640, minHeight: 420)
    }

    private var downloadsTab: some View {
        VStack(spacing: 0) {
            HStack {
                TextField("https://example.com/file.bin", text: $urlText)
                    .textFieldStyle(.roundedBorder)
                Button("Add") {
                    let text = urlText
                    urlText = ""
                    Task { await controller.addDownload(urlString: text) }
                }
                .disabled(urlText.isEmpty)
            }
            .padding()

            Divider()

            List {
                ForEach(controller.snapshot.packages) { package in
                    Section(package.name) {
                        ForEach(package.items) { item in
                            ItemRow(item: item, controller: controller)
                        }
                    }
                }
            }

            Divider()

            HStack {
                Text(formatted(controller.snapshot.globalBytesPerSecond))
                    .font(.title3.monospacedDigit())
                Spacer()
                Text("\(controller.snapshot.packages.count) packages")
                    .foregroundStyle(.secondary)
            }
            .padding()
        }
    }
```

The `.frame(minWidth: 640, minHeight: 420)` moves from the old top-level `VStack` to the new top-level `TabView`; remove it from wherever it previously sat in this file so it is not applied twice.

- [ ] **Step 5: Feed known download URLs to the grabber for duplicate badging**

Still in `SDM/ContentView.swift`, add a known-URL sync alongside the `downloadsTab`/`LinkGrabberView` switch — attach this `.onChange` to the outer `TabView`:

```swift
        .onChange(of: controller.snapshot) { _, newSnapshot in
            let urls = Set(newSnapshot.packages.flatMap { $0.items.map(\.url) })
            Task { await grabberController.setKnownDownloadURLs(urls) }
        }
```

This requires `@Environment(GrabberController.self) private var grabberController` added alongside the existing `@Environment(EngineController.self) private var controller` at the top of `ContentView`.

- [ ] **Step 6: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED.

Then run the app from Xcode (`⌘R`) and confirm by hand:
- The Linkgrabber tab opens with an empty list and a `0 / 0 checked` header.
- **Add links**, paste a real direct-download URL, click **Add** — the link appears immediately as queued, then fills in with a verdict badge (online/offline/faulty/check failed) within a couple seconds.
- Copy a direct-download URL to the clipboard (with clipboard watching on) — it appears in the Linkgrabber list without any explicit paste.
- Click **Add to downloads** on a package — it appears in the Downloads tab, disabled. **Add and start** — it appears enabled and begins downloading.
- Paste the same URL again — the existing row is badged `duplicate`.

- [ ] **Step 7: Run the full package test suite**

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite (Phase 1's engine/core tests plus every `SDMGrabberTests` test from this plan).

- [ ] **Step 8: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add -A
git commit -m "feat: wire SDMGrabber into the app with a Linkgrabber tab"
```

---

## Phase 2 completion criteria

- [ ] `swift test --package-path SDMKit` passes with no skipped tests, including every `SDMGrabberTests` target added by this plan
- [ ] No `SDMGrabber` test touches the network — every probing test runs against `FakeProbeOrigin`
- [ ] `PackageClustering.cluster` output is proven deterministic across repeated calls with the same input (Task 7)
- [ ] `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build` succeeds
- [ ] A pasted or clipboard-copied real URL is extracted, probed, verdict-badged, and clustered end-to-end through the app UI
- [ ] A grabbed package hands off to the download engine via both "Add to downloads" and "Add and start"
- [ ] A link already in the download list is badged as a duplicate rather than silently dropped
- [ ] `SDMGrabber` depends only on `SDMCore` — confirm with `grep -n "import SDMEngine" SDMKit/Sources/SDMGrabber/*.swift` returning nothing

## Deferred to later phases

Deliberately **not** in Phase 2, to keep it shippable:

- **Sharing a real connection budget with the download engine.** `GrabberSession.Budget` is its own global/per-host cap, not literally the engine's `globalMaxConnections` — that ceiling is not enforced anywhere yet (Phase 1 deferred it to Phase 3). Once Phase 3 wires a real shared limiter, revisit whether the grabber should draw from it. **Phase 3.**
- **Menu bar pending-links row, notifications for "N links grabbed."** Spec §9.7 and §9.8 — menu bar extra and notification center wiring do not exist yet. **Phase 3.**
- **NavigationSplitView / sidebar navigation.** This plan adds a plain `TabView` switch between Downloads and Linkgrabber, matching Phase 1's "plain functional UI to drive it" spirit; the real `NavigationSplitView` shell with a pinned live stats block is spec §9.1. **Phase 3/4.**
- **Segmented progress rendering, sparklines on grabbed items.** N/A to the grabber itself, but the shared row-treatment polish (§9.2) lands with the rest of the UI in Phase 3.
- **Manual package-override UI.** `GrabberSession.moveLink`/`GrabberController.moveLink` exist and are tested (Task 9), so drag-between-packages, right-click "move to existing/new package," rename, merge, and split (spec §7.4) have a backend to call — but this plan wires no UI control to them. Add the drag gesture and context menu once the row/package chrome gets its Phase 3 pass.
- **Theming.** `LinkGrabberView` and `AddLinksSheet` use system colors (`.green`, `.red`, `.orange`, `.secondary`) directly, not theme roles — spec §10.1 requires role-based color everywhere, but the theme system itself does not exist until Phase 4. Revisit every literal color in this plan's views then.
- **`NSPasteboard.detectedValues` below macOS 15.4.** `ClipboardWatcher` falls back to a full string read on 15.0–15.3; if that turns out to prompt a privacy alert in practice, spec §7.1 already flags this as a known risk to handle when it is actually observed on-device.
- **yt-dlp / YouTube links.** Grabbed YouTube URLs pass through the generic verdict/clustering pipeline like any other link — there is no resolver yet to give them a real per-format size or muxing story. **Phase 5.**
