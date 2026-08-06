# SDM Phase 3 — Shell, Scheduling Completeness & Notifications Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close out the scheduling gaps Phase 1 deferred (connection ceilings, hysteresis, manual retry), give the engine real persisted settings, and build the spec §13 Phase 3 UI shell — `NavigationSplitView`, drag-and-drop reordering, sparklines, the main bandwidth graph, the menu bar extra, notifications, and the Linkgrabber's manual package-override controls Phase 2 left unwired.

**Architecture:** Every new piece of scheduling logic (`ConnectionAllocator`, hysteresis tracking, manual retry) follows the engine's existing shape: pure value types or actor-isolated state mutated inside `DownloadEngine.reconcile()`, observable only through `EngineSnapshot`. The UI work is additive to the existing `EngineController`/`GrabberController` + `@Observable` + `@Environment` pattern — no new state-management approach is introduced. `SegmentedProgressBar` and the retry/backoff engine machinery already exist from Phase 1; this plan does not touch them except to surface `remainingAttempts`.

**Tech Stack:** Swift 6.2, Swift Testing, SwiftUI, Swift Charts, `UserNotifications`, `AppKit` (`MenuBarExtra`, `NSApp`). No third-party dependencies.

**Spec:** [docs/superpowers/specs/2026-08-03-sdm-design.md](../specs/2026-08-03-sdm-design.md). Read §5.4, §6.4, §9, §10.2 (window/dock split, not touched by this plan — that is Phase 4), §11.7 before starting. Also read the completed [Phase 1 plan](2026-08-03-phase-1-engine.md) and [Phase 2 plan](2026-08-04-phase-2-grabber.md) — this plan follows their exact TDD granularity, and Tasks 1–4 reuse Phase 1's `FakeOrigin`/`InMemoryStateStore`/`makeScratchDirectory`/`testPayload` test infrastructure directly.

## Global Constraints

- **Deployment target stays macOS 15.0.** Every API this plan uses (`NavigationSplitView`, `.draggable`/`.dropDestination`, `MenuBarExtra` with `.menuBarExtraStyle(.window)`, Swift Charts, `UNUserNotificationCenter`) is available at 15.0 — none needs an `if #available` gate. Verified against this machine's installed SDK; re-check if the implementer's toolchain differs.
- **Swift tools version 6.2**, Swift 6 language mode, strict concurrency enabled, confirmed installed: `swift --version` reports Apple Swift version 6.2 (swiftlang-6.2.0.19.9), target `x86_64-apple-macosx15.0`; `xcodebuild -version` reports Xcode 26.0.1 (Build 17A400); `swift-format --version` (at `/usr/local/bin/swift-format`, also via `xcrun`) reports 6.2.0.
- **Zero third-party dependencies.** Foundation, SwiftUI, Swift Charts, AppKit, `UserNotifications`, and Swift standard library only — all first-party frameworks.
- **Swift Testing only** (`@Test` / `#expect`). No XCTest in the package. `SDMKit`'s tests remain the only automated coverage; per spec §11.7, SwiftUI/AppKit-only code (menu bar, notifications, settings persistence) gets a "build and run, verify by hand" step, matching every prior UI-wiring task in Phases 1–2 — do not invent a snapshot-testing harness for it.
- **No `SDMEngine` test may touch the network or sleep on a real clock.** Every test in Tasks 1–5 uses `FakeOrigin`/`InMemoryStateStore`. Tasks 2, 3, and 5 need a download to still be mid-flight when the assertion runs; Task 2 adds a shared `WorkerGate`/`WorkerGatedOrigin` helper to `SDMKit/Tests/SDMEngineTests/TestSupport.swift` for exactly this — freezing every non-probe body fetch until the test explicitly opens the gate. **Reuse it in Tasks 3 and 5 rather than guessing a payload size that "probably" outlasts a tick** — `FakeOrigin` has no artificial per-chunk delay, so an ungated in-memory transfer of any size can complete before an assertion runs.
- **Format before every commit:** `swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests`, matching Phases 1–2.
- **Run tests with:** `swift test --package-path SDMKit`
- **New files under `SDM/` need no Xcode project edit.** `SDM.xcodeproj` uses `PBXFileSystemSynchronizedRootGroup` for the `SDM` folder (confirmed in `project.pbxproj`), so any `.swift` file created under `SDM/` is picked up automatically on the next build — unlike Phase 2's Task 14, which needed a manual `PBXBuildFile`/product-dependency edit only because it added a *new SPM product* (`SDMGrabber`). This plan adds no new SPM product, so no `project.pbxproj` edit is needed anywhere in this plan.
- **`EngineSettings.maxConnectionsPerHost` defaults to 8** (spec §12) so every existing call site across `SDMKit/Tests/SDMEngineTests/*.swift` that constructs `EngineSettings` without it keeps compiling unchanged.

---

### Task 1: `ConnectionAllocator` — pure per-host and global worker-pool shrinking

**Files:**
- Create: `SDMKit/Sources/SDMCore/ConnectionAllocator.swift`
- Create: `SDMKit/Tests/SDMCoreTests/ConnectionAllocatorTests.swift`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `struct ConnectionBudget: Sendable` — `global: Int`, `perHost: Int`
  - `struct ConnectionDemand: Sendable` — `id: UUID`, `host: String`, `desiredSegments: Int`
  - `enum ConnectionAllocator { static func allocate(demands: [ConnectionDemand], budget: ConnectionBudget) -> [UUID: Int] }`

Spec §6.4: "worker pools shrink below their configured N when the budget is tight, largest pool yielding first." This is a pure function over value types, table-tested exactly like `Scheduler` and `PackageClustering` — it has no knowledge of `DownloadEngine`, `DownloadTask`, or hosts beyond the bare `String` it is handed.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMCoreTests/ConnectionAllocatorTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMCore

@Test func allocateWithinBudgetReturnsRequestedCounts() {
    let a = ConnectionDemand(id: UUID(), host: "a.com", desiredSegments: 4)
    let b = ConnectionDemand(id: UUID(), host: "b.com", desiredSegments: 4)
    let allocated = ConnectionAllocator.allocate(
        demands: [a, b],
        budget: ConnectionBudget(global: 32, perHost: 8)
    )
    #expect(allocated[a.id] == 4)
    #expect(allocated[b.id] == 4)
}

@Test func allocateShrinksTheLargestPoolFirstUnderTheGlobalCap() {
    let small = ConnectionDemand(id: UUID(), host: "a.com", desiredSegments: 2)
    let large = ConnectionDemand(id: UUID(), host: "b.com", desiredSegments: 8)
    let allocated = ConnectionAllocator.allocate(
        demands: [small, large],
        budget: ConnectionBudget(global: 6, perHost: 8)
    )
    #expect(allocated[small.id] == 2)
    #expect(allocated[large.id] == 4)
}

@Test func allocateAppliesThePerHostCapIndependentlyOfTheGlobalCap() {
    let demand = ConnectionDemand(id: UUID(), host: "a.com", desiredSegments: 8)
    let allocated = ConnectionAllocator.allocate(
        demands: [demand],
        budget: ConnectionBudget(global: 32, perHost: 3)
    )
    #expect(allocated[demand.id] == 3)
}

@Test func allocateNeverShrinksAPoolBelowOne() {
    let demands = (0..<5).map { _ in ConnectionDemand(id: UUID(), host: "a.com", desiredSegments: 1) }
    let allocated = ConnectionAllocator.allocate(
        demands: demands,
        budget: ConnectionBudget(global: 2, perHost: 8)
    )
    #expect(demands.allSatisfy { (allocated[$0.id] ?? 0) == 1 })
}

@Test func allocateShrinksOnlyTheOverBudgetHostLeavingOthersUntouched() {
    let busy = ConnectionDemand(id: UUID(), host: "busy.com", desiredSegments: 10)
    let quiet = ConnectionDemand(id: UUID(), host: "quiet.com", desiredSegments: 4)
    let allocated = ConnectionAllocator.allocate(
        demands: [busy, quiet],
        budget: ConnectionBudget(global: 32, perHost: 5)
    )
    #expect(allocated[busy.id] == 5)
    #expect(allocated[quiet.id] == 4)
}

@Test func allocateOfEmptyDemandsIsEmpty() {
    #expect(ConnectionAllocator.allocate(demands: [], budget: ConnectionBudget(global: 8, perHost: 4)).isEmpty)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter ConnectionAllocatorTests`
Expected: FAIL — `cannot find 'ConnectionBudget' in scope`.

- [ ] **Step 3: Implement `ConnectionAllocator`**

`SDMKit/Sources/SDMCore/ConnectionAllocator.swift`:

```swift
import Foundation

public struct ConnectionBudget: Sendable {
    public var global: Int
    public var perHost: Int

    public init(global: Int, perHost: Int) {
        precondition(global >= 1, "global must be at least 1")
        precondition(perHost >= 1, "perHost must be at least 1")
        self.global = global
        self.perHost = perHost
    }
}

public struct ConnectionDemand: Sendable {
    public let id: UUID
    public let host: String
    public let desiredSegments: Int

    public init(id: UUID, host: String, desiredSegments: Int) {
        precondition(desiredSegments >= 1, "desiredSegments must be at least 1")
        self.id = id
        self.host = host
        self.desiredSegments = desiredSegments
    }
}

/// Shrinks per-item worker-pool sizes to fit within a global connection
/// ceiling and a per-host ceiling. Spec §6.4: "worker pools shrink below
/// their configured N when the budget is tight, largest pool yielding
/// first."
///
/// Never shrinks a pool below 1: if every pool is already at the floor and
/// the total still exceeds `budget.global`, the ceiling is a soft target
/// from here on — killing an item outright to enforce it is the scheduler's
/// job (`maxConcurrent`), not this one's.
public enum ConnectionAllocator {
    public static func allocate(
        demands: [ConnectionDemand],
        budget: ConnectionBudget
    ) -> [UUID: Int] {
        var allocated: [UUID: Int] = [:]
        for demand in demands { allocated[demand.id] = demand.desiredSegments }

        func totalForHost(_ host: String) -> Int {
            demands.filter { $0.host == host }.reduce(0) { $0 + (allocated[$1.id] ?? 0) }
        }
        func shrinkLargest(among candidates: [ConnectionDemand]) -> Bool {
            guard
                let victim = candidates
                    .filter({ (allocated[$0.id] ?? 0) > 1 })
                    .max(by: { (allocated[$0.id] ?? 0) < (allocated[$1.id] ?? 0) })
            else { return false }
            allocated[victim.id] = (allocated[victim.id] ?? 1) - 1
            return true
        }

        // Per-host pass first: each over-budget host is brought into line on
        // its own, independent of every other host.
        for host in Set(demands.map(\.host)) {
            let hostDemands = demands.filter { $0.host == host }
            while totalForHost(host) > budget.perHost {
                guard shrinkLargest(among: hostDemands) else { break }
            }
        }

        // Global pass: shrinks the largest remaining pool across every host
        // until the grand total fits.
        while allocated.values.reduce(0, +) > budget.global {
            guard shrinkLargest(among: demands) else { break }
        }

        return allocated
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
git commit -m "feat: add ConnectionAllocator for global and per-host worker-pool shrinking"
```

---

### Task 2: `DownloadEngine` enforces the global and per-host connection ceilings

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/DownloadEngine.swift`
- Modify: `SDMKit/Tests/SDMEngineTests/TestSupport.swift`
- Create: `SDMKit/Tests/SDMEngineTests/ConnectionBudgetEngineTests.swift`

**Interfaces:**
- Consumes: `ConnectionAllocator`, `ConnectionBudget`, `ConnectionDemand` from Task 1
- Produces:
  - `EngineSettings.maxConnectionsPerHost: Int` (new field, default `8`)
  - `WorkerGate` (actor) and `WorkerGatedOrigin` (`HTTPTransport`) added to `TestSupport.swift`, reusable by later tasks

Per spec §6.4, `globalMaxConnections` was carried in settings but never read (`DownloadEngine.swift:7`'s own comment: *"Carried for the UI and for Phase 3; not enforced here yet."*). This task makes `reconcile()` compute a live per-item worker-count cap on every pass and apply it both to newly started tasks and to already-running ones, via `DownloadTask.setWorkerCount` (already exists from Phase 1, used today only by `EngineController`'s manual segment-count override).

- [ ] **Step 1: Add `WorkerGate`/`WorkerGatedOrigin` to `TestSupport.swift`**

These freeze every non-probe body fetch until a test calls `open()`, so a live worker count can be inspected without racing an in-memory transfer that has no real latency. Append to `SDMKit/Tests/SDMEngineTests/TestSupport.swift`:

```swift
/// Freezes every non-probe body fetch until `open()` is called, so a test can
/// inspect `DownloadTask.activeWorkerCount` while every claimed worker is
/// still in flight. `FakeOrigin` has no artificial delay, so an ungated
/// transfer can complete before an assertion runs — this is the fix.
actor WorkerGate {
    private var isOpen = false
    private var waiters: [CheckedContinuation<Void, Never>] = []

    func waitUntilOpen() async {
        if isOpen { return }
        await withCheckedContinuation { waiters.append($0) }
    }

    func open() {
        isOpen = true
        let pending = waiters
        waiters.removeAll()
        for waiter in pending { waiter.resume() }
    }
}

struct WorkerGatedOrigin: HTTPTransport {
    let payload: Data
    let gate: WorkerGate

    func fetch(_ request: RangeRequest) async throws -> RangeResponse {
        let total = Int64(payload.count)
        let range = request.range ?? ByteRange(start: 0, end: total)
        let isProbe = range.start == 0 && range.end == 1
        guard !isProbe else {
            return RangeResponse(
                statusCode: 206,
                totalSize: total,
                acceptsRanges: true,
                validator: "etag-gated",
                body: AsyncThrowingStream { $0.finish() }
            )
        }

        let lower = Int(Swift.min(range.start, total))
        let upper = Int(Swift.min(range.end, total))
        let slice = payload.subdata(in: lower..<upper)
        let gate = self.gate
        let body = AsyncThrowingStream<Data, any Error> { continuation in
            Task {
                await gate.waitUntilOpen()
                continuation.yield(slice)
                continuation.finish()
            }
        }
        return RangeResponse(
            statusCode: 206,
            totalSize: total,
            acceptsRanges: true,
            validator: "etag-gated",
            body: body
        )
    }
}

func snapshotItem(_ id: UUID, in engine: DownloadEngine) async -> ItemSnapshot? {
    await engine.snapshot().packages.flatMap(\.items).first { $0.id == id }
}
```

- [ ] **Step 2: Write the failing test**

`SDMKit/Tests/SDMEngineTests/ConnectionBudgetEngineTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMEngine

@Test func reconcileShrinksTheLargestWorkerPoolFirstUnderTheGlobalConnectionCap() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let gate = WorkerGate()
    let engine = DownloadEngine(
        transport: WorkerGatedOrigin(payload: testPayload(200_000), gate: gate),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 2,
            segmentsPerItem: 1,
            globalMaxConnections: 3,
            maxConnectionsPerHost: 100,
            downloadFolder: dir
        )
    )

    let itemA = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    let itemB = DownloadItem(url: URL(string: "https://example.com/b.bin")!, filename: "b.bin")
    await engine.setSegmentCount(4, for: itemA.id)
    await engine.setSegmentCount(1, for: itemB.id)
    await engine.add(DownloadPackage(name: "Batch", items: [itemA, itemB]))

    var spins = 0
    while await snapshotItem(itemA.id, in: engine)?.activeSegments != 2, spins < 100_000 {
        await Task.yield()
        spins += 1
    }
    #expect(spins < 100_000)
    #expect(await snapshotItem(itemB.id, in: engine)?.activeSegments == 1)

    await gate.open()
    try await engine.runUntilIdle()
}

@Test func perHostCapAppliesEvenWhenTheGlobalCapHasRoom() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let gate = WorkerGate()
    let engine = DownloadEngine(
        transport: WorkerGatedOrigin(payload: testPayload(200_000), gate: gate),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1,
            segmentsPerItem: 1,
            globalMaxConnections: 32,
            maxConnectionsPerHost: 2,
            downloadFolder: dir
        )
    )

    let item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    await engine.setSegmentCount(4, for: item.id)
    await engine.add(DownloadPackage(name: "Batch", items: [item]))

    var spins = 0
    while await snapshotItem(item.id, in: engine)?.activeSegments != 2, spins < 100_000 {
        await Task.yield()
        spins += 1
    }
    #expect(spins < 100_000)

    await gate.open()
    try await engine.runUntilIdle()
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter ConnectionBudgetEngineTests`
Expected: FAIL — `argument 'maxConnectionsPerHost' not found` (or, once the field exists but is unused, the pool never shrinks: `activeSegments` stays 4).

- [ ] **Step 4: Add `maxConnectionsPerHost` to `EngineSettings`**

In `SDMKit/Sources/SDMEngine/DownloadEngine.swift`, replace the `EngineSettings` struct:

```swift
public struct EngineSettings: Sendable {
    public var maxConcurrent: Int
    public var segmentsPerItem: Int
    public var globalMaxConnections: Int
    /// Spec §6.4 and §12 (default 8): enforced together with
    /// `globalMaxConnections` by `ConnectionAllocator`, largest pool
    /// yielding first.
    public var maxConnectionsPerHost: Int
    public var downloadFolder: URL
    /// Bytes written per worker between sidecar checkpoints (spec §4.3's byte
    /// half). Settable so a test can drive the checkpoint path without moving
    /// eight megabytes.
    public var checkpointIntervalBytes: Int64
    /// Ticks of the 1 Hz heartbeat that must pass with no further change
    /// before durable state is written. Spec §4.2's "~2 s after the last
    /// change" debounce.
    public var persistDebounceTicks: Int

    public init(
        maxConcurrent: Int,
        segmentsPerItem: Int,
        globalMaxConnections: Int,
        maxConnectionsPerHost: Int = 8,
        downloadFolder: URL,
        checkpointIntervalBytes: Int64 = 8 * 1024 * 1024,
        persistDebounceTicks: Int = 2
    ) {
        precondition(maxConcurrent >= 1, "maxConcurrent must be at least 1")
        precondition(segmentsPerItem >= 1, "segmentsPerItem must be at least 1")
        precondition(globalMaxConnections >= 1, "globalMaxConnections must be at least 1")
        precondition(maxConnectionsPerHost >= 1, "maxConnectionsPerHost must be at least 1")
        precondition(checkpointIntervalBytes > 0, "checkpointIntervalBytes must be positive")
        precondition(persistDebounceTicks >= 1, "persistDebounceTicks must be at least 1")
        self.maxConcurrent = maxConcurrent
        self.segmentsPerItem = segmentsPerItem
        self.globalMaxConnections = globalMaxConnections
        self.maxConnectionsPerHost = maxConnectionsPerHost
        self.downloadFolder = downloadFolder
        self.checkpointIntervalBytes = checkpointIntervalBytes
        self.persistDebounceTicks = persistDebounceTicks
    }
}
```

- [ ] **Step 5: Compute and apply the allocation inside `reconcile()`**

Add this private helper just above `private func reconcile() async {` in `DownloadEngine.swift`:

```swift
/// Connection demand for a set of items: their real host and their
/// requested (not yet budget-capped) worker count.
private func connectionDemands(for itemIDs: Set<UUID>) -> [ConnectionDemand] {
    var demands: [ConnectionDemand] = []
    for package in packages {
        for item in package.items where itemIDs.contains(item.id) {
            demands.append(
                ConnectionDemand(
                    id: item.id,
                    host: item.url.host ?? "",
                    desiredSegments: segmentCount(for: item.id)
                )
            )
        }
    }
    return demands
}
```

Inside `reconcile()`, immediately after the `desired` set is computed (right after the `Scheduler.desiredRunningSet(...)` call and before `var changed = false`), add:

```swift
let allocatedSegments = ConnectionAllocator.allocate(
    demands: connectionDemands(for: desired),
    budget: ConnectionBudget(global: settings.globalMaxConnections, perHost: settings.maxConnectionsPerHost)
)
```

In the starting loop, change the `DownloadTask.Configuration` construction from `workerCount: runContext.segments` to:

```swift
workerCount: allocatedSegments[itemID] ?? runContext.segments,
```

Immediately after the starting `for itemID in desired where runners[itemID] == nil { ... }` loop closes (still inside `reconcile()`, before `for task in retiring { await task.pause() }`), add a loop that re-clamps already-running items every pass — this is what makes the cap responsive to items finishing and freeing budget for their siblings, not just to fresh starts:

```swift
// Re-clamp every already-running item too, not just fresh starts: a
// sibling finishing frees budget the survivors should grow back into,
// and a newly-added item can just as easily squeeze existing ones down.
for (itemID, runner) in runners where !runner.isRetiring && desired.contains(itemID) {
    if let allocated = allocatedSegments[itemID] {
        await runner.task.setWorkerCount(allocated)
    }
}
```

- [ ] **Step 6: Add the `SDMCore` import if not already present**

`DownloadEngine.swift` already has `import SDMCore` at the top — no change needed. Confirm this before moving on.

- [ ] **Step 7: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite (confirms the new `maxConnectionsPerHost` default parameter did not break any existing `EngineSettings(...)` call site).

- [ ] **Step 8: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: enforce global and per-host connection ceilings in DownloadEngine"
```

---

### Task 3: Hysteresis — protect recently-started items from preemption

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/DownloadEngine.swift`
- Create: `SDMKit/Tests/SDMEngineTests/HysteresisTests.swift`

**Interfaces:**
- Consumes: `Scheduler.desiredRunningSet`'s existing `startedRecently` parameter (already implemented and tested in `SchedulerTests.swift` since Phase 1 — only the engine-side wiring is missing)
- Produces: no new public API; `reconcile()` now passes a real `startedRecently` set instead of `[]`

Per spec §6.4: "An item started within the last ~5 s is not preempted." `DownloadEngine.swift:374`'s comment already flags this as deferred to Phase 3. The engine has a natural 1 Hz tick already (used for checkpoint staleness and retry backoff); hysteresis rides the same counter rather than a wall clock, per this codebase's established "no injected Clock" convention (see project `CLAUDE.md`'s Testing section).

- [ ] **Step 1: Write the failing test**

`SDMKit/Tests/SDMEngineTests/HysteresisTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMEngine

/// A running item that has not yet been probed (`isResumable == nil`) is not
/// protected by the scheduler's pass 1 (`isResumable == false` only), so this
/// isolates hysteresis specifically: without it, a higher-priority item added
/// a moment later would preempt A instantly.
@Test func recentlyStartedRunningItemsSurviveAPreemptingAdditionUntilTheHysteresisWindowElapses()
    async throws
{
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let gate = WorkerGate()
    let engine = DownloadEngine(
        transport: WorkerGatedOrigin(payload: testPayload(4000), gate: gate),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1,
            segmentsPerItem: 1,
            globalMaxConnections: 8,
            downloadFolder: dir
        )
    )

    let itemA = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    await engine.add(DownloadPackage(name: "Batch", items: [itemA]))

    var spins = 0
    while await snapshotItem(itemA.id, in: engine)?.state != .running, spins < 100_000 {
        await Task.yield()
        spins += 1
    }
    #expect(spins < 100_000)

    let itemB = DownloadItem(
        url: URL(string: "https://example.com/b.bin")!,
        filename: "b.bin",
        priority: .highest
    )
    await engine.add(DownloadPackage(name: "Batch", items: [itemB]))

    // Still inside the hysteresis window: A keeps its slot despite B outranking it.
    #expect(await snapshotItem(itemA.id, in: engine)?.state == .running)
    #expect(await snapshotItem(itemB.id, in: engine)?.state == .queued)

    for _ in 0..<5 { await engine.tick() }

    // Window elapsed: B now preempts A.
    #expect(await snapshotItem(itemA.id, in: engine)?.state == .queued)
    #expect(await snapshotItem(itemB.id, in: engine)?.state == .running)

    await gate.open()
    try await engine.runUntilIdle()
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path SDMKit --filter HysteresisTests`
Expected: FAIL — after `for _ in 0..<5 { await engine.tick() }`, item A is still `.running` (never preempted, since `startedRecently` is hardcoded `[]` and hysteresis, being unwired, does nothing either way — the real bug this exposes is that B never gets to run at all, since without real hysteresis pass 3's rank-fill should have preempted A immediately, so the *first* pair of assertions — A still running right after adding B — already fails against current code, since nothing protects A at all and pass 3 preempts it on the spot).

- [ ] **Step 3: Track start ticks and wire `startedRecently`**

In `DownloadEngine.swift`, add two new private properties near `retryHoldTicks`:

```swift
/// The heartbeat tick each currently-tracked runner started on, so
/// `reconcile()` can tell whether it is still inside spec §6.4's hysteresis
/// window. Cleared when the runner finishes, in `finish()`.
private var startedAtTick: [UUID: Int] = [:]
/// Ticks elapsed since the engine started, driven by `tick()`. Counted
/// rather than measured against a clock, matching every other tick-driven
/// mechanism here (`retryHoldTicks`, checkpoint staleness).
private var currentTick: Int = 0
/// Spec §6.4: "An item started within the last ~5 s is not preempted."
private let hysteresisWindowTicks = 5
```

At the very first line of `tick()`, add:

```swift
currentTick += 1
```

In `reconcile()`, replace `startedRecently: [],` with:

```swift
startedRecently: Set(
    startedAtTick.filter { currentTick - $0.value < hysteresisWindowTicks }.keys
),
```

and update the doc comment above `reconcile()` — replace the sentence *"`startedRecently` is empty: hysteresis needs a clock and is deferred to Phase 3."* with:

```swift
    /// `startedRecently` is derived from `startedAtTick` and the tick-counted
    /// hysteresis window (spec §6.4), rather than a wall clock — see
    /// `currentTick`.
```

In the starting loop (`for itemID in desired where runners[itemID] == nil { ... }`), right after `mutateItem(itemID) { $0.state = .running }`, add:

```swift
startedAtTick[itemID] = currentTick
```

In `finish(itemID:task:completed:totalBytes:isResumable:state:)`, right after `runners[itemID] = nil`, add:

```swift
startedAtTick[itemID] = nil
```

- [ ] **Step 4: Run test to verify it passes**

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite.

- [ ] **Step 5: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: wire tick-counted hysteresis into the scheduler's startedRecently input"
```

---

### Task 4: Manual retry and `remainingAttempts`

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/EngineSnapshot.swift`
- Modify: `SDMKit/Sources/SDMEngine/DownloadEngine.swift`
- Create: `SDMKit/Tests/SDMEngineTests/ManualRetryTests.swift`

**Interfaces:**
- Consumes: `ItemState`, `RetryPolicy` from Phase 1
- Produces:
  - `ItemSnapshot.remainingAttempts: Int?` — `nil` when the item has never failed, otherwise `retryPolicy.maxAttempts - failedAttempts[id]`
  - `public func DownloadEngine.retry(_ itemID: UUID) async` — resurrects a terminally `.failed` item; a no-op for any other state

Spec §6.4 promises a terminal `.failed(reason:)` "surfaced with a reason **and a manual retry action**." Phase 1's deferred section flagged this remainder explicitly. `Scheduler.isEligible` excludes `.failed` unconditionally (`Scheduler.swift:103`), so `retry` must flip the state back to `.queued`, not merely toggle `isEnabled`.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMEngineTests/ManualRetryTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMEngine

@Test func remainingAttemptsIsNilWhenAnItemHasNeverFailed() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(1000)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let package = DownloadPackage(
        name: "Batch",
        items: [DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")]
    )
    await engine.add(package)
    try await engine.runUntilIdle()

    #expect(await snapshotItem(package.items[0].id, in: engine)?.remainingAttempts == nil)
}

@Test func remainingAttemptsCountsDownAfterATransientFailure() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    var behavior = FakeOrigin.Behavior()
    behavior.statusOverride = 500
    let origin = FakeOrigin(payload: testPayload(1000), behavior: behavior)
    let engine = DownloadEngine(
        transport: origin,
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir),
        retryPolicy: RetryPolicy(maxAttempts: 3, baseDelay: .seconds(30))
    )
    let package = DownloadPackage(
        name: "Batch",
        items: [DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")]
    )
    await engine.add(package)
    try await engine.runUntilIdle()

    #expect(await snapshotItem(package.items[0].id, in: engine)?.remainingAttempts == 2)
    #expect(await snapshotItem(package.items[0].id, in: engine)?.state == .queued)
}

@Test func retryOnAFailedItemResumesItAfterTheOriginRecovers() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    var behavior = FakeOrigin.Behavior()
    behavior.statusOverride = 404
    let origin = FakeOrigin(payload: testPayload(1000), behavior: behavior)
    let engine = DownloadEngine(
        transport: origin,
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let package = DownloadPackage(
        name: "Batch",
        items: [DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")]
    )
    let itemID = package.items[0].id
    await engine.add(package)
    try await engine.runUntilIdle()

    guard case .failed = await snapshotItem(itemID, in: engine)?.state else {
        Issue.record("expected the item to be terminally failed after a 404")
        return
    }

    await origin.setBehavior(FakeOrigin.Behavior())
    await engine.retry(itemID)
    try await engine.runUntilIdle()

    #expect(await snapshotItem(itemID, in: engine)?.state == .completed)
    #expect(
        try Data(
            contentsOf: dir.appendingPathComponent("Batch").appendingPathComponent("a.bin")
        ) == testPayload(1000)
    )
}

@Test func retryOnAnyNonFailedItemIsANoOp() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(1000)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let package = DownloadPackage(
        name: "Batch",
        items: [DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")]
    )
    let itemID = package.items[0].id
    await engine.add(package)
    try await engine.runUntilIdle()

    await engine.retry(itemID)
    #expect(await snapshotItem(itemID, in: engine)?.state == .completed)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter ManualRetryTests`
Expected: FAIL — `value of type 'ItemSnapshot' has no member 'remainingAttempts'`.

- [ ] **Step 3: Add `remainingAttempts` to `ItemSnapshot`**

In `SDMKit/Sources/SDMEngine/EngineSnapshot.swift`, add a field to `ItemSnapshot`:

```swift
    public let checkpointFailure: String?
    /// Attempts left before this item becomes terminally `.failed`, or `nil`
    /// if it has never failed. Spec §6.4's "manual retry action" needs this
    /// to show the operator how much budget is left before giving up.
    public let remainingAttempts: Int?
```

and thread it through the initializer:

```swift
    public init(
        id: UUID,
        url: URL,
        filename: String,
        totalBytes: Int64?,
        completed: RangeSet,
        state: ItemState,
        isEnabled: Bool,
        isResumable: Bool?,
        activeSegments: Int,
        configuredSegments: Int,
        bytesPerSecond: Double,
        speedHistory: [Double],
        checkpointFailure: String? = nil,
        remainingAttempts: Int? = nil
    ) {
        self.checkpointFailure = checkpointFailure
        self.remainingAttempts = remainingAttempts
        self.id = id
        self.url = url
        self.filename = filename
        self.totalBytes = totalBytes
        self.completed = completed
        self.state = state
        self.isEnabled = isEnabled
        self.isResumable = isResumable
        self.activeSegments = activeSegments
        self.configuredSegments = configuredSegments
        self.bytesPerSecond = bytesPerSecond
        self.speedHistory = speedHistory
    }
```

- [ ] **Step 4: Populate it in `DownloadEngine.snapshot()` and add `retry(_:)`**

In `DownloadEngine.swift`'s `snapshot()`, add `remainingAttempts:` to the `ItemSnapshot(...)` construction:

```swift
                items.append(
                    ItemSnapshot(
                        id: item.id,
                        url: item.url,
                        filename: item.filename,
                        totalBytes: totalBytes,
                        completed: completed,
                        state: item.state,
                        isEnabled: item.isEnabled,
                        isResumable: item.isResumable,
                        activeSegments: active,
                        configuredSegments: segmentCount(for: item.id),
                        bytesPerSecond: sampler.bytesPerSecond,
                        speedHistory: sampler.history,
                        checkpointFailure: checkpointFailure,
                        remainingAttempts: failedAttempts[item.id].map { retryPolicy.maxAttempts - $0 }
                    )
                )
```

Add `retry(_:)` in the `// MARK: - Mutations` section, after `setSegmentCount`:

```swift
    /// Manually retries a terminally `.failed` item. Spec §6.4 promised "a
    /// terminal `failed` state surfaced with a reason and a manual retry
    /// action" — this is that action.
    ///
    /// A no-op for any other state: `Scheduler.isEligible` excludes `.failed`
    /// unconditionally, so `.queued` is what makes the item reachable again,
    /// not merely re-enabling it. Guarding on state rather than acting
    /// unconditionally means a button wired to every row cannot accidentally
    /// hand a healthy download a fresh retry budget it never asked for.
    public func retry(_ itemID: UUID) async {
        guard case .failed = itemState(for: itemID) else { return }
        mutateItem(itemID) {
            $0.state = .queued
            $0.isEnabled = true
        }
        failedAttempts[itemID] = nil
        retryHoldTicks[itemID] = nil
        await persist()
        await reconcile()
    }

    private func itemState(for itemID: UUID) -> ItemState? {
        for package in packages {
            if let item = package.items.first(where: { $0.id == itemID }) { return item.state }
        }
        return nil
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite.

- [ ] **Step 6: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: add manual retry and remainingAttempts to the engine"
```

---

### Task 5: `EngineSettingsStore` and `DownloadEngine.updateSettings`

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/DownloadEngine.swift`
- Create: `SDMKit/Tests/SDMEngineTests/UpdateSettingsTests.swift`
- Create: `SDM/EngineSettingsStore.swift`
- Modify: `SDM/EngineController.swift`

**Interfaces:**
- Consumes: `EngineSettings` from Task 2
- Produces:
  - `public func DownloadEngine.updateSettings(_ newSettings: EngineSettings) async`
  - `@MainActor enum EngineSettingsStore` — `UserDefaults`-backed, mirroring `GrabberSettings`'s existing pattern exactly (`SDM/GrabberSettings.swift`)
  - `EngineController.init()` now reads its starting `EngineSettings` from `EngineSettingsStore` instead of the hardcoded `3 / 8 / 32` literals at `SDM/EngineController.swift:43-47`
  - `EngineController.applyStoredSettings() async`

There is still no unified Settings model (confirmed: no `Settings.swift`, no settings persistence anywhere in `SDMEngine`/`SDMCore` — `StateStore` only ever persists the package/item graph). This task gives the engine side of settings the same `UserDefaults` treatment `GrabberSettings` already has for the grabber side, per spec §12's defaults.

- [ ] **Step 1: Write the failing engine-level test**

`SDMKit/Tests/SDMEngineTests/UpdateSettingsTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMEngine

@Test func updateSettingsAppliesTheNewMaxConcurrentOnItsNextReconcile() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let gate = WorkerGate()
    let engine = DownloadEngine(
        transport: WorkerGatedOrigin(payload: testPayload(4000), gate: gate),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let items = (0..<2).map {
        DownloadItem(url: URL(string: "https://example.com/\($0).bin")!, filename: "\($0).bin")
    }
    await engine.add(DownloadPackage(name: "Batch", items: items))

    #expect(await runningCount(in: engine) == 1)

    await engine.updateSettings(
        EngineSettings(
            maxConcurrent: 2, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )

    #expect(await runningCount(in: engine) == 2)

    await gate.open()
    try await engine.runUntilIdle()
}

private func runningCount(in engine: DownloadEngine) async -> Int {
    await engine.snapshot().packages.flatMap(\.items).filter { $0.state == .running }.count
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path SDMKit --filter UpdateSettingsTests`
Expected: FAIL — `value of type 'DownloadEngine' has no member 'updateSettings'`.

- [ ] **Step 3: Add `updateSettings(_:)`**

In `DownloadEngine.swift`, add to the `// MARK: - Mutations` section, after `restore()`:

```swift
    /// Applies new settings live. Spec §10.2's "changing the mode applies
    /// immediately" precedent extends to every engine setting: the very next
    /// `reconcile()` this call triggers picks up the new
    /// `maxConcurrent`/connection ceilings with no restart needed.
    public func updateSettings(_ newSettings: EngineSettings) async {
        settings = newSettings
        await reconcile()
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `swift test --package-path SDMKit`
Expected: PASS.

- [ ] **Step 5: Create `EngineSettingsStore`**

`SDM/EngineSettingsStore.swift`:

```swift
import Foundation

/// Backs the engine-side settings from spec §12: max concurrent downloads,
/// segments per file, and the two connection ceilings. Mirrors
/// `GrabberSettings`'s direct-`UserDefaults` pattern — there is still no
/// single unified Settings model, just per-area stores.
@MainActor
enum EngineSettingsStore {
    private static let defaults = UserDefaults.standard

    private enum Key {
        static let maxConcurrent = "sdm.maxConcurrentDownloads"
        static let segmentsPerItem = "sdm.segmentsPerItem"
        static let globalMaxConnections = "sdm.globalMaxConnections"
        static let maxConnectionsPerHost = "sdm.maxConnectionsPerHost"
    }

    static var maxConcurrent: Int {
        get { defaults.object(forKey: Key.maxConcurrent) as? Int ?? 3 }
        set { defaults.set(newValue, forKey: Key.maxConcurrent) }
    }

    static var segmentsPerItem: Int {
        get { defaults.object(forKey: Key.segmentsPerItem) as? Int ?? 8 }
        set { defaults.set(newValue, forKey: Key.segmentsPerItem) }
    }

    static var globalMaxConnections: Int {
        get { defaults.object(forKey: Key.globalMaxConnections) as? Int ?? 32 }
        set { defaults.set(newValue, forKey: Key.globalMaxConnections) }
    }

    static var maxConnectionsPerHost: Int {
        get { defaults.object(forKey: Key.maxConnectionsPerHost) as? Int ?? 8 }
        set { defaults.set(newValue, forKey: Key.maxConnectionsPerHost) }
    }
}
```

- [ ] **Step 6: Wire `EngineController` to read and apply it**

In `SDM/EngineController.swift`, replace the `init()` body:

```swift
    private let downloadFolder: URL

    init() {
        let support = FileManager.default.urls(
            for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("SDM", isDirectory: true)
        let downloads = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask)[0]
        downloadFolder = downloads

        engine = DownloadEngine(
            transport: URLSessionTransport(),
            stateStore: JSONStateStore(fileURL: support.appendingPathComponent("state.json")),
            settings: EngineSettings(
                maxConcurrent: EngineSettingsStore.maxConcurrent,
                segmentsPerItem: EngineSettingsStore.segmentsPerItem,
                globalMaxConnections: EngineSettingsStore.globalMaxConnections,
                maxConnectionsPerHost: EngineSettingsStore.maxConnectionsPerHost,
                downloadFolder: downloads
            )
        )
    }

    /// Re-reads every `EngineSettingsStore` value and applies it live. Called
    /// from `SettingsView` whenever the operator changes a value.
    func applyStoredSettings() async {
        await engine.updateSettings(
            EngineSettings(
                maxConcurrent: EngineSettingsStore.maxConcurrent,
                segmentsPerItem: EngineSettingsStore.segmentsPerItem,
                globalMaxConnections: EngineSettingsStore.globalMaxConnections,
                maxConnectionsPerHost: EngineSettingsStore.maxConnectionsPerHost,
                downloadFolder: downloadFolder
            )
        )
    }
```

(`private nonisolated let engine: DownloadEngine` above stays as-is — only the `init()` body and the new `downloadFolder`/`applyStoredSettings()` are added.)

- [ ] **Step 7: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED. No behavior to verify by hand yet — `SettingsView` (Task 6) is what actually calls `applyStoredSettings()`.

- [ ] **Step 8: Run the full package test suite**

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite.

- [ ] **Step 9: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add -A
git commit -m "feat: persist engine settings in UserDefaults and apply them live"
```

---

### Task 6: `SettingsView` and the `Settings` scene

**Files:**
- Create: `SDM/SettingsView.swift`
- Modify: `SDM/SDMApp.swift`

**Interfaces:**
- Consumes: `EngineSettingsStore` (Task 5), `GrabberSettings` (Phase 2)
- Produces: `struct SettingsView: View`, wired into a native SwiftUI `Settings` scene (opened via ⌘, automatically on macOS)

Spec §12's whole settings table finally gets a screen — Phase 1/2 explicitly deferred this ("There is no dedicated Settings screen yet"). This is UI-only glue; per spec §11.7 it gets a manual verification step, not an automated test.

- [ ] **Step 1: Create `SettingsView`**

`SDM/SettingsView.swift`:

```swift
import SwiftUI

struct SettingsView: View {
    @Environment(EngineController.self) private var controller
    @State private var maxConcurrent = EngineSettingsStore.maxConcurrent
    @State private var segmentsPerItem = EngineSettingsStore.segmentsPerItem
    @State private var globalMaxConnections = EngineSettingsStore.globalMaxConnections
    @State private var maxConnectionsPerHost = EngineSettingsStore.maxConnectionsPerHost
    @State private var clipboardWatchingEnabled = GrabberSettings.clipboardWatchingEnabled
    @State private var autoAddAndStartOnGrab = GrabberSettings.autoAddAndStartOnGrab
    @State private var deepSniffEnabled = GrabberSettings.deepSniffEnabled

    var body: some View {
        Form {
            Section("Downloads") {
                Stepper("Max concurrent downloads: \(maxConcurrent)", value: $maxConcurrent, in: 1...20)
                Stepper("Segments per file: \(segmentsPerItem)", value: $segmentsPerItem, in: 1...64)
                Stepper(
                    "Global max connections: \(globalMaxConnections)",
                    value: $globalMaxConnections, in: 1...256
                )
                Stepper(
                    "Max connections per host: \(maxConnectionsPerHost)",
                    value: $maxConnectionsPerHost, in: 1...64
                )
            }
            Section("Linkgrabber") {
                Toggle("Watch clipboard for links", isOn: $clipboardWatchingEnabled)
                Toggle("Auto-add and start on grab", isOn: $autoAddAndStartOnGrab)
                Toggle("Deep sniff (stage 2)", isOn: $deepSniffEnabled)
            }
        }
        .padding()
        .frame(width: 420)
        .onChange(of: maxConcurrent) { _, _ in applyEngineSettings() }
        .onChange(of: segmentsPerItem) { _, _ in applyEngineSettings() }
        .onChange(of: globalMaxConnections) { _, _ in applyEngineSettings() }
        .onChange(of: maxConnectionsPerHost) { _, _ in applyEngineSettings() }
        .onChange(of: clipboardWatchingEnabled) { _, new in
            GrabberSettings.clipboardWatchingEnabled = new
        }
        .onChange(of: autoAddAndStartOnGrab) { _, new in GrabberSettings.autoAddAndStartOnGrab = new }
        .onChange(of: deepSniffEnabled) { _, new in GrabberSettings.deepSniffEnabled = new }
    }

    private func applyEngineSettings() {
        EngineSettingsStore.maxConcurrent = maxConcurrent
        EngineSettingsStore.segmentsPerItem = segmentsPerItem
        EngineSettingsStore.globalMaxConnections = globalMaxConnections
        EngineSettingsStore.maxConnectionsPerHost = maxConnectionsPerHost
        Task { await controller.applyStoredSettings() }
    }
}
```

- [ ] **Step 2: Add the `Settings` scene**

In `SDM/SDMApp.swift`, add a second scene after the `WindowGroup`'s closing brace, still inside `var body: some Scene { ... }`:

```swift
        Settings {
            SettingsView()
                .environment(controller)
        }
```

- [ ] **Step 3: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED.

Then run the app (`⌘R`), press `⌘,` to open Settings, and confirm by hand: changing "Max concurrent downloads" and adding more than that many downloads shows only the configured number as `.running` at once; toggling "Watch clipboard for links" off and copying a link no longer grabs it.

- [ ] **Step 4: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add -A
git commit -m "feat: add a Settings screen for engine and grabber preferences"
```

---

### Task 7: `NavigationSplitView` shell, pinned stats block, and the Completed view

**Files:**
- Modify: `SDM/ContentView.swift`
- Create: `SDM/BandwidthGraph.swift`

**Interfaces:**
- Consumes: `EngineSnapshot.globalHistory` (already exists, populated since Phase 1 — `DownloadEngine.swift:94`'s 300-sample global sampler)
- Produces:
  - `struct BandwidthGraph: View` — Swift Charts filled area + running-average line, spec §9.6
  - `ContentView.SidebarItem: Hashable` — `.downloads`, `.linkgrabber`, `.completed`
  - `ContentView`'s body becomes a `NavigationSplitView`, replacing the Phase 1/2 `TabView`

Spec §9.1: sidebar with Downloads / Linkgrabber / Completed plus a pinned live stats block; "Completed is a filtered view of the same list, not a separate store." Spec §9.6 reserves Swift Charts for exactly one place: this main graph.

- [ ] **Step 1: Create `BandwidthGraph`**

`SDM/BandwidthGraph.swift`:

```swift
import Charts
import SwiftUI

/// Spec §9.6: Swift Charts is reserved for the one place it earns its
/// cost — everywhere else (per-row sparklines) uses a plain `Canvas`, since a
/// full `Chart` per row at hundreds of rows turns scrolling into a slideshow.
struct BandwidthGraph: View {
    let history: [Double]

    var body: some View {
        Chart {
            ForEach(Array(history.enumerated()), id: \.offset) { index, value in
                AreaMark(x: .value("Tick", index), y: .value("Bytes/s", value))
                    .foregroundStyle(.tint.opacity(0.25))
                LineMark(x: .value("Tick", index), y: .value("Average", runningAverage[index]))
                    .foregroundStyle(.tint)
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

- [ ] **Step 2: Rewrite `ContentView` as a `NavigationSplitView`**

Replace the whole `struct ContentView: View { ... }` block in `SDM/ContentView.swift` (everything from `struct ContentView: View {` through its closing `}`, i.e. lines 12–69 of the file read at the start of this plan) with:

```swift
struct ContentView: View {
    enum SidebarItem: Hashable {
        case downloads, linkgrabber, completed
    }

    @Environment(EngineController.self) private var controller
    @Environment(GrabberController.self) private var grabberController
    @State private var selection: SidebarItem? = .downloads
    @State private var urlText = ""

    var body: some View {
        NavigationSplitView {
            List(selection: $selection) {
                Label("Downloads", systemImage: "arrow.down.circle").tag(SidebarItem.downloads)
                Label("Linkgrabber", systemImage: "link").tag(SidebarItem.linkgrabber)
                Label("Completed", systemImage: "checkmark.circle").tag(SidebarItem.completed)
                Section("Overview") { statsBlock }
            }
            .navigationSplitViewColumnWidth(min: 200, ideal: 220)
        } detail: {
            switch selection ?? .downloads {
            case .downloads: downloadsTab
            case .linkgrabber: LinkGrabberView()
            case .completed: completedTab
            }
        }
        .frame(minWidth: 760, minHeight: 480)
        .onChange(of: controller.snapshot) { _, newSnapshot in
            let urls = Set(newSnapshot.packages.flatMap { $0.items.map(\.url) })
            Task { await grabberController.setKnownDownloadURLs(urls) }
        }
    }

    private var statsBlock: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(formatted(controller.snapshot.globalBytesPerSecond)).font(.headline.monospacedDigit())
            BandwidthGraph(history: controller.snapshot.globalHistory).frame(height: 40)
            Text("\(activeCount) active").font(.caption).foregroundStyle(.secondary)
        }
        .padding(.vertical, 4)
    }

    private var activeCount: Int {
        controller.snapshot.packages.flatMap(\.items).filter { $0.state == .running }.count
    }

    /// Spec §9.1: "Completed is a filtered view of the same list, not a
    /// separate store." A predicate over `controller.snapshot`, nothing more.
    private var completedTab: some View {
        List {
            ForEach(controller.snapshot.packages) { package in
                let completedItems = package.items.filter { $0.state == .completed }
                if !completedItems.isEmpty {
                    Section(package.name) {
                        ForEach(completedItems) { item in ItemRow(item: item, controller: controller) }
                    }
                }
            }
        }
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
}
```

Everything below `ContentView`'s closing brace in the file — `private struct ItemRow`, `struct SegmentedProgressBar`, `formatted(_:)` — is unchanged; leave it as-is. (Task 8 will further modify `downloadsTab`'s `List`/`ForEach` for drag-and-drop, and Task 4's retry button gets added to `ItemRow` there too — see that task.)

- [ ] **Step 3: Add a retry button to `ItemRow`, closing out Task 4's UI side**

In `ItemRow`'s `body`, right after the existing `Button(item.isEnabled ? "Stop" : "Start") { ... }`, add:

```swift
                if item.remainingAttempts != nil, isFailed {
                    Button("Retry") {
                        let id = item.id
                        Task { await controller.retry(id) }
                    }
                    .controlSize(.small)
                }
```

`isFailed` already exists as a computed property on `ItemRow`. Add the corresponding `EngineController` wrapper — in `SDM/EngineController.swift`, after `setEnabled`:

```swift
    func retry(_ itemID: UUID) async {
        await engine.retry(itemID)
        snapshot = await engine.snapshot()
    }
```

- [ ] **Step 4: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED.

Run the app and confirm by hand: the sidebar shows Downloads / Linkgrabber / Completed plus a live speed figure and graph; clicking Completed shows only finished items, grouped by package, and stays empty until something finishes; a download that fails (e.g. a bad URL) shows a Retry button that, after fixing the URL is out of scope to test by hand, at least does not crash when pressed.

- [ ] **Step 5: Run the full package test suite**

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite (this task touches no `SDMKit` code, but confirms nothing regressed).

- [ ] **Step 6: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add -A
git commit -m "feat: replace the TabView shell with NavigationSplitView, stats block, and Completed"
```

---

### Task 8: Drag-and-drop reordering

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/DownloadEngine.swift`
- Create: `SDMKit/Tests/SDMEngineTests/ReorderingTests.swift`
- Modify: `SDM/ContentView.swift`
- Modify: `SDM/EngineController.swift`

**Interfaces:**
- Consumes: `DownloadPackage.position`, `DownloadItem.position` (already exist since Phase 1's domain models — `Scheduler.rank` already sorts by them)
- Produces:
  - `public func DownloadEngine.reorderItems(_ itemIDs: [UUID], inPackage packageID: UUID) async`
  - `public func DownloadEngine.moveItem(_ itemID: UUID, toPackage packageID: UUID) async`
  - `struct DraggedItemID: Codable, Transferable` in `SDM/`
  - `.onMove`/`.draggable`/`.dropDestination` wiring in `downloadsTab`

Spec §9.3: "`.draggable` / `.dropDestination` with a custom `Transferable` carrying item IDs. Dropping onto a package row moves items into it; dropping between rows reorders." `moveItem` is scoped to `.queued` items — a running or completed item's bytes already live on disk under its current package's folder (`DownloadEngine.context(for:)`), and relocating those files is out of scope for this phase (see the Deferred section at the end of this plan).

- [ ] **Step 1: Write the failing engine tests**

`SDMKit/Tests/SDMEngineTests/ReorderingTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMEngine

@Test func reorderItemsAppliesTheGivenOrderWithinAPackage() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let items = (0..<3).map {
        DownloadItem(
            url: URL(string: "https://example.com/\($0).bin")!, filename: "\($0).bin", position: $0)
    }
    let packageID = UUID()
    await engine.add(DownloadPackage(id: packageID, name: "Batch", items: items))

    let newOrder = [items[2].id, items[0].id, items[1].id]
    await engine.reorderItems(newOrder, inPackage: packageID)

    let ordered = await engine.snapshot().packages[0].items.map(\.id)
    #expect(ordered == newOrder)

    try await engine.runUntilIdle()
}

@Test func moveItemRelocatesAQueuedItemIntoAnotherPackage() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(10)),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let item = DownloadItem(
        url: URL(string: "https://example.com/a.bin")!, filename: "a.bin", isEnabled: false)
    let packageAID = UUID()
    let packageBID = UUID()
    await engine.add(DownloadPackage(id: packageAID, name: "A", items: [item]))
    await engine.add(DownloadPackage(id: packageBID, name: "B"))

    await engine.moveItem(item.id, toPackage: packageBID)

    let snapshot = await engine.snapshot()
    #expect(snapshot.packages.first { $0.id == packageAID }?.items.isEmpty == true)
    #expect(snapshot.packages.first { $0.id == packageBID }?.items.map(\.id) == [item.id])
}

@Test func moveItemIsANoOpForARunningItem() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let gate = WorkerGate()
    let engine = DownloadEngine(
        transport: WorkerGatedOrigin(payload: testPayload(4000), gate: gate),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir)
    )
    let item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    let packageAID = UUID()
    let packageBID = UUID()
    await engine.add(DownloadPackage(id: packageAID, name: "A", items: [item]))
    await engine.add(DownloadPackage(id: packageBID, name: "B"))

    var spins = 0
    while await snapshotItem(item.id, in: engine)?.state != .running, spins < 100_000 {
        await Task.yield()
        spins += 1
    }
    #expect(spins < 100_000)

    await engine.moveItem(item.id, toPackage: packageBID)
    let snapshot = await engine.snapshot()
    #expect(snapshot.packages.first { $0.id == packageAID }?.items.map(\.id) == [item.id])
    #expect(snapshot.packages.first { $0.id == packageBID }?.items.isEmpty == true)

    await gate.open()
    try await engine.runUntilIdle()
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter ReorderingTests`
Expected: FAIL — `value of type 'DownloadEngine' has no member 'reorderItems'`.

- [ ] **Step 3: Implement `reorderItems` and `moveItem`**

In `DownloadEngine.swift`, add to the `// MARK: - Mutations` section, after `setPriority`:

```swift
    /// Applies a new item order within one package. Spec §9.3: "Dropping
    /// between rows reorders," and reordering writes the position field the
    /// scheduler's rank already reads — no separate re-scheduling step.
    public func reorderItems(_ itemIDs: [UUID], inPackage packageID: UUID) async {
        guard let packageIndex = packages.firstIndex(where: { $0.id == packageID }) else { return }
        var byID = Dictionary(uniqueKeysWithValues: packages[packageIndex].items.map { ($0.id, $0) })
        var reordered: [DownloadItem] = []
        for (position, id) in itemIDs.enumerated() {
            guard var item = byID.removeValue(forKey: id) else { continue }
            item.position = position
            reordered.append(item)
        }
        reordered.append(contentsOf: byID.values)
        packages[packageIndex].items = reordered
        await persist()
        await reconcile()
    }

    /// Moves a queued item into a different package. Spec §9.3: "Dropping
    /// onto a package row moves items into it."
    ///
    /// Scoped to `.queued` items deliberately: a running or completed item's
    /// bytes already live on disk under its current package's folder
    /// (`context(for:)`), and relocating those files is out of scope for
    /// this phase.
    public func moveItem(_ itemID: UUID, toPackage packageID: UUID) async {
        guard let source = location(of: itemID),
            packages[source.packageIndex].id != packageID,
            packages[source.packageIndex].items[source.itemIndex].state == .queued,
            let destinationIndex = packages.firstIndex(where: { $0.id == packageID })
        else { return }
        var item = packages[source.packageIndex].items.remove(at: source.itemIndex)
        item.position = packages[destinationIndex].items.count
        packages[destinationIndex].items.append(item)
        await persist()
        await reconcile()
    }

    private func location(of itemID: UUID) -> (packageIndex: Int, itemIndex: Int)? {
        for packageIndex in packages.indices {
            if let itemIndex = packages[packageIndex].items.firstIndex(where: { $0.id == itemID }) {
                return (packageIndex, itemIndex)
            }
        }
        return nil
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite.

- [ ] **Step 5: Add `EngineController` wrappers**

In `SDM/EngineController.swift`, after the `retry(_:)` method added in Task 7:

```swift
    func reorderItems(_ itemIDs: [UUID], inPackage packageID: UUID) async {
        await engine.reorderItems(itemIDs, inPackage: packageID)
        snapshot = await engine.snapshot()
    }

    func moveItem(_ itemID: UUID, toPackage packageID: UUID) async {
        await engine.moveItem(itemID, toPackage: packageID)
        snapshot = await engine.snapshot()
    }
```

- [ ] **Step 6: Wire drag-and-drop into `downloadsTab`**

Create `SDM/DraggedItemID.swift`:

```swift
import Foundation
import UniformTypeIdentifiers

/// Carries a dragged download item's id between rows and package headers.
/// Spec §9.3: "a custom `Transferable` carrying item IDs."
struct DraggedItemID: Codable, Transferable {
    let itemID: UUID

    static var transferRepresentation: some TransferRepresentation {
        CodableRepresentation(contentType: .sdmDraggedItem)
    }
}

extension UTType {
    static var sdmDraggedItem: UTType { UTType(exportedAs: "com.shayanoh.sdm.dragged-item") }
}
```

In `SDM/ContentView.swift`'s `downloadsTab`, replace the `List { ForEach(controller.snapshot.packages) { package in Section(package.name) { ForEach(package.items) { item in ItemRow(...) } } } }` block with:

```swift
            List {
                ForEach(controller.snapshot.packages) { package in
                    Section {
                        ForEach(package.items) { item in
                            ItemRow(item: item, controller: controller)
                                .draggable(DraggedItemID(itemID: item.id))
                        }
                        .onMove { indices, newOffset in
                            var ids = package.items.map(\.id)
                            ids.move(fromOffsets: indices, toOffset: newOffset)
                            let packageID = package.id
                            Task { await controller.reorderItems(ids, inPackage: packageID) }
                        }
                    } header: {
                        Text(package.name)
                            .dropDestination(for: DraggedItemID.self) { dragged, _ in
                                guard let dragged = dragged.first else { return false }
                                let packageID = package.id
                                Task { await controller.moveItem(dragged.itemID, toPackage: packageID) }
                                return true
                            }
                    }
                }
            }
```

- [ ] **Step 7: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED.

Run the app and confirm by hand: dragging a row within its own package's section reorders it; adding a second package and dragging a queued (not-yet-started) row onto the other package's header moves it there; dragging a currently-downloading row onto another package's header does nothing (per the `.queued`-only scope decided above).

- [ ] **Step 8: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add -A
git commit -m "feat: add drag-and-drop reordering and cross-package moves"
```

---

### Task 9: Package-level speed history and per-row sparklines

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/EngineSnapshot.swift`
- Create: `SDMKit/Tests/SDMEngineTests/PackageHistoryTests.swift`
- Create: `SDM/Sparkline.swift`
- Modify: `SDM/ContentView.swift`

**Interfaces:**
- Consumes: `ItemSnapshot.speedHistory` (exists since Phase 1, populated by each item's `SpeedSampler`)
- Produces:
  - `PackageSnapshot.bytesPerSecondHistory: [Double]` — elementwise sum of member items' histories, aligned to their trailing edge
  - `struct Sparkline: View` — single `Canvas` path, y-scaled to its own max, per spec §9.6

Spec §9.6: "Per-row sparklines are drawn in a single `Canvas` — one path, no axes, no legend, y-scaled to that row's own max." The per-item data (`ItemSnapshot.speedHistory`) already exists; only the package-level aggregate and the view are new.

- [ ] **Step 1: Write the failing test**

`SDMKit/Tests/SDMEngineTests/PackageHistoryTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMEngine

private func makeItemSnapshot(speedHistory: [Double]) -> ItemSnapshot {
    ItemSnapshot(
        id: UUID(),
        url: URL(string: "https://example.com/a.bin")!,
        filename: "a.bin",
        totalBytes: nil,
        completed: RangeSet(),
        state: .running,
        isEnabled: true,
        isResumable: nil,
        activeSegments: 1,
        configuredSegments: 1,
        bytesPerSecond: speedHistory.last ?? 0,
        speedHistory: speedHistory
    )
}

/// Items can join mid-run with shorter histories (a later addition, or a
/// sampler that has ticked fewer times); the sum aligns every array's most
/// recent sample to the same trailing edge, front-padding the shorter ones
/// with zero rather than misaligning by index.
@Test func packageHistorySumsMemberItemHistoriesAlignedToTheirTrailingEdge() {
    let a = makeItemSnapshot(speedHistory: [10, 20, 30])
    let b = makeItemSnapshot(speedHistory: [1, 2])
    let package = PackageSnapshot(id: UUID(), name: "P", priority: .normal, items: [a, b])
    #expect(package.bytesPerSecondHistory == [10, 21, 32])
}

@Test func packageHistoryOfNoItemsIsEmpty() {
    let package = PackageSnapshot(id: UUID(), name: "P", priority: .normal, items: [])
    #expect(package.bytesPerSecondHistory.isEmpty)
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path SDMKit --filter PackageHistoryTests`
Expected: FAIL — `value of type 'PackageSnapshot' has no member 'bytesPerSecondHistory'`.

- [ ] **Step 3: Implement `bytesPerSecondHistory`**

In `SDMKit/Sources/SDMEngine/EngineSnapshot.swift`, add to `PackageSnapshot`, after the existing `totalBytes` computed property:

```swift
    /// Spec §9.6's per-row sparkline data, aggregated the same way
    /// `bytesPerSecond` is: summed from the member items, never stored
    /// separately, so it cannot disagree with them. Shorter histories (an
    /// item added mid-run) are aligned to the trailing edge and front-padded
    /// with zero rather than misaligned by index.
    public var bytesPerSecondHistory: [Double] {
        let length = items.map { $0.speedHistory.count }.max() ?? 0
        guard length > 0 else { return [] }
        var summed = [Double](repeating: 0, count: length)
        for item in items {
            let padding = length - item.speedHistory.count
            for (index, value) in item.speedHistory.enumerated() {
                summed[index + padding] += value
            }
        }
        return summed
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite.

- [ ] **Step 5: Create `Sparkline`**

`SDM/Sparkline.swift`:

```swift
import SwiftUI

/// Spec §9.6: one `Canvas` path, no axes, no legend, y-scaled to its own max.
struct Sparkline: View {
    let samples: [Double]

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
            context.stroke(path, with: .color(.accentColor), lineWidth: 1)
        }
    }
}
```

- [ ] **Step 6: Wire it into `ItemRow` and the package `Section` header**

In `SDM/ContentView.swift`'s `ItemRow.body`, inside the top `HStack`, right after the `Text(formatted(item.bytesPerSecond))` line, add:

```swift
                Sparkline(samples: item.speedHistory)
                    .frame(width: 48, height: 16)
```

In `downloadsTab`'s package `Section` header (added in Task 8), change `Text(package.name)` to include a sparkline alongside it:

```swift
                    } header: {
                        HStack {
                            Text(package.name)
                            Spacer()
                            Sparkline(samples: package.bytesPerSecondHistory)
                                .frame(width: 48, height: 16)
                        }
                        .dropDestination(for: DraggedItemID.self) { dragged, _ in
                            guard let dragged = dragged.first else { return false }
                            let packageID = package.id
                            Task { await controller.moveItem(dragged.itemID, toPackage: packageID) }
                            return true
                        }
                    }
```

- [ ] **Step 7: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED. Run the app, start a real download, and confirm each row's sparkline traces its speed over time and the package header's sparkline reflects the sum of its running items.

- [ ] **Step 8: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add -A
git commit -m "feat: add package-level speed history and per-row sparklines"
```

---

### Task 10: Menu bar extra

**Files:**
- Create: `SDM/MenuBarPopoverView.swift`
- Create: `SDM/MenuBarRingIcon.swift`
- Modify: `SDM/SDMApp.swift`
- Modify: `SDM/ContentView.swift`

**Interfaces:**
- Consumes: `EngineSnapshot`, `GrabberSnapshot.totalCount`
- Produces: a `MenuBarExtra` scene with `.menuBarExtraStyle(.window)`, per spec §9.7

Confirmed greenfield: no `MenuBarExtra` anywhere in the repo today. Spec §9.7 wants a real SwiftUI popover (not the restricted `.menu` style), a pending-links row, and a determinate ring icon. `.window` style is the one that lets the label itself be an arbitrary `View` — this plan uses that instead of rasterizing an `NSImage`.

- [ ] **Step 1: Expose a shared window id and sidebar-selection binding**

In `SDM/ContentView.swift`, `SidebarItem` (added in Task 7) is already usable from outside `ContentView`; no change needed there beyond confirming it is not `private`.

In `SDM/SDMApp.swift`, add a new `@State` above the existing ones:

```swift
    @State private var sidebarSelection: ContentView.SidebarItem? = .downloads
```

Change the `WindowGroup` to carry an id and bind the selection into `ContentView`:

```swift
        WindowGroup(id: "main") {
            ContentView(selection: $sidebarSelection)
                .environment(controller)
                .environment(grabberController)
```

(keep every existing `.task`/`.onAppear`/`.onDisappear`/`.onChange` modifier on the `WindowGroup`'s content unchanged).

In `SDM/ContentView.swift`, change `ContentView`'s `selection` from an internal `@State` to a binding:

```swift
    @Binding var selection: SidebarItem?
```

removing the old `@State private var selection: SidebarItem? = .downloads` line — the initial value now comes from `SDMApp`'s `sidebarSelection`.

- [ ] **Step 2: Create `MenuBarRingIcon`**

`SDM/MenuBarRingIcon.swift`:

```swift
import SwiftUI

/// Spec §9.7: "The menu bar icon shows a determinate ring for overall
/// progress." Built as plain SwiftUI rather than a rasterized `NSImage`,
/// which `.menuBarExtraStyle(.window)`'s custom label view supports directly.
struct MenuBarRingIcon: View {
    let fraction: Double

    var body: some View {
        ZStack {
            Circle().stroke(Color.secondary.opacity(0.4), lineWidth: 2)
            Circle()
                .trim(from: 0, to: max(0.02, min(fraction, 1)))
                .stroke(Color.primary, style: StrokeStyle(lineWidth: 2, lineCap: .round))
                .rotationEffect(.degrees(-90))
        }
        .frame(width: 16, height: 16)
    }
}
```

- [ ] **Step 3: Create `MenuBarPopoverView`**

`SDM/MenuBarPopoverView.swift`:

```swift
import AppKit
import SDMEngine
import SwiftUI

/// Spec §9.7: aggregate speed, mini bandwidth graph, active downloads with
/// progress and speed, a pending-links row above the actions (the one item
/// there requiring a decision), and Pause all / Open SDM / Quit.
struct MenuBarPopoverView: View {
    @Environment(EngineController.self) private var controller
    @Environment(GrabberController.self) private var grabberController
    @Environment(\.openWindow) private var openWindow
    @Binding var selection: ContentView.SidebarItem?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(formatted(controller.snapshot.globalBytesPerSecond))
                    .font(.headline.monospacedDigit())
                Spacer()
                Text("\(activeItems.count) active").font(.caption).foregroundStyle(.secondary)
            }
            BandwidthGraph(history: controller.snapshot.globalHistory).frame(height: 32)

            Divider()

            ForEach(activeItems.prefix(5)) { item in
                HStack {
                    ProgressView(value: item.fractionCompleted)
                        .progressViewStyle(.circular)
                        .controlSize(.small)
                    Text(item.filename).lineLimit(1).font(.caption)
                    Spacer()
                    Text(formatted(item.bytesPerSecond)).font(.caption.monospacedDigit())
                }
            }

            if grabberController.snapshot.totalCount > 0 {
                Divider()
                HStack {
                    Text("\(grabberController.snapshot.totalCount) links waiting").font(.caption)
                    Spacer()
                    Button("Review") {
                        selection = .linkgrabber
                        openWindow(id: "main")
                        NSApp.activate(ignoringOtherApps: true)
                    }
                    .controlSize(.small)
                }
            }

            Divider()

            HStack {
                Button("Pause all") {
                    Task {
                        for item in activeItems { await controller.setEnabled(false, for: item.id) }
                    }
                }
                Spacer()
                Button("Open SDM") {
                    openWindow(id: "main")
                    NSApp.activate(ignoringOtherApps: true)
                }
                Button("Quit") { NSApp.terminate(nil) }
            }
        }
        .padding()
        .frame(width: 280)
    }

    private var activeItems: [ItemSnapshot] {
        controller.snapshot.packages.flatMap(\.items).filter { $0.state == .running }
    }
}
```

- [ ] **Step 4: Add the `MenuBarExtra` scene**

In `SDM/SDMApp.swift`, add a third scene after `Settings { ... }`, still inside `body: some Scene`:

```swift
        MenuBarExtra {
            MenuBarPopoverView(selection: $sidebarSelection)
                .environment(controller)
                .environment(grabberController)
        } label: {
            MenuBarRingIcon(fraction: overallFraction)
        }
        .menuBarExtraStyle(.window)
    }

    private var overallFraction: Double {
        let running = controller.snapshot.packages.flatMap(\.items).filter { $0.state == .running }
        guard !running.isEmpty else { return 0 }
        return running.reduce(0.0) { $0 + $1.fractionCompleted } / Double(running.count)
    }
```

(the closing `}` after `.menuBarExtraStyle(.window)` closes `var body: some Scene`; `overallFraction` is a new computed property on `SDMApp`, placed after `body`.)

- [ ] **Step 5: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED.

Run the app and confirm by hand: a ring icon appears in the menu bar; clicking it opens a popover showing speed, the mini graph, and any active downloads; grabbing a link shows the "N links waiting" row with a working Review button that opens the main window on the Linkgrabber tab; Pause all stops every active download; Quit exits the app.

- [ ] **Step 6: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add -A
git commit -m "feat: add a menu bar extra with popover, pending-links row, and ring icon"
```

---

### Task 11: Notifications

**Files:**
- Create: `SDM/NotificationSettings.swift`
- Create: `SDM/NotificationManager.swift`
- Modify: `SDM/EngineController.swift`
- Modify: `SDM/SDMApp.swift`
- Modify: `SDM/SettingsView.swift`

**Interfaces:**
- Consumes: `EngineSnapshot` diffing (new, app-target only)
- Produces: `NotificationManager`, `NotificationSettings`, each per-type toggle from spec §9.8

Confirmed greenfield: zero `UNUserNotificationCenter` references anywhere in the repo today. Spec §9.8: "each individually toggleable: download finished, package finished, download failed (with reason), 'N links grabbed, waiting for confirmation.'" This is `UserNotifications`/AppKit glue with no automated coverage, per spec §11.7 — the app-target `SDMTests` smoke test is not extended for it.

- [ ] **Step 1: Create `NotificationSettings`**

`SDM/NotificationSettings.swift`:

```swift
import Foundation

/// Per-type toggles from spec §9.8. Mirrors `GrabberSettings`'s pattern.
@MainActor
enum NotificationSettings {
    private static let defaults = UserDefaults.standard

    private enum Key {
        static let downloadFinished = "sdm.notify.downloadFinished"
        static let packageFinished = "sdm.notify.packageFinished"
        static let downloadFailed = "sdm.notify.downloadFailed"
        static let linksGrabbed = "sdm.notify.linksGrabbed"
    }

    static var downloadFinishedEnabled: Bool {
        get { defaults.object(forKey: Key.downloadFinished) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Key.downloadFinished) }
    }

    static var packageFinishedEnabled: Bool {
        get { defaults.object(forKey: Key.packageFinished) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Key.packageFinished) }
    }

    static var downloadFailedEnabled: Bool {
        get { defaults.object(forKey: Key.downloadFailed) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Key.downloadFailed) }
    }

    static var linksGrabbedEnabled: Bool {
        get { defaults.object(forKey: Key.linksGrabbed) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Key.linksGrabbed) }
    }
}
```

- [ ] **Step 2: Create `NotificationManager`**

`SDM/NotificationManager.swift`:

```swift
import UserNotifications

@MainActor
final class NotificationManager {
    func requestAuthorization() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    func notifyDownloadFinished(filename: String) {
        guard NotificationSettings.downloadFinishedEnabled else { return }
        post(title: "Download finished", body: filename)
    }

    func notifyPackageFinished(name: String) {
        guard NotificationSettings.packageFinishedEnabled else { return }
        post(title: "Package finished", body: name)
    }

    func notifyDownloadFailed(filename: String, reason: String) {
        guard NotificationSettings.downloadFailedEnabled else { return }
        post(title: "Download failed", body: "\(filename): \(reason)")
    }

    func notifyLinksGrabbed(count: Int) {
        guard NotificationSettings.linksGrabbedEnabled, count > 0 else { return }
        post(title: "\(count) links grabbed", body: "Waiting for confirmation in the Linkgrabber")
    }

    private func post(title: String, body: String) {
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        let request = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }
}
```

- [ ] **Step 3: Diff snapshots in `EngineController` and fire notifications**

In `SDM/EngineController.swift`, add two new private properties:

```swift
    private let notifications = NotificationManager()
    private var previousSnapshot: EngineSnapshot?
```

In `startHeartbeat()`, right after `await engine.restore()` and its `snapshot = await engine.snapshot()` line, add:

```swift
        notifications.requestAuthorization()
```

Inside the `while !Task.isCancelled { ... }` loop, right after `snapshot = await engine.snapshot()`, add:

```swift
            notifyChanges(from: previousSnapshot, to: snapshot)
            previousSnapshot = snapshot
```

Add the diffing method after `startHeartbeat()`:

```swift
    /// Compares two heartbeats' worth of snapshot and fires exactly the
    /// notifications spec §9.8 lists, once per transition — never a repeat
    /// for a state that has already been announced.
    private func notifyChanges(from old: EngineSnapshot?, to new: EngineSnapshot) {
        guard let old else { return }
        let oldStates = Dictionary(
            uniqueKeysWithValues: old.packages.flatMap(\.items).map { ($0.id, $0.state) })
        for item in new.packages.flatMap(\.items) {
            guard oldStates[item.id] != item.state else { continue }
            switch item.state {
            case .completed:
                notifications.notifyDownloadFinished(filename: item.filename)
            case .failed(let reason):
                notifications.notifyDownloadFailed(filename: item.filename, reason: reason)
            default:
                break
            }
        }

        let oldPackagesDone = Dictionary(
            uniqueKeysWithValues: old.packages.map {
                ($0.id, !$0.items.isEmpty && $0.completedCount == $0.items.count)
            })
        for package in new.packages {
            let isDone = !package.items.isEmpty && package.completedCount == package.items.count
            guard isDone, oldPackagesDone[package.id] != true else { continue }
            notifications.notifyPackageFinished(name: package.name)
        }
    }
```

- [ ] **Step 4: Notify on newly grabbed links**

In `SDM/SDMApp.swift`, add a new `@State`:

```swift
    @State private var linkNotifications = NotificationManager()
    @State private var notifiedLinkIDs: Set<UUID> = []
```

In the existing `.onChange(of: grabberController.snapshot)` closure (the one that already handles auto-add-and-start), add at the top of the closure body:

```swift
                    let freshIDs = Set(newSnapshot.links.map(\.id)).subtracting(notifiedLinkIDs)
                    if !freshIDs.isEmpty {
                        notifiedLinkIDs.formUnion(freshIDs)
                        linkNotifications.notifyLinksGrabbed(count: freshIDs.count)
                    }
```

(leave the rest of that closure — the auto-add-and-start loop — unchanged below it.)

- [ ] **Step 5: Add per-type toggles to `SettingsView`**

In `SDM/SettingsView.swift`, add four more `@State` properties:

```swift
    @State private var downloadFinishedEnabled = NotificationSettings.downloadFinishedEnabled
    @State private var packageFinishedEnabled = NotificationSettings.packageFinishedEnabled
    @State private var downloadFailedEnabled = NotificationSettings.downloadFailedEnabled
    @State private var linksGrabbedEnabled = NotificationSettings.linksGrabbedEnabled
```

Add a new `Section` inside the `Form`, after the `"Linkgrabber"` section:

```swift
            Section("Notifications") {
                Toggle("Download finished", isOn: $downloadFinishedEnabled)
                Toggle("Package finished", isOn: $packageFinishedEnabled)
                Toggle("Download failed", isOn: $downloadFailedEnabled)
                Toggle("Links grabbed", isOn: $linksGrabbedEnabled)
            }
```

Add matching `.onChange` modifiers alongside the existing ones:

```swift
        .onChange(of: downloadFinishedEnabled) { _, new in
            NotificationSettings.downloadFinishedEnabled = new
        }
        .onChange(of: packageFinishedEnabled) { _, new in
            NotificationSettings.packageFinishedEnabled = new
        }
        .onChange(of: downloadFailedEnabled) { _, new in NotificationSettings.downloadFailedEnabled = new }
        .onChange(of: linksGrabbedEnabled) { _, new in NotificationSettings.linksGrabbedEnabled = new }
```

- [ ] **Step 6: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED.

Run the app, grant the notification permission prompt, and confirm by hand: pasting a link produces an "N links grabbed" banner; downloading a small real file to completion produces a "Download finished" banner with the filename; downloading a package until every item finishes produces a "Package finished" banner; a download that fails produces a "Download failed" banner with the reason; turning any of the four toggles off in Settings suppresses that banner on the next occurrence.

- [ ] **Step 7: Run the full package test suite**

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite (this task touches no `SDMKit` code).

- [ ] **Step 8: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add -A
git commit -m "feat: add per-type notifications for finished, failed, and grabbed"
```

---

### Task 12: Linkgrabber manual package overrides — drag and context menu

**Files:**
- Modify: `SDMKit/Sources/SDMGrabber/GrabberSession.swift`
- Create: `SDMKit/Tests/SDMGrabberTests/PackageOverrideTests.swift`
- Modify: `SDM/GrabberController.swift`
- Modify: `SDM/LinkGrabberView.swift`

**Interfaces:**
- Consumes: `GrabberSession.moveLink` (exists since Phase 2), `PackageClustering`
- Produces:
  - `public func GrabberSession.renamePackage(_ oldName: String, to newName: String)`
  - `public func GrabberSession.mergePackages(_ sourceName: String, into destinationName: String)`
  - `public func GrabberSession.splitPackage(_ name: String)`
  - Drag-and-drop plus a context menu wired into `LinkGrabberView`

Spec §7.4: "All of it is fully overridable: drag items between packages, right-click → move to existing/new package, rename, merge, split." Phase 2's deferred section flagged exactly this — `moveLink` existed with no UI, and rename/merge/split did not exist at all.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMGrabberTests/PackageOverrideTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMGrabber

private func makeSession() -> GrabberSession {
    GrabberSession(prober: LinkProber(transport: FakeProbeOrigin(), deepSniffEnabled: false))
}

@Test func renamePackageMovesEveryMemberLinkUnderTheNewName() async throws {
    let session = makeSession()
    await session.ingest(urls: [URL(string: "https://example.com/a.zip")!])
    let originalName = await session.snapshot().packages[0].name

    await session.renamePackage(originalName, to: "My Archive")

    let snapshot = await session.snapshot()
    #expect(snapshot.packages.map(\.name) == ["My Archive"])
    #expect(snapshot.packages[0].linkIDs.count == 1)
}

@Test func mergePackagesCombinesBothIntoTheDestinationName() async throws {
    let session = makeSession()
    await session.ingest(
        urls: [
            URL(string: "https://example.com/a.zip")!,
            URL(string: "https://other.example.com/b.zip")!,
        ])
    let names = await session.snapshot().packages.map(\.name)
    guard names.count >= 2 else {
        // Clustering may already have grouped these; force two packages via a
        // manual rename so the merge itself is what's under test.
        await session.renamePackage(names[0], to: "First")
        let secondSnapshot = await session.snapshot()
        for package in secondSnapshot.packages where package.name != "First" {
            await session.renamePackage(package.name, to: "Second")
        }
        await session.mergePackages("Second", into: "First")
        let merged = await session.snapshot()
        #expect(merged.packages.map(\.name) == ["First"])
        #expect(merged.packages[0].linkIDs.count == 2)
        return
    }
    await session.mergePackages(names[1], into: names[0])
    let merged = await session.snapshot()
    #expect(merged.packages.map(\.name) == [names[0]])
    #expect(merged.packages[0].linkIDs.count == 2)
}

@Test func splitPackageGivesEachLinkItsOwnPackage() async throws {
    let session = makeSession()
    await session.ingest(
        urls: [
            URL(string: "https://example.com/Show.S01E01.mkv")!,
            URL(string: "https://example.com/Show.S01E02.mkv")!,
        ])
    let originalName = await session.snapshot().packages[0].name
    #expect(await session.snapshot().packages[0].linkIDs.count == 2)

    await session.splitPackage(originalName)

    let snapshot = await session.snapshot()
    #expect(snapshot.packages.count == 2)
    #expect(snapshot.packages.allSatisfy { $0.linkIDs.count == 1 })
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter PackageOverrideTests`
Expected: FAIL — `value of type 'GrabberSession' has no member 'renamePackage'`.

- [ ] **Step 3: Implement the three methods**

In `SDMKit/Sources/SDMGrabber/GrabberSession.swift`, add after `moveLink`:

```swift
    /// Renames a package by moving every member link's manual override to
    /// the new name. Spec §7.4: "rename."
    public func renamePackage(_ oldName: String, to newName: String) {
        guard !newName.isEmpty, oldName != newName,
            let package = packages.first(where: { $0.name == oldName })
        else { return }
        for id in package.linkIDs { manualOverrides[id] = newName }
        recluster()
    }

    /// Combines one package's links into another's. Spec §7.4: "merge."
    public func mergePackages(_ sourceName: String, into destinationName: String) {
        guard sourceName != destinationName,
            let source = packages.first(where: { $0.name == sourceName })
        else { return }
        for id in source.linkIDs { manualOverrides[id] = destinationName }
        recluster()
    }

    /// Splits a package so each member link becomes its own single-link
    /// package, named after that link. Spec §7.4: "split."
    public func splitPackage(_ name: String) {
        guard let package = packages.first(where: { $0.name == name }) else { return }
        for id in package.linkIDs {
            guard let link = links[id] else { continue }
            manualOverrides[id] = link.effectiveFilename
        }
        recluster()
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite.

- [ ] **Step 5: Add `GrabberController` wrappers**

In `SDM/GrabberController.swift`, after `moveLink`:

```swift
    func renamePackage(_ oldName: String, to newName: String) async {
        await session.renamePackage(oldName, to: newName)
        snapshot = await session.snapshot()
    }

    func mergePackages(_ sourceName: String, into destinationName: String) async {
        await session.mergePackages(sourceName, into: destinationName)
        snapshot = await session.snapshot()
    }

    func splitPackage(_ name: String) async {
        await session.splitPackage(name)
        snapshot = await session.snapshot()
    }
```

- [ ] **Step 6: Wire drag, context menu, and a rename alert into `LinkGrabberView`**

Create `SDM/DraggedLinkID.swift`:

```swift
import Foundation
import UniformTypeIdentifiers

struct DraggedLinkID: Codable, Transferable {
    let linkID: UUID

    static var transferRepresentation: some TransferRepresentation {
        CodableRepresentation(contentType: .sdmDraggedLink)
    }
}

extension UTType {
    static var sdmDraggedLink: UTType { UTType(exportedAs: "com.shayanoh.sdm.dragged-link") }
}
```

In `SDM/LinkGrabberView.swift`, add state for the rename alert:

```swift
    @State private var isShowingRenameAlert = false
    @State private var renamingPackage = ""
    @State private var newPackageName = ""
```

Change `LinkRow`'s usage inside the `ForEach(links(in: package))` to add a drag modifier:

```swift
                        ForEach(links(in: package)) { link in
                            LinkRow(link: link, controller: controller)
                                .draggable(DraggedLinkID(linkID: link.id))
                        }
```

Replace `packageHeader(_:)` to add the drop target and context menu:

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
        .dropDestination(for: DraggedLinkID.self) { dragged, _ in
            guard let dragged = dragged.first else { return false }
            let name = package.name
            Task { await controller.moveLink(dragged.linkID, toPackageNamed: name) }
            return true
        }
        .contextMenu {
            Button("Rename…") {
                renamingPackage = package.name
                newPackageName = package.name
                isShowingRenameAlert = true
            }
            Menu("Merge into") {
                ForEach(
                    controller.snapshot.packages.filter { $0.name != package.name }, id: \.name
                ) { other in
                    Button(other.name) {
                        let source = package.name
                        let destination = other.name
                        Task { await controller.mergePackages(source, into: destination) }
                    }
                }
            }
            Button("Split") {
                let name = package.name
                Task { await controller.splitPackage(name) }
            }
        }
    }
```

Add the rename alert as a modifier on `LinkGrabberView`'s outermost `VStack` in `body`, alongside the existing `.onDrop`/`.toolbar`/`.sheet`:

```swift
        .alert("Rename package", isPresented: $isShowingRenameAlert) {
            TextField("Name", text: $newPackageName)
            Button("Rename") {
                let old = renamingPackage
                let new = newPackageName
                Task { await controller.renamePackage(old, to: new) }
            }
            Button("Cancel", role: .cancel) {}
        }
```

- [ ] **Step 7: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED.

Run the app and confirm by hand: grabbing several links from different hosts, right-clicking a package header offers Rename / Merge into / Split, each of which visibly regroups the list; dragging a link row onto a different package's header moves it there.

- [ ] **Step 8: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add -A
git commit -m "feat: add rename, merge, split, and drag-to-move for grabbed packages"
```

---

### Task 13: Grabber probe budget draws from the same persisted connection settings

**Files:**
- Modify: `SDM/GrabberController.swift`

**Interfaces:**
- Consumes: `EngineSettingsStore.globalMaxConnections`, `EngineSettingsStore.maxConnectionsPerHost` (Task 5)
- Produces: no new public API — `GrabberSession.Budget`'s two numbers now come from the same settings the engine's connection ceilings use, instead of the hardcoded defaults (16 / 4)

Phase 2's deferred section: *"`GrabberSession.Budget` is its own global/per-host cap, not literally the engine's `globalMaxConnections`... Once Phase 3 wires a real shared limiter, revisit whether the grabber should draw from it."* Task 2 gave the engine a real limiter (`ConnectionAllocator`); this task closes the loop on the "revisit" by making both sides read the *same numbers*, while keeping the two runtime mechanisms separate on purpose — the grabber's probing concurrency is a `TaskGroup` budget for short-lived HEAD/ranged-GET requests, structurally different from the engine's long-lived per-item worker pools, and merging the two mechanisms is not warranted by anything in spec §7.2 or §6.4.

- [ ] **Step 1: Change `GrabberController.init()`**

In `SDM/GrabberController.swift`, replace the `init()` body:

```swift
    init() {
        session = GrabberSession(
            prober: LinkProber(
                transport: URLSessionProbeTransport(),
                deepSniffEnabled: GrabberSettings.deepSniffEnabled
            ),
            budget: GrabberSession.Budget(
                globalMaxConcurrentProbes: EngineSettingsStore.globalMaxConnections,
                maxConcurrentPerHost: EngineSettingsStore.maxConnectionsPerHost
            )
        )
    }
```

- [ ] **Step 2: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED.

Run the app, set "Global max connections" very low (e.g. 2) in Settings, quit and relaunch (this budget is read once at `GrabberController.init()`, not live-updated — note this in the Deferred section below), then paste a large batch of links and confirm probing still completes, just more slowly, without the app hammering every host at once.

- [ ] **Step 3: Run the full package test suite**

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite (this task touches no `SDMKit` code).

- [ ] **Step 4: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add -A
git commit -m "feat: derive the grabber's probe budget from the persisted connection settings"
```

---

## Phase 3 completion criteria

- [ ] `swift test --package-path SDMKit` passes with no skipped tests, including every new test file added by this plan
- [ ] No `SDMEngine`/`SDMGrabber`/`SDMCore` test touches the network or sleeps on a real clock; every test needing a still-in-flight download uses `WorkerGate`/`WorkerGatedOrigin`
- [ ] `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build` succeeds
- [ ] Adding more items than `maxConcurrent` × `globalMaxConnections` allows visibly throttles worker pools rather than opening unbounded connections (Tasks 1–2)
- [ ] A running item started within the last ~5 s survives a higher-priority addition; the same item is preempted once the window elapses (Task 3)
- [ ] A `.failed` item shows a Retry button that resumes it once the underlying problem is fixed (Task 4)
- [ ] Settings changes in the Settings screen (⌘,) apply to running downloads without a restart (Tasks 5–6)
- [ ] The app shell is `NavigationSplitView` with Downloads / Linkgrabber / Completed and a live pinned stats block; Completed is provably a filter, not a separate store (Task 7)
- [ ] Rows and packages can be reordered and moved between packages by drag; a running or completed item cannot be moved cross-package (Task 8)
- [ ] Every row and package header shows a live sparkline; the sidebar stats block shows a Swift Charts bandwidth graph with a running-average line (Tasks 7, 9)
- [ ] A menu bar icon with a determinate ring opens a popover with speed, mini graph, active downloads, a pending-links row when applicable, and working Pause all / Open SDM / Quit actions (Task 10)
- [ ] Download finished, package finished, download failed, and "N links grabbed" each produce a real macOS notification, individually toggleable in Settings (Task 11)
- [ ] A grabbed package can be renamed, merged into another, split, and moved by drag (Task 12)
- [ ] `SDMGrabber` still depends only on `SDMCore` — confirm with `grep -n "import SDMEngine" SDMKit/Sources/SDMGrabber/*.swift` returning nothing (unchanged invariant from Phase 2, worth re-checking since this plan touches `GrabberSession.swift`)

## Deferred to later phases

Deliberately **not** in Phase 3, to keep it shippable:

- **Relocating a running or completed item's already-downloaded bytes when moved cross-package.** `DownloadEngine.moveItem` (Task 8) is scoped to `.queued` items only — moving a `.running`/`.completed` item would require relocating its file on disk under `context(for:)`'s package-name-derived path, which this plan does not attempt. Revisit if this proves a real workflow gap.
- **Live-updating `GrabberSession.Budget` when Settings changes.** Task 13 reads `EngineSettingsStore` once, at `GrabberController.init()` — unlike the engine side (`DownloadEngine.updateSettings`), there is no live-apply path for the grabber's probe budget. A setting change only takes effect on the next launch.
- **A true shared connection-limiter type used by both the engine's `ConnectionAllocator` and the grabber's `GrabberSession.Budget` at the mechanism level**, as opposed to just sharing the same *numbers* (Task 13). The two remain structurally separate — long-lived worker pools vs. short-lived probe requests — per that task's rationale.
- **Custom theme editor, theme import, and any role-based color system.** Every view added by this plan (`BandwidthGraph`, `Sparkline`, `MenuBarRingIcon`, `SettingsView`) uses system colors (`.accentColor`, `.secondary`, `.primary`) directly, not theme roles — spec §10.1's theme system does not exist until Phase 4. Revisit every literal color then, matching Phase 2's identical deferral for `LinkGrabberView`.
- **Activation policy modes (menu bar only / dock only / both) and Liquid Glass.** Spec §9.9 and §10.2 are explicitly Phase 4. This plan's `MenuBarExtra` always coexists with the dock icon; the "menu bar only" mode's `⌘Q`-unavailable-without-a-window edge case (spec §10.2) is unhandled until then.
- **Quit confirmation for active non-resumable downloads.** Spec §10.2's "quitting with active non-resumable downloads shows a confirmation" is Phase 4 territory (tied to activation-policy/window-lifecycle work) and is not added here.
- **Per-error-class retry policies.** `RetryPolicy.classify` remains one global policy shared by every item (Task 4 only adds visibility into and a manual override of its outcome, not per-error-class tuning).
- **yt-dlp / YouTube resolver, muxing, format tables.** Phase 5, unchanged.
