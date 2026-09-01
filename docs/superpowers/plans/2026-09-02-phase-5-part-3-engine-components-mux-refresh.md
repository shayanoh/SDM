# Phase 5 Part 3 — Engine: Parallel Components, Muxing, URL Refresh: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `DownloadEngine` actually download a multi-component `DownloadItem` — one `DownloadTask` per component, running in parallel, drawing from the shared connection budget — then assemble a muxed item with `ffmpeg -c copy`, and transparently refresh a `403`/`410`-expired `googlevideo` URL mid-download via the injected `LinkResolver`.

**Architecture:** `DownloadTask` is **not** taught about multiple files — it stays one URL → one file. Instead `DownloadEngine.Runner` holds one `ComponentRun` (task + destination) per incomplete component of the item, plus a single `job` that awaits them all and then runs assembly. The engine's per-item maps stay keyed by `itemID`; `snapshot()` sums each component's `completedRanges` shifted by the item-space base offsets. A `403`/`410` on a component whose `origin` is `.resolved` throws `DownloadError.urlExpired(formatID:)`; the engine catches it, calls `resolver.refresh`, checks the itag and size still match, and restarts that one component's task against its existing per-component sidecar. Per-component `.incomplete`/`.sdmpart` files already work (one `DownloadTask` = one file = one sidecar) — **no `ResumeSidecar` v2**. During assembly the item stays `.running` and the engine tracks it in an `assembling: Set<UUID>` exposed on the snapshot as `isAssembling` — a distinct `ItemState.assembling` and its UI are Part 4.

**Tech Stack:** Swift 6 language mode (strict concurrency), Swift Testing, local SPM package `SDMKit`, `Foundation.Process` via `SDMResolve.ProcessRunner`, `ffmpeg`.

**Spec:** `docs/superpowers/specs/2026-09-02-phase-5-youtube-resolver-design.md` — Part 3 implements §7.1 (per-component pools), §7.2 (assembly / `Muxer`), §7.3 (403 refresh), §7.5 (`DownloadEngine.init(resolver:)`), and the parts of §5.3 the engine owns (concatenated snapshot). Deferred to Part 4: the distinct `ItemState.assembling`, `ItemSnapshot.components` detail breakout for the UI, grabber, Settings, picker.

## Global Constraints

- **Swift 6 language mode, strict concurrency.** Parent spec §2.
- **macOS 15.0 baseline.**
- **The full pre-existing suite (362 tests) MUST stay green after every task.** A one-component `DownloadItem` must download, resume, checkpoint, preempt, and finalize exactly as before.
- **No test may touch the network or sleep on a real clock.** Time advances via `DownloadEngine.tick()`. Parent spec §11.1. Subprocess access is injected (`ProcessRunner`); one real-`ffmpeg` smoke test is `.enabled(if:)` the binary exists.
- **No injected `Clock`.** Parent spec / CLAUDE.md.
- **Format with `./format.sh`, lint clean with `./lint.sh` before every commit.**
- **Tests:** `cd SDMKit && swift test`; single `--filter '<Type>/<name>'`, always confirm a non-zero count.
- `SDMEngine` gains a dependency on `SDMResolve` (for `ProcessRunner`) — add it to `Package.swift`. `SDMResolve` depends only on `SDMCore`, so there is no cycle.
- Muxing output container follows the video component's container; `ffmpeg -c copy` (no re-encode). Parent spec §12(b).
- 403 refresh attempts consume the existing `RetryPolicy` attempt cap — **no** separate budget. Parent spec §7.3(b).

---

## File Structure

**`SDMKit/Sources/SDMEngine/` (changes):**

| File | Change |
|---|---|
| `Muxer.swift` *(new)* | `Muxer` protocol, `FFmpegMuxer`, `MuxError` |
| `DownloadTask.swift` | `Configuration.refreshableFormatID: String?`; `DownloadError.urlExpired(formatID:)`; `download()` and `prepare()` classify `403`/`410` as `.urlExpired` when that field is set |
| `DownloadEngine.swift` | `Runner` → `[ComponentRun]`; `context(for:)` → per-component; `run()` spawns a task per incomplete component, awaits all, assembles; `snapshot()` concatenates; `init(resolver:muxer:)`; `assembling: Set<UUID>`; 403→refresh; per-component file ops in `finish`/`moveItem`/`removeItem`/`resetDownload` |
| `EngineSnapshot.swift` | `ItemSnapshot.isAssembling: Bool` |

**`SDMKit/Sources/SDMEngine/` no change:** `Scheduler.swift` (still item-keyed), `RetryPolicy.swift`, `SparseFile.swift`, `ResumeSidecar.swift`.

**`SDMKit/Package.swift`:** `SDMEngine` target gains `dependencies: ["SDMCore", "SDMResolve"]`.

**`SDMKit/Tests/SDMEngineTests/` (new):**

| File | Covers |
|---|---|
| `MuxerTests.swift` | `FFmpegMuxer` argument building + fake-runner success/failure; real-ffmpeg smoke skipped when absent |
| `MultiComponentDownloadTests.swift` | two-component parallel byte identity under churn; concatenated snapshot; one component permanently failing fails the item |
| `MuxAssemblyTests.swift` | all-components-complete → `FakeMuxer` runs → output kept, parts deleted; mux failure → `.failed`, files kept; **Retry mux** re-runs only the mux |
| `URLRefreshTests.swift` | `FakeOrigin` 403 on a `.resolved` component + fake resolver returns fresh URL → component resumes against existing set, final file hash-matches; format-gone / size-changed → item `.failed` |
| `Support` additions | `FakeMuxer`, a fake `LinkResolver` for the engine |

**`SDMKit/Tests/SDMResolveTests/` no change.** **App (`SDM/`):** `EngineController` passes a real `YtDlpResolver` + `FFmpegMuxer` into `DownloadEngine.init` (one-line change + wiring — kept in Task 7 so the plan stays engine-focused; the app already builds a `YtDlpResolver` only in Part 4, so Task 7 constructs a minimal one here).

---

## Task 1: `Muxer` — protocol, `FFmpegMuxer`, fake

**Files:**
- Modify: `SDMKit/Package.swift` (add `SDMResolve` to `SDMEngine` deps)
- Create: `SDMKit/Sources/SDMEngine/Muxer.swift`
- Create: `SDMKit/Tests/SDMEngineTests/MuxerTests.swift`

**Interfaces:**
- Consumes: `ProcessRunner`, `ProcessOutput`, `ProcessRunError`, `BinaryLocator` (from `SDMResolve`, Part 1); `MediaContainer` (`SDMCore`).
- Produces:
  - `enum MuxError: Error, Equatable, Sendable { case ffmpegMissing; case ffmpegFailed(stderrTail: String); case timedOut }`
  - `protocol Muxer: Sendable { func mux(videoPart: URL, audioPart: URL, into output: URL, container: MediaContainer) async throws }`
  - `struct FFmpegMuxer: Muxer` — `init(runner: any ProcessRunner, locator: BinaryLocator, timeout: Duration = .seconds(120))`
    - `static func arguments(video: URL, audio: URL, output: URL, container: MediaContainer) -> [String]` — `["-y", "-i", video.path, "-i", audio.path, "-c", "copy", "-map", "0:v:0", "-map", "1:a:0"] + movflags(container) + [output.path]` where `movflags(.mp4) == ["-movflags", "+faststart"]` and `movflags` is `[]` for other containers *(verify `+faststart` applicability against current ffmpeg docs at implementation)*
    - `mux(...)`: `locate("ffmpeg")` → nil ⇒ `throw .ffmpegMissing`; run; `ProcessRunError.timedOut` ⇒ `.timedOut`; non-zero exit ⇒ `.ffmpegFailed(stderrTail: last 800 chars of stderr)`; success ⇒ return

- [ ] **Step 1: `Package.swift`** — change the `SDMEngine` target line to:
```swift
        .target(name: "SDMEngine", dependencies: ["SDMCore", "SDMResolve"]),
```

- [ ] **Step 2: Write the failing test**

```swift
// SDMKit/Tests/SDMEngineTests/MuxerTests.swift
import Foundation
import Testing

@testable import SDMEngine

@testable import SDMResolve

@testable import SDMCore

private func loc(hasFfmpeg: Bool) -> BinaryLocator {
    let present: Set<String> = hasFfmpeg ? ["/bin/ffmpeg"] : []
    return BinaryLocator(
        searchPaths: [URL(fileURLWithPath: "/bin")], isExecutable: { present.contains($0.path) })
}

@Test func ffmpegArgumentsCopyStreamsAndSetFaststartForMp4() {
    let args = FFmpegMuxer.arguments(
        video: URL(fileURLWithPath: "/tmp/v.f137.mp4"),
        audio: URL(fileURLWithPath: "/tmp/a.f140.m4a"),
        output: URL(fileURLWithPath: "/tmp/out.mp4"), container: .mp4)
    #expect(args.contains("-c"))
    #expect(args.contains("copy"))
    #expect(args.contains("+faststart"))
    #expect(args.last == "/tmp/out.mp4")
}

@Test func ffmpegArgumentsOmitFaststartForWebm() {
    let args = FFmpegMuxer.arguments(
        video: URL(fileURLWithPath: "/tmp/v.webm"), audio: URL(fileURLWithPath: "/tmp/a.webm"),
        output: URL(fileURLWithPath: "/tmp/out.webm"), container: .webm)
    #expect(!args.contains("+faststart"))
}

@Test func muxWithoutFfmpegThrowsMissing() async {
    let m = FFmpegMuxer(runner: FakeProcessRunner(), locator: loc(hasFfmpeg: false))
    await #expect(throws: MuxError.ffmpegMissing) {
        try await m.mux(
            videoPart: URL(fileURLWithPath: "/tmp/v"), audioPart: URL(fileURLWithPath: "/tmp/a"),
            into: URL(fileURLWithPath: "/tmp/o.mp4"), container: .mp4)
    }
}

@Test func muxSurfacesFfmpegStderrOnFailure() async {
    let runner = FakeProcessRunner()
    runner.defaultOutput = fail("Invalid data found when processing input\n", exitCode: 1)
    let m = FFmpegMuxer(runner: runner, locator: loc(hasFfmpeg: true))
    await #expect(throws: MuxError.ffmpegFailed(stderrTail: "Invalid data found when processing input\n")) {
        try await m.mux(
            videoPart: URL(fileURLWithPath: "/tmp/v"), audioPart: URL(fileURLWithPath: "/tmp/a"),
            into: URL(fileURLWithPath: "/tmp/o.mp4"), container: .mp4)
    }
}

@Test func muxSucceedsOnZeroExit() async throws {
    let m = FFmpegMuxer(runner: FakeProcessRunner(), locator: loc(hasFfmpeg: true))
    try await m.mux(
        videoPart: URL(fileURLWithPath: "/tmp/v"), audioPart: URL(fileURLWithPath: "/tmp/a"),
        into: URL(fileURLWithPath: "/tmp/o.mp4"), container: .mp4)
}
```

> `FakeProcessRunner`, `ok`, `fail` live in `SDMResolveTests/Support.swift`. That
> file is not visible from `SDMEngineTests`. Copy the minimal `FakeProcessRunner`
> + `ok`/`fail` helpers into a new `SDMEngineTests/ProcessRunnerSupport.swift`
> (they are ~30 lines; duplication across test targets is acceptable and the
> existing suite already does this for `makeScratchDirectory`).

- [ ] **Step 3: Run test to verify it fails** — `swift test --filter 'MuxerTests'` → FAIL (`FFmpegMuxer` not found).

- [ ] **Step 4: Write the implementation**

```swift
// SDMKit/Sources/SDMEngine/Muxer.swift
import Foundation
import SDMCore
import SDMResolve

public enum MuxError: Error, Equatable, Sendable {
    case ffmpegMissing
    case ffmpegFailed(stderrTail: String)
    case timedOut
}

/// Combines a video-only and audio-only part into one container with
/// `ffmpeg -c copy` (no re-encode). Parent spec §7.2.
public protocol Muxer: Sendable {
    func mux(
        videoPart: URL, audioPart: URL, into output: URL, container: MediaContainer
    ) async throws
}

public struct FFmpegMuxer: Muxer {
    private let runner: any ProcessRunner
    private let locator: BinaryLocator
    private let timeout: Duration

    public init(
        runner: any ProcessRunner, locator: BinaryLocator, timeout: Duration = .seconds(120)
    ) {
        self.runner = runner
        self.locator = locator
        self.timeout = timeout
    }

    public static func arguments(
        video: URL, audio: URL, output: URL, container: MediaContainer
    ) -> [String] {
        var args = [
            "-y", "-i", video.path, "-i", audio.path,
            "-c", "copy", "-map", "0:v:0", "-map", "1:a:0",
        ]
        if container == .mp4 { args += ["-movflags", "+faststart"] }
        args.append(output.path)
        return args
    }

    public func mux(
        videoPart: URL, audioPart: URL, into output: URL, container: MediaContainer
    ) async throws {
        guard let ffmpeg = await locator.locate("ffmpeg") else { throw MuxError.ffmpegMissing }
        let out: ProcessOutput
        do {
            out = try await runner.run(
                executable: ffmpeg,
                arguments: Self.arguments(
                    video: videoPart, audio: audioPart, output: output, container: container),
                timeout: timeout)
        } catch ProcessRunError.timedOut {
            throw MuxError.timedOut
        }
        guard out.exitCode == 0 else {
            throw MuxError.ffmpegFailed(
                stderrTail: String(String(decoding: out.stderr, as: UTF8.self).suffix(800)))
        }
    }
}
```

- [ ] **Step 5: Add the real-ffmpeg smoke test** (append to `MuxerTests.swift`)

```swift
private let ffmpegOnDisk =
    FileManager.default.isExecutableFile(atPath: "/usr/local/bin/ffmpeg")
    || FileManager.default.isExecutableFile(atPath: "/opt/homebrew/bin/ffmpeg")

@Test(.enabled(if: ffmpegOnDisk))
func realFfmpegMuxesTwoTinyStreams() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let ffmpeg = try #require(await BinaryLocator().locate("ffmpeg"))
    // Generate a 0.2s test video and a 0.2s tone with ffmpeg itself.
    let v = dir.appendingPathComponent("v.mp4")
    let a = dir.appendingPathComponent("a.m4a")
    _ = try await SystemProcessRunner().run(
        executable: ffmpeg,
        arguments: [
            "-y", "-f", "lavfi", "-i", "testsrc=duration=0.2:size=64x64:rate=10",
            "-c:v", "libx264", "-pix_fmt", "yuv420p", v.path,
        ], timeout: .seconds(30))
    _ = try await SystemProcessRunner().run(
        executable: ffmpeg,
        arguments: [
            "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=0.2",
            "-c:a", "aac", a.path,
        ], timeout: .seconds(30))
    let out = dir.appendingPathComponent("out.mp4")
    try await FFmpegMuxer(runner: SystemProcessRunner(), locator: BinaryLocator())
        .mux(videoPart: v, audioPart: a, into: out, container: .mp4)
    #expect(FileManager.default.fileExists(atPath: out.path))
    #expect((try Data(contentsOf: out)).count > 0)
}
```

- [ ] **Step 6: Run tests** — `swift test --filter 'MuxerTests'` → PASS (5 fake + 1 smoke, or 5 + skipped).

- [ ] **Step 7: Full suite, format, lint, commit**

```bash
cd SDMKit && swift test        # 362 + new
cd .. && ./format.sh && ./lint.sh
git add SDMKit/Package.swift SDMKit/Sources/SDMEngine/Muxer.swift SDMKit/Tests/SDMEngineTests/
git commit -m "feat(engine): FFmpegMuxer for -c copy assembly of video+audio parts"
```

---

## Task 2: `DownloadError.urlExpired` + refreshable-format classification

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/DownloadTask.swift`
- Modify: `SDMKit/Sources/SDMEngine/RetryPolicy.swift` (classify the new error as `.transient` so an un-refreshed expiry still retries, not loops)
- Test: `SDMKit/Tests/SDMEngineTests/DownloadTaskTests.swift` (append), `RetryPolicyTests.swift` (append)

**Interfaces:**
- Produces:
  - `DownloadError.urlExpired(formatID: String)` — a new case
  - `DownloadTask.Configuration.refreshableFormatID: String?` — `nil` for a plain HTTP component; the yt-dlp `format_id` for a `.resolved` one. Added as a trailing defaulted parameter (`= nil`) so every existing `Configuration(...)` call keeps compiling.
  - Behavior: in `prepare()` and `download()`, a `403` or `410` response status becomes `throw DownloadError.urlExpired(formatID: id)` **iff** `configuration.refreshableFormatID` is non-nil; otherwise the existing `DownloadError.serverError(status:)` path is unchanged.
  - `RetryPolicy.classify` maps `DownloadError.urlExpired` to `.transient` (so if the engine has no resolver, or refresh fails upstream, the item still backs off and eventually terminates rather than hot-looping).

- [ ] **Step 1: Write the failing tests**

```swift
// append to DownloadTaskTests.swift
@Test func expiredUrlOnARefreshableComponentThrowsUrlExpired() async {
    let origin = FakeOrigin(payload: testPayload(2000), behavior: {
        var b = FakeOrigin.Behavior(); b.statusOverride = 403; return b
    }())
    let dir = try! makeScratchDirectory()
    let task = DownloadTask(
        id: UUID(), sourceURL: URL(string: "https://gv/v137")!,
        destinationURL: dir.appendingPathComponent("v.f137.mp4"),
        transport: origin,
        configuration: DownloadTask.Configuration(
            workerCount: 1, minChunk: 64, checkpointInterval: 128,
            cachedCompleted: nil, refreshableFormatID: "137"))
    await #expect(throws: DownloadError.urlExpired(formatID: "137")) { _ = try await task.start() }
}

@Test func expiredUrlWithoutARefreshableFormatIsAPlainServerError() async {
    let origin = FakeOrigin(payload: testPayload(2000), behavior: {
        var b = FakeOrigin.Behavior(); b.statusOverride = 403; return b
    }())
    let dir = try! makeScratchDirectory()
    let task = DownloadTask(
        id: UUID(), sourceURL: URL(string: "https://x/f")!,
        destinationURL: dir.appendingPathComponent("f.bin"),
        transport: origin,
        configuration: DownloadTask.Configuration(
            workerCount: 1, minChunk: 64, checkpointInterval: 128, cachedCompleted: nil))
    await #expect(throws: DownloadError.serverError(status: 403)) { _ = try await task.start() }
}
```

```swift
// append to RetryPolicyTests.swift
@Test func urlExpiredIsTransient() {
    if case .permanent = RetryPolicy().classify(DownloadError.urlExpired(formatID: "137")) {
        Issue.record("urlExpired must classify transient")
    }
}
```

> Check `FakeOrigin.Behavior.statusOverride` actually forces the probe (`bytes=0-1`)
> response status — read `FakeOrigin.swift`. If `statusOverride` only affects body
> fetches, drive the 403 through whichever knob the probe path honours, or set
> `cachedCompleted` to a non-empty set so `start()` proceeds to a body fetch.

- [ ] **Step 2: Run to verify failure** — `swift test --filter 'expiredUrlOnARefreshableComponentThrowsUrlExpired'` → FAIL (`refreshableFormatID` unknown / wrong error).

- [ ] **Step 3: Implement**

In `DownloadTask.Configuration`: add `public var refreshableFormatID: String?`, parameter `refreshableFormatID: String? = nil` (last), `self.refreshableFormatID = refreshableFormatID`.

Add a helper in `DownloadTask`:
```swift
    private func statusError(_ status: Int) -> DownloadError {
        if (status == 403 || status == 410), let formatID = configuration.refreshableFormatID {
            return .urlExpired(formatID: formatID)
        }
        return .serverError(status: status)
    }
```
Replace the two `throw DownloadError.serverError(status: probe.statusCode)` / `throw DownloadError.serverError(status: response.statusCode)` sites with `throw statusError(probe.statusCode)` / `throw statusError(response.statusCode)`.

Add `case urlExpired(formatID: String)` to `DownloadError`.

In `RetryPolicy.classify`, add `case DownloadError.urlExpired: return .transient` (alongside the other `DownloadError` cases; match the file's existing switch style).

- [ ] **Step 4: Run** — the three new tests PASS; `swift test` full suite green (362 + 3).

- [ ] **Step 5: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDMKit/Sources/SDMEngine/DownloadTask.swift SDMKit/Sources/SDMEngine/RetryPolicy.swift SDMKit/Tests/SDMEngineTests/
git commit -m "feat(engine): classify 403/410 on a resolved component as urlExpired"
```

---

## Task 3: `DownloadEngine.Runner` holds per-component `DownloadTask`s

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/DownloadEngine.swift`
- Test: `SDMKit/Tests/SDMEngineTests/MultiComponentDownloadTests.swift` *(new)*

This is the load-bearing refactor. It is **behavior-preserving for one-component items** (proved by the 362-test suite) and adds parallel download for ≥2 components.

**Design:**

```swift
private struct ComponentRun {
    let componentID: UUID
    let task: DownloadTask
    let destinationURL: URL   // the component's .incomplete-backed final part path
}

private struct Runner {
    var components: [ComponentRun]        // one per incomplete component at start
    let job: Task<Void, Never>
    let allDestinationURLs: [URL]          // every component dest, for the claim-guard
    var retireIntent: RetireIntent = .none
}
```

- **`context(for itemID:)`** returns `[ComponentContext]` — one per component that is **not already `isComplete`** — each `{ componentID, sourceURL, destinationURL, origin, cachedCompleted }`. `destinationURL = <packageFolder>/<component.partFilename>`. The package folder creation is unchanged.
- **`reconcile()`** starts a runner for a desired item by building a `DownloadTask` per `ComponentContext` (worker count = `allocatedSegments[itemID] ?? segments`, split `max(1, N / contexts.count)` per task), collecting their `destinationURL`s into `claimed` (the "two tasks, one destination" guard now covers every component path).
- **`run(itemID:)`** awaits every component task with a `withThrowingTaskGroup`/`async let` fan-out:
  - all succeed → **assembly** (Task 5) → `.completed`.
  - one throws `.urlExpired` → **refresh** (Task 6) then restart that one component's task; siblings keep running.
  - one throws anything else → cancel siblings, `finish` with `failureState(for: error)` — the whole item fails, sibling partials stay on disk (parent spec §5.2(c)).
  - `.preempted` / `.userStopped` retire intents propagate to every component task's `pause()`.
- **`setWorkerCount`** in `reconcile()`'s re-clamp loop fans out to each component task: `task.setWorkerCount(max(1, allocated / runner.components.count))`.
- **`finish(itemID:)`** writes back per component: for each `ComponentRun`, `mutateItem { $0.components[k].completed = <task.completedRanges> ; $0.components[k].isResumable = <task.probedSupportsRanges> ; if let tb = task.expectedTotalBytes { $0.components[k].totalBytes = tb } }`. The `precondition(components.count == 1)` on `DownloadItem.completed`/`totalBytes` **setters** is now bypassed — write `components[k]` directly, never the aggregate setter. (This is the Part 2 tripwire firing as designed.)
- **`snapshot()`**: `completed` = union of each running component task's `completedRanges` shifted by `item.componentBaseOffsets[k]` (fall back to `item.completed` when no runner); `totalBytes` = `item.totalBytes` (already the sum); `activeSegments` = Σ `task.activeWorkerCount`; `checkpointFailure` = first non-nil across component tasks.
- **`relocateItemFiles` / `removeItem` / `resetDownload`**: iterate `item.components`, operating on `<folder>/<component.partFilename>` and its `.incomplete` / `.sdmpart` (plus the final `<outputFilename>` for the completed/relocate case). Replace the single `item.filename` path with the per-component loop.
- **`connectionDemands`**: unchanged for now — one demand per item keyed by `item.id`, host from `item.url` (component 0). *(Per-component host demands are a documented Part 3 follow-up: two `googlevideo` components currently count as one against the per-host cap. Acceptable — the cap is politeness, not correctness.)*

- [ ] **Step 1: Write the failing test** (`MultiComponentDownloadTests.swift`)

```swift
import Foundation
import Testing

@testable import SDMEngine

@testable import SDMCore

private func twoComponentItem(_ videoURL: URL, _ audioURL: URL) -> DownloadItem {
    DownloadItem(
        components: [
            FileComponent(
                url: videoURL, partFilename: "clip.f137.mp4", totalBytes: nil, origin: .http),
            FileComponent(
                url: audioURL, partFilename: "clip.f251.webm", totalBytes: nil, origin: .http),
        ],
        outputFilename: "clip.mp4", assembly: .none, state: .queued)
}

@Test func twoComponentsDownloadInParallelAndBothPartsMatchTheirSources() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let videoPayload = testPayload(6000)
    let audioPayload = testPayload(1500).map { $0 ^ 0x5A }.withUnsafeBytes { Data($0) }
    // FakeOrigin serves one payload; use two origins keyed by host via a router,
    // OR run two FakeOrigins behind a routing HTTPTransport. If the test infra
    // has no router, build a minimal one here: a struct HTTPTransport that
    // switches on request.url.host and forwards to the right FakeOrigin.
    let router = TwoHostRouter(
        videoHost: "v.example", videoPayload: videoPayload,
        audioHost: "a.example", audioPayload: audioPayload)
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 4, globalMaxConnections: 16, downloadFolder: dir))
    let item = twoComponentItem(
        URL(string: "https://v.example/f137")!, URL(string: "https://a.example/f251")!)
    await engine.add(DownloadPackage(name: "P", items: [item]))
    try await engine.runUntilIdle()

    let snap = try #require(await engine.snapshot().packages.first?.items.first)
    #expect(snap.state == .completed)
    // The two component part files, finalized (no .incomplete suffix):
    #expect(try Data(contentsOf: dir.appendingPathComponent("P/clip.f137.mp4")) == videoPayload)
    #expect(try Data(contentsOf: dir.appendingPathComponent("P/clip.f251.webm")) == audioPayload)
}

@Test func oneComponentPermanentlyFailingFailsTheWholeItem() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    var badBehavior = FakeOrigin.Behavior()
    badBehavior.statusOverride = 404
    let router = TwoHostRouter(
        videoHost: "v.example", videoPayload: testPayload(6000),
        audioHost: "a.example", audioPayload: testPayload(1500),
        audioBehavior: badBehavior)
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 4, globalMaxConnections: 16, downloadFolder: dir))
    await engine.add(
        DownloadPackage(
            name: "P",
            items: [
                twoComponentItem(
                    URL(string: "https://v.example/f137")!, URL(string: "https://a.example/f251")!)
            ]))
    try await engine.runUntilIdle()
    let snap = try #require(await engine.snapshot().packages.first?.items.first)
    if case .failed = snap.state {} else { Issue.record("expected .failed, got \(snap.state)") }
}
```

> `TwoHostRouter` is a small `HTTPTransport` that dispatches on `request.url.host`
> to one of two `FakeOrigin`s. Put it in `SDMEngineTests/ProcessRunnerSupport.swift`
> or a new `SDMEngineTests/RoutingTransport.swift`. Model it on how existing tests
> construct `FakeOrigin` and conform to `HTTPTransport` (`func fetch(_:) async throws
> -> RangeResponse`). Confirm `HTTPTransport`'s exact requirement in
> `Sources/SDMEngine/HTTPTransport.swift`.

- [ ] **Step 2: Run** → FAIL (item never completes / crash on aggregate setter).

- [ ] **Step 3: Implement the refactor** in `DownloadEngine.swift`, section by section, running `swift test` after each:
  1. `ComponentRun` + `Runner.components` (keep a computed `Runner.task`/`destinationURL` returning `components[0]`'s during the transition if it shortens diffs, then remove).
  2. `context(for:)` → `[ComponentContext]`.
  3. `reconcile()` start-loop: build a task per context; `claimed` over all component dests.
  4. `run(itemID:)`: fan-out await; failure/preempt/userStopped propagation; (assembly + refresh are stubs that just proceed / rethrow until Tasks 5–6).
  5. `finish(itemID:)`: per-component writeback.
  6. `snapshot()`: concatenation.
  7. `relocateItemFiles`, `removeItem`, `resetDownload`: per-component file loops.
  8. `refreshResumability()`: `mutateItem { for k in indices { $0.components[k].isResumable = <that component task's probedSupportsRanges> } }`.

- [ ] **Step 4: Run the full suite** — 362 pre-existing tests green + the 2 new (the `oneComponentPermanentlyFailingFailsTheWholeItem` may need Task 5's assembly path to be a no-op for `.none`; that is fine — assembly stub returns immediately for `.none`).

- [ ] **Step 5: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDMKit/Sources/SDMEngine/DownloadEngine.swift SDMKit/Tests/SDMEngineTests/
git commit -m "feat(engine): run one DownloadTask per component, in parallel, per item"
```

---

## Task 4: Concatenated snapshot under churn

**Files:**
- Test only: `SDMKit/Tests/SDMEngineTests/MultiComponentDownloadTests.swift` (append)

Task 3 already wrote the `snapshot()` concatenation. This task pins it with a randomized-churn property test, mirroring parent spec §11.2 / §10.3.

- [ ] **Step 1: Write the test**

```swift
@Test(arguments: [1, 7, 42, 128, 999])
func concatenatedProgressEqualsShiftedUnionOfComponentSetsUnderChurn(seed: UInt64) async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let videoPayload = testPayload(20_000)
    let audioPayload = testPayload(4000)
    let router = TwoHostRouter(
        videoHost: "v.example", videoPayload: videoPayload,
        audioHost: "a.example", audioPayload: audioPayload)
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 8, globalMaxConnections: 32, downloadFolder: dir))
    let item = DownloadItem(
        components: [
            FileComponent(url: URL(string: "https://v.example/v")!, partFilename: "c.f137.mp4"),
            FileComponent(url: URL(string: "https://a.example/a")!, partFilename: "c.f251.webm"),
        ], outputFilename: "c.mp4", assembly: .none, state: .queued)
    await engine.add(DownloadPackage(name: "P", items: [item]))

    var rng = SplitMix64(seed: seed)
    for _ in 0..<40 {
        await engine.tick()
        if Bool.random(using: &rng) {
            await engine.updateSettings(
                EngineSettings(
                    maxConcurrent: 1, segmentsPerItem: Int.random(in: 1...8, using: &rng),
                    globalMaxConnections: 32, downloadFolder: dir))
        }
        // Invariant: item-space completed == shifted union of the running
        // component tasks' completed sets (or the stored item set when idle).
        let snap = await engine.snapshot().packages.first!.items.first!
        // completed must be a subset of [0, total) and strictly sorted/disjoint
        for r in snap.completed.ranges { #expect(r.start >= 0) }
        if let total = snap.totalBytes {
            for r in snap.completed.ranges { #expect(r.end <= total) }
        }
    }
    try await engine.runUntilIdle()
    let snap = await engine.snapshot().packages.first!.items.first!
    #expect(snap.state == .completed)
    #expect(snap.completed.totalBytes == 24_000)
}
```

> Reuse the project's existing seeded RNG helper if one exists (grep the test
> tree for `SplitMix64` / `seed:` — `randomizedWorkerChurnPreservesByteIdentity`
> already uses one; use the same type). If none is exposed to this file, lift it
> into shared test support.

- [ ] **Step 2: Run** → PASS for all 5 seeds. If a seed exposes a concatenation bug, fix `snapshot()` (not the test).

- [ ] **Step 3: Full suite, format, commit**

```bash
cd SDMKit && swift test
cd .. && ./format.sh && ./lint.sh
git add SDMKit/Tests/SDMEngineTests/MultiComponentDownloadTests.swift
git commit -m "test(engine): concatenated multi-component progress holds under churn"
```

---

## Task 5: Assembly — mux on completion, retry on failure

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/DownloadEngine.swift`
- Modify: `SDMKit/Sources/SDMEngine/EngineSnapshot.swift` (`isAssembling`)
- Test: `SDMKit/Tests/SDMEngineTests/MuxAssemblyTests.swift` *(new)*

**Interfaces:**
- Consumes: `Muxer`, `MuxError` (Task 1).
- Produces:
  - `DownloadEngine.init(..., muxer: (any Muxer)? = nil)` — stored; `nil` means "assembly of a `.mux` item fails with a clear reason" (until Task 7 wires the real one). Existing `init` calls keep working (defaulted).
  - `private var assembling: Set<UUID>` — items currently in the mux step. `snapshot()` sets `ItemSnapshot.isAssembling = assembling.contains(item.id)`.
  - `ItemSnapshot.isAssembling: Bool` (defaulted `false` in `init`, like `fileMissing`).
  - `retryMux(_ itemID: UUID) async` — public. For a `.failed` item whose components are all `isComplete` and `assembly == .mux`: clears the failure, re-enters the assembly step. (Parent spec §7.2, §9.3 "Retry mux".)
  - **Assembly flow** in `run(itemID:)` after all component tasks succeed:
    - `assembly == .none` → each component task already `finalize()`d its own file; item → `.completed`. (One-component and progressive-YouTube path — unchanged.)
    - `assembly == .mux` → the two component tasks `finalize()`d their `.part` files (no `.incomplete` suffix). `assembling.insert(itemID)`; `guard let muxer else { fail("ffmpeg not configured") }`; `outputURL = <folder>/<outputFilename>`; `videoPart`/`audioPart` = the component part paths (video = the component whose `origin` formatID matches the video stream — for Part 3, **component 0 is video, component 1 is audio** by construction from the grabber; assert `components.count == 2`); `try await muxer.mux(videoPart:audioPart:into:outputURL:container: <MediaContainer from outputFilename extension>)`; on success delete both part files, `assembling.remove`, item → `.completed`; on `MuxError` → `assembling.remove`, item → `.failed(reason: "mux failed: \(tail)")`, **part files kept**.

- [ ] **Step 1: Write the failing test** (`MuxAssemblyTests.swift`)

```swift
import Foundation
import Testing

@testable import SDMEngine

@testable import SDMCore

/// Records mux calls and can be scripted to succeed (writing a stub output
/// file) or fail.
final class FakeMuxer: Muxer, @unchecked Sendable {
    private let lock = NSLock()
    private(set) var calls: [(video: URL, audio: URL, output: URL)] = []
    var behavior: @Sendable (URL) throws -> Void = { output in
        try Data("MUXED".utf8).write(to: output)
    }
    func mux(videoPart: URL, audioPart: URL, into output: URL, container: MediaContainer) async throws {
        lock.withLock { calls.append((videoPart, audioPart, output)) }
        try behavior(output)
    }
}

private func muxItem(_ v: URL, _ a: URL) -> DownloadItem {
    DownloadItem(
        components: [
            FileComponent(url: v, partFilename: "clip.f137.mp4", origin: .http),
            FileComponent(url: a, partFilename: "clip.f251.webm", origin: .http),
        ], outputFilename: "clip.mp4", assembly: .mux, state: .queued)
}

@Test func muxRunsAfterBothComponentsCompleteThenDeletesParts() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let muxer = FakeMuxer()
    let router = TwoHostRouter(
        videoHost: "v.example", videoPayload: testPayload(4000),
        audioHost: "a.example", audioPayload: testPayload(1000))
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 4, globalMaxConnections: 16, downloadFolder: dir),
        muxer: muxer)
    await engine.add(
        DownloadPackage(
            name: "P",
            items: [muxItem(URL(string: "https://v.example/v")!, URL(string: "https://a.example/a")!)]))
    try await engine.runUntilIdle()

    let snap = try #require(await engine.snapshot().packages.first?.items.first)
    #expect(snap.state == .completed)
    #expect(muxer.calls.count == 1)
    #expect(FileManager.default.fileExists(atPath: dir.appendingPathComponent("P/clip.mp4").path))
    #expect(!FileManager.default.fileExists(atPath: dir.appendingPathComponent("P/clip.f137.mp4").path))
    #expect(!FileManager.default.fileExists(atPath: dir.appendingPathComponent("P/clip.f251.webm").path))
}

@Test func muxFailureFailsTheItemAndKeepsTheParts() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let muxer = FakeMuxer()
    muxer.behavior = { _ in throw MuxError.ffmpegFailed(stderrTail: "Invalid data") }
    let router = TwoHostRouter(
        videoHost: "v.example", videoPayload: testPayload(4000),
        audioHost: "a.example", audioPayload: testPayload(1000))
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 4, globalMaxConnections: 16, downloadFolder: dir),
        muxer: muxer)
    await engine.add(
        DownloadPackage(
            name: "P",
            items: [muxItem(URL(string: "https://v.example/v")!, URL(string: "https://a.example/a")!)]))
    try await engine.runUntilIdle()

    let snap = try #require(await engine.snapshot().packages.first?.items.first)
    guard case .failed(let reason) = snap.state else { Issue.record("expected .failed"); return }
    #expect(reason.contains("mux"))
    #expect(FileManager.default.fileExists(atPath: dir.appendingPathComponent("P/clip.f137.mp4").path))

    // Retry mux — succeeds this time, no re-download.
    muxer.behavior = { output in try Data("MUXED".utf8).write(to: output) }
    await engine.retryMux(snap.id)
    let after = try #require(await engine.snapshot().packages.first?.items.first)
    #expect(after.state == .completed)
    #expect(muxer.calls.count == 2)
}
```

- [ ] **Step 2: Run** → FAIL.

- [ ] **Step 3: Implement** the assembly flow + `isAssembling` + `retryMux` per the interfaces above.

- [ ] **Step 4: Run** the two new tests + full suite (362 + Task 3/4/5 new). Note: `runUntilIdle()` does not `tick()`, and the mux step is synchronous within `run()` — confirm `runUntilIdle`'s "await each runner job" loop still terminates (the mux happens inside the job, before it returns). If `retryMux` needs a `reconcile()`/snapshot refresh, call it.

- [ ] **Step 5: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDMKit/Sources/SDMEngine/DownloadEngine.swift SDMKit/Sources/SDMEngine/EngineSnapshot.swift SDMKit/Tests/SDMEngineTests/MuxAssemblyTests.swift
git commit -m "feat(engine): mux assembly step with retry on failure"
```

---

## Task 6: 403 → silent URL refresh

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/DownloadEngine.swift`
- Test: `SDMKit/Tests/SDMEngineTests/URLRefreshTests.swift` *(new)*

**Interfaces:**
- Consumes: `LinkResolver`, `RefreshedFormat`, `ResolveError` (`SDMCore`, Part 1).
- Produces:
  - `DownloadEngine.init(..., resolver: (any LinkResolver)? = nil, muxer: (any Muxer)? = nil)`
  - When a component task throws `DownloadError.urlExpired(formatID:)` and the component's `origin` is `.resolved(extractor, videoID, formatID)`:
    1. `guard let resolver else { throw the error }` (falls through to `failureState` → transient retry).
    2. `let refreshed = try await resolver.refresh(extractor:videoID:formatID:)` — a thrown `ResolveError` → item `.failed(reason:)`.
    3. Verify `refreshed.formatID == formatID`; if the component's `totalBytes` is known, `refreshed.filesize == totalBytes`. Mismatch → item `.failed(reason: "format changed")`, keep partials.
    4. `mutateItem { $0.components[k].url = refreshed.url }`, persist, and **restart that component's `DownloadTask`** with a fresh task pointed at the new URL, `cachedCompleted:` = the component's current `completed` (so it resumes against the existing `.incomplete`/sidecar). Siblings are untouched.
    5. The refresh consumes one `failedAttempts[itemID]` increment (so a genuinely broken video still terminates via the existing cap) — reuse `failureState`'s bookkeeping or a direct `failedAttempts[itemID, default: 0] += 1` with the same `>= maxAttempts` check.

- [ ] **Step 1: Write the failing test** (`URLRefreshTests.swift`)

```swift
import Foundation
import Testing

@testable import SDMEngine

@testable import SDMCore

/// A fake resolver that hands back a URL on a different host the router also
/// knows about, so the restarted component actually downloads.
final class FakeResolver: LinkResolver, @unchecked Sendable {
    var refreshResult: @Sendable (String) throws -> RefreshedFormat
    init(_ f: @escaping @Sendable (String) throws -> RefreshedFormat) { refreshResult = f }
    func canHandle(_ url: URL) -> Bool { false }
    func resolve(_ url: URL) async throws -> ResolvedTarget { throw ResolveError.unsupported }
    func refresh(extractor: String, videoID: String, formatID: String) async throws -> RefreshedFormat {
        try refreshResult(formatID)
    }
}

@Test func expiredComponentUrlIsRefreshedAndTheDownloadFinishes() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let videoPayload = testPayload(8000)

    // `stale.example` 403s after N bytes; `fresh.example` serves the whole
    // payload. Router knows both.
    var staleBehavior = FakeOrigin.Behavior()
    staleBehavior.statusOverrideAfterBytes = (bytes: 3000, status: 403)  // add this knob to FakeOrigin if absent
    let router = TwoHostRouter(
        videoHost: "stale.example", videoPayload: videoPayload, videoBehavior: staleBehavior,
        audioHost: "fresh.example", audioPayload: videoPayload)

    let resolver = FakeResolver { formatID in
        RefreshedFormat(
            url: URL(string: "https://fresh.example/f137")!, filesize: 8000, formatID: formatID)
    }
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 2, globalMaxConnections: 8, downloadFolder: dir),
        resolver: resolver)
    let item = DownloadItem(
        components: [
            FileComponent(
                url: URL(string: "https://stale.example/f137")!, partFilename: "clip.f137.mp4",
                totalBytes: nil,
                origin: .resolved(extractor: "youtube", videoID: "abc", formatID: "137"))
        ], outputFilename: "clip.mp4", assembly: .none, state: .queued)
    await engine.add(DownloadPackage(name: "P", items: [item]))
    try await engine.runUntilIdle()

    let snap = try #require(await engine.snapshot().packages.first?.items.first)
    #expect(snap.state == .completed)
    #expect(try Data(contentsOf: dir.appendingPathComponent("P/clip.f137.mp4")) == videoPayload)
}

@Test func refreshReturningAChangedSizeFailsTheItem() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    var staleBehavior = FakeOrigin.Behavior()
    staleBehavior.statusOverride = 403
    let router = TwoHostRouter(
        videoHost: "stale.example", videoPayload: testPayload(8000), videoBehavior: staleBehavior,
        audioHost: "fresh.example", audioPayload: testPayload(8000))
    let resolver = FakeResolver { formatID in
        RefreshedFormat(
            url: URL(string: "https://fresh.example/f137")!, filesize: 9999, formatID: formatID)
    }
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 2, globalMaxConnections: 8, downloadFolder: dir),
        resolver: resolver)
    await engine.add(
        DownloadPackage(
            name: "P",
            items: [
                DownloadItem(
                    components: [
                        FileComponent(
                            url: URL(string: "https://stale.example/f137")!,
                            partFilename: "clip.f137.mp4", totalBytes: 8000,
                            origin: .resolved(extractor: "youtube", videoID: "abc", formatID: "137"))
                    ], outputFilename: "clip.mp4", state: .queued)
            ]))
    try await engine.runUntilIdle()
    let snap = try #require(await engine.snapshot().packages.first?.items.first)
    if case .failed = snap.state {} else { Issue.record("expected .failed") }
}
```

> If `FakeOrigin` has no "403 after N bytes" knob, add `statusOverrideAfterBytes:
> (bytes: Int, status: Int)?` to `FakeOrigin.Behavior` and honour it in its
> `fetch` body streaming (emit `bytes` bytes then a response whose status is
> `status` on the *next* claim, or throw a synthetic `serverError`). Keep the
> addition minimal and covered by one `FakeOriginTests` assertion.

- [ ] **Step 2: Run** → FAIL.

- [ ] **Step 3: Implement** the refresh branch in `run(itemID:)`.

- [ ] **Step 4: Run** both new tests + full suite green.

- [ ] **Step 5: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDMKit/Sources/SDMEngine/DownloadEngine.swift SDMKit/Tests/SDMEngineTests/URLRefreshTests.swift
git commit -m "feat(engine): silently refresh an expired resolved-component URL on 403"
```

---

## Task 7: Wire the real resolver + muxer into the app

**Files:**
- Modify: `SDM/EngineController.swift`
- Modify: `SDMKit/Package.swift` (the `SDM` Xcode target already links `SDMEngine`; ensure it links `SDMResolve` too — via the Xcode project, not `Package.swift`, if that is where app deps live)

**Interfaces:**
- Consumes: `YtDlpResolver`, `SystemProcessRunner`, `BinaryLocator` (Part 1); `FFmpegMuxer` (Task 1); `DownloadEngine.init(resolver:muxer:)` (Tasks 5–6).
- Produces: `EngineController` constructs, once:
  ```swift
  let processRunner = SystemProcessRunner()
  let binaryLocator = BinaryLocator()
  let resolver = YtDlpResolver(
      runner: processRunner, locator: binaryLocator,
      cookieSource: { CookieSource(rawValue: GrabberSettings.cookieSourceRaw) ?? .none },
      maxPlaylistVideos: { EngineSettingsStore.maxPlaylistVideos })
  let muxer = FFmpegMuxer(runner: processRunner, locator: binaryLocator)
  ```
  passed into `DownloadEngine(transport:stateStore:settings:resolver:muxer:)`.
  - `GrabberSettings.cookieSourceRaw` and `EngineSettingsStore.maxPlaylistVideos` do not exist yet — **Part 4 adds the Settings UI**. For Task 7, use literals: `cookieSource: { .none }`, `maxPlaylistVideos: { 50 }`, with a `// Part 4: read from Settings` comment. This keeps Part 3 self-contained and the engine fully wired.

- [ ] **Step 1** — read `SDM/EngineController.swift`'s `init` / engine construction; add the four lines and the two new `init` arguments.
- [ ] **Step 2** — `xcodebuild -scheme SDM -destination 'platform=macOS' build` → BUILD SUCCEEDED. If `SDMResolve` is not visible to the app target, add it as a linked library in the Xcode project (Frameworks, Libraries, and Embedded Content), mirroring how `SDMEngine` is linked.
- [ ] **Step 3** — `cd SDMKit && swift test` full green; the launch smoke test (`SDMTests`) still passes under `xcodebuild test` if that is part of the project's check.
- [ ] **Step 4: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDM/EngineController.swift SDM.xcodeproj
git commit -m "feat(app): wire YtDlpResolver and FFmpegMuxer into the download engine"
```

---

## Self-Review

**1. Spec coverage (Part 3 scope = parent spec §7.1–§7.3, §7.5, engine half of §5.3):**

| Spec item | Task |
|---|---|
| Per-component worker pools, parallel (§7.1) | Task 3 |
| Item = one scheduler slot; pools share the connection budget (§7.1) | Task 3 (item-keyed; per-host precision a noted follow-up) |
| `isResumable` = false if any component false (§7.1) | Part 2 getter + Task 3 `refreshResumability` |
| Assembly on all-components-complete; `-c copy`; delete parts + sidecar (§7.2) | Tasks 1 + 5 |
| Mux failure → `.failed` with stderr tail, files kept, **Retry mux** re-runs only the mux (§7.2, §9.3) | Task 5 |
| `assembling` holds the slot (§7.2) | Task 5 (`assembling` set; item stays `.running`; distinct `ItemState.assembling` deferred to Part 4) |
| 403/410 on a `.resolved` component → `resolver.refresh`, itag+size check, resume existing set (§7.3) | Tasks 2 + 6 |
| Format gone / size changed → component fails → item fails (§7.3) | Task 6 |
| Refresh consumes the `RetryPolicy` attempt cap, no separate budget (§7.3b) | Task 6 |
| `DownloadEngine.init` takes an optional `LinkResolver` (§7.5) | Task 6 |
| Concatenated item-space `completed` / `activeSegments` in the snapshot (§5.3) | Tasks 3 + 4 |
| Recorded-fixture / fake-injected tests, no network, no real clock (§10.3, §11) | Tasks 1–6 |
| Real-ffmpeg smoke skipped when absent (§10.3) | Task 1 |

**Explicitly deferred to Part 4:** distinct `ItemState.assembling` + its exhaustive-switch churn and UI; `ItemSnapshot.components` detail breakout for the details panel; `GrabberRow`/`MediaRow`; playlist expansion; handoff building multi-component items; Settings (`cookieSourceRaw`, `maxPlaylistVideos`, quality allowlists, binary path overrides); format-picker; per-component connection-demand precision.

**2. Placeholder scan:** No "TBD"/"add error handling"/"similar to". Every task has concrete code or a concrete diff description with named types and signatures. Five steps carry "verify X in the existing file, here is the fallback" notes (FakeOrigin knobs, HTTPTransport shape, SplitMix64 helper, ffmpeg `+faststart`, app link setup) — verification instructions with a stated fallback, not placeholders. Task 3's implementation is a described 8-part refactor rather than line-by-line code because it restructures a 1470-line actor; each part is independently `swift test`-checkpointed and the 362-test suite is the behaviour spec.

**3. Type consistency:** `ComponentRun`/`Runner.components` names fixed in Task 3, used in Tasks 5–6. `Muxer.mux(videoPart:audioPart:into:container:)` signature fixed in Task 1, called in Task 5, faked identically in `FakeMuxer`. `DownloadError.urlExpired(formatID:)` — one case, Task 2, matched in Task 6 and `RetryPolicy`. `DownloadTask.Configuration.refreshableFormatID` — one name, Tasks 2/3. `DownloadEngine.init(resolver:muxer:)` argument order fixed (Task 5 adds `muxer`, Task 6 adds `resolver` before it — final order: `resolver:muxer:` — Task 6's step 1 states the final signature; Task 5 must use `muxer:` as the last param and Task 6 inserts `resolver:` before it, or simplest: Task 5 adds **both** defaulted params in the final `resolver: … = nil, muxer: … = nil` order and only wires `muxer`). **Adopt the latter: Task 5 introduces both parameters in final order; Task 6 only adds behaviour.** Fix applied here.

**Known tripwire (intended):** Task 3 step 5 stops using `DownloadItem.completed`/`totalBytes` **aggregate setters** in the engine, writing `components[k]` directly — the Part 2 `precondition(components.count == 1)` on those setters is the guard that catches any missed call site.

---

## Execution Handoff

Executing inline in this session via superpowers:executing-plans, on a fresh branch (no worktree), immediately after writing.
