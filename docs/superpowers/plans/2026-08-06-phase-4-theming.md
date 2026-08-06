# SDM Phase 4 — Theming, Activation Policy & Timing Overhaul Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build spec §13 Phase 4 — the role-based theme system (§10.1), activation-policy modes and the quit-confirmation edge case (§10.2), and the Liquid Glass polish pass (§9.9) — plus three behavioral fixes requested alongside it: a shared, tunable heartbeat-rate constant that the speed sampler and every tick-counted duration derive from instead of assuming 1 Hz, smarter/prettier package-name generation for season/episode packs, and an unadded-links badge on the Linkgrabber sidebar entry.

**Architecture:** Themes are data, not code (spec §10.1) — a `Theme` value type in `SDMCore` with ~20 semantic-role hex fields, shipped as JSON resources inside `SDMCore`'s own SPM resource bundle (loaded via `Bundle.module`) rather than the Xcode app bundle literally, so every built-in theme and its WCAG-AA contrast gate stay testable with `swift test` — this project's established bar (everything but AppKit/SwiftUI glue is testable without launching an app) outweighs the spec's literal "JSON in the app bundle" wording, and the two are the same shipped artifact either way. A new `AppTiming.ticksPerSecond` constant in `SDMCore` becomes the one number every tick-counted duration in `SDMEngine` (hysteresis window, checkpoint staleness, persist debounce, retry backoff) multiplies by, so raising the heartbeat rate does not silently shrink every wall-clock guarantee to a fraction of a second. `SpeedSampler` moves from an EMA to a true rolling average of the trailing two real seconds while a download is active, and reports zero the instant it is not — no decay. Activation-policy and theme selection are small `@MainActor @Observable` controllers mirroring `EngineController`'s existing shape, injected via `@Environment` exactly like the rest of the app.

**Tech Stack:** Swift 6.2, Swift Testing, SwiftUI, AppKit (`NSApplicationDelegate`, `NSApp.setActivationPolicy`, `NSAlert`, `MenuBarExtra(isInserted:)`), Foundation `JSONDecoder`/`Bundle.module`. No third-party dependencies.

**Spec:** [docs/superpowers/specs/2026-08-03-sdm-design.md](../specs/2026-08-03-sdm-design.md). Read §5.4 (speed measurement), §9.9 (Liquid Glass), §10 (theming and activation policy) in full before starting. Also read the completed [Phase 1](2026-08-03-phase-1-engine.md), [Phase 2](2026-08-04-phase-2-grabber.md), and [Phase 3](2026-08-06-phase-3-shell.md) plans — this plan follows their exact TDD granularity and reuses Phase 1's `FakeOrigin`/`InMemoryStateStore`/`makeScratchDirectory`/`testPayload`/`WorkerGate`/`WorkerGatedOrigin` test infrastructure directly. Phase 3's own "Deferred to later phases" section names three items this plan exists to pick up: the role-based color system (every view added by Phase 2/3 uses system colors directly), activation-policy modes plus Liquid Glass, and the quit-confirmation edge case for active non-resumable downloads.

## Global Constraints

- **Deployment target stays macOS 15.0**, with Liquid Glass gated behind `if #available(macOS 26, *)` in exactly one file, per spec §9.9. Every other API this plan uses (`MenuBarExtra(isInserted:)`, `NSWindow.willCloseNotification`/`didBecomeMainNotification`, `NSApp.setActivationPolicy`, `Bundle.module`, `NSAlert`) is available at 15.0.
- **Swift tools version 6.2**, Swift 6 language mode, strict concurrency enabled. Confirmed on this machine: `swift --version` reports Apple Swift version 6.2 (swiftlang-6.2.0.19.9), target `x86_64-apple-macosx15.0`; `xcodebuild -version` reports Xcode 26.0.1 (Build 17A400); `swift-format --version` reports 603.0.0. Re-check if the implementer's toolchain differs.
- **Zero third-party dependencies.** Foundation, SwiftUI, AppKit, and the Swift standard library only.
- **Swift Testing only** (`@Test` / `#expect`). Per spec §11.7, SwiftUI/AppKit-only code (activation policy, quit confirmation, theme picker UI, Liquid Glass) gets a "build and run, verify by hand" step, matching every prior UI-wiring task in Phases 1–3 — do not invent a snapshot-testing harness for it.
- **No `SDMEngine`/`SDMCore` test may touch the network or sleep on a real clock.** Task 3 reuses `WorkerGate`/`WorkerGatedOrigin` from Phase 3's `TestSupport.swift` for any assertion needing a download mid-flight.
- **Format before every commit:** `swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests`.
- **Run tests with:** `swift test --package-path SDMKit`.
- **New files under `SDM/` need no Xcode project edit** — `SDM.xcodeproj` uses `PBXFileSystemSynchronizedRootGroup` for the `SDM` folder (confirmed in Phase 3's plan), so any `.swift` file created under `SDM/` is picked up automatically on the next build.
- **Palette attribution.** Every community theme JSON's `source` field names the project its core colors were taken from (Nord, Dracula, Solarized, Gruvbox, Catppuccin, Tokyo Night, One Dark, Rosé Pine), matching spec §10.1's "each carries its license attribution" — all eight are MIT-licensed at the time this plan was written. Text-tier colors (`textPrimary`/`textSecondary`/`textTertiary`) are **not** copied from each palette's own muted "comment" gray — see Task 7's rationale for why, and Task 9 for the WCAG-AA gate this exists to satisfy.
- **`AppTiming.ticksPerSecond` defaults to `5`.** Every task below derives its "N seconds" from this constant rather than hardcoding a tick count, specifically so this one line is the only thing to change to try `10`.

---

### Task 1: `AppTiming` — the shared heartbeat-rate constant

**Files:**
- Create: `SDMKit/Sources/SDMCore/AppTiming.swift`
- Create: `SDMKit/Tests/SDMCoreTests/AppTimingTests.swift`

**Interfaces:**
- Consumes: nothing
- Produces: `public enum AppTiming { public static let ticksPerSecond: Int }`

Every later task in this plan that touches a tick-counted duration reads this constant instead of assuming 1 tick == 1 second.

- [ ] **Step 1: Write the failing test**

`SDMKit/Tests/SDMCoreTests/AppTimingTests.swift`:

```swift
import Testing

@testable import SDMCore

@Test func ticksPerSecondIsPositive() {
    #expect(AppTiming.ticksPerSecond > 0)
}

@Test func ticksPerSecondDefaultsToFive() {
    #expect(AppTiming.ticksPerSecond == 5)
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter AppTimingTests`
Expected: FAIL — `cannot find 'AppTiming' in scope`.

- [ ] **Step 3: Implement `AppTiming`**

`SDMKit/Sources/SDMCore/AppTiming.swift`:

```swift
import Foundation

/// Single source of truth for the engine's heartbeat rate. Every mechanism
/// that counts ticks to measure real time — checkpoint staleness, the
/// hysteresis window, the persistence debounce, retry backoff, and speed-
/// sampler history length — derives its tick count from this rather than
/// assuming 1 tick == 1 second.
///
/// Raising this makes the UI redraw more often and every sparkline/graph
/// visibly smoother; it does not change how many *seconds* any of the above
/// waits, since every consumer multiplies its "N seconds" by this value
/// rather than hardcoding a tick count.
public enum AppTiming {
    /// Heartbeat ticks per second. `DownloadEngine.tick()` is driven at this
    /// rate by `EngineController`'s loop (Task 4); a slower or faster
    /// refresh is a one-line change here.
    public static let ticksPerSecond: Int = 5
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `swift test --package-path SDMKit --filter AppTimingTests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: add AppTiming, the shared heartbeat-rate constant"
```

---

### Task 2: Rewrite `SpeedSampler` — rolling two-second average, instant zero when idle

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/SpeedSampler.swift`
- Modify: `SDMKit/Tests/SDMEngineTests/SpeedSamplerTests.swift`

**Interfaces:**
- Consumes: `AppTiming.ticksPerSecond` from Task 1
- Produces:
  - `SpeedSampler.init(historyLength:averagingWindowSeconds:ticksPerSecond:)` — replaces the old `init(historyLength:smoothingFactor:)`
  - `SpeedSampler.tick()` — for an item that is actively transferring
  - `SpeedSampler.idle()` — new: for an item that is not, reports zero immediately
  - `bytesPerSecond`, `history`, `runningAverage` — same names, new semantics

The old EMA smoothing (`smoothed = factor*raw + (1-factor)*smoothed`) decays toward zero over several ticks after a download pauses, which is the "takes time to reach zero" behavior this task removes. It also computed `raw` as "bytes since last tick," which was numerically bytes/second only because the tick rate was implicitly 1 Hz — Task 1 breaks that assumption, so `tick()` now explicitly scales a tick's byte count up to a bytes/second estimate.

- [ ] **Step 1: Write the failing tests**

Replace `SDMKit/Tests/SDMEngineTests/SpeedSamplerTests.swift` in full:

```swift
import Testing

import SDMCore

@testable import SDMEngine

@Test func newSamplerReportsZero() {
    #expect(SpeedSampler().bytesPerSecond == 0)
}

@Test func firstTickReportsBytesRecordedInThatTickScaledToOneSecond() {
    var sampler = SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 1)
    sampler.record(bytes: 1000)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 1000)
}

@Test func recordAccumulatesWithinOneTick() {
    var sampler = SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 1)
    sampler.record(bytes: 400)
    sampler.record(bytes: 600)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 1000)
}

@Test func tickScalesAPartialSecondWindowUpToBytesPerSecond() {
    // At 5 ticks/second each tick covers 0.2 s, so 200 bytes in one tick is
    // 1000 bytes/s.
    var sampler = SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 5)
    sampler.record(bytes: 200)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 1000)
}

@Test func bytesPerSecondAveragesOnlyTheTrailingWindow() {
    // A 2-second window at 1 tick/second is 2 samples: the third tick's
    // 3000 must push the first tick's 1000 out of the average.
    var sampler = SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 1)
    sampler.record(bytes: 1000)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 1000)
    sampler.record(bytes: 1000)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 1000)
    sampler.record(bytes: 3000)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 2000)
}

@Test func idleReportsZeroImmediatelyRatherThanDecaying() {
    var sampler = SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 1)
    sampler.record(bytes: 1_000_000)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 1_000_000)
    sampler.idle()
    #expect(sampler.bytesPerSecond == 0)
}

@Test func tickAfterIdleResumesReportingImmediately() {
    var sampler = SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 1)
    sampler.record(bytes: 1000)
    sampler.tick()
    sampler.idle()
    #expect(sampler.bytesPerSecond == 0)
    sampler.record(bytes: 500)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 500)
}

@Test func historyRecordsRawPerSecondEstimatesInOrder() {
    var sampler = SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 1)
    sampler.record(bytes: 100)
    sampler.tick()
    sampler.record(bytes: 200)
    sampler.tick()
    #expect(sampler.history == [100, 200])
}

@Test func idleAppendsAZeroSampleToHistory() {
    var sampler = SpeedSampler(historyLength: 60, averagingWindowSeconds: 2, ticksPerSecond: 1)
    sampler.record(bytes: 100)
    sampler.tick()
    sampler.idle()
    #expect(sampler.history == [100, 0])
}

@Test func historyIsCappedAtItsLength() {
    var sampler = SpeedSampler(historyLength: 3, averagingWindowSeconds: 2, ticksPerSecond: 1)
    for value in 1...5 {
        sampler.record(bytes: Int64(value * 100))
        sampler.tick()
    }
    #expect(sampler.history == [300, 400, 500])
}

@Test func runningAverageIgnoresEmptyHistory() {
    #expect(SpeedSampler().runningAverage == 0)
}

@Test func runningAverageIsTheMeanOfHistory() {
    var sampler = SpeedSampler(historyLength: 10, averagingWindowSeconds: 2, ticksPerSecond: 1)
    sampler.record(bytes: 100)
    sampler.tick()
    sampler.record(bytes: 300)
    sampler.tick()
    #expect(sampler.runningAverage == 200)
}

@Test func defaultHistoryLengthScalesWithAppTiming() {
    var sampler = SpeedSampler()
    for value in 1...(AppTiming.ticksPerSecond * 60 + 5) {
        sampler.record(bytes: Int64(value))
        sampler.tick()
    }
    #expect(sampler.history.count == AppTiming.ticksPerSecond * 60)
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `swift test --package-path SDMKit --filter SpeedSamplerTests`
Expected: FAIL — the old `init(historyLength:smoothingFactor:)` signature does not accept `averagingWindowSeconds`/`ticksPerSecond`, and `idle()` does not exist.

- [ ] **Step 3: Rewrite `SpeedSampler`**

Replace `SDMKit/Sources/SDMEngine/SpeedSampler.swift` in full:

```swift
import SDMCore

/// Per-item speed measurement, driven by the engine's heartbeat tick
/// (`AppTiming.ticksPerSecond` Hz). See spec §5.4.
///
/// Reports a **rolling average of the last two real seconds** of transferred
/// bytes while active — not an exponential moving average, so the displayed
/// number and the sparkline agree about what "current speed" means — and
/// **snaps to zero immediately** via `idle()` once the item stops actually
/// transferring, rather than decaying toward zero over several more ticks.
public struct SpeedSampler: Sendable {
    private let historyLength: Int
    private let averagingWindowTicks: Int
    private let ticksPerSecond: Int
    private var pendingBytes: Int64 = 0
    private var samples: [Double] = []
    private var isActive = false

    /// - Parameters:
    ///   - historyLength: sparkline sample count kept, in ticks. Defaults to
    ///     60 seconds' worth at the current `AppTiming.ticksPerSecond`.
    ///   - averagingWindowSeconds: how many trailing seconds `bytesPerSecond`
    ///     averages over while active. Spec calls for "the last two
    ///     seconds".
    ///   - ticksPerSecond: injected rather than read from `AppTiming`
    ///     directly, so a test can exercise a different rate deterministically
    ///     without depending on the shared constant's current value.
    public init(
        historyLength: Int = AppTiming.ticksPerSecond * 60,
        averagingWindowSeconds: Double = 2,
        ticksPerSecond: Int = AppTiming.ticksPerSecond
    ) {
        precondition(historyLength > 0, "historyLength must be positive")
        precondition(averagingWindowSeconds > 0, "averagingWindowSeconds must be positive")
        precondition(ticksPerSecond > 0, "ticksPerSecond must be positive")
        self.historyLength = historyLength
        self.ticksPerSecond = ticksPerSecond
        self.averagingWindowTicks = Swift.max(
            1, Int(averagingWindowSeconds * Double(ticksPerSecond)))
    }

    /// Adds bytes transferred since the last tick.
    public mutating func record(bytes: Int64) {
        precondition(bytes >= 0, "bytes must be non-negative")
        pendingBytes += bytes
    }

    /// Closes the current tick's window for an item that is actively
    /// transferring. A tick covers `1 / ticksPerSecond` seconds, so the raw
    /// byte count is scaled up to a bytes/second estimate before it is
    /// appended to history.
    public mutating func tick() {
        isActive = true
        let bytesPerSecondEstimate = Double(pendingBytes) * Double(ticksPerSecond)
        pendingBytes = 0
        append(bytesPerSecondEstimate)
    }

    /// Closes the window for an item that is not currently running
    /// (stopped, queued, completed, failed). Reports zero on the very next
    /// read — no decay — and still appends a zero sample so the sparkline
    /// visibly drops rather than freezing at its last value.
    public mutating func idle() {
        isActive = false
        pendingBytes = 0
        append(0)
    }

    private mutating func append(_ value: Double) {
        samples.append(value)
        if samples.count > historyLength {
            samples.removeFirst(samples.count - historyLength)
        }
    }

    /// Mean of the trailing `averagingWindowSeconds` of samples — a running
    /// average, not an EMA. Zero immediately whenever the sampler is not
    /// active, regardless of what the window would otherwise average to.
    public var bytesPerSecond: Double {
        guard isActive, !samples.isEmpty else { return 0 }
        let window = samples.suffix(averagingWindowTicks)
        return window.reduce(0, +) / Double(window.count)
    }

    public var history: [Double] { samples }

    public var runningAverage: Double {
        guard !samples.isEmpty else { return 0 }
        return samples.reduce(0, +) / Double(samples.count)
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: FAIL at this point — `DownloadEngine.swift` still constructs `SpeedSampler(historyLength: 300, smoothingFactor: 0.4)` for `globalSampler`, which no longer compiles. That call site is fixed in Task 3; for now, confirm the failure is exactly that one compile error and nothing in `SpeedSamplerTests` itself.

Run: `swift test --package-path SDMKit --filter SpeedSamplerTests`
Expected: this alone still fails to *build* because the package doesn't compile until Task 3 fixes `DownloadEngine.swift`. Proceed to Task 3 immediately — these two tasks are not independently greenable, unlike every other task in this plan.

- [ ] **Step 5: Commit**

Do **not** commit yet — the package does not build until Task 3's `DownloadEngine.swift` fix lands. Continue directly into Task 3, then run the full suite and commit both together as instructed there.

---

### Task 3: Wire tick-scaled durations through `DownloadEngine`/`DownloadTask`

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/DownloadEngine.swift`
- Modify: `SDMKit/Sources/SDMEngine/DownloadTask.swift`
- Modify: `SDMKit/Tests/SDMEngineTests/CheckpointTickTests.swift`
- Modify: `SDMKit/Tests/SDMEngineTests/HysteresisTests.swift`
- Modify: `SDMKit/Tests/SDMEngineTests/DownloadEngineTests.swift`
- Modify: `SDMKit/Tests/SDMEngineTests/EngineRetryTests.swift`

**Interfaces:**
- Consumes: `AppTiming.ticksPerSecond` (Task 1), the new `SpeedSampler.tick()`/`.idle()` split (Task 2)
- Produces: no new public API — every existing tick-counted default now scales with `AppTiming.ticksPerSecond` instead of assuming 1 Hz

This is the task that makes Task 2's compile error go away, so its steps are interleaved with re-running the full suite rather than one isolated `--filter`.

- [ ] **Step 1: Scale `hysteresisWindowTicks`, `globalSampler`'s construction, and route samplers to `tick()`/`idle()`**

In `SDMKit/Sources/SDMEngine/DownloadEngine.swift`, change:

```swift
    private var globalSampler = SpeedSampler(historyLength: 300, smoothingFactor: 0.4)
```

to:

```swift
    private var globalSampler = SpeedSampler(
        historyLength: AppTiming.ticksPerSecond * 300, averagingWindowSeconds: 2)
```

Change:

```swift
    /// Spec §6.4: "An item started within the last ~5 s is not preempted."
    private let hysteresisWindowTicks = 5
```

to:

```swift
    /// Spec §6.4: "An item started within the last ~5 s is not preempted,"
    /// expressed in ticks at `AppTiming.ticksPerSecond` rather than a raw
    /// tick count, so raising the heartbeat rate does not shrink the window.
    private let hysteresisWindowTicks = AppTiming.ticksPerSecond * 5
```

In `tick()`, change:

```swift
        for itemID in Array(samplers.keys) { samplers[itemID]?.tick() }
        globalSampler.tick()
```

to:

```swift
        // Spec's "immediately drop to zero when not downloading": a sampler
        // whose item is not `.running` gets `idle()`, not `tick()`, so it
        // reports zero on the very next read instead of decaying toward it.
        for itemID in Array(samplers.keys) {
            if itemState(for: itemID) == .running {
                samplers[itemID]?.tick()
            } else {
                samplers[itemID]?.idle()
            }
        }
        globalSampler.tick()
```

- [ ] **Step 2: Scale `EngineSettings.persistDebounceTicks`'s default**

Change:

```swift
    /// Ticks of the 1 Hz heartbeat that must pass with no further change
    /// before durable state is written. Spec §4.2's "~2 s after the last
    /// change" debounce.
    public var persistDebounceTicks: Int
```

to:

```swift
    /// Heartbeat ticks that must pass with no further change before durable
    /// state is written. Spec §4.2's "~2 s after the last change" debounce,
    /// expressed in ticks at `AppTiming.ticksPerSecond`.
    public var persistDebounceTicks: Int
```

and change the initializer's default:

```swift
        persistDebounceTicks: Int = 2
```

to:

```swift
        persistDebounceTicks: Int = AppTiming.ticksPerSecond * 2
```

- [ ] **Step 3: Scale the retry-backoff-to-ticks conversion**

Find the doc comment and line reading:

```swift
        // `delay(forAttempt:)` is seconds; the heartbeat is 1 Hz, so seconds
        // and ticks are the same unit. At least one tick, so a sub-second
        // backoff still costs a beat rather than re-attempting immediately.
        let seconds = retryPolicy.delay(forAttempt: attempt - 1).components.seconds
        retryHoldTicks[itemID] = Swift.max(1, Int(seconds))
```

Replace with:

```swift
        // `delay(forAttempt:)` is seconds; scale by the heartbeat rate to
        // get ticks. At least one tick, so a sub-second backoff still costs
        // a beat rather than re-attempting immediately.
        let seconds = retryPolicy.delay(forAttempt: attempt - 1).components.seconds
        retryHoldTicks[itemID] = Swift.max(1, Int(seconds) * AppTiming.ticksPerSecond)
```

- [ ] **Step 4: Scale `DownloadTask.Configuration.checkpointStalenessTicks`'s default**

In `SDMKit/Sources/SDMEngine/DownloadTask.swift`, change:

```swift
        /// Elapsed 1 Hz ticks without a checkpoint before `checkpointTick()`
        /// forces one — the wall-clock half of spec §4.3's "every ~8 MB per
        /// worker or every 5 s, whichever comes first". Five seconds at 1 Hz.
        public var checkpointStalenessTicks: Int

        public init(
            workerCount: Int,
            minChunk: Int64,
            checkpointInterval: Int64,
            checkpointStalenessTicks: Int = 5
        ) {
```

to:

```swift
        /// Elapsed heartbeat ticks without a checkpoint before
        /// `checkpointTick()` forces one — the wall-clock half of spec
        /// §4.3's "every ~8 MB per worker or every 5 s, whichever comes
        /// first". Five seconds' worth of ticks at `AppTiming.ticksPerSecond`.
        public var checkpointStalenessTicks: Int

        public init(
            workerCount: Int,
            minChunk: Int64,
            checkpointInterval: Int64,
            checkpointStalenessTicks: Int = AppTiming.ticksPerSecond * 5
        ) {
```

- [ ] **Step 5: Run the full suite to see the now-outdated tests fail**

Run: `swift test --package-path SDMKit`
Expected: FAIL — the package now builds, but several tests hardcode tick counts that assumed `AppTiming.ticksPerSecond == 1`:
- `CheckpointTickTests.checkpointTickWritesSidecarAfterFiveTicks` and `checkpointTickResetsStalenessCounterAfterFiring` (assumed staleness threshold of exactly 5)
- `HysteresisTests.recentlyStartedRunningItemsSurviveAPreemptingAdditionUntilTheHysteresisWindowElapses` (assumed window of exactly 5)
- `DownloadEngineTests.tickingCheckpointsRunningDownloads`, `preemptedItemReturnsToQueuedAndResumesLater`, `unprobedRunningItemIsStillPreemptible` (same two assumptions)
- `EngineRetryTests`'s `pump(engine, ticks:)` call sites, whose margins were sized for a 1 Hz backoff

- [ ] **Step 6: Update `CheckpointTickTests.swift`**

In `checkpointTickWritesSidecarAfterFiveTicks`, replace:

```swift
    for _ in 0..<4 {
        await task.checkpointTick()
    }
    #expect(!FileManager.default.fileExists(atPath: sidecarURL.path))

    await task.checkpointTick()
    #expect(FileManager.default.fileExists(atPath: sidecarURL.path))
}
```

with:

```swift
    let staleness = AppTiming.ticksPerSecond * 5
    for _ in 0..<(staleness - 1) {
        await task.checkpointTick()
    }
    #expect(!FileManager.default.fileExists(atPath: sidecarURL.path))

    await task.checkpointTick()
    #expect(FileManager.default.fileExists(atPath: sidecarURL.path))
}
```

In `checkpointTickResetsStalenessCounterAfterFiring`, replace:

```swift
    let sidecarURL = ResumeSidecar.url(for: destination)
    for _ in 0..<5 {
        await task.checkpointTick()
    }
    #expect(FileManager.default.fileExists(atPath: sidecarURL.path))
    try FileManager.default.removeItem(at: sidecarURL)

    // If the counter weren't reset when the checkpoint fired, it would
    // already be past threshold and the very next tick would re-fire.
    for _ in 0..<4 {
        await task.checkpointTick()
    }
    #expect(!FileManager.default.fileExists(atPath: sidecarURL.path))

    await task.checkpointTick()
    #expect(FileManager.default.fileExists(atPath: sidecarURL.path))
}
```

with:

```swift
    let sidecarURL = ResumeSidecar.url(for: destination)
    let staleness = AppTiming.ticksPerSecond * 5
    for _ in 0..<staleness {
        await task.checkpointTick()
    }
    #expect(FileManager.default.fileExists(atPath: sidecarURL.path))
    try FileManager.default.removeItem(at: sidecarURL)

    // If the counter weren't reset when the checkpoint fired, it would
    // already be past threshold and the very next tick would re-fire.
    for _ in 0..<(staleness - 1) {
        await task.checkpointTick()
    }
    #expect(!FileManager.default.fileExists(atPath: sidecarURL.path))

    await task.checkpointTick()
    #expect(FileManager.default.fileExists(atPath: sidecarURL.path))
}
```

- [ ] **Step 7: Update `HysteresisTests.swift`**

Replace:

```swift
    for _ in 0..<5 { await engine.tick() }

    // Window elapsed: B now preempts A.
```

with:

```swift
    for _ in 0..<(AppTiming.ticksPerSecond * 5) { await engine.tick() }

    // Window elapsed: B now preempts A.
```

- [ ] **Step 8: Update `DownloadEngineTests.swift`**

In `tickingCheckpointsRunningDownloads`, replace:

```swift
    for _ in 0..<4 { await engine.tick() }
    #expect(!FileManager.default.fileExists(atPath: sidecarURL.path))

    await engine.tick()
    #expect(FileManager.default.fileExists(atPath: sidecarURL.path))
```

with:

```swift
    for _ in 0..<(AppTiming.ticksPerSecond * 5 - 1) { await engine.tick() }
    #expect(!FileManager.default.fileExists(atPath: sidecarURL.path))

    await engine.tick()
    #expect(FileManager.default.fileExists(atPath: sidecarURL.path))
```

In `preemptedItemReturnsToQueuedAndResumesLater`, replace:

```swift
    // Past the hysteresis window (spec §6.4), so the urgent package below is
    // free to preempt it — this test is about preservation of progress
    // across a preemption, not about hysteresis itself (see HysteresisTests).
    for _ in 0..<5 { await engine.tick() }
```

with:

```swift
    // Past the hysteresis window (spec §6.4), so the urgent package below is
    // free to preempt it — this test is about preservation of progress
    // across a preemption, not about hysteresis itself (see HysteresisTests).
    for _ in 0..<(AppTiming.ticksPerSecond * 5) { await engine.tick() }
```

In `unprobedRunningItemIsStillPreemptible`, replace:

```swift
    // Past the hysteresis window (spec §6.4) so the urgent package below is
    // free to preempt it — this test is about preemptibility while unprobed,
    // not about hysteresis itself (see HysteresisTests).
    for _ in 0..<5 { await engine.tick() }
```

with:

```swift
    // Past the hysteresis window (spec §6.4) so the urgent package below is
    // free to preempt it — this test is about preemptibility while unprobed,
    // not about hysteresis itself (see HysteresisTests).
    for _ in 0..<(AppTiming.ticksPerSecond * 5) { await engine.tick() }
```

- [ ] **Step 9: Update `EngineRetryTests.swift`**

Replace each of the four `pump(engine, ticks: N)` call sites' literal `N` with `N * AppTiming.ticksPerSecond`:

```swift
    try await pump(engine, ticks: 30)
```
→
```swift
    try await pump(engine, ticks: 30 * AppTiming.ticksPerSecond)
```

```swift
    try await pump(engine, ticks: 10)
```
(both occurrences)
→
```swift
    try await pump(engine, ticks: 10 * AppTiming.ticksPerSecond)
```

```swift
    try await pump(engine, ticks: 20)
```
→
```swift
    try await pump(engine, ticks: 20 * AppTiming.ticksPerSecond)
```

The single un-pumped `await engine.tick()` in `anItemHeldInBackoffResumesOnceTheOriginRecovers` (testing "ticking once must not re-attempt it") is unaffected — one tick is still nowhere near any backoff threshold regardless of `AppTiming.ticksPerSecond`.

- [ ] **Step 10: Run the full suite to verify everything passes**

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite — this also confirms Task 2's `SpeedSampler` rewrite compiles cleanly against `DownloadEngine.swift`.

- [ ] **Step 11: Commit both Task 2 and Task 3 together**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: rewrite SpeedSampler as a rolling average and scale every tick-counted duration by AppTiming"
```

---

### Task 4: `EngineController`'s heartbeat runs at `AppTiming.ticksPerSecond` Hz

**Files:**
- Modify: `SDM/EngineController.swift`

**Interfaces:**
- Consumes: `AppTiming.ticksPerSecond` (Task 1)
- Produces: no new API — the heartbeat loop's sleep interval changes

This is the app-layer half of the timing overhaul: nothing in `SDMKit` schedules `DownloadEngine.tick()` on its own, so the actual refresh rate is entirely a function of how often `EngineController` calls it. Per spec §11.7 this is UI-wiring glue with no automated test.

- [ ] **Step 1: Change the heartbeat's sleep interval**

In `SDM/EngineController.swift`, in `startHeartbeat()`, replace:

```swift
        while !Task.isCancelled {
            await engine.tick()
            snapshot = await engine.snapshot()
            notifyChanges(from: previousSnapshot, to: snapshot)
            previousSnapshot = snapshot
            try? await Task.sleep(for: .seconds(1))
        }
```

with:

```swift
        while !Task.isCancelled {
            await engine.tick()
            snapshot = await engine.snapshot()
            notifyChanges(from: previousSnapshot, to: snapshot)
            previousSnapshot = snapshot
            try? await Task.sleep(for: .seconds(1.0 / Double(AppTiming.ticksPerSecond)))
        }
```

Also update the doc comment above `startHeartbeat()`, which currently says *"Loads durable state and runs the engine's 1 Hz heartbeat"* — change "1 Hz" to "`AppTiming.ticksPerSecond` Hz" there and anywhere else in this file's comments that says "1 Hz"/"once per second".

- [ ] **Step 2: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED.

Run the app (`⌘R`) and confirm by hand:
- Start a download of a reasonably large file. The segmented progress bar, percentage, and speed figure now visibly update about 5 times a second rather than once — noticeably livelier than before.
- Pause the download. The speed figure drops to `Zero KB/s` immediately, not after a second or two of decay.
- Watch the bandwidth graph in the sidebar's Overview section: it should redraw at the same faster cadence.

- [ ] **Step 3: Commit**

```bash
git add SDM/EngineController.swift
git commit -m "feat: drive the app heartbeat at AppTiming.ticksPerSecond instead of a hardcoded 1 Hz"
```

---

### Task 5: Smarter, prettier package names for season/episode packs

**Files:**
- Modify: `SDMKit/Sources/SDMGrabber/PackageClustering.swift`
- Modify: `SDMKit/Tests/SDMGrabberTests/PackageClusteringTests.swift`

**Interfaces:**
- Consumes: nothing new
- Produces: no new public API — `PackageClustering.cluster(_:)`'s output `name` field changes for season/episode packs and for any name containing `.`/`_`/`-` separators

Two independent problems, both visible in the existing (already-passing, now-wrong) fixture `episodesWithTheSameTemplateClusterTogether`, which currently asserts `candidates[0].name == "Show.S01E0"`:

1. **The naive longest-common-prefix stops mid-token.** `Show.S01E01.1080p.mkv` and `Show.S01E02.1080p.mkv` diverge at the very next character after `"E0"`, so the raw character-by-character common prefix is the meaningless `"Show.S01E0"` — for a bare `S01E01.mkv` / `S01E02.mkv` pair with no show-name prefix at all, it is worse: just `"S0"`. A season/episode-aware prefix detector fixes this by keeping the whole `"S01"` token and dropping only the episode-specific remainder.
2. **Dots and dashes read as a filename, not a title.** `beautify(_:)` turns separators into spaces and title-cases any word that was entirely lowercase, leaving already-mixed-case or all-caps words (`"1080p"`, `"BluRay"`, `"NASA"`) untouched rather than risking mangling them.

- [ ] **Step 1: Write the failing tests**

In `SDMKit/Tests/SDMGrabberTests/PackageClusteringTests.swift`, update the existing test:

```swift
@Test func episodesWithTheSameTemplateClusterTogether() {
    let e1 = link("Show.S01E01.1080p.mkv")
    let e2 = link("Show.S01E02.1080p.mkv")
    let candidates = PackageClustering.cluster([e1, e2])

    #expect(candidates.count == 1)
    #expect(candidates[0].name == "Show S01")
    #expect(Set(candidates[0].linkIDs) == Set([e1.id, e2.id]))
    #expect(candidates[0].isArchive == false)
}
```

and:

```swift
@Test func archivePartsLockTogetherRegardlessOfTemplate() {
    let parts = [
        link("Movie.part01.rar"),
        link("Movie.part02.rar"),
        link("Movie.part03.rar"),
    ]
    let candidates = PackageClustering.cluster(parts)

    #expect(candidates.count == 1)
    #expect(candidates[0].name == "Movie")
    #expect(candidates[0].isArchive == true)
    #expect(Set(candidates[0].linkIDs) == Set(parts.map(\.id)))
}
```

Add two new tests, after `episodesWithTheSameTemplateClusterTogether`:

```swift
@Test func seasonEpisodePatternKeepsTheFullSeasonNumberNotJustItsFirstDigit() {
    // Regression case: a naive character-by-character common prefix over
    // "S01E01.1080p" / "S01E02.1080p" stops at "S0" — the very first digit
    // where the two diverge — which is meaningless. This must keep "S01"
    // whole and drop only the episode-specific remainder.
    let e1 = link("S01E01.1080p.mkv")
    let e2 = link("S01E02.1080p.mkv")
    let candidates = PackageClustering.cluster([e1, e2])

    #expect(candidates.count == 1)
    #expect(candidates[0].name == "S01")
}

@Test func dotsAndDashesInAPackageNameBecomeSpaces() {
    let a = link("the.matrix.1999.bluray.mkv")
    let b = link("the.matrix.1999.bluray.nfo")
    let candidates = PackageClustering.cluster([a, b])

    #expect(candidates.count == 1)
    #expect(candidates[0].name == "The Matrix 1999 Bluray")
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `swift test --package-path SDMKit --filter PackageClusteringTests`
Expected: FAIL — `episodesWithTheSameTemplateClusterTogether` still expects the old `"Show.S01E0"` (now updated to expect `"Show S01"`, so it fails against current code), `archivePartsLockTogetherRegardlessOfTemplate` fails (`"movie"` vs. expected `"Movie"`), and the two new tests fail outright.

- [ ] **Step 3: Add `seasonEpisodePrefix(for:)` and `beautify(_:)`, and wire them into `name(for:)`**

In `SDMKit/Sources/SDMGrabber/PackageClustering.swift`, add two new private static functions after `commonPrefix(_:_:)`:

```swift
    /// Detects a shared "S01E0x" style season/episode structure across
    /// every member's stem and, when found, returns the name up through the
    /// season token — "Show.S01E01" and "Show.S01E02" name as "Show.S01",
    /// not the meaningless "Show.S01E0" a naive character-by-character
    /// common prefix produces, since that stops the instant the episode
    /// digits diverge, mid-token.
    private static func seasonEpisodePrefix(for stems: [String]) -> String? {
        let pattern = #"(?i)^(.*?)(s\d{1,2})e\d{1,2}.*$"#
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return nil }

        var sharedLeading: String?
        var sharedSeason: String?
        for stem in stems {
            let range = NSRange(stem.startIndex..<stem.endIndex, in: stem)
            guard let match = regex.firstMatch(in: stem, range: range),
                let leadingRange = Range(match.range(at: 1), in: stem),
                let seasonRange = Range(match.range(at: 2), in: stem)
            else { return nil }
            let leading = String(stem[leadingRange])
            let season = String(stem[seasonRange]).uppercased()
            if let currentLeading = sharedLeading, let currentSeason = sharedSeason {
                guard currentLeading.caseInsensitiveCompare(leading) == .orderedSame,
                    currentSeason == season
                else { return nil }
            } else {
                sharedLeading = leading
                sharedSeason = season
            }
        }
        guard let sharedLeading, let sharedSeason else { return nil }
        return sharedLeading + sharedSeason
    }

    /// Replaces `.`/`_`/`-` separators with spaces and collapses runs of
    /// them, so a package name reads like a title instead of a filename.
    /// Only capitalizes words that are entirely lowercase — "1080p",
    /// "BluRay", and "NASA" are left exactly as the source spelled them,
    /// since re-casing an already-mixed-case or all-caps word is as likely
    /// to mangle it as improve it.
    private static func beautify(_ raw: String) -> String {
        let separators = CharacterSet(charactersIn: "._-")
        let words = raw.components(separatedBy: separators).filter { !$0.isEmpty }
        let capitalized = words.map { word -> String in
            guard word == word.lowercased() else { return word }
            return word.prefix(1).uppercased() + word.dropFirst()
        }
        return capitalized.joined(separator: " ")
    }
```

Replace `name(for:)` in full:

```swift
    /// Cleaned longest common prefix of member stems, falling back to the
    /// host. Tries the season/episode-aware prefix first, since a naive
    /// character-by-character common prefix mangles that specific pattern.
    private static func name(for members: [ClusterableLink]) -> String {
        let stems = members.map { stripExtension($0.filename) }
        guard !stems.isEmpty else { return "Package" }

        if let seasonPrefix = seasonEpisodePrefix(for: stems) {
            return beautify(seasonPrefix)
        }

        guard var prefix = stems.first else { return members.first?.host ?? "Package" }
        for stem in stems.dropFirst() {
            prefix = commonPrefix(prefix, stem)
            if prefix.isEmpty { break }
        }
        let cleaned = prefix.trimmingCharacters(in: CharacterSet(charactersIn: " ._-"))
        guard !cleaned.isEmpty else { return members.first?.host ?? "Package" }
        return beautify(cleaned)
    }
```

In `cluster(_:)`, the archive-group naming branch currently reads:

```swift
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
```

Change the `name:` line to beautify the cleaned base too:

```swift
        for (base, members) in archiveGroups {
            let cleaned = base.trimmingCharacters(in: CharacterSet(charactersIn: " ._-"))
            candidates.append(
                PackageCandidate(
                    name: cleaned.isEmpty ? name(for: members) : beautify(cleaned),
                    linkIDs: members.map(\.id),
                    isArchive: true
                )
            )
        }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite. Confirm in particular that `singletonTemplatesGroupByHostAndPath` (name falls back to the raw host `"cdn.example.com"`, never passed through `beautify`) and `dissimilarTemplatesOnDifferentHostsStaySeparate` are unaffected.

- [ ] **Step 5: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: keep the full season number and beautify separators in generated package names"
```

---

### Task 6: Overview "No running downloads" vs. "Zero KB/s", and a Linkgrabber sidebar badge

**Files:**
- Modify: `SDM/MainWindowView.swift`

**Interfaces:**
- Consumes: `GrabberController.snapshot.totalCount` (existing, already used by `MenuBarPopoverView`'s "N links waiting" row)
- Produces: no new API — two small UI changes

Today `statsBlock` always calls `formatted(controller.snapshot.globalBytesPerSecond)`, which already renders `"Zero KB/s"` when the figure is genuinely zero (`ByteCountFormatter` renders 0 bytes as `"Zero KB"`, and `formatted(_:)` appends `"/s"`) — the missing case is distinguishing that from *nothing running at all*, which should read differently.

- [ ] **Step 1: Split "no downloads" from "zero speed" in the Overview stats block**

In `SDM/MainWindowView.swift`, replace `statsBlock`:

```swift
    private var statsBlock: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(formatted(controller.snapshot.globalBytesPerSecond)).font(
                .headline.monospacedDigit())
            BandwidthGraph(history: controller.snapshot.globalHistory).frame(height: 40)
            Text("\(activeCount) active").font(.caption).foregroundStyle(.secondary)
        }
        .padding(.vertical, 4)
    }
```

with:

```swift
    private var statsBlock: some View {
        VStack(alignment: .leading, spacing: 4) {
            if activeCount == 0 {
                Text("No running downloads")
                    .font(.headline)
                    .foregroundStyle(.secondary)
            } else {
                Text(formatted(controller.snapshot.globalBytesPerSecond)).font(
                    .headline.monospacedDigit())
            }
            BandwidthGraph(history: controller.snapshot.globalHistory).frame(height: 40)
            Text("\(activeCount) active").font(.caption).foregroundStyle(.secondary)
        }
        .padding(.vertical, 4)
    }
```

- [ ] **Step 2: Badge the Linkgrabber sidebar entry with its unadded-link count**

In the same file's `body`, find:

```swift
                Label("Linkgrabber", systemImage: "link").tag(SidebarItem.linkgrabber)
```

Replace with:

```swift
                Label("Linkgrabber", systemImage: "link")
                    .tag(SidebarItem.linkgrabber)
                    .badge(grabberController.snapshot.totalCount)
```

`grabberController.snapshot.totalCount` is exactly "links the grabber is holding that have not yet been handed to the download engine" — `LinkGrabberView.addToDownloads(_:startImmediately:)` calls `controller.removeLink(_:)` for every link the moment it is added, so a link only counts here while it is genuinely still waiting. SwiftUI's `.badge(_:)` on an `Int` hides itself automatically at `0`, so no conditional is needed.

- [ ] **Step 3: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED.

Run the app and confirm by hand:
- With no downloads added, the sidebar's Overview section reads "No running downloads" rather than "Zero KB/s".
- Start a download against a slow or throttled source (or simply glance at it mid-negotiation before the first byte arrives): the Overview should read "Zero KB/s", not "No running downloads", while it is `.running` with nothing yet received.
- Paste a few links into the Linkgrabber. The sidebar's "Linkgrabber" row shows a numeric badge matching the link count. Click "Add to downloads" on a package — the badge count drops by that package's link count. Add every link — the badge disappears.

- [ ] **Step 4: Commit**

```bash
git add SDM/MainWindowView.swift
git commit -m "feat: distinguish no-running-downloads from zero-speed in the Overview, badge unadded Linkgrabber links"
```

---

### Task 7: `Theme` value type and `ContrastRatio` — pure, testable, no UI dependency

**Files:**
- Create: `SDMKit/Sources/SDMCore/Theme.swift`
- Create: `SDMKit/Sources/SDMCore/ContrastRatio.swift`
- Create: `SDMKit/Tests/SDMCoreTests/ThemeTests.swift`
- Create: `SDMKit/Tests/SDMCoreTests/ContrastRatioTests.swift`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `public struct Theme: Codable, Equatable, Sendable, Identifiable` — the ~20 semantic roles from spec §10.1, as `#RRGGBB` hex strings
  - `public enum ContrastRatio { static func between(_:_:) -> Double; static func passesAA(_:_:) -> Bool }`

Spec §10.1: "A `Theme` value type maps ~20 semantic roles... Views reference roles only, never literal colors." `Theme` stays hex-string-based rather than importing `SwiftUI`/`AppKit`, matching `SDMCore`'s existing "domain models, no I/O, no platform dependency" charter — Task 11 adds a `Color(hex:)` bridge in the app layer, not here.

The WCAG-AA contrast gate (spec §10.1: "A test asserts WCAG AA contrast for every text-on-surface pair in every bundled theme") needs real math, not a placeholder — `ContrastRatio` implements the WCAG 2.1 relative-luminance and contrast-ratio formulas directly.

- [ ] **Step 1: Write the failing `ContrastRatio` tests**

`SDMKit/Tests/SDMCoreTests/ContrastRatioTests.swift`:

```swift
import Testing

@testable import SDMCore

@Test func blackOnWhiteHasTheMaximumContrastRatio() {
    let ratio = ContrastRatio.between("#000000", "#FFFFFF")
    #expect(abs(ratio - 21.0) < 0.01)
}

@Test func identicalColorsHaveARatioOfOne() {
    let ratio = ContrastRatio.between("#808080", "#808080")
    #expect(abs(ratio - 1.0) < 0.01)
}

@Test func contrastRatioIsSymmetric() {
    #expect(ContrastRatio.between("#000000", "#FFFFFF") == ContrastRatio.between("#FFFFFF", "#000000"))
}

@Test func blackOnWhitePassesAA() {
    #expect(ContrastRatio.passesAA("#000000", "#FFFFFF"))
}

@Test func identicalGraysFailAA() {
    #expect(!ContrastRatio.passesAA("#808080", "#808080"))
}

@Test func hexWithoutALeadingHashIsAccepted() {
    #expect(ContrastRatio.between("000000", "FFFFFF") == ContrastRatio.between("#000000", "#FFFFFF"))
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `swift test --package-path SDMKit --filter ContrastRatioTests`
Expected: FAIL — `cannot find 'ContrastRatio' in scope`.

- [ ] **Step 3: Implement `ContrastRatio`**

`SDMKit/Sources/SDMCore/ContrastRatio.swift`:

```swift
import Foundation

/// WCAG 2.1 contrast-ratio math over sRGB hex colors, used to gate every
/// bundled theme's text-on-surface pairs at AA level (spec §10.1). Pure so
/// it is testable without touching AppKit/SwiftUI color types.
public enum ContrastRatio {
    /// The WCAG contrast ratio between two `#RRGGBB` hex colors, from `1`
    /// (identical) to `21` (black on white).
    public static func between(_ hexA: String, _ hexB: String) -> Double {
        let luminanceA = relativeLuminance(of: hexA)
        let luminanceB = relativeLuminance(of: hexB)
        let lighter = Swift.max(luminanceA, luminanceB)
        let darker = Swift.min(luminanceA, luminanceB)
        return (lighter + 0.05) / (darker + 0.05)
    }

    /// WCAG AA for normal text: a contrast ratio of at least 4.5:1.
    public static func passesAA(_ hexA: String, _ hexB: String) -> Bool {
        between(hexA, hexB) >= 4.5
    }

    private static func relativeLuminance(of hex: String) -> Double {
        let (r, g, b) = components(of: hex)
        func linearize(_ channel: Double) -> Double {
            channel <= 0.03928 ? channel / 12.92 : pow((channel + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * linearize(r) + 0.7152 * linearize(g) + 0.0722 * linearize(b)
    }

    private static func components(of hex: String) -> (Double, Double, Double) {
        var cleaned = hex.trimmingCharacters(in: .whitespaces)
        if cleaned.hasPrefix("#") { cleaned.removeFirst() }
        precondition(cleaned.count == 6, "expected a #RRGGBB hex color, got \(hex)")
        let scanner = Scanner(string: cleaned)
        var value: UInt64 = 0
        scanner.scanHexInt64(&value)
        let r = Double((value & 0xFF0000) >> 16) / 255
        let g = Double((value & 0x00FF00) >> 8) / 255
        let b = Double(value & 0x0000FF) / 255
        return (r, g, b)
    }
}
```

- [ ] **Step 4: Run the `ContrastRatio` tests to verify they pass**

Run: `swift test --package-path SDMKit --filter ContrastRatioTests`
Expected: PASS.

- [ ] **Step 5: Write the failing `Theme` test**

`SDMKit/Tests/SDMCoreTests/ThemeTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMCore

@Test func themeRoundTripsThroughJSON() throws {
    let theme = Theme(
        id: "test", name: "Test", isDark: true, source: "SDM",
        surfacePrimary: "#111111", surfaceSecondary: "#222222", surfaceTertiary: "#333333",
        textPrimary: "#FFFFFF", textSecondary: "#DDDDDD", textTertiary: "#BBBBBB",
        accent: "#4488FF", border: "#444444",
        statusOnline: "#33CC66", statusFaulty: "#FFAA33", statusOffline: "#888888",
        statusFailed: "#FF4444",
        progressFill: "#4488FF", completedSegmentFill: "#33CC66", activeHeadTint: "#66CCFF",
        graphStroke: "#4488FF", graphAverageStroke: "#AA66FF",
        sidebarBackground: "#0A0A0A", selectionTint: "#4488FF"
    )
    let data = try JSONEncoder().encode(theme)
    let decoded = try JSONDecoder().decode(Theme.self, from: data)
    #expect(decoded == theme)
}
```

- [ ] **Step 6: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter ThemeTests`
Expected: FAIL — `cannot find 'Theme' in scope`.

- [ ] **Step 7: Implement `Theme`**

`SDMKit/Sources/SDMCore/Theme.swift`:

```swift
import Foundation

/// One of spec §10.1's ~20 semantic roles, resolved to a color per theme.
/// Views read roles, never literal colors, so a new theme is a new JSON
/// file with no view code touched. Colors are `#RRGGBB` hex strings rather
/// than a platform color type, keeping this pure and dependency-free like
/// every other `SDMCore` model — `Theme`'s app-layer counterpart bridges
/// each field to `SwiftUI.Color`.
///
/// Text-tier colors (`textPrimary`/`textSecondary`/`textTertiary`) are
/// deliberately shared across every dark-appearance theme, and separately
/// across every light-appearance theme, rather than one independent set per
/// palette — see `ThemeCatalog`'s bundled JSON for the values and Task 9's
/// WCAG-AA contrast test for why: a palette's own "muted"/"comment" color is
/// often tuned for a code editor's de-emphasis, not spec §10.1's AA gate.
public struct Theme: Codable, Equatable, Sendable, Identifiable {
    public var id: String
    public var name: String
    public var isDark: Bool
    /// Attribution for this theme's core palette, e.g. "Nord (MIT)" or
    /// "SDM" for the four built-ins that are not sourced from anywhere
    /// else. Spec §10.1: "each carries its license attribution."
    public var source: String

    public var surfacePrimary: String
    public var surfaceSecondary: String
    public var surfaceTertiary: String
    public var textPrimary: String
    public var textSecondary: String
    public var textTertiary: String
    public var accent: String
    public var border: String
    public var statusOnline: String
    public var statusFaulty: String
    public var statusOffline: String
    public var statusFailed: String
    public var progressFill: String
    public var completedSegmentFill: String
    public var activeHeadTint: String
    public var graphStroke: String
    public var graphAverageStroke: String
    public var sidebarBackground: String
    public var selectionTint: String

    public init(
        id: String,
        name: String,
        isDark: Bool,
        source: String,
        surfacePrimary: String,
        surfaceSecondary: String,
        surfaceTertiary: String,
        textPrimary: String,
        textSecondary: String,
        textTertiary: String,
        accent: String,
        border: String,
        statusOnline: String,
        statusFaulty: String,
        statusOffline: String,
        statusFailed: String,
        progressFill: String,
        completedSegmentFill: String,
        activeHeadTint: String,
        graphStroke: String,
        graphAverageStroke: String,
        sidebarBackground: String,
        selectionTint: String
    ) {
        self.id = id
        self.name = name
        self.isDark = isDark
        self.source = source
        self.surfacePrimary = surfacePrimary
        self.surfaceSecondary = surfaceSecondary
        self.surfaceTertiary = surfaceTertiary
        self.textPrimary = textPrimary
        self.textSecondary = textSecondary
        self.textTertiary = textTertiary
        self.accent = accent
        self.border = border
        self.statusOnline = statusOnline
        self.statusFaulty = statusFaulty
        self.statusOffline = statusOffline
        self.statusFailed = statusFailed
        self.progressFill = progressFill
        self.completedSegmentFill = completedSegmentFill
        self.activeHeadTint = activeHeadTint
        self.graphStroke = graphStroke
        self.graphAverageStroke = graphAverageStroke
        self.sidebarBackground = sidebarBackground
        self.selectionTint = selectionTint
    }
}
```

- [ ] **Step 8: Run the test to verify it passes**

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite.

- [ ] **Step 9: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: add the Theme value type and WCAG contrast-ratio math"
```

---

### Task 8: `ThemeCatalog` — package resources and the four SDM-original built-ins

**Files:**
- Modify: `SDMKit/Package.swift`
- Create: `SDMKit/Sources/SDMCore/ThemeCatalog.swift`
- Create: `SDMKit/Sources/SDMCore/Resources/Themes/light.json`
- Create: `SDMKit/Sources/SDMCore/Resources/Themes/dark.json`
- Create: `SDMKit/Sources/SDMCore/Resources/Themes/midnight-blue.json`
- Create: `SDMKit/Sources/SDMCore/Resources/Themes/deep-purple.json`
- Create: `SDMKit/Tests/SDMCoreTests/ThemeCatalogTests.swift`

**Interfaces:**
- Consumes: `Theme` (Task 7)
- Produces: `public enum ThemeCatalog { static func builtInThemes() -> [Theme] }`

Every built-in theme's `id` is written in *this* task or Task 9 and never again — `"light"` and `"dark"` in particular are load-bearing IDs that `ThemeStore` (Task 10) resolves the `"System"` selection against.

- [ ] **Step 1: Declare `SDMCore`'s resource bundle**

In `SDMKit/Package.swift`, change:

```swift
        .target(name: "SDMCore"),
```

to:

```swift
        .target(name: "SDMCore", resources: [.process("Resources")]),
```

- [ ] **Step 2: Write the failing test**

`SDMKit/Tests/SDMCoreTests/ThemeCatalogTests.swift`:

```swift
import Testing

@testable import SDMCore

@Test func builtInThemesIncludesLightAndDark() {
    let themes = ThemeCatalog.builtInThemes()
    #expect(themes.contains { $0.id == "light" })
    #expect(themes.contains { $0.id == "dark" })
}

@Test func everyBuiltInThemeHasAUniqueID() {
    let ids = ThemeCatalog.builtInThemes().map(\.id)
    #expect(Set(ids).count == ids.count)
}

@Test func lightThemeIsNotDarkAndDarkThemeIsDark() {
    let themes = ThemeCatalog.builtInThemes()
    #expect(themes.first { $0.id == "light" }?.isDark == false)
    #expect(themes.first { $0.id == "dark" }?.isDark == true)
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter ThemeCatalogTests`
Expected: FAIL — `cannot find 'ThemeCatalog' in scope`.

- [ ] **Step 4: Add the four JSON theme files**

`SDMKit/Sources/SDMCore/Resources/Themes/light.json`:

```json
{
  "id": "light",
  "name": "Light",
  "isDark": false,
  "source": "SDM",
  "surfacePrimary": "#FFFFFF",
  "surfaceSecondary": "#F5F5F7",
  "surfaceTertiary": "#E8E8ED",
  "textPrimary": "#17171A",
  "textSecondary": "#47474B",
  "textTertiary": "#5F5F63",
  "accent": "#007AFF",
  "border": "#D2D2D7",
  "statusOnline": "#34C759",
  "statusFaulty": "#FF9500",
  "statusOffline": "#8E8E93",
  "statusFailed": "#FF3B30",
  "progressFill": "#007AFF",
  "completedSegmentFill": "#30D158",
  "activeHeadTint": "#32ADE6",
  "graphStroke": "#007AFF",
  "graphAverageStroke": "#5E5CE6",
  "sidebarBackground": "#F5F5F7",
  "selectionTint": "#0A84FF"
}
```

`SDMKit/Sources/SDMCore/Resources/Themes/dark.json`:

```json
{
  "id": "dark",
  "name": "Dark",
  "isDark": true,
  "source": "SDM",
  "surfacePrimary": "#1C1C1E",
  "surfaceSecondary": "#2C2C2E",
  "surfaceTertiary": "#3A3A3C",
  "textPrimary": "#FFFFFF",
  "textSecondary": "#DCDCE1",
  "textTertiary": "#C4C4CA",
  "accent": "#0A84FF",
  "border": "#48484A",
  "statusOnline": "#30D158",
  "statusFaulty": "#FF9F0A",
  "statusOffline": "#8E8E93",
  "statusFailed": "#FF453A",
  "progressFill": "#0A84FF",
  "completedSegmentFill": "#32D74B",
  "activeHeadTint": "#64D2FF",
  "graphStroke": "#0A84FF",
  "graphAverageStroke": "#5E5CE6",
  "sidebarBackground": "#000000",
  "selectionTint": "#0A84FF"
}
```

`SDMKit/Sources/SDMCore/Resources/Themes/midnight-blue.json`:

```json
{
  "id": "midnight-blue",
  "name": "Midnight Blue",
  "isDark": true,
  "source": "SDM",
  "surfacePrimary": "#0B1929",
  "surfaceSecondary": "#122436",
  "surfaceTertiary": "#1B3349",
  "textPrimary": "#FFFFFF",
  "textSecondary": "#DCDCE1",
  "textTertiary": "#C4C4CA",
  "accent": "#4DA3FF",
  "border": "#1B3349",
  "statusOnline": "#3DD68C",
  "statusFaulty": "#F5B841",
  "statusOffline": "#5C7A99",
  "statusFailed": "#FF5C5C",
  "progressFill": "#4DA3FF",
  "completedSegmentFill": "#3DD68C",
  "activeHeadTint": "#63C9E8",
  "graphStroke": "#4DA3FF",
  "graphAverageStroke": "#7C8CFF",
  "sidebarBackground": "#0B1929",
  "selectionTint": "#4DA3FF"
}
```

`SDMKit/Sources/SDMCore/Resources/Themes/deep-purple.json`:

```json
{
  "id": "deep-purple",
  "name": "Deep Purple",
  "isDark": true,
  "source": "SDM",
  "surfacePrimary": "#1A0F2E",
  "surfaceSecondary": "#241640",
  "surfaceTertiary": "#332057",
  "textPrimary": "#FFFFFF",
  "textSecondary": "#DCDCE1",
  "textTertiary": "#C4C4CA",
  "accent": "#B084F5",
  "border": "#332057",
  "statusOnline": "#4ADE9C",
  "statusFaulty": "#F5C563",
  "statusOffline": "#8875A8",
  "statusFailed": "#FF6B8A",
  "progressFill": "#B084F5",
  "completedSegmentFill": "#4ADE9C",
  "activeHeadTint": "#7FE0D8",
  "graphStroke": "#B084F5",
  "graphAverageStroke": "#E894D4",
  "sidebarBackground": "#1A0F2E",
  "selectionTint": "#B084F5"
}
```

- [ ] **Step 5: Implement `ThemeCatalog`**

`SDMKit/Sources/SDMCore/ThemeCatalog.swift`:

```swift
import Foundation

/// Loads every bundled theme JSON file from `SDMCore`'s own resource
/// bundle. Spec §10.1: "adding a theme is adding a JSON file" — this is the
/// one place that enumerates them; nothing else changes to add a
/// seventeenth (Task 9 adds twelve more, all in the same
/// `Resources/Themes/` directory).
public enum ThemeCatalog {
    public static func builtInThemes() -> [Theme] {
        guard
            let urls = Bundle.module.urls(
                forResourcesWithExtension: "json", subdirectory: "Themes")
        else { return [] }
        let decoder = JSONDecoder()
        return urls
            .compactMap { url -> Theme? in
                guard let data = try? Data(contentsOf: url) else { return nil }
                return try? decoder.decode(Theme.self, from: data)
            }
            .sorted { $0.name < $1.name }
    }
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite.

- [ ] **Step 7: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: add ThemeCatalog and the four SDM-original built-in themes"
```

---

### Task 9: The twelve community palettes, and the WCAG-AA contrast gate

**Files:**
- Create: `SDMKit/Sources/SDMCore/Resources/Themes/nord.json`
- Create: `SDMKit/Sources/SDMCore/Resources/Themes/dracula.json`
- Create: `SDMKit/Sources/SDMCore/Resources/Themes/solarized-light.json`
- Create: `SDMKit/Sources/SDMCore/Resources/Themes/solarized-dark.json`
- Create: `SDMKit/Sources/SDMCore/Resources/Themes/gruvbox.json`
- Create: `SDMKit/Sources/SDMCore/Resources/Themes/catppuccin-latte.json`
- Create: `SDMKit/Sources/SDMCore/Resources/Themes/catppuccin-frappe.json`
- Create: `SDMKit/Sources/SDMCore/Resources/Themes/catppuccin-macchiato.json`
- Create: `SDMKit/Sources/SDMCore/Resources/Themes/catppuccin-mocha.json`
- Create: `SDMKit/Sources/SDMCore/Resources/Themes/tokyo-night.json`
- Create: `SDMKit/Sources/SDMCore/Resources/Themes/one-dark.json`
- Create: `SDMKit/Sources/SDMCore/Resources/Themes/rose-pine.json`
- Modify: `SDMKit/Tests/SDMCoreTests/ThemeCatalogTests.swift`

**Interfaces:**
- Consumes: `Theme`, `ThemeCatalog` (Tasks 7–8), `ContrastRatio` (Task 7)
- Produces: no new API — 16 total bundled themes, and the WCAG-AA gate spec §10.1 calls for

Surface, accent, and status colors below are each palette's own published core swatches (attributed in `source`). Text-tier colors use the same two shared values from every other dark/light theme (Task 8's rationale) rather than each palette's own muted "comment" color — several of those (Gruvbox's `gray`, One Dark's comment gray, Nord's `nord3`) sit well under 4.5:1 against their own lightest surface tier by design, since they exist to visually recede in a code editor, not to satisfy an AA gate for a download manager's body text. **Before shipping, diff every hex value below against each project's currently-published palette** — spec §10.1: "Palette values are taken from each project's own published source at implementation time, not from memory." The test in Step 3 is the actual gate regardless of any arithmetic in this plan.

- [ ] **Step 1: Add the twelve JSON theme files**

`SDMKit/Sources/SDMCore/Resources/Themes/nord.json`:

```json
{
  "id": "nord",
  "name": "Nord",
  "isDark": true,
  "source": "Nord (MIT) — nordtheme.com",
  "surfacePrimary": "#2E3440",
  "surfaceSecondary": "#3B4252",
  "surfaceTertiary": "#434C5E",
  "textPrimary": "#FFFFFF",
  "textSecondary": "#DCDCE1",
  "textTertiary": "#C4C4CA",
  "accent": "#88C0D0",
  "border": "#4C566A",
  "statusOnline": "#A3BE8C",
  "statusFaulty": "#EBCB8B",
  "statusOffline": "#4C566A",
  "statusFailed": "#BF616A",
  "progressFill": "#88C0D0",
  "completedSegmentFill": "#A3BE8C",
  "activeHeadTint": "#8FBCBB",
  "graphStroke": "#88C0D0",
  "graphAverageStroke": "#B48EAD",
  "sidebarBackground": "#2E3440",
  "selectionTint": "#5E81AC"
}
```

`SDMKit/Sources/SDMCore/Resources/Themes/dracula.json`:

```json
{
  "id": "dracula",
  "name": "Dracula",
  "isDark": true,
  "source": "Dracula (MIT) — draculatheme.com",
  "surfacePrimary": "#282A36",
  "surfaceSecondary": "#343746",
  "surfaceTertiary": "#44475A",
  "textPrimary": "#FFFFFF",
  "textSecondary": "#DCDCE1",
  "textTertiary": "#C4C4CA",
  "accent": "#BD93F9",
  "border": "#44475A",
  "statusOnline": "#50FA7B",
  "statusFaulty": "#F1FA8C",
  "statusOffline": "#6272A4",
  "statusFailed": "#FF5555",
  "progressFill": "#BD93F9",
  "completedSegmentFill": "#50FA7B",
  "activeHeadTint": "#8BE9FD",
  "graphStroke": "#BD93F9",
  "graphAverageStroke": "#FF79C6",
  "sidebarBackground": "#21222C",
  "selectionTint": "#BD93F9"
}
```

`SDMKit/Sources/SDMCore/Resources/Themes/solarized-light.json`:

```json
{
  "id": "solarized-light",
  "name": "Solarized Light",
  "isDark": false,
  "source": "Solarized (MIT) — ethanschoonover.com/solarized",
  "surfacePrimary": "#FDF6E3",
  "surfaceSecondary": "#EEE8D5",
  "surfaceTertiary": "#E4DEC9",
  "textPrimary": "#17171A",
  "textSecondary": "#47474B",
  "textTertiary": "#5F5F63",
  "accent": "#268BD2",
  "border": "#D3CBB7",
  "statusOnline": "#859900",
  "statusFaulty": "#B58900",
  "statusOffline": "#93A1A1",
  "statusFailed": "#DC322F",
  "progressFill": "#268BD2",
  "completedSegmentFill": "#859900",
  "activeHeadTint": "#2AA198",
  "graphStroke": "#268BD2",
  "graphAverageStroke": "#6C71C4",
  "sidebarBackground": "#EEE8D5",
  "selectionTint": "#268BD2"
}
```

`SDMKit/Sources/SDMCore/Resources/Themes/solarized-dark.json`:

```json
{
  "id": "solarized-dark",
  "name": "Solarized Dark",
  "isDark": true,
  "source": "Solarized (MIT) — ethanschoonover.com/solarized",
  "surfacePrimary": "#002B36",
  "surfaceSecondary": "#073642",
  "surfaceTertiary": "#0D3B47",
  "textPrimary": "#FFFFFF",
  "textSecondary": "#DCDCE1",
  "textTertiary": "#C4C4CA",
  "accent": "#268BD2",
  "border": "#073642",
  "statusOnline": "#859900",
  "statusFaulty": "#B58900",
  "statusOffline": "#586E75",
  "statusFailed": "#DC322F",
  "progressFill": "#268BD2",
  "completedSegmentFill": "#859900",
  "activeHeadTint": "#2AA198",
  "graphStroke": "#268BD2",
  "graphAverageStroke": "#6C71C4",
  "sidebarBackground": "#002B36",
  "selectionTint": "#268BD2"
}
```

`SDMKit/Sources/SDMCore/Resources/Themes/gruvbox.json`:

```json
{
  "id": "gruvbox",
  "name": "Gruvbox",
  "isDark": true,
  "source": "Gruvbox (MIT) — github.com/morhetz/gruvbox",
  "surfacePrimary": "#282828",
  "surfaceSecondary": "#3C3836",
  "surfaceTertiary": "#504945",
  "textPrimary": "#FFFFFF",
  "textSecondary": "#DCDCE1",
  "textTertiary": "#C4C4CA",
  "accent": "#83A598",
  "border": "#504945",
  "statusOnline": "#B8BB26",
  "statusFaulty": "#FABD2F",
  "statusOffline": "#928374",
  "statusFailed": "#FB4934",
  "progressFill": "#83A598",
  "completedSegmentFill": "#B8BB26",
  "activeHeadTint": "#8EC07C",
  "graphStroke": "#83A598",
  "graphAverageStroke": "#D3869B",
  "sidebarBackground": "#282828",
  "selectionTint": "#83A598"
}
```

`SDMKit/Sources/SDMCore/Resources/Themes/catppuccin-latte.json`:

```json
{
  "id": "catppuccin-latte",
  "name": "Catppuccin Latte",
  "isDark": false,
  "source": "Catppuccin (MIT) — github.com/catppuccin/catppuccin",
  "surfacePrimary": "#EFF1F5",
  "surfaceSecondary": "#E6E9EF",
  "surfaceTertiary": "#DCE0E8",
  "textPrimary": "#17171A",
  "textSecondary": "#47474B",
  "textTertiary": "#5F5F63",
  "accent": "#8839EF",
  "border": "#CCD0DA",
  "statusOnline": "#40A02B",
  "statusFaulty": "#DF8E1D",
  "statusOffline": "#9CA0B0",
  "statusFailed": "#D20F39",
  "progressFill": "#8839EF",
  "completedSegmentFill": "#40A02B",
  "activeHeadTint": "#179299",
  "graphStroke": "#8839EF",
  "graphAverageStroke": "#1E66F5",
  "sidebarBackground": "#E6E9EF",
  "selectionTint": "#8839EF"
}
```

`SDMKit/Sources/SDMCore/Resources/Themes/catppuccin-frappe.json`:

```json
{
  "id": "catppuccin-frappe",
  "name": "Catppuccin Frappé",
  "isDark": true,
  "source": "Catppuccin (MIT) — github.com/catppuccin/catppuccin",
  "surfacePrimary": "#303446",
  "surfaceSecondary": "#292C3C",
  "surfaceTertiary": "#414559",
  "textPrimary": "#FFFFFF",
  "textSecondary": "#DCDCE1",
  "textTertiary": "#C4C4CA",
  "accent": "#CA9EE6",
  "border": "#414559",
  "statusOnline": "#A6D189",
  "statusFaulty": "#E5C890",
  "statusOffline": "#737994",
  "statusFailed": "#E78284",
  "progressFill": "#CA9EE6",
  "completedSegmentFill": "#A6D189",
  "activeHeadTint": "#81C8BE",
  "graphStroke": "#CA9EE6",
  "graphAverageStroke": "#8CAAEE",
  "sidebarBackground": "#292C3C",
  "selectionTint": "#CA9EE6"
}
```

`SDMKit/Sources/SDMCore/Resources/Themes/catppuccin-macchiato.json`:

```json
{
  "id": "catppuccin-macchiato",
  "name": "Catppuccin Macchiato",
  "isDark": true,
  "source": "Catppuccin (MIT) — github.com/catppuccin/catppuccin",
  "surfacePrimary": "#24273A",
  "surfaceSecondary": "#1E2030",
  "surfaceTertiary": "#363A4F",
  "textPrimary": "#FFFFFF",
  "textSecondary": "#DCDCE1",
  "textTertiary": "#C4C4CA",
  "accent": "#C6A0F6",
  "border": "#363A4F",
  "statusOnline": "#A6DA95",
  "statusFaulty": "#EED49F",
  "statusOffline": "#6E738D",
  "statusFailed": "#ED8796",
  "progressFill": "#C6A0F6",
  "completedSegmentFill": "#A6DA95",
  "activeHeadTint": "#8BD5CA",
  "graphStroke": "#C6A0F6",
  "graphAverageStroke": "#8AADF4",
  "sidebarBackground": "#1E2030",
  "selectionTint": "#C6A0F6"
}
```

`SDMKit/Sources/SDMCore/Resources/Themes/catppuccin-mocha.json`:

```json
{
  "id": "catppuccin-mocha",
  "name": "Catppuccin Mocha",
  "isDark": true,
  "source": "Catppuccin (MIT) — github.com/catppuccin/catppuccin",
  "surfacePrimary": "#1E1E2E",
  "surfaceSecondary": "#181825",
  "surfaceTertiary": "#313244",
  "textPrimary": "#FFFFFF",
  "textSecondary": "#DCDCE1",
  "textTertiary": "#C4C4CA",
  "accent": "#CBA6F7",
  "border": "#313244",
  "statusOnline": "#A6E3A1",
  "statusFaulty": "#F9E2AF",
  "statusOffline": "#6C7086",
  "statusFailed": "#F38BA8",
  "progressFill": "#CBA6F7",
  "completedSegmentFill": "#A6E3A1",
  "activeHeadTint": "#94E2D5",
  "graphStroke": "#CBA6F7",
  "graphAverageStroke": "#89B4FA",
  "sidebarBackground": "#181825",
  "selectionTint": "#CBA6F7"
}
```

`SDMKit/Sources/SDMCore/Resources/Themes/tokyo-night.json`:

```json
{
  "id": "tokyo-night",
  "name": "Tokyo Night",
  "isDark": true,
  "source": "Tokyo Night (MIT) — github.com/enkia/tokyo-night-vscode-theme",
  "surfacePrimary": "#1A1B26",
  "surfaceSecondary": "#292E42",
  "surfaceTertiary": "#343B58",
  "textPrimary": "#FFFFFF",
  "textSecondary": "#DCDCE1",
  "textTertiary": "#C4C4CA",
  "accent": "#7AA2F7",
  "border": "#414868",
  "statusOnline": "#9ECE6A",
  "statusFaulty": "#E0AF68",
  "statusOffline": "#565F89",
  "statusFailed": "#F7768E",
  "progressFill": "#7AA2F7",
  "completedSegmentFill": "#9ECE6A",
  "activeHeadTint": "#7DCFFF",
  "graphStroke": "#7AA2F7",
  "graphAverageStroke": "#BB9AF7",
  "sidebarBackground": "#16161E",
  "selectionTint": "#7AA2F7"
}
```

`SDMKit/Sources/SDMCore/Resources/Themes/one-dark.json`:

```json
{
  "id": "one-dark",
  "name": "One Dark",
  "isDark": true,
  "source": "One Dark (MIT) — github.com/atom/atom (One Dark syntax theme)",
  "surfacePrimary": "#282C34",
  "surfaceSecondary": "#2C323C",
  "surfaceTertiary": "#3E4451",
  "textPrimary": "#FFFFFF",
  "textSecondary": "#DCDCE1",
  "textTertiary": "#C4C4CA",
  "accent": "#61AFEF",
  "border": "#3E4451",
  "statusOnline": "#98C379",
  "statusFaulty": "#E5C07B",
  "statusOffline": "#5C6370",
  "statusFailed": "#E06C75",
  "progressFill": "#61AFEF",
  "completedSegmentFill": "#98C379",
  "activeHeadTint": "#56B6C2",
  "graphStroke": "#61AFEF",
  "graphAverageStroke": "#C678DD",
  "sidebarBackground": "#21252B",
  "selectionTint": "#61AFEF"
}
```

`SDMKit/Sources/SDMCore/Resources/Themes/rose-pine.json`:

```json
{
  "id": "rose-pine",
  "name": "Rosé Pine",
  "isDark": true,
  "source": "Rosé Pine (MIT) — rose-pine.dev",
  "surfacePrimary": "#191724",
  "surfaceSecondary": "#1F1D2E",
  "surfaceTertiary": "#26233A",
  "textPrimary": "#FFFFFF",
  "textSecondary": "#DCDCE1",
  "textTertiary": "#C4C4CA",
  "accent": "#C4A7E7",
  "border": "#26233A",
  "statusOnline": "#31748F",
  "statusFaulty": "#F6C177",
  "statusOffline": "#6E6A86",
  "statusFailed": "#EB6F92",
  "progressFill": "#C4A7E7",
  "completedSegmentFill": "#31748F",
  "activeHeadTint": "#9CCFD8",
  "graphStroke": "#C4A7E7",
  "graphAverageStroke": "#EBBCBA",
  "sidebarBackground": "#191724",
  "selectionTint": "#C4A7E7"
}
```

- [ ] **Step 2: Update `ThemeCatalogTests.swift` with the count and the WCAG-AA gate**

Append to `SDMKit/Tests/SDMCoreTests/ThemeCatalogTests.swift`:

```swift
@Test func thereAreSixteenBuiltInThemes() {
    #expect(ThemeCatalog.builtInThemes().count == 16)
}

/// Spec §10.1: "A test asserts WCAG AA contrast for every text-on-surface
/// pair in every bundled theme, so an attractive palette cannot ship
/// unreadable secondary text."
@Test func everyBuiltInThemePassesWCAGAAForAllTextSurfacePairs() {
    for theme in ThemeCatalog.builtInThemes() {
        let textColors = [theme.textPrimary, theme.textSecondary, theme.textTertiary]
        let surfaceColors = [theme.surfacePrimary, theme.surfaceSecondary, theme.surfaceTertiary]
        for text in textColors {
            for surface in surfaceColors {
                #expect(
                    ContrastRatio.passesAA(text, surface),
                    "\(theme.id): \(text) on \(surface) is below WCAG AA (ratio \(ContrastRatio.between(text, surface)))"
                )
            }
        }
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `swift test --package-path SDMKit --filter ThemeCatalogTests`
Expected: PASS. If `everyBuiltInThemePassesWCAGAAForAllTextSurfacePairs` fails for a specific theme, the failure message names the exact theme and hex pair — darken (for a light theme) or lighten (for a dark theme) that theme's `surfaceTertiary` slightly, or fall back to a text-tier value closer to the shared dark/light ramp used elsewhere in this file, and re-run.

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite.

- [ ] **Step 4: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: add the twelve community-palette themes and the WCAG-AA contrast gate"
```

---

### Task 10: `ThemeStore` — selection, live "System" resolution, `NSApp.appearance` sync

**Files:**
- Create: `SDM/ThemeStore.swift`
- Create: `SDM/ThemeColor.swift`
- Modify: `SDM/SDMApp.swift`
- Modify: `SDM/MainWindowView.swift`

**Interfaces:**
- Consumes: `Theme`, `ThemeCatalog` (Tasks 7–9)
- Produces:
  - `@MainActor @Observable final class ThemeStore` — `selectedID: String`, `catalog: [Theme]`, `resolved(for: ColorScheme) -> Theme`
  - `extension Color { init(hex:) }` and `extension Theme { var accentColor: Color; ... }` in `ThemeColor.swift`
  - Injected into the environment alongside `EngineController`/`GrabberController`

Spec §10.1: "System resolves live to the light or dark variant, following `effectiveAppearance`." Rather than model "System" as a seventeenth `Theme` with placeholder colors, it is a selection sentinel (`ThemeStore.systemSelectionID`) that resolves to the bundled `"light"`/`"dark"` theme depending on the caller's `ColorScheme` — passed in from a view's own `@Environment(\.colorScheme)`, which is already live-reactive to appearance changes, so no manual `NSApp.effectiveAppearance` observation is needed for that half.

- [ ] **Step 1: Implement `ThemeStore`**

`SDM/ThemeStore.swift`:

```swift
import Observation
import SDMCore
import SwiftUI

/// Selection and live resolution for spec §10.1's theme system, mirroring
/// `EngineController`'s shape.
@MainActor
@Observable
final class ThemeStore {
    /// Sentinel `selectedID` meaning "follow the system appearance" rather
    /// than a fixed theme.
    static let systemSelectionID = "system"

    private static let key = "sdm.selectedThemeID"

    let catalog: [Theme]
    var selectedID: String {
        didSet {
            guard selectedID != oldValue else { return }
            UserDefaults.standard.set(selectedID, forKey: Self.key)
        }
    }

    init(catalog: [Theme] = ThemeCatalog.builtInThemes()) {
        self.catalog = catalog
        selectedID =
            UserDefaults.standard.string(forKey: Self.key) ?? Self.systemSelectionID
    }

    /// The theme actually in effect right now. "System" resolves against
    /// the caller's live `ColorScheme` rather than anything this class
    /// tracks itself.
    func resolved(for colorScheme: ColorScheme) -> Theme {
        let id =
            selectedID == Self.systemSelectionID
            ? (colorScheme == .dark ? "dark" : "light")
            : selectedID
        return catalog.first { $0.id == id } ?? catalog.first { $0.id == "light" } ?? fallback
    }

    private var fallback: Theme {
        catalog.first
            ?? Theme(
                id: "fallback", name: "Fallback", isDark: false, source: "SDM",
                surfacePrimary: "#FFFFFF", surfaceSecondary: "#F0F0F0", surfaceTertiary: "#E0E0E0",
                textPrimary: "#000000", textSecondary: "#444444", textTertiary: "#666666",
                accent: "#007AFF", border: "#CCCCCC", statusOnline: "#34C759",
                statusFaulty: "#FF9500", statusOffline: "#8E8E93", statusFailed: "#FF3B30",
                progressFill: "#007AFF", completedSegmentFill: "#34C759",
                activeHeadTint: "#32ADE6", graphStroke: "#007AFF", graphAverageStroke: "#5E5CE6",
                sidebarBackground: "#F0F0F0", selectionTint: "#007AFF")
    }
}
```

The `fallback` branch only matters if `ThemeCatalog.builtInThemes()` ever returns an empty array (a corrupted or missing resource bundle) — real launches always have 16 themes from Task 9, so this is defense against that one failure mode, not a normal code path.

- [ ] **Step 2: Implement the `Color(hex:)` bridge and `Theme` color accessors**

`SDM/ThemeColor.swift`:

```swift
import SDMCore
import SwiftUI

extension Color {
    /// Decodes a `#RRGGBB` hex string from a `Theme` role into a `Color`.
    /// Every bundled theme is a fixture this project controls, so a
    /// malformed hex is a bug to catch immediately via `precondition`, not a
    /// runtime condition to recover from.
    init(hex: String) {
        var cleaned = hex.trimmingCharacters(in: .whitespaces)
        if cleaned.hasPrefix("#") { cleaned.removeFirst() }
        precondition(cleaned.count == 6, "expected a #RRGGBB hex color, got \(hex)")
        var value: UInt64 = 0
        Scanner(string: cleaned).scanHexInt64(&value)
        self.init(
            red: Double((value & 0xFF0000) >> 16) / 255,
            green: Double((value & 0x00FF00) >> 8) / 255,
            blue: Double(value & 0x0000FF) / 255
        )
    }
}

extension Theme {
    var surfacePrimaryColor: Color { Color(hex: surfacePrimary) }
    var surfaceSecondaryColor: Color { Color(hex: surfaceSecondary) }
    var surfaceTertiaryColor: Color { Color(hex: surfaceTertiary) }
    var textPrimaryColor: Color { Color(hex: textPrimary) }
    var textSecondaryColor: Color { Color(hex: textSecondary) }
    var textTertiaryColor: Color { Color(hex: textTertiary) }
    var accentColor: Color { Color(hex: accent) }
    var borderColor: Color { Color(hex: border) }
    var onlineColor: Color { Color(hex: statusOnline) }
    var faultyColor: Color { Color(hex: statusFaulty) }
    var offlineColor: Color { Color(hex: statusOffline) }
    var failedColor: Color { Color(hex: statusFailed) }
    var progressFillColor: Color { Color(hex: progressFill) }
    var completedSegmentFillColor: Color { Color(hex: completedSegmentFill) }
    var activeHeadTintColor: Color { Color(hex: activeHeadTint) }
    var graphStrokeColor: Color { Color(hex: graphStroke) }
    var graphAverageStrokeColor: Color { Color(hex: graphAverageStroke) }
    var sidebarBackgroundColor: Color { Color(hex: sidebarBackground) }
    var selectionTintColor: Color { Color(hex: selectionTint) }
}
```

- [ ] **Step 3: Inject `ThemeStore` into the environment**

In `SDM/SDMApp.swift`, add a `@State` alongside `controller`/`grabberController`:

```swift
    @State private var themeStore = ThemeStore()
```

Add `.environment(themeStore)` to the `WindowGroup`'s content and to `MenuBarPopoverView`'s content:

```swift
        WindowGroup(id: "main") {
            MainWindowView(selection: $sidebarSelection)
                .environment(controller)
                .environment(grabberController)
                .environment(themeStore)
                .task {
```

```swift
        MenuBarExtra {
            MenuBarPopoverView(selection: $sidebarSelection)
                .environment(controller)
                .environment(grabberController)
                .environment(themeStore)
        } label: {
```

Also add it to the `Settings` scene:

```swift
        Settings {
            SettingsView()
                .environment(controller)
                .environment(themeStore)
        }
```

- [ ] **Step 4: Sync `NSApp.appearance` to the resolved theme**

In `SDM/MainWindowView.swift`, add:

```swift
    @Environment(ThemeStore.self) private var themeStore
    @Environment(\.colorScheme) private var colorScheme
```

alongside the existing `@Environment` properties, and add to `body`'s modifier chain (after `.frame(minWidth:minHeight:)`):

```swift
        .task(id: themeStore.selectedID) { applyNativeAppearance() }
        .onChange(of: colorScheme) { _, _ in applyNativeAppearance() }
```

Add the helper method:

```swift
    /// Spec §10.1: "Each theme declares whether it is dark, so
    /// `NSApp.appearance` is set correctly for native controls." `nil`
    /// (System) leaves `NSApp.appearance` unset so native chrome simply
    /// follows the OS; any fixed theme forces `NSApp.appearance` to match
    /// its own `isDark`, overriding the system setting.
    private func applyNativeAppearance() {
        guard themeStore.selectedID != ThemeStore.systemSelectionID else {
            NSApp.appearance = nil
            return
        }
        let theme = themeStore.resolved(for: colorScheme)
        NSApp.appearance = NSAppearance(named: theme.isDark ? .darkAqua : .aqua)
    }
```

- [ ] **Step 5: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED. No visible theme change yet — Task 11 is what makes views actually read `themeStore`; this task only wires selection storage and native-appearance sync. Confirm by hand that the app still launches and runs normally.

- [ ] **Step 6: Commit**

```bash
git add SDM/ThemeStore.swift SDM/ThemeColor.swift SDM/SDMApp.swift SDM/MainWindowView.swift
git commit -m "feat: add ThemeStore, live System resolution, and NSApp.appearance sync"
```

---

### Task 11: Apply theme roles across every existing literal-color view

**Files:**
- Modify: `SDM/PackagesListView.swift`
- Modify: `SDM/LinkGrabberView.swift`
- Modify: `SDM/Sparkline.swift`
- Modify: `SDM/BandwidthGraph.swift`
- Modify: `SDM/MenuBarRingIcon.swift`
- Modify: `SDM/SDMApp.swift`
- Modify: `SDM/MainWindowView.swift`

**Interfaces:**
- Consumes: `ThemeStore`, `Theme`'s `Color` accessors (Task 10)
- Produces: no new API — every `Color.accentColor`/`.secondary`/`.red`/`.green`/`.orange`/`.tint` literal named in this task is replaced with a theme-role read

This is the sweep Phase 3's own "Deferred to later phases" section named: *"Every view added by this plan... uses system colors directly, not theme roles — spec §10.1's theme system does not exist until Phase 4. Revisit every literal color then."* `Sparkline` and `MenuBarRingIcon` gain a `theme: Theme` parameter (they render outside a `List`/`Form` context where reaching into `@Environment` mid-`Canvas` is awkward, and `MenuBarRingIcon` in particular is rendered via `ImageRenderer` outside the live view tree entirely); every other view reads `@Environment(ThemeStore.self)` and `@Environment(\.colorScheme)` directly.

- [ ] **Step 1: `Sparkline` takes an explicit `Color`**

Replace `SDM/Sparkline.swift` in full:

```swift
import SwiftUI

/// Spec §9.6: one `Canvas` path, no axes, no legend, y-scaled to its own
/// max. Takes its stroke color explicitly rather than reading
/// `@Environment(ThemeStore.self)` itself, since it is used inside `Canvas`
/// contexts where that plumbing would be awkward for no benefit — every
/// caller already has a `Theme` in scope.
struct Sparkline: View {
    let samples: [Double]
    let color: Color

    var body: some View {
        Canvas { context, size in
            guard samples.count > 1, let maxValue = samples.max(), maxValue > 0 else { return }
            var path = Path()
            let stepX = size.width / CGFloat(samples.count - 1)
            for (index, value) in samples.enumerated() {
                let x = CGFloat(index) * stepX
                let y = size.height - CGFloat(value / maxValue) * size.height
                if index == 0 {
                    path.move(to: CGPoint(x: x, y: y))
                } else {
                    path.addLine(to: CGPoint(x: x, y: y))
                }
            }
            context.stroke(path, with: .color(color), lineWidth: 1)
        }
    }
}
```

- [ ] **Step 2: `BandwidthGraph` uses `graphStroke`/`graphAverageStroke`**

Replace `SDM/BandwidthGraph.swift` in full:

```swift
import Charts
import SwiftUI

/// Spec §9.6: Swift Charts is reserved for the one place it earns its
/// cost — everywhere else (per-row sparklines) uses a plain `Canvas`, since a
/// full `Chart` per row at hundreds of rows turns scrolling into a
/// slideshow. The filled area uses spec §10.1's `graphStroke` role, and the
/// running-average line its distinct `graphAverageStroke` role.
struct BandwidthGraph: View {
    let history: [Double]
    let strokeColor: Color
    let averageStrokeColor: Color

    var body: some View {
        Chart {
            ForEach(Array(history.enumerated()), id: \.offset) { index, value in
                AreaMark(x: .value("Tick", index), y: .value("Bytes/s", value))
                    .foregroundStyle(strokeColor.opacity(0.25))
                LineMark(x: .value("Tick", index), y: .value("Average", runningAverage[index]))
                    .foregroundStyle(averageStrokeColor)
            }
        }
        .chartXAxis(.hidden)
        .chartYAxis(.hidden)
    }

    private var runningAverage: [Double] {
        guard !history.isEmpty else { return [] }
        var result: [Double] = []
        var sum = 0.0
        for (index, value) in history.enumerated() {
            sum += value
            result.append(sum / Double(index + 1))
        }
        return result
    }
}
```

- [ ] **Step 3: `MenuBarRingIcon` takes an explicit `Theme`**

Replace `SDM/MenuBarRingIcon.swift` in full:

```swift
import SDMCore
import SwiftUI

#Preview {
    let fraction = 0.25
    MenuBarRingIcon(fraction: fraction, theme: ThemeCatalog.builtInThemes()[0])
}

/// Spec §9.7: "The menu bar icon shows a determinate ring for overall
/// progress." Built as plain SwiftUI rather than a rasterized `NSImage`,
/// which `.menuBarExtraStyle(.window)`'s custom label view supports
/// directly. Takes `theme` explicitly — it is rendered via `ImageRenderer`
/// outside the live view tree (see `SDMApp.statusItemImage`), where
/// `@Environment` is unavailable.
struct MenuBarRingIcon: View {
    let fraction: Double
    let theme: Theme

    var body: some View {
        ZStack {
            Circle().stroke(theme.borderColor.opacity(0.6), lineWidth: 1)
                .padding(2)
            Circle()
                .trim(from: 0, to: max(0.02, min(fraction, 1)))
                .stroke(theme.accentColor, style: StrokeStyle(lineWidth: 1, lineCap: .round))
                .rotationEffect(.degrees(-90))
                .padding(2)
            Image("MenuBarLogo")
                .resizable()
                .scaledToFit()
                .frame(width: 14, height: 14)
        }
        .frame(width: 16, height: 16)
    }
}
```

- [ ] **Step 4: `SDMApp.statusItemImage` resolves a theme without going through SwiftUI's environment**

In `SDM/SDMApp.swift`, replace `statusItemImage`:

```swift
    private var statusItemImage: NSImage {
        let renderer = ImageRenderer(content: MenuBarRingIcon(fraction: overallFraction))
        renderer.scale = 2
        return renderer.nsImage ?? NSImage()
    }
```

with:

```swift
    private var statusItemImage: NSImage {
        let isDark = NSApp.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        let theme = themeStore.resolved(for: isDark ? .dark : .light)
        let renderer = ImageRenderer(
            content: MenuBarRingIcon(fraction: overallFraction, theme: theme))
        renderer.scale = 2
        return renderer.nsImage ?? NSImage()
    }
```

- [ ] **Step 5: `PackagesListView` reads theme roles throughout**

In `SDM/PackagesListView.swift`, add to `PackagesListView`'s properties:

```swift
    @Environment(ThemeStore.self) private var themeStore
    @Environment(\.colorScheme) private var colorScheme

    private var theme: Theme { themeStore.resolved(for: colorScheme) }
```

Replace `packageHeaderBackground(index:)`:

```swift
    private func packageHeaderBackground(index: Int) -> Color {
        theme.surfaceSecondaryColor.opacity(index.isMultiple(of: 2) ? 0.5 : 0.9)
    }
```

In `packageHeader(_:index:)`, replace the caption's color:

```swift
                    Text(
                        "\(formattedBytes(package.completedBytes)) / \(formattedBytes(package.totalBytes))"
                    )
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
```

with:

```swift
                    Text(
                        "\(formattedBytes(package.completedBytes)) / \(formattedBytes(package.totalBytes))"
                    )
                    .font(.caption)
                    .foregroundStyle(theme.textSecondaryColor)
                    .monospacedDigit()
```

and its `Sparkline(...)` call:

```swift
                Sparkline(samples: package.bytesPerSecondHistory)
                    .frame(width: 60, height: 20)
```

with:

```swift
                Sparkline(samples: package.bytesPerSecondHistory, color: theme.graphStrokeColor)
                    .frame(width: 60, height: 20)
```

Replace `bottomBar`'s "N packages" caption:

```swift
            Text("\(packages.count) packages")
                .foregroundStyle(.secondary)
```

with:

```swift
            Text("\(packages.count) packages")
                .foregroundStyle(theme.textSecondaryColor)
```

Replace `SegmentedProgressBar` in full:

```swift
/// Renders the completed `RangeSet` directly, rasterized to the bar's pixel
/// width so it stays correct at any segment count. See spec §9.4. Uses
/// spec §10.1's `progressFill` role for the fill and `surfaceTertiary` for
/// the empty track.
struct SegmentedProgressBar: View {
    let completed: RangeSet
    let total: Int64
    let theme: Theme

    var body: some View {
        Canvas { context, size in
            let background = Path(
                roundedRect: CGRect(origin: .zero, size: size),
                cornerRadius: size.height / 2
            )
            context.fill(background, with: .color(theme.surfaceTertiaryColor.opacity(0.6)))

            guard total > 0 else { return }
            for range in completed.ranges {
                let x = size.width * CGFloat(range.start) / CGFloat(total)
                let width = size.width * CGFloat(range.length) / CGFloat(total)
                context.fill(
                    Path(CGRect(x: x, y: 0, width: max(width, 0.5), height: size.height)),
                    with: .color(theme.progressFillColor)
                )
            }
        }
    }
}
```

`ItemRow` (nested at the bottom of this file) also needs `theme` — add to its properties:

```swift
    let theme: Theme
```

Update its `#Preview` at the top of the file to pass one:

```swift
#Preview {
    var isEnabled = true
    var isResumable = true
    var state = ItemState.running
    var isSelected = false
    let item = ItemSnapshot(
        id: UUID(), url: URL(fileURLWithPath: ""), filename: "Filename", totalBytes: 1_000_000,
        completed: RangeSet([ByteRange(start: 10000, end: 20000)]), state: state,
        isEnabled: isEnabled, isResumable: isResumable, activeSegments: 1, configuredSegments: 3,
        bytesPerSecond: 100000, speedHistory: [100000, 90000, 80000])
    ItemRow(
        item: item, index: 1, controller: EngineController(), isSelected: isSelected,
        theme: ThemeCatalog.builtInThemes()[0])
}
```

Update `itemRow(_:index:)` in `PackagesListView` to pass it through:

```swift
    private func itemRow(_ item: ItemSnapshot, index: Int) -> some View {
        ItemRow(
            item: item, index: index, controller: controller,
            isSelected: selectedItemIDs.contains(item.id), theme: theme
        )
        .tag(item.id)
        .contextMenu {
            itemsContextMenu(selectedItemIDs.contains(item.id) ? selectedItemIDs : [item.id])
        }
    }
```

Inside `ItemRow.body`, replace the filename `foregroundStyle`:

```swift
                    Text(item.filename)
                        .lineLimit(1)
                        .strikethrough(!item.isEnabled)
                        .foregroundStyle(
                            item.isEnabled ? AnyShapeStyle(.primary) : AnyShapeStyle(.secondary))
```

with:

```swift
                    Text(item.filename)
                        .lineLimit(1)
                        .strikethrough(!item.isEnabled)
                        .foregroundStyle(
                            item.isEnabled
                                ? AnyShapeStyle(theme.textPrimaryColor)
                                : AnyShapeStyle(theme.textSecondaryColor))
```

Replace the "file missing" label:

```swift
                    if item.fileMissing {
                        Label("file missing", systemImage: "exclamationmark.triangle.fill")
                            .font(.caption)
                            .foregroundStyle(.orange)
                    }
```

with:

```swift
                    if item.fileMissing {
                        Label("file missing", systemImage: "exclamationmark.triangle.fill")
                            .font(.caption)
                            .foregroundStyle(theme.faultyColor)
                    }
```

Replace the segment-count and speed captions plus the `Sparkline` call:

```swift
                    Text("\(item.activeSegments)/\(item.configuredSegments) seg")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                    Text(formatted(item.bytesPerSecond))
                        .font(.caption.monospacedDigit())
                    Sparkline(samples: item.speedHistory)
                        .frame(width: 48, height: 16)
```

with:

```swift
                    Text("\(item.activeSegments)/\(item.configuredSegments) seg")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(theme.textSecondaryColor)
                    Text(formatted(item.bytesPerSecond))
                        .font(.caption.monospacedDigit())
                    Sparkline(samples: item.speedHistory, color: theme.graphStrokeColor)
                        .frame(width: 48, height: 16)
```

Replace `SegmentedProgressBar(...)`'s call site:

```swift
                SegmentedProgressBar(completed: item.completed, total: item.totalBytes ?? 0)
                    .frame(height: 6)
```

with:

```swift
                SegmentedProgressBar(completed: item.completed, total: item.totalBytes ?? 0, theme: theme)
                    .frame(height: 6)
```

Replace the trailing byte-count caption:

```swift
                    Text(
                        "\(formattedBytes(item.completed.totalBytes)) / \(formattedBytes(item.totalBytes ?? 0))"
                    )
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
```

with:

```swift
                    Text(
                        "\(formattedBytes(item.completed.totalBytes)) / \(formattedBytes(item.totalBytes ?? 0))"
                    )
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(theme.textSecondaryColor)
```

Replace `alternatingRowBackground`:

```swift
    private var alternatingRowBackground: Color {
        guard !isSelected else { return Color.accentColor.opacity(0.35) }
        return Color(nsColor: NSColor.alternatingContentBackgroundColors[index % 2])
    }
```

with:

```swift
    private var alternatingRowBackground: Color {
        guard !isSelected else { return theme.selectionTintColor.opacity(0.35) }
        return Color(nsColor: NSColor.alternatingContentBackgroundColors[index % 2])
    }
```

(the native zebra stripe itself stays — it is not one of spec §10.1's ~20 roles, and it already adapts correctly to light/dark on its own).

Replace `stateIcon`:

```swift
    @ViewBuilder
    private var stateIcon: some View {
        switch item.state {
        case .running:
            Image(systemName: "arrow.down.circle.fill").foregroundStyle(.tint)
        case .completed:
            Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
        case .failed:
            Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.red)
        case .queued:
            Image(systemName: "clock.fill").foregroundStyle(.secondary)
        case .stopped:
            Image(systemName: "pause.circle.fill").foregroundStyle(.secondary)
        }
    }
```

with:

```swift
    @ViewBuilder
    private var stateIcon: some View {
        switch item.state {
        case .running:
            Image(systemName: "arrow.down.circle.fill").foregroundStyle(theme.accentColor)
        case .completed:
            Image(systemName: "checkmark.circle.fill").foregroundStyle(theme.onlineColor)
        case .failed:
            Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(theme.failedColor)
        case .queued:
            Image(systemName: "clock.fill").foregroundStyle(theme.textSecondaryColor)
        case .stopped:
            Image(systemName: "pause.circle.fill").foregroundStyle(theme.textSecondaryColor)
        }
    }
```

Replace `statusLine`:

```swift
    @ViewBuilder
    private var statusLine: some View {
        HStack(spacing: 6) {
            Text(Self.describe(item))
                .font(.caption)
                .foregroundStyle(isFailed ? AnyShapeStyle(.red) : AnyShapeStyle(.secondary))
            if let checkpointFailure = item.checkpointFailure {
                // Not a failure of the download, but it means a crash would
                // lose everything transferred so far.
                Text(checkpointFailure)
                    .font(.caption)
                    .foregroundStyle(.orange)
                    .lineLimit(2)
            }
        }
    }
```

with:

```swift
    @ViewBuilder
    private var statusLine: some View {
        HStack(spacing: 6) {
            Text(Self.describe(item))
                .font(.caption)
                .foregroundStyle(
                    isFailed ? AnyShapeStyle(theme.failedColor) : AnyShapeStyle(theme.textSecondaryColor)
                )
            if let checkpointFailure = item.checkpointFailure {
                // Not a failure of the download, but it means a crash would
                // lose everything transferred so far.
                Text(checkpointFailure)
                    .font(.caption)
                    .foregroundStyle(theme.faultyColor)
                    .lineLimit(2)
            }
        }
    }
```

Replace `resumabilityBadge`:

```swift
    @ViewBuilder
    private var resumabilityBadge: some View {
        switch item.isResumable {
        case false:
            Text("not resumable")
                .font(.caption)
                .foregroundStyle(.secondary)
        case true, nil:
            EmptyView()
        }
    }
```

with:

```swift
    @ViewBuilder
    private var resumabilityBadge: some View {
        switch item.isResumable {
        case false:
            Text("not resumable")
                .font(.caption)
                .foregroundStyle(theme.textSecondaryColor)
        case true, nil:
            EmptyView()
        }
    }
```

- [ ] **Step 6: `LinkGrabberView` reads theme roles, and each verdict finally gets its own distinct status color**

In `SDM/LinkGrabberView.swift`, add to `LinkGrabberView`'s properties:

```swift
    @Environment(ThemeStore.self) private var themeStore
    @Environment(\.colorScheme) private var colorScheme

    private var theme: Theme { themeStore.resolved(for: colorScheme) }
```

Replace the header caption:

```swift
                Text("\(snapshot.checkedCount) / \(snapshot.totalCount) checked")
                    .font(.caption)
                    .foregroundStyle(.secondary)
```

with:

```swift
                Text("\(snapshot.checkedCount) / \(snapshot.totalCount) checked")
                    .font(.caption)
                    .foregroundStyle(theme.textSecondaryColor)
```

Replace `filterChip(_:count:)`:

```swift
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
```

with:

```swift
    private func filterChip(_ filter: VerdictFilter, count: Int) -> some View {
        Button {
            activeFilter = activeFilter == filter ? .all : filter
        } label: {
            Text("\(filter.label) \(count)")
                .font(.caption)
        }
        .buttonStyle(.bordered)
        .tint(activeFilter == filter ? theme.accentColor : theme.textSecondaryColor)
    }
```

`LinkRow` needs `theme` threaded through — add to its properties and to the call site in `body`:

```swift
private struct LinkRow: View {
    let link: ProbedLink
    let controller: GrabberController
    let theme: Theme
```

```swift
                        ForEach(links(in: package)) { link in
                            LinkRow(link: link, controller: controller, theme: theme)
                                .draggable(DraggedLinkID(linkID: link.id))
                        }
```

Inside `LinkRow.body`, replace the duplicate badge:

```swift
            if link.isDuplicate {
                Text("duplicate")
                    .font(.caption)
                    .foregroundStyle(.orange)
            }
```

with:

```swift
            if link.isDuplicate {
                Text("duplicate")
                    .font(.caption)
                    .foregroundStyle(theme.faultyColor)
            }
```

Replace `verdictBadge` — each of the four verdicts now maps to its own distinct spec §10.1 status role, rather than "offline" and "check failed" sharing the same gray a plain `.secondary` gave them before this theme system existed:

```swift
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
```

with:

```swift
    @ViewBuilder
    private var verdictBadge: some View {
        switch link.verdict {
        case .online:
            Text("online").font(.caption).foregroundStyle(theme.onlineColor)
        case .offline:
            Text("offline").font(.caption).foregroundStyle(theme.offlineColor)
        case .checkFailed:
            Text("check failed").font(.caption).foregroundStyle(theme.failedColor)
        case .faulty(let reason):
            // Spec §7.3: the faulty reason *is* the badge text.
            Text(reason).font(.caption).foregroundStyle(theme.faultyColor)
        case nil:
            // No verdict yet: spec §7.5's queued → probing → sniffing → done
            // per-link state, shown literally rather than a bare spinner.
            HStack(spacing: 4) {
                ProgressView().controlSize(.small)
                Text(stageLabel).font(.caption).foregroundStyle(theme.textSecondaryColor)
            }
        }
    }
```

- [ ] **Step 7: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED.

Run the app and confirm by hand: nothing visibly changes yet from a user's perspective (Task 14 adds the picker that actually lets someone choose a theme) — but the app must launch, download, and grab links exactly as before, proving every color read now resolves through `themeStore.resolved(for: .light)` (System's default) to the same "Light" theme's values as the old literal `.accentColor`/`.secondary` etc. did visually.

- [ ] **Step 8: Commit**

```bash
git add SDM/PackagesListView.swift SDM/LinkGrabberView.swift SDM/Sparkline.swift SDM/BandwidthGraph.swift SDM/MenuBarRingIcon.swift SDM/SDMApp.swift
git commit -m "feat: apply theme roles across every view that used literal colors"
```

---

### Task 12: `ActivationPolicyController` and the three window/dock modes

**Files:**
- Create: `SDM/ActivationPolicyStore.swift`
- Create: `SDM/ActivationPolicyController.swift`
- Modify: `SDM/SDMApp.swift`

**Interfaces:**
- Consumes: nothing new
- Produces:
  - `enum ActivationPolicyMode: String, CaseIterable, Identifiable { case menuBarOnly, dockOnly, both }`
  - `@MainActor enum ActivationPolicyStore { static var mode: ActivationPolicyMode }`
  - `@MainActor @Observable final class ActivationPolicyController { var mode: ActivationPolicyMode; func apply() }`

Spec §10.2's table: **Menu bar only** shows the dock icon only while a window is open (policy → `.accessory` on close, `.regular` on reopen); **Dock only** always shows the dock icon and hides the menu bar icon entirely; **Both** (default) always shows both and window close never touches the dock icon.

- [ ] **Step 1: `ActivationPolicyStore` — the `UserDefaults`-backed setting**

`SDM/ActivationPolicyStore.swift`:

```swift
import Foundation

enum ActivationPolicyMode: String, CaseIterable, Identifiable {
    case menuBarOnly, dockOnly, both

    var id: String { rawValue }

    var label: String {
        switch self {
        case .menuBarOnly: return "Menu Bar Only"
        case .dockOnly: return "Dock Only"
        case .both: return "Both"
        }
    }
}

/// Spec §12: "Dock / menu bar mode", default `.both`. Mirrors
/// `GrabberSettings`'s direct-`UserDefaults` pattern.
@MainActor
enum ActivationPolicyStore {
    private static let defaults = UserDefaults.standard
    private static let key = "sdm.activationPolicyMode"

    static var mode: ActivationPolicyMode {
        get { ActivationPolicyMode(rawValue: defaults.string(forKey: key) ?? "") ?? .both }
        set { defaults.set(newValue.rawValue, forKey: key) }
    }
}
```

- [ ] **Step 2: `ActivationPolicyController` — applies the mode live**

`SDM/ActivationPolicyController.swift`:

```swift
import AppKit
import Observation

/// Applies spec §10.2's dock/menu-bar mode to `NSApp`'s activation policy.
/// Mirrors `EngineController`'s shape: an `@Observable` wrapper around a
/// `UserDefaults`-backed store, so a `Picker` bound to `mode` applies
/// immediately with no separate "Apply" step — spec §10.2: "Changing the
/// mode applies immediately, including when no window is open."
@MainActor
@Observable
final class ActivationPolicyController {
    var mode: ActivationPolicyMode {
        didSet {
            guard mode != oldValue else { return }
            ActivationPolicyStore.mode = mode
            apply()
        }
    }

    init() {
        mode = ActivationPolicyStore.mode
    }

    /// Applies the current mode's dock-icon policy. Safe to call any time,
    /// including from `AppDelegate`'s window-open/close observers (Step 3).
    func apply() {
        switch mode {
        case .dockOnly, .both:
            NSApp.setActivationPolicy(.regular)
        case .menuBarOnly:
            // Only `.accessory` while genuinely windowless — a mode switch
            // made while a window is open should not yank the dock icon out
            // from under it.
            let hasVisibleWindow = NSApp.windows.contains { $0.isVisible }
            NSApp.setActivationPolicy(hasVisibleWindow ? .regular : .accessory)
        }
    }

    /// Spec §10.2: "Dock only... hidden" menu bar icon. Every other mode
    /// shows it. Bound to `MenuBarExtra(isInserted:)` in `SDMApp`.
    var showsMenuBarIcon: Bool { mode != .dockOnly }
}
```

- [ ] **Step 3: Wire `AppDelegate` to apply the policy on launch, window close, and reopen**

In `SDM/SDMApp.swift`, replace `AppDelegate` in full:

```swift
final class AppDelegate: NSObject, NSApplicationDelegate {
    /// Assigned by the scene once the controller exists. Weak so the delegate,
    /// which AppKit keeps for the process lifetime, does not decide the
    /// controller's.
    weak var controller: EngineController?
    weak var activationPolicyController: ActivationPolicyController?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NotificationCenter.default.addObserver(
            self, selector: #selector(windowVisibilityChanged(_:)),
            name: NSWindow.willCloseNotification, object: nil)
        NotificationCenter.default.addObserver(
            self, selector: #selector(windowVisibilityChanged(_:)),
            name: NSWindow.didBecomeMainNotification, object: nil)
        activationPolicyController?.apply()
    }

    /// Spec §10.2's "Menu bar only" mode toggles the dock icon on every
    /// window open/close. Deferred one runloop turn: `willCloseNotification`
    /// fires before the window is actually removed from `NSApp.windows`, so
    /// checking visibility synchronously here would see the closing window
    /// as still open.
    @objc private func windowVisibilityChanged(_ notification: Notification) {
        DispatchQueue.main.async { [weak self] in
            self?.activationPolicyController?.apply()
        }
    }

    /// Dock-icon click when the app has no visible windows (`.accessory`
    /// mode with a closed window). Restores dock visibility per spec §10.2's
    /// "Reopen via: menu bar icon (policy → `.regular`)" — the equivalent
    /// gesture for `.accessory` apps is a dock-icon click if one is still
    /// showing, or the menu bar icon itself, which independently opens the
    /// window and lets `windowVisibilityChanged` restore the policy.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool)
        -> Bool
    {
        activationPolicyController?.apply()
        return true
    }

    func applicationWillTerminate(_ notification: Notification) {
        controller?.shutdownBlocking()
    }
}
```

Add `@State private var activationPolicyController = ActivationPolicyController()` to `SDMApp`, assign it to the delegate, and bind the `MenuBarExtra`'s insertion:

```swift
    @State private var controller = EngineController()
    @State private var grabberController = GrabberController()
    @State private var themeStore = ThemeStore()
    @State private var activationPolicyController = ActivationPolicyController()
```

In the `WindowGroup`'s `.task`, add the assignment alongside the existing one:

```swift
                .task {
                    appDelegate.controller = controller
                    appDelegate.activationPolicyController = activationPolicyController
                    await controller.startHeartbeat()
                }
```

Replace the `MenuBarExtra` scene:

```swift
        MenuBarExtra {
            MenuBarPopoverView(selection: $sidebarSelection)
                .environment(controller)
                .environment(grabberController)
                .environment(themeStore)
        } label: {
            Image(nsImage: statusItemImage)
        }
        .menuBarExtraStyle(.window)
```

with:

```swift
        MenuBarExtra(isInserted: menuBarInsertedBinding) {
            MenuBarPopoverView(selection: $sidebarSelection)
                .environment(controller)
                .environment(grabberController)
                .environment(themeStore)
        } label: {
            Image(nsImage: statusItemImage)
        }
        .menuBarExtraStyle(.window)
```

and add the computed binding near `statusItemImage`:

```swift
    /// `MenuBarExtra(isInserted:)` needs a `Binding`, but there is nothing to
    /// persist here beyond what `activationPolicyController` already tracks —
    /// this just projects `showsMenuBarIcon` through a no-op setter, since
    /// the only way this value changes is `activationPolicyController.mode`
    /// itself changing, which `SwiftUI` already observes.
    private var menuBarInsertedBinding: Binding<Bool> {
        Binding(get: { activationPolicyController.showsMenuBarIcon }, set: { _ in })
    }
```

Also add `.environment(activationPolicyController)` to the `Settings` scene (Task 14 reads it from `SettingsView`):

```swift
        Settings {
            SettingsView()
                .environment(controller)
                .environment(themeStore)
                .environment(activationPolicyController)
        }
```

- [ ] **Step 4: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED.

Run the app and confirm by hand (default mode is "Both", so nothing changes yet — Task 14 adds the picker; this step just proves the plumbing does not break normal launch):
- The app launches with both a dock icon and a menu bar icon, exactly as before.
- Quit and relaunch — same result.

- [ ] **Step 5: Commit**

```bash
git add SDM/ActivationPolicyStore.swift SDM/ActivationPolicyController.swift SDM/SDMApp.swift
git commit -m "feat: add ActivationPolicyController and wire dock/menu-bar mode into AppDelegate"
```

---

### Task 13: Quit confirmation for active non-resumable downloads

**Files:**
- Modify: `SDM/SDMApp.swift`

**Interfaces:**
- Consumes: `EngineController.snapshot` (existing)
- Produces: no new API — `AppDelegate` gains `applicationShouldTerminate(_:)`

Spec §10.2: "Quitting with active non-resumable downloads shows a confirmation, since that progress cannot be recovered." `ItemSnapshot.isResumable == false` combined with `state == .running` is exactly "actively transferring and cannot be resumed if interrupted now." `NSAlert.runModal()` blocks synchronously, so — like `shutdownBlocking()` already does a few lines below it in this same file — there is no need for AppKit's asynchronous `.terminateLater`/`reply(toApplicationShouldTerminate:)` dance.

- [ ] **Step 1: Add `applicationShouldTerminate(_:)` to `AppDelegate`**

In `SDM/SDMApp.swift`, add to `AppDelegate`, after `applicationShouldHandleReopen(_:hasVisibleWindows:)`:

```swift
    /// Spec §10.2: "Quitting with active non-resumable downloads shows a
    /// confirmation, since that progress cannot be recovered." `runModal()`
    /// blocks synchronously — the same "blocking is the point" reasoning
    /// `EngineController.shutdownBlocking` already documents for the
    /// termination path this gates — so no `.terminateLater` bookkeeping is
    /// needed.
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard let controller, hasActiveNonResumableDownloads(controller) else {
            return .terminateNow
        }
        let alert = NSAlert()
        alert.messageText = "Quit with active downloads in progress?"
        alert.informativeText =
            "One or more downloads cannot be resumed. Quitting now will lose their progress permanently."
        alert.addButton(withTitle: "Quit")
        alert.addButton(withTitle: "Cancel")
        alert.alertStyle = .warning
        let response = alert.runModal()
        return response == .alertFirstButtonReturn ? .terminateNow : .terminateCancel
    }

    private func hasActiveNonResumableDownloads(_ controller: EngineController) -> Bool {
        controller.snapshot.packages.flatMap(\.items).contains {
            $0.state == .running && $0.isResumable == false
        }
    }
```

- [ ] **Step 2: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED.

Run the app and confirm by hand:
- With nothing downloading (or only resumable downloads running), `⌘Q` quits immediately with no dialog — same as before this task.
- Start a download against a source known not to honor `Range` requests (any plain static-file host that responds `200` instead of `206` to a ranged `GET`, or briefly point a download at a URL that returns the whole body regardless of the `Range` header) so the item's badge reads "not resumable" and it is actively `.running`. Press `⌘Q`: the confirmation alert appears. "Cancel" leaves the app running and the download continues; "Quit" proceeds to quit.

- [ ] **Step 3: Commit**

```bash
git add SDM/SDMApp.swift
git commit -m "feat: confirm before quitting with active non-resumable downloads"
```

---

### Task 14: `SettingsView` gets Appearance and Window sections

**Files:**
- Modify: `SDM/SettingsView.swift`

**Interfaces:**
- Consumes: `ThemeStore` (Task 10), `ActivationPolicyController` (Task 12)
- Produces: no new API — two new `Form` sections

- [ ] **Step 1: Add the two sections**

In `SDM/SettingsView.swift`, add to the property list:

```swift
    @Environment(ThemeStore.self) private var themeStore
    @Environment(ActivationPolicyController.self) private var activationPolicyController
```

Add two new `Section`s to the `Form`, after the existing `Section("Notifications")`:

```swift
            Section("Appearance") {
                Picker(
                    "Theme",
                    selection: Binding(
                        get: { themeStore.selectedID },
                        set: { themeStore.selectedID = $0 }
                    )
                ) {
                    Text("System").tag(ThemeStore.systemSelectionID)
                    ForEach(themeStore.catalog) { theme in
                        Text(theme.name).tag(theme.id)
                    }
                }
            }
            Section("Window") {
                Picker(
                    "Dock / Menu Bar",
                    selection: Binding(
                        get: { activationPolicyController.mode },
                        set: { activationPolicyController.mode = $0 }
                    )
                ) {
                    ForEach(ActivationPolicyMode.allCases) { mode in
                        Text(mode.label).tag(mode)
                    }
                }
            }
```

`Binding(get:set:)` is used here rather than `$themeStore.selectedID`/`$activationPolicyController.mode` because both are `@Environment`-injected `@Observable` reference types in a `View` that does not own them — SwiftUI's `@Bindable` projection needs a locally-declared property wrapper, which `@Environment` does not provide directly; wrapping the read/write in an explicit `Binding` sidesteps that without introducing a redundant `@State` copy.

- [ ] **Step 2: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED.

Run the app, press `⌘,`, and confirm by hand:
- The Theme picker lists "System" plus all 16 built-in themes. Selecting "Dracula" (or any other) immediately re-colors every status icon, progress bar, sparkline, and the bandwidth graph across the Downloads, Completed, and Linkgrabber tabs, plus the menu bar icon's ring — no restart needed.
- Selecting "System" and then toggling the Mac's own Appearance setting between Light and Dark (System Settings → Appearance) live-updates SDM's theme to the matching bundled "Light"/"Dark" theme.
- The Dock / Menu Bar picker's three options each produce the icon behavior spec §10.2's table describes: "Both" shows both icons and window close does nothing to the dock icon; "Dock Only" hides the menu bar icon entirely; "Menu Bar Only" makes the dock icon disappear when the last window closes and reappear when the menu bar icon reopens one.

- [ ] **Step 3: Commit**

```bash
git add SDM/SettingsView.swift
git commit -m "feat: add Appearance and Window sections to SettingsView"
```

---

### Task 15: Liquid Glass — the isolated `.sdmSurface(_:)` modifier

**Files:**
- Create: `SDM/LiquidGlassSurface.swift`
- Modify: `SDM/MainWindowView.swift`
- Modify: `SDM/PackagesListView.swift`
- Modify: `SDM/LinkGrabberView.swift`

**Interfaces:**
- Consumes: nothing
- Produces: `extension View { func sdmSurface(_ kind: SDMSurfaceKind) -> some View }`

Spec §9.9: "Isolated in a single `ViewModifier`... applying `glassEffect` under `if #available(macOS 26)` and falling back to `.regularMaterial` on macOS 15. Views never write availability checks; there is exactly one file to change when the baseline moves." **`glassEffect`'s exact API surface (parameters, `in:` shape argument) is a macOS 26 SDK addition — verify it against current SwiftUI documentation (Context7 or Apple's) before committing**, per this project's established practice of verifying rather than recalling toolchain/SDK facts; the signature below is this plan's best-confidence draft, not a guarantee.

- [ ] **Step 1: Implement the modifier**

`SDM/LiquidGlassSurface.swift`:

```swift
import SwiftUI

enum SDMSurfaceKind {
    case sidebar, toolbar
}

private struct SDMSurfaceModifier: ViewModifier {
    let kind: SDMSurfaceKind

    func body(content: Content) -> some View {
        if #available(macOS 26, *) {
            content.glassEffect(.regular, in: .rect)
        } else {
            content.background(.regularMaterial)
        }
    }
}

extension View {
    /// Spec §9.9: Liquid Glass isolated to this one file. Every other view
    /// calls `.sdmSurface(_:)` rather than writing its own `if #available`;
    /// there is exactly one place to change when the deployment baseline
    /// moves past macOS 26.
    func sdmSurface(_ kind: SDMSurfaceKind) -> some View {
        modifier(SDMSurfaceModifier(kind: kind))
    }
}
```

- [ ] **Step 2: Apply it to the sidebar and to the two toolbar-like surfaces**

In `SDM/MainWindowView.swift`, add `.sdmSurface(.sidebar)` to the sidebar `List`:

```swift
            List(selection: $selection) {
                Label("Downloads", systemImage: "arrow.down.circle").tag(SidebarItem.downloads)
                Label("Completed", systemImage: "checkmark.circle").tag(SidebarItem.completed)
                Label("Linkgrabber", systemImage: "link")
                    .tag(SidebarItem.linkgrabber)
                    .badge(grabberController.snapshot.totalCount)
                Section("Overview") { statsBlock }
            }
            .navigationSplitViewColumnWidth(min: 200, ideal: 220)
            .sdmSurface(.sidebar)
```

In `SDM/PackagesListView.swift`, add `.sdmSurface(.toolbar)` to `bottomBar`'s outermost modifier chain:

```swift
    private var bottomBar: some View {
        HStack {
            ...
        }
        .padding()
        .sdmSurface(.toolbar)
    }
```

In `SDM/LinkGrabberView.swift`, add `.sdmSurface(.toolbar)` to `header`'s modifier chain:

```swift
    private var header: some View {
        let snapshot = controller.snapshot
        return VStack(alignment: .leading, spacing: 6) {
            ...
        }
        .padding()
        .sdmSurface(.toolbar)
    }
```

- [ ] **Step 3: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED. If `glassEffect(_:in:)`'s signature does not match this SDK exactly, the compiler error names the mismatch — adjust the call in `LiquidGlassSurface.swift` per the current SwiftUI documentation; every other file in this task is unaffected either way, which is the entire point of isolating it here.

Run the app on this machine (macOS 15 baseline, so the `.regularMaterial` fallback branch is what actually renders) and confirm by hand: the sidebar and the two toolbar-like bars still render with a translucent material background, matching their appearance before this task — this task should be visually a no-op on macOS 15, and its effect is unverifiable here except on an actual macOS 26 machine.

- [ ] **Step 4: Commit**

```bash
git add SDM/LiquidGlassSurface.swift SDM/MainWindowView.swift SDM/PackagesListView.swift SDM/LinkGrabberView.swift
git commit -m "feat: add the isolated Liquid Glass surface modifier and apply it to the sidebar and toolbars"
```

---

## Phase 4 completion criteria

- [ ] `swift test --package-path SDMKit` passes with no skipped tests, including every new test file added by this plan
- [ ] No `SDMEngine`/`SDMGrabber`/`SDMCore` test touches the network or sleeps on a real clock
- [ ] `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build` succeeds
- [ ] `AppTiming.ticksPerSecond` is the single place that controls the UI refresh rate; changing it to `10` and rebuilding requires no other code change (Tasks 1–4)
- [ ] The speed figure and sparklines visibly update several times a second while downloading, and the speed figure drops to `Zero KB/s` the instant a download pauses or completes rather than decaying (Tasks 2–4)
- [ ] A `Show.S01E01.*` / `Show.S01E02.*` link pair clusters and names as `"Show S01"`, not `"Show.S01E0"`; separators in generated names render as spaces with sensible capitalization (Task 5)
- [ ] The Overview panel reads "No running downloads" when nothing is active and "Zero KB/s" when something is running but idle; the Linkgrabber sidebar entry shows a numeric badge that tracks links not yet added to downloads (Task 6)
- [ ] `ThemeCatalog.builtInThemes()` returns 16 themes; every one passes the WCAG-AA contrast test for all nine text-tier/surface-tier pairs (Tasks 7–9)
- [ ] Selecting a theme in Settings re-colors every view that previously used a literal system color, with no restart; "System" tracks the OS appearance live (Tasks 10–11, 14)
- [ ] All three Dock/Menu Bar modes behave per spec §10.2's table, applying immediately including when no window is open (Task 12, 14)
- [ ] Quitting with an active non-resumable download shows a confirmation; quitting otherwise does not (Task 13)
- [ ] Liquid Glass is isolated to `LiquidGlassSurface.swift`; every other file this plan touches contains no `if #available(macOS 26` (Task 15)

## Deferred to later phases

Deliberately **not** in Phase 4, to keep it shippable:

- **Custom theme editor and theme import.** Spec §10.1 and §14 both explicitly defer this — `ThemeCatalog`'s JSON-file loader already generalizes to a second directory (e.g. Application Support) with no code change, but no UI for that exists yet. Not part of this plan.
- **Relocating a running or completed item's already-downloaded bytes when moved cross-package**, and **live-updating `GrabberSession.Budget` when Settings changes** — both carried forward unchanged from Phase 3's own deferred list; nothing in this plan touches either.
- **Per-error-class retry policies.** `RetryPolicy.classify` remains one global policy shared by every item; this plan only rescales its output from seconds to ticks, it does not change what gets classified how.
- **Verifying `.sdmSurface`'s macOS 26 `glassEffect` branch on real hardware.** This plan's own manual-verification step (Task 15) can only exercise the macOS 15 `.regularMaterial` fallback on this machine; the `if #available(macOS 26, *)` branch is unverified until run on macOS 26.
- **A theme-aware `NSColor`/native-control palette beyond `NSApp.appearance`.** Task 10 sets light/dark for native chrome (menus, window titlebar) but does not attempt to recolor native controls (buttons, steppers) to match a theme's specific accent hue — AppKit's own accent-color system, not spec §10.1's role list, governs those, and spec does not ask for it.
- **yt-dlp / YouTube resolver, muxing, format tables.** Phase 5, unchanged.
