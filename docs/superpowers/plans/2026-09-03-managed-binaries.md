# Managed Binaries Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans (inline). Steps use `- [ ]` checkboxes.

**Goal:** SDM provisions its own `yt-dlp` (downloaded, self-updating), `ffmpeg` and `qjs` (bundled, inflated) instead of requiring `brew install`.

**Architecture:** A `ManagedBinaries` actor in `SDMResolve` owns `~/Library/Application Support/SDM/bin/`. ffmpeg/qjs ship as per-arch LZFSE blobs in the app bundle and are inflated on first run / version bump. yt-dlp is downloaded from GitHub releases with our own SHA-256-verified version check + atomic replace (never `yt-dlp -U`). Time is a 1 Hz tick counter, not a wall clock. Tests use a fake fetcher + fake process runner; no network, no real sleep.

**Tech Stack:** Swift 6.2, Swift Testing, `Compression` framework (LZFSE), `FileManager.replaceItemAt`, Git LFS.

**Spec:** `docs/superpowers/specs/2026-09-03-managed-binaries-design.md`

## Global Constraints

- macOS 15.0 baseline, Swift 6 strict concurrency.
- No third-party dependencies. LZFSE via the `Compression` framework only.
- No test may touch the network or sleep on a real clock. Time advances via `ManagedBinaries.tick()`.
- Format Swift with `swift-format` before committing (`swift-format -i <files>`).
- `SDMResolve` must not depend on `SDMEngine` or `AppKit`/`SwiftUI`.
- yt-dlp asset name: `yt-dlp_macos` (universal2). Stable repo `yt-dlp/yt-dlp`, nightly repo `yt-dlp/yt-dlp-nightly-builds`. Release info: `https://api.github.com/repos/<repo>/releases/latest`.
- Large vendored blobs (`SDM/Resources/vendor/*.lzfse`) tracked with Git LFS.

---

### Task 1: LZFSE helper

**Files:**
- Create: `SDMKit/Sources/SDMResolve/LZFSE.swift`
- Test: `SDMKit/Tests/SDMResolveTests/LZFSETests.swift`

**Interfaces:**
- Produces: `enum LZFSE { static func decompress(_ data: Data, expandedSizeHint: Int = 64 << 20) throws -> Data; static func compress(_ data: Data) throws -> Data }`, `enum LZFSEError: Error { case failed }`

- [ ] **Step 1: Failing test** — `SDMKit/Tests/SDMResolveTests/LZFSETests.swift`:

```swift
import Foundation
import Testing
@testable import SDMResolve

@Test func lzfseRoundTrips() throws {
    let original = Data((0..<200_000).map { UInt8($0 % 251) })
    let compressed = try LZFSE.compress(original)
    #expect(compressed.count < original.count)
    #expect(try LZFSE.decompress(compressed) == original)
}

@Test func lzfseRejectsGarbage() {
    #expect(throws: (any Error).self) {
        try LZFSE.decompress(Data([1, 2, 3, 4, 5]))
    }
}
```

- [ ] **Step 2: Run, expect fail** — `cd SDMKit && swift test --filter LZFSE` → FAIL (no `LZFSE`).

- [ ] **Step 3: Implement** — `SDMKit/Sources/SDMResolve/LZFSE.swift`:

```swift
import Compression
import Foundation

public enum LZFSEError: Error, Sendable { case failed }

/// Thin wrapper over `Compression`'s LZFSE stream. Used to ship the
/// bundled `ffmpeg` / `qjs` binaries compressed inside the `.app` and
/// inflate them into the managed `bin/` directory on first run.
public enum LZFSE {
    public static func compress(_ data: Data) throws -> Data {
        try transform(data, operation: COMPRESSION_STREAM_ENCODE,
                      dstCapacity: max(64, data.count))
    }

    public static func decompress(_ data: Data, expandedSizeHint: Int = 64 << 20) throws -> Data {
        try transform(data, operation: COMPRESSION_STREAM_DECODE,
                      dstCapacity: max(expandedSizeHint, data.count * 4))
    }

    private static func transform(
        _ source: Data, operation: compression_stream_operation, dstCapacity: Int
    ) throws -> Data {
        guard !source.isEmpty else { return Data() }
        var stream = compression_stream(dst_ptr: nil, dst_size: 0, src_ptr: nil, src_size: 0, state: nil)
        guard compression_stream_init(&stream, operation, COMPRESSION_LZFSE) == COMPRESSION_STATUS_OK
        else { throw LZFSEError.failed }
        defer { compression_stream_destroy(&stream) }

        var output = Data()
        let bufferSize = max(64 << 10, dstCapacity)
        let dst = UnsafeMutablePointer<UInt8>.allocate(capacity: bufferSize)
        defer { dst.deallocate() }

        return try source.withUnsafeBytes { (src: UnsafeRawBufferPointer) throws -> Data in
            stream.src_ptr = src.bindMemory(to: UInt8.self).baseAddress!
            stream.src_size = src.count
            let flags = Int32(COMPRESSION_STREAM_FINALIZE.rawValue)
            while true {
                stream.dst_ptr = dst
                stream.dst_size = bufferSize
                let status = compression_stream_process(&stream, flags)
                switch status {
                case COMPRESSION_STATUS_OK, COMPRESSION_STATUS_END:
                    output.append(dst, count: bufferSize - stream.dst_size)
                    if status == COMPRESSION_STATUS_END { return output }
                default:
                    throw LZFSEError.failed
                }
            }
        }
    }
}
```

- [ ] **Step 4: Run, expect pass** — `swift test --filter LZFSE` → PASS.
- [ ] **Step 5: Commit** — `swift-format -i SDMKit/Sources/SDMResolve/LZFSE.swift SDMKit/Tests/SDMResolveTests/LZFSETests.swift && git add -A && git commit -m "feat(resolve): LZFSE compress/decompress helper"`

---

### Task 2: Manifest, channel, and version types

**Files:**
- Create: `SDMKit/Sources/SDMResolve/ManagedBinariesTypes.swift`
- Test: `SDMKit/Tests/SDMResolveTests/ManagedBinariesTypesTests.swift`

**Interfaces:**
- Produces:
  - `enum YtDlpChannel: String, Codable, Sendable, CaseIterable { case stable, nightly; var releasesAPI: URL }`
  - `struct YtDlpVersion: Comparable, Sendable, CustomStringConvertible { init?(_ tag: String); let components: [Int] }`
  - `struct BinariesManifest: Codable, Sendable, Equatable { var ytDlpVersion: String?; var ytDlpChannel: YtDlpChannel; var lastCheckTick: Int; var ffmpegVersion: String?; var qjsVersion: String?; var lastError: String?; static var empty: BinariesManifest }`
  - `struct VendorAsset: Sendable { var name: String; var compressedURL: URL; var version: String }` (`name` ∈ `"ffmpeg"`, `"qjs"`)
  - `enum CheckReason: Sendable { case launch, timer, resolveNeeded, channelChanged, manual }`
  - `enum CheckOutcome: Sendable, Equatable { case upToDate(String?); case updated(String); case noNetwork; case failed(String); case skipped }`
  - `struct ManagedBinariesStatus: Sendable, Equatable { var ytDlpVersion: String?; var latestKnown: String?; var channel: YtDlpChannel; var ffmpegVersion: String?; var qjsVersion: String?; var lastCheckTick: Int?; var lastError: String? }`

- [ ] **Step 1: Failing test**:

```swift
import Foundation
import Testing
@testable import SDMResolve

@Test func versionComparesComponentWise() {
    #expect(YtDlpVersion("2026.09.01")! > YtDlpVersion("2026.08.19")!)
    #expect(YtDlpVersion("2026.08.19.120000")! > YtDlpVersion("2026.08.19")!)
    #expect(YtDlpVersion("2026.08.19")! == YtDlpVersion("2026.08.19")!)
    #expect(YtDlpVersion("garbage") == nil)
}

@Test func manifestRoundTrips() throws {
    var m = BinariesManifest.empty
    m.ytDlpVersion = "2026.08.19"
    m.ytDlpChannel = .nightly
    m.lastError = "boom"
    let data = try JSONEncoder().encode(m)
    #expect(try JSONDecoder().decode(BinariesManifest.self, from: data) == m)
}

@Test func channelPickstheRightRepo() {
    #expect(YtDlpChannel.stable.releasesAPI.absoluteString
        == "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest")
    #expect(YtDlpChannel.nightly.releasesAPI.absoluteString
        == "https://api.github.com/repos/yt-dlp/yt-dlp-nightly-builds/releases/latest")
}
```

- [ ] **Step 2: Run, expect fail.**
- [ ] **Step 3: Implement** `ManagedBinariesTypes.swift` — the types above. `YtDlpVersion.init?` splits on `.`, maps to `Int`, returns `nil` if empty or any component non-numeric. `Comparable` pads the shorter array with zeros. `description` re-joins with `.`. `BinariesManifest.empty` = all nils, `ytDlpChannel: .stable`, `lastCheckTick: 0`. `releasesAPI` builds the URL from a `repo` string.
- [ ] **Step 4: Run, expect pass.**
- [ ] **Step 5: Commit** — `feat(resolve): managed-binaries manifest, channel, version types`

---

### Task 3: `BinaryFetching` seam

**Files:**
- Create: `SDMKit/Sources/SDMResolve/BinaryFetching.swift`
- Test: `SDMKit/Tests/SDMResolveTests/BinaryFetchingTests.swift`

**Interfaces:**
- Produces:
  - `protocol BinaryFetching: Sendable { func data(from url: URL) async throws -> Data }`
  - `struct URLSessionBinaryFetcher: BinaryFetching { init(session: URLSession = .shared); ... }` — GET, throw `BinaryFetchError.http(Int)` on non-2xx, `BinaryFetchError.transport(String)` on `URLError`.
  - `enum BinaryFetchError: Error, Equatable, Sendable { case http(Int); case transport(String) }`

- [ ] **Step 1: Failing test** — only the error type + a `FakeBinaryFetcher` test double placed in test support (see Task 4). Minimal test:

```swift
import Foundation
import Testing
@testable import SDMResolve

@Test func fakeFetcherReplaysAndThrows() async throws {
    let f = FakeBinaryFetcher()
    f.responses["https://x/a"] = .success(Data("hi".utf8))
    f.responses["https://x/b"] = .failure(BinaryFetchError.transport("offline"))
    #expect(try await f.data(from: URL(string: "https://x/a")!) == Data("hi".utf8))
    await #expect(throws: BinaryFetchError.transport("offline")) {
        try await f.data(from: URL(string: "https://x/b")!)
    }
}
```

- [ ] **Step 2: Run, expect fail.**
- [ ] **Step 3: Implement** `BinaryFetching.swift` (protocol + `URLSessionBinaryFetcher` + `BinaryFetchError`) and add `FakeBinaryFetcher` to `SDMKit/Tests/SDMResolveTests/Support.swift`:

```swift
final class FakeBinaryFetcher: BinaryFetching, @unchecked Sendable {
    private let lock = NSLock()
    var responses: [String: Result<Data, any Error>] = [:]
    private(set) var requestCount = 0
    func data(from url: URL) async throws -> Data {
        lock.withLock { requestCount += 1 }
        guard let r = responses[url.absoluteString] else { throw BinaryFetchError.http(404) }
        return try r.get()
    }
}
```

- [ ] **Step 4: Run, expect pass.**
- [ ] **Step 5: Commit** — `feat(resolve): BinaryFetching seam + fake`

---

### Task 4: `ManagedBinaries` — bundled inflation

**Files:**
- Create: `SDMKit/Sources/SDMResolve/ManagedBinaries.swift`
- Test: `SDMKit/Tests/SDMResolveTests/ManagedBinariesInflationTests.swift`

**Interfaces:**
- Consumes: Tasks 1–3.
- Produces:
  ```swift
  public actor ManagedBinaries {
      public init(binDirectory: URL, fetcher: any BinaryFetching, runner: any ProcessRunner,
                  vendorAssets: @Sendable @escaping () -> [VendorAsset],
                  channel: @Sendable @escaping () -> YtDlpChannel,
                  onBinariesChanged: @Sendable @escaping () async -> Void = {},
                  notify: @Sendable @escaping (String) -> Void = { _ in })
      public func provisionBundledIfNeeded() async
      public func status() -> ManagedBinariesStatus
      var manifestForTesting: BinariesManifest { get }   // internal, @testable
  }
  ```

- [ ] **Step 1: Failing test** — write a real LZFSE blob to a temp dir, point a `VendorAsset` at it, assert the decoded file lands at `bin/ffmpeg` with mode `0755` and `manifest.ffmpegVersion == "7.1"`; a second call with the same version does not rewrite (capture mtime); a bumped version rewrites. Use `FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)`.

```swift
@Test func inflatesBundledFFmpegOnce() async throws {
    let tmp = URL.tmp(); defer { try? FileManager.default.removeItem(at: tmp) }
    let bin = tmp.appendingPathComponent("bin")
    let blob = tmp.appendingPathComponent("ffmpeg.lzfse")
    try LZFSE.compress(Data("#!/bin/sh\necho ff\n".utf8)).write(to: blob)
    let assets = { [VendorAsset(name: "ffmpeg", compressedURL: blob, version: "7.1")] }
    let mb = ManagedBinaries(binDirectory: bin, fetcher: FakeBinaryFetcher(),
        runner: FakeProcessRunner(), vendorAssets: assets, channel: { .stable })
    await mb.provisionBundledIfNeeded()
    let ff = bin.appendingPathComponent("ffmpeg")
    #expect(FileManager.default.isExecutableFile(atPath: ff.path))
    #expect(await mb.manifestForTesting.ffmpegVersion == "7.1")
}
```

Add `extension URL { static func tmp() -> URL }` to Support.swift.

- [ ] **Step 2: Run, expect fail.**
- [ ] **Step 3: Implement** — `ManagedBinaries.swift`. Store `binDirectory` and closures. `manifestURL = binDirectory/manifest.json`. Private `loadManifest()` (returns `.empty` on missing/corrupt), `saveManifest(_:)` (create `binDirectory` with intermediate dirs first). `provisionBundledIfNeeded`: for each asset, target `binDirectory/<name>`; if `!fileExists || manifest.version(for: name) != asset.version` → `try LZFSE.decompress(Data(contentsOf: asset.compressedURL))`, write to `target.appendingPathExtension("new")`, `FileManager.setAttributes([.posixPermissions: 0o755])`, `replaceItemAt`, update the manifest field, save. Wrap each asset in `do/catch`, on error set `manifest.lastError` and continue. `status()` maps manifest → `ManagedBinariesStatus`.
- [ ] **Step 4: Run, expect pass.**
- [ ] **Step 5: Commit** — `feat(resolve): ManagedBinaries bundled-asset inflation`

---

### Task 5: `ManagedBinaries` — yt-dlp download, verify, atomic replace, single-flight

**Files:**
- Modify: `SDMKit/Sources/SDMResolve/ManagedBinaries.swift`
- Test: `SDMKit/Tests/SDMResolveTests/ManagedBinariesUpdateTests.swift`

**Interfaces:**
- Produces: `public func checkNow(reason: CheckReason) async -> CheckOutcome` on `ManagedBinaries`.

- [ ] **Step 1: Failing tests**:
  - `freshInstallDownloadsYtDlp`: `bin/yt-dlp` absent, fetcher serves release JSON `{"tag_name":"2026.08.19"}` for the stable API URL, `yt-dlp_macos` bytes for the asset URL, and a `SHA2-256SUMS` body containing the correct hash line. After `checkNow(.launch)` → `bin/yt-dlp` exists & executable, `manifest.ytDlpVersion == "2026.08.19"`, outcome `.updated("2026.08.19")`, `onBinariesChanged` fired once (use an actor counter box).
  - `upToDateSkipsDownload`: manifest already `2026.08.19`, binary present → `.upToDate`, asset URL never requested (`fetcher.requestCount` only the release-info call).
  - `newerRemoteReplaces`: manifest `2026.08.19`, remote `2026.09.01` → replaced, old bytes gone.
  - `shaMismatchAborts`: `SHA2-256SUMS` has a wrong hash → outcome `.failed`, `manifest.lastError != nil`, existing binary (write a sentinel first) unchanged.
  - `offlineRecordsNoNetwork`: fetcher throws `BinaryFetchError.transport` → `.noNetwork`, `lastError` set.
  - `singleFlightCoalesces`: `async let` five `checkNow` concurrently → release-info requested once.

  Release-info URL to key in the fake: `YtDlpChannel.stable.releasesAPI.absoluteString`. The asset + sums URLs: derive from the spec's `https://github.com/yt-dlp/yt-dlp/releases/download/<tag>/yt-dlp_macos` and `.../SHA2-256SUMS` — hard-code these in both the impl and the fake.

- [ ] **Step 2: Run, expect fail.**
- [ ] **Step 3: Implement**:
  - `private var inFlight: Task<CheckOutcome, Never>?`. `checkNow`: `if let t = inFlight { return await t.value }`; else `let t = Task { await self.performCheck(reason:) }; inFlight = t; let r = await t.value; inFlight = nil; return r`.
  - `performCheck`: load manifest; `let repo = channel()`; fetch `repo.releasesAPI`; JSON-decode `{ tag_name: String }`; `let remote = YtDlpVersion(tag)`. Determine `needsDownload = !FileManager.default.fileExists(atPath: ytDlpURL.path) || manifest.ytDlpVersion.flatMap(YtDlpVersion.init).map { remote > $0 } ?? true`.
  - If not → return `.upToDate(manifest.ytDlpVersion)` (also refresh `lastCheckTick`, save).
  - Download: `let bytes = try await fetcher.data(from: assetURL(tag:))`; `let sums = String(decoding: try await fetcher.data(from: sumsURL(tag:)), as: UTF8.self)`.
  - `verify(bytes:against:sums)`: find the line ending in `  yt-dlp_macos`, compare `SHA256(bytes)` hex (`import Crypto`? No — use `CryptoKit`: `import CryptoKit; SHA256.hash(data:)`). CryptoKit is a system framework, allowed.
  - Write `ytDlpURL.new`, `chmod 0755`, `runner.run(executable: /usr/bin/xattr, ["-c", path])` then `runner.run(executable: /usr/bin/codesign, ["-s", "-", "--force", path])` (ignore failures — wrap in `try?`), `replaceItemAt`.
  - Update manifest (`ytDlpVersion = tag`, `ytDlpChannel = ...` from a channel-name map, `lastError = nil`, `lastCheckTick`), save, `await onBinariesChanged()`, `notify("yt-dlp updated to \(tag)")`, return `.updated(tag)`.
  - `catch`: set `manifest.lastError = "\(error)"`, save; return `.noNetwork` for `BinaryFetchError.transport`, else `.failed("\(error)")`.
- [ ] **Step 4: Run, expect pass.**
- [ ] **Step 5: Commit** — `feat(resolve): yt-dlp version check, SHA-256 verify, atomic replace`

---

### Task 6: `ManagedBinaries` — tick cadence

**Files:**
- Modify: `SDMKit/Sources/SDMResolve/ManagedBinaries.swift`
- Test: `SDMKit/Tests/SDMResolveTests/ManagedBinariesCadenceTests.swift`

**Interfaces:**
- Produces: `public func tick() async` on `ManagedBinaries`. Constants `presentInterval = 21_600`, `absentInterval = 900`.

- [ ] **Step 1: Failing tests**:
  - `noCheckBeforeInterval`: with `bin/yt-dlp` present (write sentinel + manifest version) and fetcher serving up-to-date, call `tick()` 21_599× → `fetcher.requestCount == 0`; the 21_600th → `== 1`.
  - `absentRetriesEvery900`: binary absent, fetcher throws → after 900 ticks one attempt, after 1800 two.
  - Use a helper that calls `tick()` in a loop; still no real sleep.

- [ ] **Step 2: Run, expect fail.**
- [ ] **Step 3: Implement** — `private var tick = 0`, `private var lastCheckAtTick = -1` (so the first tick after launch is not auto-triggered — launch check is explicit via `checkNow(.launch)`; actually: initialise `lastCheckAtTick = 0` and let the app call `checkNow(.launch)`). `func tick()`: `self.tick += 1`; `let interval = FileManager.default.fileExists(atPath: ytDlpURL.path) ? presentInterval : absentInterval`; `guard inFlight == nil, self.tick - lastCheckAtTick >= interval else { return }`; `lastCheckAtTick = self.tick`; `Task { _ = await self.checkNow(reason: .timer) }`. Update `lastCheckAtTick` also inside `performCheck` on completion so an explicit `checkNow` resets the timer.
- [ ] **Step 4: Run, expect pass.**
- [ ] **Step 5: Commit** — `feat(resolve): tick-driven update cadence (6h present / 15m absent)`

---

### Task 7: `YtDlpResolver` extra arguments

**Files:**
- Modify: `SDMKit/Sources/SDMResolve/YtDlpResolver.swift`
- Test: `SDMKit/Tests/SDMResolveTests/YtDlpResolverTests.swift` (add cases)

**Interfaces:**
- Produces: `YtDlpResolver.init(..., extraArguments: @Sendable @escaping () -> [String] = { [] })`; every `runYtDlp` invocation includes those tokens.

- [ ] **Step 1: Failing test**:

```swift
@Test func splicesExtraArgumentsIntoEveryInvocation() async throws {
    let runner = FakeProcessRunner()
    runner.defaultOutput = ok(try fixtureData("single_video"))
    let locator = BinaryLocator(searchPaths: [URL(fileURLWithPath: "/b")],
        isExecutable: { _ in true })
    let r = YtDlpResolver(runner: runner, locator: locator,
        extraArguments: { ["--extractor-args", "youtube:jsruntime=quickjs"] })
    _ = try? await r.resolve(URL(string: "https://youtube.com/watch?v=abc")!)
    #expect(runner.calls.last!.arguments.contains("youtube:jsruntime=quickjs"))
}
```

(Confirm `single_video.json` fixture name against `Fixtures/`; use whatever single-video fixture exists.)

- [ ] **Step 2: Run, expect fail.**
- [ ] **Step 3: Implement** — add stored `extraArguments` closure; in `runYtDlp(_:_:timeout:)` change the call to `runner.run(executable: executable, arguments: arguments + extraArguments(), timeout: timeout)`.
- [ ] **Step 4: Run, expect pass** — plus the full `swift test` to confirm no regression in existing resolver tests.
- [ ] **Step 5: Commit** — `feat(resolve): YtDlpResolver extraArguments hook`

---

### Task 8: App wiring — shared locator + ManagedBinaries

**Files:**
- Modify: `SDM/EngineController.swift`, `SDM/GrabberController.swift`, `SDM/SDMApp.swift`
- Create: `SDM/ManagedBinariesController.swift`

**Interfaces:**
- Consumes: `ManagedBinaries`, `BinaryLocator`.
- Produces:
  - `SDM/ManagedBinariesController.swift`: `@MainActor @Observable final class ManagedBinariesController` wrapping a `ManagedBinaries` + the shared `BinaryLocator`; exposes `let binaryLocator: BinaryLocator`, `func start()` (launch: `provisionBundledIfNeeded()` then `checkNow(.launch)`, then a 1 Hz `tick()` loop `Task`), `func checkNow() async`, `var status: ManagedBinariesStatus` (refreshed after each op), `func kick()` (`Task { await mb.checkNow(reason: .resolveNeeded) }`).
  - `EngineController.init(notificationManager:binaryLocator:)` and `GrabberController.init(binaryLocator:managedBinaries:)` — accept injected deps.

- [ ] **Step 1:** `SDM/ManagedBinariesController.swift`:

```swift
import Foundation
import Observation
import SDMResolve

@MainActor
@Observable
final class ManagedBinariesController {
    let binaryLocator: BinaryLocator
    private let mb: ManagedBinaries
    private(set) var status = ManagedBinariesStatus(
        ytDlpVersion: nil, latestKnown: nil, channel: .stable,
        ffmpegVersion: nil, qjsVersion: nil, lastCheckTick: nil, lastError: nil)
    private var tickTask: Task<Void, Never>?

    static var binDirectory: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("SDM/bin", isDirectory: true)
    }

    init(notify: @escaping @Sendable (String) -> Void) {
        let bin = Self.binDirectory
        let locator = BinaryLocator(searchPaths: [bin])
        binaryLocator = locator
        mb = ManagedBinaries(
            binDirectory: bin,
            fetcher: URLSessionBinaryFetcher(),
            runner: SystemProcessRunner(),
            vendorAssets: { Self.bundledVendorAssets() },
            channel: { YouTubeSettingsStore.ytDlpChannel },
            onBinariesChanged: { await locator.invalidate() },
            notify: notify)
    }

    func start() {
        Task {
            await mb.provisionBundledIfNeeded()
            _ = await mb.checkNow(reason: .launch)
            status = await mb.status()
        }
        tickTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(1))
                guard let self else { return }
                await self.mb.tick()
                self.status = await self.mb.status()
            }
        }
    }

    func checkNow() async { _ = await mb.checkNow(reason: .manual); status = await mb.status() }
    func kick() { Task { _ = await mb.checkNow(reason: .resolveNeeded); status = await mb.status() } }

    private static func bundledVendorAssets() -> [VendorAsset] {
        guard let dir = Bundle.main.url(forResource: "vendor", withExtension: nil),
            let vm = try? JSONDecoder().decode(
                [String: String].self,
                from: Data(contentsOf: dir.appendingPathComponent("vendor-manifest.json")))
        else { return [] }
        #if arch(arm64)
        let ffmpegBlob = "ffmpeg-arm64.lzfse"
        #else
        let ffmpegBlob = "ffmpeg-x86_64.lzfse"
        #endif
        var assets: [VendorAsset] = []
        if let v = vm["ffmpegVersion"] {
            assets.append(VendorAsset(name: "ffmpeg",
                compressedURL: dir.appendingPathComponent(ffmpegBlob), version: v))
        }
        if let v = vm["qjsVersion"] {
            assets.append(VendorAsset(name: "qjs",
                compressedURL: dir.appendingPathComponent("qjs.lzfse"), version: v))
        }
        return assets
    }
}
```

- [ ] **Step 2:** `EngineController.init` — add `binaryLocator: BinaryLocator` param, delete the local `let binaryLocator = BinaryLocator()`, use the param, and pass `extraArguments: { Self.ytDlpExtraArgs }` into `YtDlpResolver`. Add:

```swift
static var ytDlpExtraArgs: [String] {
    ["--extractor-args", "youtube:jsruntime=quickjs",
     "--ffmpeg-location", ManagedBinariesController.binDirectory.path]
}
```

- [ ] **Step 3:** `GrabberController.init` — add `binaryLocator: BinaryLocator, managedBinaries: ManagedBinariesController` params; use them; `ingest(urls:)` and `ingest(text:)` call `managedBinaries.kick()` first thing; `ffmpegOnDisk` → `FileManager.default.isExecutableFile(atPath: ManagedBinariesController.binDirectory.appendingPathComponent("ffmpeg").path)`; pass the same `extraArguments`.

- [ ] **Step 4:** `SDMApp.init` — construct `let managedBinaries = ManagedBinariesController(notify: { notification.postGeneric($0) })` (add a small `postGeneric` to `NotificationManager`, or reuse an existing post method); pass `binaryLocator: managedBinaries.binaryLocator` to `EngineController`, and `binaryLocator:managedBinaries:` to `GrabberController`; store `managedBinaries` in `@State`; call `managedBinaries.start()` after `engine.startHeartbeatIfNeeded()`.

- [ ] **Step 5:** Build the app: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build 2>&1 | tail -20`. Expect BUILD SUCCEEDED. Fix compile errors.
- [ ] **Step 6: Commit** — `feat(app): shared BinaryLocator + ManagedBinaries wiring`

---

### Task 9: Settings — channel + Components section

**Files:**
- Modify: `SDM/YouTubeSettingsStore.swift`, `SDM/SettingsView.swift`

**Interfaces:**
- Produces: `YouTubeSettingsStore.ytDlpChannel: YtDlpChannel` (`UserDefaults` key `sdm.yt.ytDlpChannel`, default `.stable`).

- [ ] **Step 1:** `YouTubeSettingsStore` — add `Key.ytDlpChannel = "sdm.yt.ytDlpChannel"` and:

```swift
static var ytDlpChannel: YtDlpChannel {
    get { YtDlpChannel(rawValue: d.string(forKey: Key.ytDlpChannel) ?? "") ?? .stable }
    set { d.set(newValue.rawValue, forKey: Key.ytDlpChannel) }
}
```

- [ ] **Step 2:** `SettingsView` — thread `@Environment(ManagedBinariesController.self) private var managedBinaries` (add `.environment(managedBinaries)` at the `SettingsView` call site in `SDMApp`). Add to `youtubeTab` a `GroupBox("Components")`:
  - yt-dlp row: `Text(status.ytDlpVersion ?? "not installed")`, `Text("latest: \(status.latestKnown ?? "—")")`, `Picker` bound to a `@State ytChannel` writing `YouTubeSettingsStore.ytDlpChannel` and calling `await managedBinaries.checkNow()`, `Button("Check now") { Task { await managedBinaries.checkNow() } }`, `if let e = status.lastError { Text(e).foregroundStyle(.red) }`.
  - ffmpeg row: `Text(status.ffmpegVersion ?? "—") + Text(" (bundled)")`.
  - JS runtime row: `Text("QuickJS-ng \(status.qjsVersion ?? "—") (bundled)")`.
  - Add `ytChannel` to the `@State` block and to `applySettings()`.

- [ ] **Step 3:** Build the app (`xcodebuild ... build`). Expect BUILD SUCCEEDED.
- [ ] **Step 4: Commit** — `feat(settings): yt-dlp channel picker + Components status section`

---

### Task 10: Vendor build script + bundle resources + LFS

**Files:**
- Create: `scripts/vendor-binaries.sh`, `scripts/lzfse-pack/main.swift`, `SDM/Resources/vendor/.gitkeep`, `SDM/Resources/vendor/vendor-manifest.json`, `.gitattributes`
- Modify: `SDM.xcodeproj/project.pbxproj` (add `SDM/Resources/vendor` as a folder reference in the app target's Copy Bundle Resources)

- [ ] **Step 1:** `.gitattributes`:

```
SDM/Resources/vendor/*.lzfse filter=lfs diff=lfs merge=lfs -text
```

Run `git lfs install --local` and `git lfs track "SDM/Resources/vendor/*.lzfse"` (verifies the pattern; keep the `.gitattributes` above authoritative).

- [ ] **Step 2:** `scripts/lzfse-pack/main.swift` — a tiny CLI: `swift <file> compress|decompress <in> <out>` using the same `Compression` LZFSE code as Task 1 (copy it; the script builds this on the fly with `swiftc`). Keeps the shell script dependency-free.

- [ ] **Step 3:** `scripts/vendor-binaries.sh` (`set -euo pipefail`):
  - Pin `FFMPEG_VERSION` and `QJS_TAG` at the top.
  - `curl` the martin-riedl.de JSON API, extract the `arm64` and `amd64` macOS release download URLs for `FFMPEG_VERSION`, download, verify their published SHA-256, unzip, keep the `ffmpeg` binary for each arch.
  - `git clone --depth 1 --branch "$QJS_TAG" https://github.com/quickjs-ng/quickjs "$TMP/qjs"`; build twice with `cmake -DCMAKE_OSX_ARCHITECTURES=arm64` / `x86_64`; `lipo -create ... -output qjs`.
  - `swiftc scripts/lzfse-pack/main.swift -O -o "$TMP/lzfse-pack"`; compress each binary → `SDM/Resources/vendor/{ffmpeg-arm64,ffmpeg-x86_64,qjs}.lzfse`.
  - Write `SDM/Resources/vendor/vendor-manifest.json`: `{"ffmpegVersion":"<FFMPEG_VERSION>","qjsVersion":"<QJS_TAG stripped of leading v>"}`.
  - Copy `COPYING`/`LICENSE` texts next to the blobs.
  - `echo` a reminder: `git add`, the blobs are LFS-tracked, run a build.
  - Make executable: `chmod +x scripts/vendor-binaries.sh`.

- [ ] **Step 4:** Seed `SDM/Resources/vendor/vendor-manifest.json` with placeholder `{"ffmpegVersion":"0","qjsVersion":"0"}` and a `.gitkeep`, so the app builds before the script is ever run (empty `vendorAssets` when versions are `"0"` and blobs absent → `provisionBundledIfNeeded` no-ops gracefully; verify it does — guard `FileManager.fileExists(atPath: asset.compressedURL.path)` in `provisionBundledIfNeeded`, skip missing blobs).

- [ ] **Step 5:** `project.pbxproj` — add `SDM/Resources/vendor` as a **folder reference** (blue folder) to the SDM target's Resources build phase so the whole directory copies to `SDM.app/Contents/Resources/vendor/`. (Do this in Xcode if editing pbxproj by hand is error-prone; document the exact steps in the commit message.)

- [ ] **Step 6:** Run `./scripts/vendor-binaries.sh`. Then build the app and confirm `ManagedBinariesController` inflates `bin/ffmpeg` + `bin/qjs` on first launch (add a temporary `print` or check `~/Library/Application Support/SDM/bin/` after a run).

- [ ] **Step 7: Commit** — `feat(build): vendor-binaries.sh, LFS-tracked bundle blobs, Xcode resource` (commit the real `.lzfse` blobs via LFS).

---

### Task 11: Guard `provisionBundledIfNeeded` against missing blobs

**Files:**
- Modify: `SDMKit/Sources/SDMResolve/ManagedBinaries.swift`
- Test: `SDMKit/Tests/SDMResolveTests/ManagedBinariesInflationTests.swift` (add case)

- [ ] **Step 1: Failing test** — `VendorAsset` pointing at a nonexistent path → `provisionBundledIfNeeded()` does not throw, `bin/ffmpeg` absent, no crash, `manifest.lastError` may be nil (missing blob is not an error, just skip).
- [ ] **Step 2: Run, expect fail** (if not already passing).
- [ ] **Step 3: Implement** — `guard FileManager.default.fileExists(atPath: asset.compressedURL.path) else { continue }` at the top of the per-asset loop.
- [ ] **Step 4: Run, expect pass** + full `swift test`.
- [ ] **Step 5: Commit** — `fix(resolve): skip absent vendor blobs during inflation`

---

### Task 12: Docs

**Files:**
- Modify: `README.md`, `CLAUDE.md`, `todo.md`, `docs/superpowers/specs/2026-08-03-sdm-design.md`

- [ ] **Step 1:** `README.md` — add a "Vendored binaries" section: `ffmpeg` + `qjs` (QuickJS-ng) ship inside the app as LZFSE blobs under `SDM/Resources/vendor/` (Git LFS), inflated to `~/Library/Application Support/SDM/bin/` on first launch; `yt-dlp` is downloaded there on first launch and self-updates (stable or nightly, set in YouTube Settings), SHA-256-verified, never via `yt-dlp -U`; to refresh the pinned `ffmpeg`/`qjs` run `./scripts/vendor-binaries.sh` and commit the new blobs.
- [ ] **Step 2:** `CLAUDE.md` — under "Current state" note managed-binaries is implemented; under "Conventions" add: "Vendored `ffmpeg`/`qjs` blobs are Git LFS; regenerate with `scripts/vendor-binaries.sh`."
- [ ] **Step 3:** `todo.md` — check off items 4 and 5; under item 5 note the Settings file-picker fields are superseded by managed-only resolution. Add a line to "Decided" if useful.
- [ ] **Step 4:** `docs/superpowers/specs/2026-08-03-sdm-design.md` §2 — add an "IMPLEMENTED (2026-09-03) — see 2026-09-03-managed-binaries-design.md" note against the "yt-dlp bundling + self-update" deferred item.
- [ ] **Step 5:** `swift-format` any touched Swift, run full `swift test` + app build one final time.
- [ ] **Step 6: Commit** — `docs: managed binaries implemented — README, CLAUDE, todo, spec`

---

## Self-Review

- **Spec coverage:** layout (T4), manifest (T2), `ManagedBinaries` actor (T4–6), cadence table (T6), update algorithm incl. SHA-256 + atomic replace + single-flight (T5), bundled inflation (T4, T11), `BinaryLocator` wiring (T8), `YtDlpResolver` extra args (T7), `SystemProcessRunner` (already fine — noted, no task needed), app target shared instances + tick loop + grabber trigger (T8), Settings tab + channel (T9), `vendor-binaries.sh` + `.gitattributes` LFS (T10), tests (each task), docs (T12). ✅
- **Placeholders:** none — every code step has literal code or a precise edit description. `project.pbxproj` step (T10.5) is described as a manual Xcode action with a documented fallback; acceptable for a pbxproj change.
- **Type consistency:** `ManagedBinaries` init signature identical across T4/T5/T6/T8; `CheckOutcome`/`CheckReason`/`ManagedBinariesStatus` from T2 used unchanged; `VendorAsset` fields (`name`/`compressedURL`/`version`) consistent T2→T4→T8→T10; `binDirectory` computed the same way in `ManagedBinariesController` and `EngineController.ytDlpExtraArgs`.
- **Risk:** martin-riedl.de API shape and yt-dlp's exact `--extractor-args` token must be confirmed live during T10/T7 (flagged in-spec). CryptoKit `SHA256` is a system framework — no dependency rule violation.
