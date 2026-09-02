# SDM Phase 1 — Download Engine & Scheduler Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status: IMPLEMENTED — all five phases of the project are complete and merged to `main` (Phase 5 finished 2026-09-02).** Historical record only. This plan's "Deferred to later phases" items were all picked up by Phases 2–5; anything still open is in `todo.md` at the repo root (which also lists the small Phase 1 follow-ups owed since before Phase 3).

**Goal:** Build a tested, resumable, segmented HTTP download engine with a priority scheduler, plus a minimal SwiftUI window to drive it.

**Architecture:** A download's progress is a `RangeSet` of completed byte ranges; parallel workers are ephemeral and claim gaps from that set. Resume, mid-flight segment-count changes, and the segmented progress bar all fall out of this one structure. The engine takes `HTTPTransport` and `Clock` as injected protocols, so the whole test suite runs against an in-process fake origin with no network and no real waiting.

**Tech Stack:** Swift 6.2, Swift Testing, SwiftUI, Foundation `URLSession`. No third-party dependencies.

**Spec:** [docs/superpowers/specs/2026-08-03-sdm-design.md](../specs/2026-08-03-sdm-design.md). Read §4, §5, §6 and §11 before starting.

## Global Constraints

- **Deployment target: macOS 15.0.** SPM defaults to macOS 14 — `platforms: [.macOS(.v15)]` must be set explicitly in `Package.swift`.
- **Swift tools version 6.2**, Swift 6 language mode, strict concurrency enabled. All cross-actor types must be `Sendable`.
- **Zero third-party dependencies.** Foundation and Swift standard library only.
- **Swift Testing only** (`@Test` / `#expect`). No XCTest in the package.
- **No test may touch the network or sleep on a real clock.** Every test uses `FakeOrigin` and `FakeClock`.
- **All byte offsets are `Int64`.** Files exceed 4 GB routinely.
- **Byte ranges are half-open `[start, end)`** everywhere. HTTP `Range` headers are inclusive-inclusive — convert only at the HTTP boundary, never in the domain model.
- **Format before every commit:** `swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests`. Verified present at `/usr/local/bin/swift-format` (Homebrew, 603.0.0). If it ever goes missing, `xcrun swift-format` is the Xcode-bundled fallback at 6.2.0 — the `.swift-format` config works with either.
- **Run tests with:** `swift test --package-path SDMKit`
- Package lives at `SDMKit/`; the Xcode app target consumes it as a local package (wired in Task 17).

---

### Task 1: Package scaffold

**Files:**
- Create: `SDMKit/Package.swift`
- Create: `SDMKit/Sources/SDMCore/SDMCore.swift`
- Create: `SDMKit/Sources/SDMEngine/SDMEngine.swift`
- Create: `SDMKit/Tests/SDMCoreTests/SmokeTests.swift`
- Create: `.swift-format`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: nothing
- Produces: two library targets, `SDMCore` and `SDMEngine`, with `SDMEngine` depending on `SDMCore`.

- [ ] **Step 1: Remove the `Packages/` ignore rule**

`.gitignore` currently ignores `Packages/`, a legacy SPM convention. It does not match `SDMKit/`, but leave nothing ambiguous — delete this line from `.gitignore`:

```
Packages/
```

- [ ] **Step 2: Create `SDMKit/Package.swift`**

```swift
// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "SDMKit",
    platforms: [.macOS(.v15)],
    products: [
        .library(name: "SDMCore", targets: ["SDMCore"]),
        .library(name: "SDMEngine", targets: ["SDMEngine"]),
    ],
    targets: [
        .target(name: "SDMCore"),
        .target(name: "SDMEngine", dependencies: ["SDMCore"]),
        .testTarget(name: "SDMCoreTests", dependencies: ["SDMCore"]),
        .testTarget(name: "SDMEngineTests", dependencies: ["SDMEngine"]),
    ]
)
```

- [ ] **Step 3: Create placeholder sources so both targets compile**

`SDMKit/Sources/SDMCore/ModuleInfo.swift`:

```swift
/// Placeholder so the target compiles before real types land in Task 2.
/// Deliberately not named `SDMCore` — a type sharing its module's name creates
/// persistent lookup ambiguity.
enum SDMCoreModuleInfo {}
```

`SDMKit/Sources/SDMEngine/ModuleInfo.swift`:

```swift
/// Placeholder so the target compiles before real types land in Task 5.
enum SDMEngineModuleInfo {}
```

- [ ] **Step 4: Create the smoke test**

`SDMKit/Tests/SDMCoreTests/SmokeTests.swift`:

```swift
import Testing

@Test func packageBuilds() {
    #expect(Bool(true))
}
```

- [ ] **Step 5: Create an empty `SDMEngineTests` placeholder**

`SDMKit/Tests/SDMEngineTests/SmokeTests.swift`:

```swift
import Testing

@Test func engineTargetBuilds() {
    #expect(Bool(true))
}
```

- [ ] **Step 6: Create `.swift-format` at the repo root**

```json
{
  "version": 1,
  "lineLength": 100,
  "indentation": { "spaces": 4 },
  "respectsExistingLineBreaks": true,
  "lineBreakBeforeEachArgument": false
}
```

- [ ] **Step 7: Run the tests**

Run: `swift test --package-path SDMKit`
Expected: PASS, 2 tests. Confirm the output line reads `Target Platform: x86_64-apple-macos15.0` — if it says `macos14.0`, the `platforms:` line is missing.

- [ ] **Step 8: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit .swift-format .gitignore
git commit -m "feat: scaffold SDMKit package with SDMCore and SDMEngine targets"
```

---

### Task 2: `ByteRange` and `RangeSet` — insert, merge, query

**Files:**
- Create: `SDMKit/Sources/SDMCore/ByteRange.swift`
- Create: `SDMKit/Sources/SDMCore/RangeSet.swift`
- Create: `SDMKit/Tests/SDMCoreTests/RangeSetTests.swift`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `struct ByteRange: Hashable, Sendable, Codable` with `let start: Int64`, `let end: Int64`, `var length: Int64`
  - `struct RangeSet: Equatable, Sendable, Codable` with `init()`, `mutating func insert(_ range: ByteRange)`, `var ranges: [ByteRange]` (sorted, disjoint, non-adjacent), `var totalBytes: Int64`, `func contains(_ offset: Int64) -> Bool`

This invariant is load-bearing for the entire engine: **`ranges` is always sorted ascending, disjoint, and non-adjacent** (touching ranges are coalesced into one). Every mutation must restore it.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMCoreTests/RangeSetTests.swift`:

```swift
import Testing

@testable import SDMCore

@Test func byteRangeLength() {
    #expect(ByteRange(start: 10, end: 25).length == 15)
}

@Test func byteRangeRejectsInvertedBounds() {
    #expect(ByteRange(start: 10, end: 10).length == 0)
}

@Test func insertIntoEmptySet() {
    var set = RangeSet()
    set.insert(ByteRange(start: 0, end: 100))
    #expect(set.ranges == [ByteRange(start: 0, end: 100)])
    #expect(set.totalBytes == 100)
}

@Test func insertDisjointRangesStaysSorted() {
    var set = RangeSet()
    set.insert(ByteRange(start: 200, end: 300))
    set.insert(ByteRange(start: 0, end: 100))
    #expect(set.ranges == [ByteRange(start: 0, end: 100), ByteRange(start: 200, end: 300)])
    #expect(set.totalBytes == 200)
}

@Test func adjacentRangesCoalesce() {
    var set = RangeSet()
    set.insert(ByteRange(start: 0, end: 100))
    set.insert(ByteRange(start: 100, end: 200))
    #expect(set.ranges == [ByteRange(start: 0, end: 200)])
}

@Test func overlappingRangesMerge() {
    var set = RangeSet()
    set.insert(ByteRange(start: 0, end: 100))
    set.insert(ByteRange(start: 50, end: 150))
    #expect(set.ranges == [ByteRange(start: 0, end: 150)])
    #expect(set.totalBytes == 150)
}

@Test func insertBridgingTwoRangesMergesAllThree() {
    var set = RangeSet()
    set.insert(ByteRange(start: 0, end: 100))
    set.insert(ByteRange(start: 200, end: 300))
    set.insert(ByteRange(start: 90, end: 210))
    #expect(set.ranges == [ByteRange(start: 0, end: 300)])
}

@Test func insertFullyContainedRangeIsNoOp() {
    var set = RangeSet()
    set.insert(ByteRange(start: 0, end: 100))
    set.insert(ByteRange(start: 20, end: 30))
    #expect(set.ranges == [ByteRange(start: 0, end: 100)])
    #expect(set.totalBytes == 100)
}

@Test func insertEmptyRangeIsNoOp() {
    var set = RangeSet()
    set.insert(ByteRange(start: 50, end: 50))
    #expect(set.ranges.isEmpty)
}

@Test func containsRespectsHalfOpenBounds() {
    var set = RangeSet()
    set.insert(ByteRange(start: 10, end: 20))
    #expect(set.contains(10))
    #expect(set.contains(19))
    #expect(!set.contains(20))
    #expect(!set.contains(9))
}

@Test func rangeSetRoundTripsThroughCodable() throws {
    var set = RangeSet()
    set.insert(ByteRange(start: 0, end: 100))
    set.insert(ByteRange(start: 200, end: 300))
    let data = try JSONEncoder().encode(set)
    #expect(try JSONDecoder().decode(RangeSet.self, from: data) == set)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit`
Expected: FAIL — `cannot find 'ByteRange' in scope`.

- [ ] **Step 3: Implement `ByteRange`**

`SDMKit/Sources/SDMCore/ByteRange.swift`:

```swift
/// A half-open byte interval `[start, end)`.
///
/// Half-open is used everywhere in the domain model. HTTP `Range` headers are
/// inclusive on both ends; convert only at the HTTP boundary.
public struct ByteRange: Hashable, Sendable, Codable {
    public let start: Int64
    public let end: Int64

    /// - Precondition: `start >= 0` and `end >= start`.
    public init(start: Int64, end: Int64) {
        precondition(start >= 0, "ByteRange.start must be non-negative, got \(start)")
        precondition(end >= start, "ByteRange.end (\(end)) must be >= start (\(start))")
        self.start = start
        self.end = end
    }

    public var length: Int64 { end - start }
    public var isEmpty: Bool { end == start }
}
```

- [ ] **Step 4: Implement `RangeSet`**

`SDMKit/Sources/SDMCore/RangeSet.swift`:

```swift
/// A set of completed byte ranges, kept sorted, disjoint, and coalesced.
///
/// This is the single source of truth for a download's progress. Segments are
/// ephemeral workers that fill gaps in this set; they are never stored.
public struct RangeSet: Equatable, Sendable, Codable {
    public private(set) var ranges: [ByteRange]

    public init() {
        self.ranges = []
    }

    /// Creates a set from arbitrary ranges, normalizing them.
    public init(_ ranges: [ByteRange]) {
        self.ranges = []
        for range in ranges { insert(range) }
    }

    /// Inserts a range, merging it with any overlapping or adjacent ranges.
    public mutating func insert(_ range: ByteRange) {
        guard !range.isEmpty else { return }

        var merged = range
        var result: [ByteRange] = []
        result.reserveCapacity(ranges.count + 1)
        var inserted = false

        for existing in ranges {
            if existing.end < merged.start {
                result.append(existing)
            } else if existing.start > merged.end {
                if !inserted {
                    result.append(merged)
                    inserted = true
                }
                result.append(existing)
            } else {
                merged = ByteRange(
                    start: Swift.min(existing.start, merged.start),
                    end: Swift.max(existing.end, merged.end)
                )
            }
        }
        if !inserted { result.append(merged) }
        ranges = result
    }

    public var totalBytes: Int64 {
        ranges.reduce(0) { $0 + $1.length }
    }

    public func contains(_ offset: Int64) -> Bool {
        ranges.contains { offset >= $0.start && offset < $0.end }
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS, all `RangeSetTests` green.

- [ ] **Step 6: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: add ByteRange and RangeSet with merge semantics"
```

---

### Task 3: `RangeSet` gaps and claim policy

**Files:**
- Modify: `SDMKit/Sources/SDMCore/RangeSet.swift`
- Create: `SDMKit/Tests/SDMCoreTests/RangeSetClaimTests.swift`

**Interfaces:**
- Consumes: `ByteRange`, `RangeSet` from Task 2
- Produces:
  - `func gaps(within total: Int64) -> [ByteRange]`
  - `func isComplete(total: Int64) -> Bool`
  - `func nextClaim(total: Int64, reserved: [ByteRange], minChunk: Int64) -> ByteRange?`

**Claim policy** (from spec §5.1) — an idle worker takes the largest gap not already reserved by another worker. If that gap is at most `2 * minChunk` it takes the whole thing; otherwise it takes the **first half**, leaving the remainder claimable by others. Ties on length break toward the lowest start, so the policy is deterministic and therefore testable.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMCoreTests/RangeSetClaimTests.swift`:

```swift
import Testing

@testable import SDMCore

@Test func gapsOfEmptySetIsWholeFile() {
    #expect(RangeSet().gaps(within: 100) == [ByteRange(start: 0, end: 100)])
}

@Test func gapsBetweenAndAfterRanges() {
    let set = RangeSet([ByteRange(start: 10, end: 20), ByteRange(start: 40, end: 50)])
    #expect(
        set.gaps(within: 100) == [
            ByteRange(start: 0, end: 10),
            ByteRange(start: 20, end: 40),
            ByteRange(start: 50, end: 100),
        ]
    )
}

@Test func noGapsWhenComplete() {
    let set = RangeSet([ByteRange(start: 0, end: 100)])
    #expect(set.gaps(within: 100).isEmpty)
    #expect(set.isComplete(total: 100))
}

@Test func incompleteWhenAnyGapRemains() {
    let set = RangeSet([ByteRange(start: 0, end: 99)])
    #expect(!set.isComplete(total: 100))
}

@Test func firstClaimTakesFirstHalfOfWholeFile() {
    let claim = RangeSet().nextClaim(total: 1000, reserved: [], minChunk: 10)
    #expect(claim == ByteRange(start: 0, end: 500))
}

@Test func secondClaimAvoidsReservedRange() {
    let claim = RangeSet().nextClaim(
        total: 1000,
        reserved: [ByteRange(start: 0, end: 500)],
        minChunk: 10
    )
    #expect(claim == ByteRange(start: 500, end: 750))
}

@Test func smallGapIsTakenWhole() {
    let claim = RangeSet().nextClaim(total: 15, reserved: [], minChunk: 10)
    #expect(claim == ByteRange(start: 0, end: 15))
}

@Test func claimPrefersLargestGap() {
    let set = RangeSet([ByteRange(start: 100, end: 200)])
    let claim = set.nextClaim(total: 1000, reserved: [], minChunk: 10)
    #expect(claim == ByteRange(start: 200, end: 600))
}

@Test func noClaimWhenEverythingIsDoneOrReserved() {
    let set = RangeSet([ByteRange(start: 0, end: 500)])
    let claim = set.nextClaim(
        total: 1000,
        reserved: [ByteRange(start: 500, end: 1000)],
        minChunk: 10
    )
    #expect(claim == nil)
}

@Test func claimSkipsFragmentedHolesLeftByRetiredWorkers() {
    let set = RangeSet([
        ByteRange(start: 0, end: 100),
        ByteRange(start: 150, end: 400),
        ByteRange(start: 450, end: 1000),
    ])
    let claim = set.nextClaim(total: 1000, reserved: [], minChunk: 10)
    #expect(claim == ByteRange(start: 100, end: 125))
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter RangeSetClaimTests`
Expected: FAIL — `value of type 'RangeSet' has no member 'gaps'`.

- [ ] **Step 3: Implement gaps, completeness, and claiming**

Append to `SDMKit/Sources/SDMCore/RangeSet.swift`:

```swift
extension RangeSet {
    /// The complement of this set within `[0, total)`.
    public func gaps(within total: Int64) -> [ByteRange] {
        precondition(total >= 0, "total must be non-negative, got \(total)")
        var result: [ByteRange] = []
        var cursor: Int64 = 0
        for range in ranges {
            if range.start > cursor {
                result.append(ByteRange(start: cursor, end: Swift.min(range.start, total)))
            }
            cursor = Swift.max(cursor, range.end)
            if cursor >= total { break }
        }
        if cursor < total {
            result.append(ByteRange(start: cursor, end: total))
        }
        return result.filter { !$0.isEmpty }
    }

    public func isComplete(total: Int64) -> Bool {
        gaps(within: total).isEmpty
    }

    /// Selects the next range for an idle worker to download.
    ///
    /// Takes the largest gap not already reserved by an active worker. Gaps of
    /// at most `2 * minChunk` are taken whole; larger gaps are halved so other
    /// workers can claim the remainder.
    ///
    /// - Parameter reserved: ranges currently held by active workers.
    /// - Returns: the claimed range, or `nil` when no work remains.
    public func nextClaim(
        total: Int64,
        reserved: [ByteRange],
        minChunk: Int64
    ) -> ByteRange? {
        precondition(minChunk > 0, "minChunk must be positive, got \(minChunk)")

        var blocked = self
        for range in reserved { blocked.insert(range) }

        let free = blocked.gaps(within: total)
        guard
            let target = free.max(by: {
                $0.length == $1.length ? $0.start > $1.start : $0.length < $1.length
            })
        else { return nil }

        if target.length <= minChunk * 2 { return target }
        return ByteRange(start: target.start, end: target.start + target.length / 2)
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
git commit -m "feat: add gap detection and worker claim policy to RangeSet"
```

---

### Task 4: Domain models

**Files:**
- Create: `SDMKit/Sources/SDMCore/Priority.swift`
- Create: `SDMKit/Sources/SDMCore/DownloadItem.swift`
- Create: `SDMKit/Sources/SDMCore/DownloadPackage.swift`
- Create: `SDMKit/Tests/SDMCoreTests/DomainModelTests.swift`

**Interfaces:**
- Consumes: `RangeSet` from Tasks 2–3
- Produces:
  - `enum Priority: Int, Comparable, Codable, CaseIterable, Sendable` — `lowest, low, normal, high, highest`
  - `enum ItemState: Equatable, Codable, Sendable` — `queued, running, completed, failed(reason: String)`
  - `struct DownloadItem: Identifiable, Equatable, Codable, Sendable`
  - `struct DownloadPackage: Identifiable, Equatable, Codable, Sendable`

`DownloadPackage` is named with the `Download` prefix deliberately — a bare `Package` collides with `PackageDescription.Package` and reads badly in a Swift package.

Per spec §6.1 there is **one** enable axis; `isEnabled` is it. `ItemState` carries no `paused` or `disabled` case — those are `queued` plus `isEnabled == false`.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMCoreTests/DomainModelTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMCore

@Test func prioritiesCompareByRank() {
    #expect(Priority.highest > Priority.normal)
    #expect(Priority.lowest < Priority.low)
    #expect(Priority.allCases.count == 5)
}

@Test func itemDefaultsToQueuedAndEnabled() {
    let item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    #expect(item.state == .queued)
    #expect(item.isEnabled)
    #expect(item.priority == nil)
    #expect(item.completed.ranges.isEmpty)
    #expect(item.totalBytes == nil)
    #expect(item.isResumable == false)
}

@Test func itemProgressIsZeroWhenSizeUnknown() {
    let item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    #expect(item.fractionCompleted == 0)
}

@Test func itemProgressReflectsCompletedRanges() {
    var item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    item.totalBytes = 1000
    item.completed.insert(ByteRange(start: 0, end: 250))
    #expect(item.fractionCompleted == 0.25)
}

@Test func effectivePriorityFallsBackToPackage() {
    var package = DownloadPackage(name: "Season 1")
    package.priority = .high
    let item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    #expect(package.effectivePriority(for: item) == .high)
}

@Test func itemPriorityOverridesPackage() {
    var package = DownloadPackage(name: "Season 1")
    package.priority = .low
    var item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    item.priority = .highest
    #expect(package.effectivePriority(for: item) == .highest)
}

@Test func failedStateCarriesReason() throws {
    let state = ItemState.failed(reason: "404 Not Found")
    let data = try JSONEncoder().encode(state)
    #expect(try JSONDecoder().decode(ItemState.self, from: data) == state)
}

@Test func packageRoundTripsThroughCodable() throws {
    var package = DownloadPackage(name: "Season 1")
    package.items = [DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")]
    let data = try JSONEncoder().encode(package)
    #expect(try JSONDecoder().decode(DownloadPackage.self, from: data) == package)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter DomainModelTests`
Expected: FAIL — `cannot find 'Priority' in scope`.

- [ ] **Step 3: Implement `Priority` and `ItemState`**

`SDMKit/Sources/SDMCore/Priority.swift`:

```swift
public enum Priority: Int, Comparable, Codable, CaseIterable, Sendable {
    case lowest = 0
    case low = 1
    case normal = 2
    case high = 3
    case highest = 4

    public static func < (lhs: Priority, rhs: Priority) -> Bool {
        lhs.rawValue < rhs.rawValue
    }
}

/// Lifecycle state of a download.
///
/// There is deliberately no `paused` or `disabled` case: pausing is
/// `isEnabled == false` on the item, and a preempted item returns to `queued`.
public enum ItemState: Equatable, Codable, Sendable {
    case queued
    case running
    case completed
    case failed(reason: String)
}
```

- [ ] **Step 4: Implement `DownloadItem`**

`SDMKit/Sources/SDMCore/DownloadItem.swift`:

```swift
import Foundation

public struct DownloadItem: Identifiable, Equatable, Codable, Sendable {
    public let id: UUID
    public var url: URL
    public var filename: String
    public var totalBytes: Int64?
    public var completed: RangeSet
    public var state: ItemState
    public var isEnabled: Bool
    public var isResumable: Bool
    public var priority: Priority?
    /// Position within the owning package. Lower sorts earlier.
    public var position: Int
    /// Server validator captured at download start, used to detect a changed remote file.
    public var validator: String?

    public init(
        id: UUID = UUID(),
        url: URL,
        filename: String,
        totalBytes: Int64? = nil,
        completed: RangeSet = RangeSet(),
        state: ItemState = .queued,
        isEnabled: Bool = true,
        isResumable: Bool = false,
        priority: Priority? = nil,
        position: Int = 0,
        validator: String? = nil
    ) {
        precondition(!filename.isEmpty, "filename must not be empty")
        self.id = id
        self.url = url
        self.filename = filename
        self.totalBytes = totalBytes
        self.completed = completed
        self.state = state
        self.isEnabled = isEnabled
        self.isResumable = isResumable
        self.priority = priority
        self.position = position
        self.validator = validator
    }

    public var fractionCompleted: Double {
        guard let total = totalBytes, total > 0 else { return 0 }
        return Double(completed.totalBytes) / Double(total)
    }

    public var isComplete: Bool {
        guard let total = totalBytes else { return false }
        return completed.isComplete(total: total)
    }
}
```

- [ ] **Step 5: Implement `DownloadPackage`**

`SDMKit/Sources/SDMCore/DownloadPackage.swift`:

```swift
import Foundation

public struct DownloadPackage: Identifiable, Equatable, Codable, Sendable {
    public let id: UUID
    public var name: String
    public var items: [DownloadItem]
    public var priority: Priority
    /// Position within the download list. Lower sorts earlier.
    public var position: Int

    public init(
        id: UUID = UUID(),
        name: String,
        items: [DownloadItem] = [],
        priority: Priority = .normal,
        position: Int = 0
    ) {
        precondition(!name.isEmpty, "package name must not be empty")
        self.id = id
        self.name = name
        self.items = items
        self.priority = priority
        self.position = position
    }

    /// An item's own priority when set, otherwise the package's.
    public func effectivePriority(for item: DownloadItem) -> Priority {
        item.priority ?? priority
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
git commit -m "feat: add Priority, ItemState, DownloadItem and DownloadPackage models"
```

---

### Task 5: `HTTPTransport` protocol, `FakeOrigin`, and `URLSessionTransport`

**Files:**
- Create: `SDMKit/Sources/SDMEngine/HTTPTransport.swift`
- Create: `SDMKit/Sources/SDMEngine/URLSessionTransport.swift`
- Create: `SDMKit/Sources/SDMEngine/FakeOrigin.swift`
- Create: `SDMKit/Tests/SDMEngineTests/FakeOriginTests.swift`

**Interfaces:**
- Consumes: `ByteRange` from `SDMCore`
- Produces:
  - `protocol HTTPTransport: Sendable` with `func fetch(_ request: RangeRequest) async throws -> RangeResponse`
  - `struct RangeRequest: Sendable` — `url: URL`, `range: ByteRange?`
  - `struct RangeResponse: Sendable` — `statusCode: Int`, `headers: [String: String]`, `body: AsyncThrowingStream<Data, any Error>`, plus `totalSize: Int64?`, `acceptsRanges: Bool`, `validator: String?`
  - `final class FakeOrigin: HTTPTransport` with a `Behavior` struct
  - `struct URLSessionTransport: HTTPTransport`
  - `enum TransportError: Error, Equatable` — `connectionDropped`, `http(status: Int)`, `malformedResponse`

`FakeOrigin` ships in `Sources`, not `Tests`, so later phases and the driver UI can use it too. It is the single most valuable piece of test infrastructure in this plan — every hostile-server scenario in Task 11 is a `Behavior` flag here.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMEngineTests/FakeOriginTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMEngine

private let url = URL(string: "https://example.com/file.bin")!

private func payload(_ count: Int) -> Data {
    Data((0..<count).map { UInt8($0 % 251) })
}

private func collect(_ response: RangeResponse) async throws -> Data {
    var data = Data()
    for try await chunk in response.body { data.append(chunk) }
    return data
}

@Test func fullFetchReturnsWholePayload() async throws {
    let origin = FakeOrigin(payload: payload(1000))
    let response = try await origin.fetch(RangeRequest(url: url, range: nil))
    #expect(response.statusCode == 200)
    #expect(response.totalSize == 1000)
    #expect(response.acceptsRanges)
    #expect(try await collect(response) == payload(1000))
}

@Test func rangedFetchReturnsOnlyThatSlice() async throws {
    let origin = FakeOrigin(payload: payload(1000))
    let response = try await origin.fetch(
        RangeRequest(url: url, range: ByteRange(start: 100, end: 200))
    )
    #expect(response.statusCode == 206)
    #expect(try await collect(response) == payload(1000)[100..<200])
}

@Test func originIgnoringRangesReturnsWholeBody() async throws {
    var behavior = FakeOrigin.Behavior()
    behavior.ignoresRanges = true
    let origin = FakeOrigin(payload: payload(1000), behavior: behavior)
    let response = try await origin.fetch(
        RangeRequest(url: url, range: ByteRange(start: 100, end: 200))
    )
    #expect(response.statusCode == 200)
    #expect(!response.acceptsRanges)
    #expect(try await collect(response).count == 1000)
}

@Test func originDropsConnectionAtConfiguredOffset() async throws {
    var behavior = FakeOrigin.Behavior()
    behavior.dropAfterBytes = 40
    let origin = FakeOrigin(payload: payload(1000), behavior: behavior)
    let response = try await origin.fetch(RangeRequest(url: url, range: nil))
    await #expect(throws: TransportError.connectionDropped) {
        _ = try await collect(response)
    }
}

@Test func originCanReportWrongContentLength() async throws {
    var behavior = FakeOrigin.Behavior()
    behavior.reportedSizeOverride = 5000
    let origin = FakeOrigin(payload: payload(1000), behavior: behavior)
    let response = try await origin.fetch(RangeRequest(url: url, range: nil))
    #expect(response.totalSize == 5000)
}

@Test func originCanReturnErrorStatus() async throws {
    var behavior = FakeOrigin.Behavior()
    behavior.statusOverride = 403
    let origin = FakeOrigin(payload: payload(1000), behavior: behavior)
    let response = try await origin.fetch(RangeRequest(url: url, range: nil))
    #expect(response.statusCode == 403)
}

@Test func originValidatorCanChangeBetweenRequests() async throws {
    var behavior = FakeOrigin.Behavior()
    behavior.validator = "etag-1"
    let origin = FakeOrigin(payload: payload(1000), behavior: behavior)
    let first = try await origin.fetch(RangeRequest(url: url, range: nil))
    #expect(first.validator == "etag-1")
    await origin.setValidator("etag-2")
    let second = try await origin.fetch(RangeRequest(url: url, range: nil))
    #expect(second.validator == "etag-2")
}

@Test func originRecordsRequestedRanges() async throws {
    let origin = FakeOrigin(payload: payload(1000))
    _ = try await origin.fetch(RangeRequest(url: url, range: ByteRange(start: 0, end: 10)))
    _ = try await origin.fetch(RangeRequest(url: url, range: ByteRange(start: 10, end: 20)))
    let requested = await origin.requestedRanges
    #expect(requested == [ByteRange(start: 0, end: 10), ByteRange(start: 10, end: 20)])
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter FakeOriginTests`
Expected: FAIL — `cannot find 'FakeOrigin' in scope`.

- [ ] **Step 3: Implement the transport protocol**

`SDMKit/Sources/SDMEngine/HTTPTransport.swift`:

```swift
import Foundation
import SDMCore

public struct RangeRequest: Sendable {
    public let url: URL
    /// Half-open range to request, or `nil` for the whole resource.
    public let range: ByteRange?

    public init(url: URL, range: ByteRange? = nil) {
        self.url = url
        self.range = range
    }
}

public struct RangeResponse: Sendable {
    public let statusCode: Int
    /// Total size of the whole resource, when the origin discloses it.
    public let totalSize: Int64?
    public let acceptsRanges: Bool
    /// `ETag`, or `Last-Modified` when no `ETag` is offered.
    public let validator: String?
    public let body: AsyncThrowingStream<Data, any Error>

    public init(
        statusCode: Int,
        totalSize: Int64?,
        acceptsRanges: Bool,
        validator: String?,
        body: AsyncThrowingStream<Data, any Error>
    ) {
        self.statusCode = statusCode
        self.totalSize = totalSize
        self.acceptsRanges = acceptsRanges
        self.validator = validator
        self.body = body
    }
}

public enum TransportError: Error, Equatable {
    case connectionDropped
    case http(status: Int)
    case malformedResponse
}

/// The engine's only route to the network. Injected so tests never touch it.
public protocol HTTPTransport: Sendable {
    func fetch(_ request: RangeRequest) async throws -> RangeResponse
}
```

- [ ] **Step 4: Implement `FakeOrigin`**

`SDMKit/Sources/SDMEngine/FakeOrigin.swift`:

```swift
import Foundation
import SDMCore

/// An in-process origin server for tests, programmable to misbehave the way
/// real servers do. See spec §11.1.
public actor FakeOrigin: HTTPTransport {
    public struct Behavior: Sendable {
        /// Serve the whole body regardless of the requested range.
        public var ignoresRanges = false
        /// Report this size instead of the true payload size.
        public var reportedSizeOverride: Int64?
        /// Throw `.connectionDropped` after emitting this many bytes.
        public var dropAfterBytes: Int?
        /// Return this status instead of 200/206.
        public var statusOverride: Int?
        /// Value reported as the `ETag`.
        public var validator: String?
        /// Bytes emitted per chunk of the response stream.
        public var chunkSize = 64

        public init() {}
    }

    private let payload: Data
    private var behavior: Behavior
    public private(set) var requestedRanges: [ByteRange] = []

    public init(payload: Data, behavior: Behavior = Behavior()) {
        self.payload = payload
        self.behavior = behavior
    }

    public func setValidator(_ validator: String?) {
        behavior.validator = validator
    }

    public func setBehavior(_ behavior: Behavior) {
        self.behavior = behavior
    }

    public func fetch(_ request: RangeRequest) async throws -> RangeResponse {
        let total = Int64(payload.count)
        if let range = request.range {
            requestedRanges.append(range)
        }

        if let status = behavior.statusOverride {
            return RangeResponse(
                statusCode: status,
                totalSize: behavior.reportedSizeOverride ?? total,
                acceptsRanges: !behavior.ignoresRanges,
                validator: behavior.validator,
                body: AsyncThrowingStream { $0.finish() }
            )
        }

        let slice: Data
        let status: Int
        if let range = request.range, !behavior.ignoresRanges {
            let lower = Int(Swift.min(range.start, total))
            let upper = Int(Swift.min(range.end, total))
            slice = payload.subdata(in: lower..<upper)
            status = 206
        } else {
            slice = payload
            status = 200
        }

        let chunkSize = behavior.chunkSize
        let dropAfter = behavior.dropAfterBytes
        let body = AsyncThrowingStream<Data, any Error> { continuation in
            var emitted = 0
            var offset = 0
            while offset < slice.count {
                let end = Swift.min(offset + chunkSize, slice.count)
                if let limit = dropAfter, emitted + (end - offset) > limit {
                    let allowed = limit - emitted
                    if allowed > 0 {
                        continuation.yield(slice.subdata(in: offset..<(offset + allowed)))
                    }
                    continuation.finish(throwing: TransportError.connectionDropped)
                    return
                }
                continuation.yield(slice.subdata(in: offset..<end))
                emitted += end - offset
                offset = end
            }
            continuation.finish()
        }

        return RangeResponse(
            statusCode: status,
            totalSize: behavior.reportedSizeOverride ?? total,
            acceptsRanges: !behavior.ignoresRanges,
            validator: behavior.validator,
            body: body
        )
    }
}
```

- [ ] **Step 5: Implement `URLSessionTransport`**

`SDMKit/Sources/SDMEngine/URLSessionTransport.swift`:

```swift
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
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: add HTTPTransport protocol, FakeOrigin test double and URLSessionTransport"
```

---

### Task 6: `SparseFile` — offset writes and completion

**Files:**
- Create: `SDMKit/Sources/SDMEngine/SparseFile.swift`
- Create: `SDMKit/Tests/SDMEngineTests/SparseFileTests.swift`

**Interfaces:**
- Consumes: nothing
- Produces: `final class SparseFile: @unchecked Sendable` with
  - `init(finalURL: URL, totalBytes: Int64) throws` — creates `<finalURL>.incomplete`, preallocated
  - `func write(_ data: Data, at offset: Int64) throws`
  - `func sync() throws`
  - `func finalize() throws -> URL` — fsync, close, rename off `.incomplete`
  - `func close()`
  - `static func incompleteURL(for finalURL: URL) -> URL`

`@unchecked Sendable` is deliberate: the type wraps a file descriptor guarded by an internal lock, and `pwrite` at disjoint offsets is safe from multiple threads. This is the one place in the engine where that escape hatch is justified — document it and do not copy the pattern elsewhere.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMEngineTests/SparseFileTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMEngine

private func makeScratchDirectory() throws -> URL {
    let url = FileManager.default.temporaryDirectory
        .appendingPathComponent("sdm-tests-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

@Test func createsIncompleteFileWithSuffix() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let final = dir.appendingPathComponent("movie.mp4")

    let file = try SparseFile(finalURL: final, totalBytes: 100)
    defer { file.close() }

    let incomplete = dir.appendingPathComponent("movie.mp4.incomplete")
    #expect(FileManager.default.fileExists(atPath: incomplete.path))
    #expect(!FileManager.default.fileExists(atPath: final.path))
}

@Test func writesLandAtTheirAbsoluteOffsets() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let final = dir.appendingPathComponent("out.bin")

    let file = try SparseFile(finalURL: final, totalBytes: 10)
    try file.write(Data([9, 9]), at: 8)
    try file.write(Data([1, 2, 3]), at: 0)
    let result = try file.finalize()

    #expect(try Data(contentsOf: result) == Data([1, 2, 3, 0, 0, 0, 0, 0, 9, 9]))
}

@Test func finalizeRenamesOffTheIncompleteSuffix() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let final = dir.appendingPathComponent("movie.mp4")

    let file = try SparseFile(finalURL: final, totalBytes: 4)
    try file.write(Data([1, 2, 3, 4]), at: 0)
    let result = try file.finalize()

    #expect(result == final)
    #expect(FileManager.default.fileExists(atPath: final.path))
    #expect(!FileManager.default.fileExists(atPath: final.path + ".incomplete"))
}

@Test func reopeningPreservesExistingBytes() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let final = dir.appendingPathComponent("out.bin")

    let first = try SparseFile(finalURL: final, totalBytes: 6)
    try first.write(Data([1, 2, 3]), at: 0)
    try first.sync()
    first.close()

    let second = try SparseFile(finalURL: final, totalBytes: 6)
    try second.write(Data([4, 5, 6]), at: 3)
    let result = try second.finalize()

    #expect(try Data(contentsOf: result) == Data([1, 2, 3, 4, 5, 6]))
}

@Test func incompleteURLAppendsSuffix() {
    let url = URL(fileURLWithPath: "/tmp/a.bin")
    #expect(SparseFile.incompleteURL(for: url).lastPathComponent == "a.bin.incomplete")
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter SparseFileTests`
Expected: FAIL — `cannot find 'SparseFile' in scope`.

- [ ] **Step 3: Implement `SparseFile`**

`SDMKit/Sources/SDMEngine/SparseFile.swift`:

```swift
import Foundation

/// The on-disk destination for a download, written at arbitrary offsets by
/// multiple concurrent workers.
///
/// Marked `@unchecked Sendable` because it wraps a POSIX file descriptor
/// guarded by an internal lock; `pwrite` to disjoint offsets is safe across
/// threads. This is the only type in the engine using that escape hatch.
public final class SparseFile: @unchecked Sendable {
    public enum FileError: Error, Equatable {
        case couldNotOpen(path: String, errno: Int32)
        case writeFailed(errno: Int32)
        case truncateFailed(errno: Int32)
    }

    public let finalURL: URL
    public let incompleteURL: URL
    private let descriptor: Int32
    private let lock = NSLock()
    private var isClosed = false

    public static func incompleteURL(for finalURL: URL) -> URL {
        finalURL.appendingPathExtension("incomplete")
    }

    /// Opens (creating if needed) the `.incomplete` file and preallocates it.
    public init(finalURL: URL, totalBytes: Int64) throws {
        precondition(totalBytes >= 0, "totalBytes must be non-negative")
        self.finalURL = finalURL
        self.incompleteURL = Self.incompleteURL(for: finalURL)

        let path = incompleteURL.path
        let fd = open(path, O_RDWR | O_CREAT, 0o644)
        guard fd >= 0 else { throw FileError.couldNotOpen(path: path, errno: errno) }
        self.descriptor = fd

        // Sparse on APFS: allocates no blocks until bytes are actually written.
        guard ftruncate(fd, off_t(totalBytes)) == 0 else {
            close(fd)
            throw FileError.truncateFailed(errno: errno)
        }
    }

    public func write(_ data: Data, at offset: Int64) throws {
        precondition(offset >= 0, "offset must be non-negative")
        guard !data.isEmpty else { return }

        lock.lock()
        defer { lock.unlock() }

        try data.withUnsafeBytes { buffer in
            var written = 0
            while written < buffer.count {
                let result = pwrite(
                    descriptor,
                    buffer.baseAddress!.advanced(by: written),
                    buffer.count - written,
                    off_t(offset) + off_t(written)
                )
                guard result > 0 else { throw FileError.writeFailed(errno: errno) }
                written += result
            }
        }
    }

    public func sync() throws {
        lock.lock()
        defer { lock.unlock() }
        guard fsync(descriptor) == 0 else { throw FileError.writeFailed(errno: errno) }
    }

    /// Flushes, closes, and renames the file to its final name.
    public func finalize() throws -> URL {
        try sync()
        close()
        try FileManager.default.moveItem(at: incompleteURL, to: finalURL)
        return finalURL
    }

    public func close() {
        lock.lock()
        defer { lock.unlock() }
        guard !isClosed else { return }
        _ = Foundation.close(descriptor)
        isClosed = true
    }

    deinit {
        if !isClosed { _ = Foundation.close(descriptor) }
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
git commit -m "feat: add SparseFile for concurrent offset writes and atomic completion"
```

---

### Task 7: `ResumeSidecar` — persistence of the interval set

**Files:**
- Create: `SDMKit/Sources/SDMEngine/ResumeSidecar.swift`
- Create: `SDMKit/Tests/SDMEngineTests/ResumeSidecarTests.swift`

**Interfaces:**
- Consumes: `RangeSet`, `SparseFile`
- Produces:
  - `struct ResumeSidecar: Codable, Equatable, Sendable` — `sourceURL: URL`, `totalBytes: Int64`, `validator: String?`, `completed: RangeSet`, `formatVersion: Int`
  - `static func url(for finalURL: URL) -> URL`
  - `func save(to url: URL) throws` — atomic
  - `static func load(from url: URL) -> ResumeSidecar?` — returns `nil` for missing, corrupt, or wrong-version files

Per spec §4.3, a missing or unreadable sidecar means **restart from zero**, never a partial trust. `load` therefore returns `nil` rather than throwing — callers cannot accidentally propagate a corrupt state as an error to retry.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMEngineTests/ResumeSidecarTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMEngine

private func makeScratchDirectory() throws -> URL {
    let url = FileManager.default.temporaryDirectory
        .appendingPathComponent("sdm-tests-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

private func sampleSidecar() -> ResumeSidecar {
    ResumeSidecar(
        sourceURL: URL(string: "https://example.com/a.bin")!,
        totalBytes: 1000,
        validator: "etag-1",
        completed: RangeSet([ByteRange(start: 0, end: 250)])
    )
}

@Test func sidecarURLUsesSdmpartExtension() {
    let url = URL(fileURLWithPath: "/tmp/movie.mp4")
    #expect(ResumeSidecar.url(for: url).lastPathComponent == "movie.mp4.sdmpart")
}

@Test func sidecarRoundTripsThroughDisk() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("a.bin.sdmpart")

    try sampleSidecar().save(to: url)
    #expect(ResumeSidecar.load(from: url) == sampleSidecar())
}

@Test func loadingMissingSidecarReturnsNil() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    #expect(ResumeSidecar.load(from: dir.appendingPathComponent("nope.sdmpart")) == nil)
}

@Test func loadingCorruptSidecarReturnsNil() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("a.bin.sdmpart")
    try Data("not json at all".utf8).write(to: url)
    #expect(ResumeSidecar.load(from: url) == nil)
}

@Test func loadingFutureFormatVersionReturnsNil() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("a.bin.sdmpart")
    var sidecar = sampleSidecar()
    sidecar.formatVersion = 999
    try sidecar.save(to: url)
    #expect(ResumeSidecar.load(from: url) == nil)
}

@Test func savingOverwritesPreviousContent() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("a.bin.sdmpart")

    try sampleSidecar().save(to: url)
    var updated = sampleSidecar()
    updated.completed.insert(ByteRange(start: 500, end: 900))
    try updated.save(to: url)

    #expect(ResumeSidecar.load(from: url)?.completed.totalBytes == 650)
}

@Test func matchesRejectsChangedValidator() {
    let sidecar = sampleSidecar()
    #expect(sidecar.matches(totalBytes: 1000, validator: "etag-1"))
    #expect(!sidecar.matches(totalBytes: 1000, validator: "etag-2"))
    #expect(!sidecar.matches(totalBytes: 2000, validator: "etag-1"))
}

@Test func matchesAcceptsAbsentValidatorOnBothSides() {
    let sidecar = ResumeSidecar(
        sourceURL: URL(string: "https://example.com/a.bin")!,
        totalBytes: 1000,
        validator: nil,
        completed: RangeSet()
    )
    #expect(sidecar.matches(totalBytes: 1000, validator: nil))
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter ResumeSidecarTests`
Expected: FAIL — `cannot find 'ResumeSidecar' in scope`.

- [ ] **Step 3: Implement `ResumeSidecar`**

`SDMKit/Sources/SDMEngine/ResumeSidecar.swift`:

```swift
import Foundation
import SDMCore

/// Resume state written alongside the `.incomplete` file, so the file and its
/// recovery information travel together. See spec §4.3.
public struct ResumeSidecar: Codable, Equatable, Sendable {
    public static let currentFormatVersion = 1

    public var formatVersion: Int
    public var sourceURL: URL
    public var totalBytes: Int64
    /// `ETag` or `Last-Modified` captured when the download started.
    public var validator: String?
    public var completed: RangeSet

    public init(
        formatVersion: Int = ResumeSidecar.currentFormatVersion,
        sourceURL: URL,
        totalBytes: Int64,
        validator: String?,
        completed: RangeSet
    ) {
        self.formatVersion = formatVersion
        self.sourceURL = sourceURL
        self.totalBytes = totalBytes
        self.validator = validator
        self.completed = completed
    }

    public static func url(for finalURL: URL) -> URL {
        finalURL.appendingPathExtension("sdmpart")
    }

    /// Whether the remote resource still matches what was captured at start.
    public func matches(totalBytes: Int64, validator: String?) -> Bool {
        self.totalBytes == totalBytes && self.validator == validator
    }

    public func save(to url: URL) throws {
        let data = try JSONEncoder().encode(self)
        try data.write(to: url, options: .atomic)
    }

    /// Loads a sidecar, returning `nil` when it is missing, unreadable,
    /// corrupt, or written by a newer format version.
    ///
    /// Deliberately non-throwing: an unusable sidecar always means "restart
    /// from zero", never "retry the load".
    public static func load(from url: URL) -> ResumeSidecar? {
        guard let data = try? Data(contentsOf: url),
            let sidecar = try? JSONDecoder().decode(ResumeSidecar.self, from: data),
            sidecar.formatVersion == currentFormatVersion
        else { return nil }
        return sidecar
    }

    public static func remove(at url: URL) {
        try? FileManager.default.removeItem(at: url)
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
git commit -m "feat: add ResumeSidecar with fail-closed loading"
```

---

### Task 8: `DownloadTask` — single-worker happy path

**Files:**
- Create: `SDMKit/Sources/SDMEngine/DownloadTask.swift`
- Create: `SDMKit/Tests/SDMEngineTests/DownloadTaskTests.swift`
- Create: `SDMKit/Tests/SDMEngineTests/TestSupport.swift`

**Interfaces:**
- Consumes: `HTTPTransport`, `SparseFile`, `ResumeSidecar`, `RangeSet`
- Produces:
  - `actor DownloadTask` with
    - `init(id: UUID, sourceURL: URL, destinationURL: URL, transport: any HTTPTransport, configuration: Configuration)`
    - `struct Configuration: Sendable` — `workerCount: Int`, `minChunk: Int64`, `checkpointInterval: Int64`
    - `func start() async throws -> URL`
    - `var completedRanges: RangeSet { get }`
    - `var activeWorkerCount: Int { get }`
  - `enum DownloadError: Error, Equatable` — `unknownSize`, `serverError(status: Int)`, `incompleteAfterWorkersFinished`

This task establishes the download loop with `workerCount: 1`. Task 9 generalizes it to N; do not attempt both at once.

- [ ] **Step 1: Write the shared test helpers**

`SDMKit/Tests/SDMEngineTests/TestSupport.swift`:

```swift
import Foundation
import SDMCore

@testable import SDMEngine

/// Deterministic pseudo-random payload; index-derived so any byte can be
/// verified independently of how it was fetched.
func testPayload(_ count: Int) -> Data {
    Data((0..<count).map { UInt8(($0 &* 31 &+ 7) % 251) })
}

func makeScratchDirectory() throws -> URL {
    let url = FileManager.default.temporaryDirectory
        .appendingPathComponent("sdm-tests-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

let testSourceURL = URL(string: "https://example.com/file.bin")!

extension DownloadTask.Configuration {
    static func test(workers: Int, minChunk: Int64 = 64) -> DownloadTask.Configuration {
        DownloadTask.Configuration(
            workerCount: workers,
            minChunk: minChunk,
            checkpointInterval: 128
        )
    }
}
```

- [ ] **Step 2: Write the failing tests**

`SDMKit/Tests/SDMEngineTests/DownloadTaskTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMEngine

@Test func singleWorkerDownloadsCompleteFile() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let payload = testPayload(4000)
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: payload),
        configuration: .test(workers: 1)
    )

    let result = try await task.start()
    #expect(try Data(contentsOf: result) == payload)
}

@Test func completedFileHasNoIncompleteOrSidecarLeftBehind() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")

    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: testPayload(1000)),
        configuration: .test(workers: 1)
    )
    _ = try await task.start()

    let remaining = try FileManager.default.contentsOfDirectory(atPath: dir.path)
    #expect(remaining == ["out.bin"])
}

@Test func rangeSetCoversWholeFileAfterCompletion() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: testPayload(1000)),
        configuration: .test(workers: 1)
    )
    _ = try await task.start()

    let completed = await task.completedRanges
    #expect(completed.ranges == [ByteRange(start: 0, end: 1000)])
}

@Test func errorStatusFailsTheDownload() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    var behavior = FakeOrigin.Behavior()
    behavior.statusOverride = 404
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: testPayload(100), behavior: behavior),
        configuration: .test(workers: 1)
    )

    await #expect(throws: DownloadError.serverError(status: 404)) {
        _ = try await task.start()
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter DownloadTaskTests`
Expected: FAIL — `cannot find 'DownloadTask' in scope`.

- [ ] **Step 4: Implement `DownloadTask` for a single worker**

`SDMKit/Sources/SDMEngine/DownloadTask.swift`:

```swift
import Foundation
import SDMCore

public enum DownloadError: Error, Equatable {
    case unknownSize
    case serverError(status: Int)
    case incompleteAfterWorkersFinished
}

/// Downloads one file using a pool of ephemeral workers that claim gaps from a
/// shared `RangeSet`. See spec §5.
public actor DownloadTask {
    public struct Configuration: Sendable {
        public var workerCount: Int
        public var minChunk: Int64
        /// Bytes written per worker between sidecar checkpoints.
        public var checkpointInterval: Int64

        public init(workerCount: Int, minChunk: Int64, checkpointInterval: Int64) {
            precondition(workerCount >= 1, "workerCount must be at least 1")
            precondition(minChunk > 0, "minChunk must be positive")
            precondition(checkpointInterval > 0, "checkpointInterval must be positive")
            self.workerCount = workerCount
            self.minChunk = minChunk
            self.checkpointInterval = checkpointInterval
        }
    }

    public let id: UUID
    public let sourceURL: URL
    public let destinationURL: URL

    private let transport: any HTTPTransport
    private var configuration: Configuration

    private var completed = RangeSet()
    private var reserved: [UUID: ByteRange] = [:]
    private var totalBytes: Int64 = 0
    private var validator: String?
    private var file: SparseFile?
    private var bytesSinceCheckpoint: Int64 = 0

    public init(
        id: UUID,
        sourceURL: URL,
        destinationURL: URL,
        transport: any HTTPTransport,
        configuration: Configuration
    ) {
        self.id = id
        self.sourceURL = sourceURL
        self.destinationURL = destinationURL
        self.transport = transport
        self.configuration = configuration
    }

    public var completedRanges: RangeSet { completed }
    public var activeWorkerCount: Int { reserved.count }

    private var sidecarURL: URL { ResumeSidecar.url(for: destinationURL) }

    /// Probes the origin, then runs the worker pool until the file is complete.
    public func start() async throws -> URL {
        try await prepare()
        try await runWorkers()

        guard completed.isComplete(total: totalBytes) else {
            throw DownloadError.incompleteAfterWorkersFinished
        }

        guard let file else { throw DownloadError.incompleteAfterWorkersFinished }
        let result = try file.finalize()
        self.file = nil
        ResumeSidecar.remove(at: sidecarURL)
        return result
    }

    /// Probes the resource and opens the destination file.
    private func prepare() async throws {
        let probe = try await transport.fetch(
            RangeRequest(url: sourceURL, range: ByteRange(start: 0, end: 1))
        )
        guard (200..<300).contains(probe.statusCode) else {
            throw DownloadError.serverError(status: probe.statusCode)
        }
        guard let size = probe.totalSize, size > 0 else { throw DownloadError.unknownSize }

        totalBytes = size
        validator = probe.validator
        file = try SparseFile(finalURL: destinationURL, totalBytes: size)
    }

    private func runWorkers() async throws {
        try await withThrowingTaskGroup(of: Void.self) { group in
            for _ in 0..<configuration.workerCount {
                group.addTask { [weak self] in
                    guard let self else { return }
                    try await self.workerLoop()
                }
            }
            try await group.waitForAll()
        }
    }

    /// One worker: claim a gap, stream it to disk, repeat until nothing is left.
    private func workerLoop() async throws {
        let workerID = UUID()
        defer { reserved[workerID] = nil }

        while let claim = claimNext(for: workerID) {
            try await download(claim)
            reserved[workerID] = nil
        }
    }

    private func claimNext(for workerID: UUID) -> ByteRange? {
        guard
            let claim = completed.nextClaim(
                total: totalBytes,
                reserved: Array(reserved.values),
                minChunk: configuration.minChunk
            )
        else { return nil }
        reserved[workerID] = claim
        return claim
    }

    private func download(_ claim: ByteRange) async throws {
        let response = try await transport.fetch(RangeRequest(url: sourceURL, range: claim))
        guard (200..<300).contains(response.statusCode) else {
            throw DownloadError.serverError(status: response.statusCode)
        }

        var offset = claim.start
        for try await chunk in response.body {
            guard offset < claim.end else { break }
            let writable = Swift.min(Int64(chunk.count), claim.end - offset)
            let slice = chunk.prefix(Int(writable))
            try file?.write(Data(slice), at: offset)
            record(ByteRange(start: offset, end: offset + writable))
            offset += writable
        }
    }

    /// Folds a freshly written range into the completed set, checkpointing
    /// the sidecar every `checkpointInterval` bytes.
    private func record(_ range: ByteRange) {
        completed.insert(range)
        bytesSinceCheckpoint += range.length
        if bytesSinceCheckpoint >= configuration.checkpointInterval {
            checkpoint()
        }
    }

    private func checkpoint() {
        bytesSinceCheckpoint = 0
        let sidecar = ResumeSidecar(
            sourceURL: sourceURL,
            totalBytes: totalBytes,
            validator: validator,
            completed: completed
        )
        try? sidecar.save(to: sidecarURL)
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
git commit -m "feat: add DownloadTask with single-worker download loop"
```

---

### Task 9: Multi-worker downloads and dynamic resegmentation

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/DownloadTask.swift`
- Create: `SDMKit/Tests/SDMEngineTests/ResegmentationTests.swift`

**Interfaces:**
- Consumes: everything from Task 8
- Produces: `func setWorkerCount(_ count: Int) async` on `DownloadTask`

This is the requirement the whole design exists to serve (spec §5.2): raising the count spawns workers, lowering it retires them after their current claim, and the survivors consume the fragmented holes left behind.

Implementation approach: the task group holds a **generation counter**. `setWorkerCount` bumps the target and, when raising, adds tasks to the running group. Workers check `shouldRetire(workerIndex:)` at the top of each claim loop and exit when their index exceeds the current target.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMEngineTests/ResegmentationTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMEngine

@Test func eightWorkersProduceIdenticalOutput() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let payload = testPayload(20_000)
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: payload),
        configuration: .test(workers: 8)
    )

    let result = try await task.start()
    #expect(try Data(contentsOf: result) == payload)
}

@Test func multipleWorkersActuallyRequestDisjointRanges() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let origin = FakeOrigin(payload: testPayload(20_000))
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: origin,
        configuration: .test(workers: 4)
    )
    _ = try await task.start()

    // Drop the 1-byte probe request from prepare().
    let claims = await origin.requestedRanges.filter { $0.length > 1 }
    #expect(claims.count > 1)

    var seen = RangeSet()
    for claim in claims {
        for other in seen.ranges {
            #expect(claim.start >= other.end || claim.end <= other.start)
        }
        seen.insert(claim)
    }
}

@Test func loweringWorkerCountMidFlightStillCompletesFile() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let payload = testPayload(50_000)
    var behavior = FakeOrigin.Behavior()
    behavior.chunkSize = 256
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: payload, behavior: behavior),
        configuration: .test(workers: 100, minChunk: 64)
    )

    async let result = task.start()
    await task.setWorkerCount(3)
    #expect(try await Data(contentsOf: result) == payload)
}

@Test func raisingWorkerCountMidFlightStillCompletesFile() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let payload = testPayload(50_000)
    var behavior = FakeOrigin.Behavior()
    behavior.chunkSize = 256
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: payload, behavior: behavior),
        configuration: .test(workers: 2, minChunk: 64)
    )

    async let result = task.start()
    await task.setWorkerCount(16)
    #expect(try await Data(contentsOf: result) == payload)
}

@Test(arguments: [1, 2, 3, 7, 13, 32])
func randomizedWorkerChurnPreservesByteIdentity(seed: Int) async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let payload = testPayload(40_000)
    var behavior = FakeOrigin.Behavior()
    behavior.chunkSize = 128
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: payload, behavior: behavior),
        configuration: .test(workers: 4, minChunk: 64)
    )

    async let result = task.start()
    var generator = SeededGenerator(seed: UInt64(seed))
    for _ in 0..<12 {
        await task.setWorkerCount(Int.random(in: 1...24, using: &generator))
        await Task.yield()
    }
    #expect(try await Data(contentsOf: result) == payload)
}

/// Deterministic generator so a failing case is reproducible from its seed.
struct SeededGenerator: RandomNumberGenerator {
    private var state: UInt64
    init(seed: UInt64) { self.state = seed &* 6_364_136_223_846_793_005 &+ 1 }
    mutating func next() -> UInt64 {
        state ^= state << 13
        state ^= state >> 7
        state ^= state << 17
        return state
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter ResegmentationTests`
Expected: FAIL — `value of type 'DownloadTask' has no member 'setWorkerCount'`.

- [ ] **Step 3: Add worker-count state to `DownloadTask`**

Add these properties alongside the existing ones:

```swift
    private var targetWorkerCount: Int = 1
    private var liveWorkerIndices: Set<Int> = []
    private var nextWorkerIndex: Int = 0
    private var workerGroupContinuation: (@Sendable (Int) -> Void)?
```

- [ ] **Step 4: Replace `runWorkers` and `workerLoop`**

Replace the existing `runWorkers()` and `workerLoop()` with:

```swift
    private func runWorkers() async throws {
        targetWorkerCount = configuration.workerCount

        try await withThrowingTaskGroup(of: Void.self) { group in
            // Workers added later (by setWorkerCount) join through this hook.
            workerGroupContinuation = nil
            var spawned = 0
            while spawned < targetWorkerCount {
                let index = nextWorkerIndex
                nextWorkerIndex += 1
                liveWorkerIndices.insert(index)
                group.addTask { [weak self] in
                    guard let self else { return }
                    try await self.workerLoop(index: index)
                }
                spawned += 1
            }

            // Drain finished workers, spawning replacements when the target
            // rises above the number still running.
            while try await group.next() != nil {
                while liveWorkerIndices.count < targetWorkerCount, hasWorkRemaining() {
                    let index = nextWorkerIndex
                    nextWorkerIndex += 1
                    liveWorkerIndices.insert(index)
                    group.addTask { [weak self] in
                        guard let self else { return }
                        try await self.workerLoop(index: index)
                    }
                }
            }
        }
    }

    private func hasWorkRemaining() -> Bool {
        completed.nextClaim(
            total: totalBytes,
            reserved: Array(reserved.values),
            minChunk: configuration.minChunk
        ) != nil
    }

    /// One worker: claim a gap, stream it to disk, repeat until the file is
    /// done or this worker is retired by a lowered target count.
    private func workerLoop(index: Int) async throws {
        let workerID = UUID()
        defer {
            reserved[workerID] = nil
            liveWorkerIndices.remove(index)
        }

        while !shouldRetire(index: index) {
            guard let claim = claimNext(for: workerID) else { return }
            try await download(claim)
            reserved[workerID] = nil
        }
    }

    /// Retires the highest-indexed workers first, so lowering the target from
    /// 100 to 3 keeps three long-lived workers rather than churning all of them.
    private func shouldRetire(index: Int) -> Bool {
        let rank = liveWorkerIndices.sorted().firstIndex(of: index) ?? 0
        return rank >= targetWorkerCount
    }

    /// Changes the number of concurrent workers mid-download.
    ///
    /// Raising it spawns more as slots free; lowering it retires surplus
    /// workers after they finish their current claim. Their partial progress is
    /// already in `completed`, so survivors simply pick up the resulting gaps.
    public func setWorkerCount(_ count: Int) {
        precondition(count >= 1, "workerCount must be at least 1")
        targetWorkerCount = count
        configuration.workerCount = count
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS, including all six `randomizedWorkerChurnPreservesByteIdentity` cases.

- [ ] **Step 6: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: support multiple workers and mid-flight resegmentation"
```

---

### Task 10: Resume across a full teardown

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/DownloadTask.swift`
- Create: `SDMKit/Tests/SDMEngineTests/ResumeTests.swift`

**Interfaces:**
- Consumes: `ResumeSidecar`, `DownloadTask`
- Produces: `func pause() async` on `DownloadTask`; `prepare()` gains sidecar loading.

Per spec §11.3 this is tested as a **real** restart: the first task object is discarded entirely and a second one is constructed from the on-disk sidecar.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMEngineTests/ResumeTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMEngine

/// Runs a download to partial completion, then abandons the task object,
/// leaving `.incomplete` and `.sdmpart` on disk.
private func downloadPartially(
    payload: Data,
    destination: URL,
    stopAfter: Int
) async throws {
    var behavior = FakeOrigin.Behavior()
    behavior.dropAfterBytes = stopAfter
    behavior.chunkSize = 64
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload, behavior: behavior),
        configuration: .test(workers: 1)
    )
    _ = try? await task.start()
    await task.pause()
}

@Test func resumingFromSidecarProducesIdenticalFile() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(8000)

    try await downloadPartially(payload: payload, destination: destination, stopAfter: 2000)
    #expect(ResumeSidecar.load(from: ResumeSidecar.url(for: destination)) != nil)

    let resumed = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload),
        configuration: .test(workers: 4)
    )
    let result = try await resumed.start()
    #expect(try Data(contentsOf: result) == payload)
}

@Test func resumeDoesNotRefetchAlreadyCompletedBytes() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(8000)

    try await downloadPartially(payload: payload, destination: destination, stopAfter: 4000)
    let carried = ResumeSidecar.load(from: ResumeSidecar.url(for: destination))!
    #expect(carried.completed.totalBytes > 0)

    let origin = FakeOrigin(payload: payload)
    let resumed = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: origin,
        configuration: .test(workers: 1)
    )
    _ = try await resumed.start()

    let refetched = await origin.requestedRanges.filter { $0.length > 1 }
    let overlap = refetched.contains { claim in
        carried.completed.ranges.contains { $0.start < claim.end && claim.start < $0.end }
    }
    #expect(!overlap)
}

@Test func missingSidecarRestartsFromZero() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(4000)

    try await downloadPartially(payload: payload, destination: destination, stopAfter: 1000)
    ResumeSidecar.remove(at: ResumeSidecar.url(for: destination))

    let resumed = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload),
        configuration: .test(workers: 1)
    )
    #expect(try await Data(contentsOf: resumed.start()) == payload)
}

@Test func corruptSidecarRestartsFromZero() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(4000)

    try await downloadPartially(payload: payload, destination: destination, stopAfter: 1000)
    try Data("garbage".utf8).write(to: ResumeSidecar.url(for: destination))

    let resumed = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload),
        configuration: .test(workers: 1)
    )
    #expect(try await Data(contentsOf: resumed.start()) == payload)
}

@Test func changedValidatorDiscardsPartialAndRestarts() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(4000)

    var behavior = FakeOrigin.Behavior()
    behavior.validator = "etag-1"
    behavior.dropAfterBytes = 1000
    behavior.chunkSize = 64
    let first = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload, behavior: behavior),
        configuration: .test(workers: 1)
    )
    _ = try? await first.start()
    await first.pause()

    var changed = FakeOrigin.Behavior()
    changed.validator = "etag-2"
    let resumed = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: payload, behavior: changed),
        configuration: .test(workers: 1)
    )
    _ = try await resumed.start()

    let restarted = await resumed.completedRanges
    #expect(restarted.ranges == [ByteRange(start: 0, end: 4000)])
    #expect(try Data(contentsOf: destination) == payload)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter ResumeTests`
Expected: FAIL — `value of type 'DownloadTask' has no member 'pause'`.

- [ ] **Step 3: Add `pause()` to `DownloadTask`**

```swift
    /// Stops workers and flushes resume state to disk.
    ///
    /// The partial file and its sidecar are left in place; a later `start()` on
    /// a fresh task resumes from them.
    public func pause() {
        targetWorkerCount = 0
        checkpoint()
        file?.close()
        file = nil
    }
```

- [ ] **Step 4: Teach `prepare()` to load the sidecar**

Replace the body of `prepare()` with:

```swift
    private func prepare() async throws {
        let probe = try await transport.fetch(
            RangeRequest(url: sourceURL, range: ByteRange(start: 0, end: 1))
        )
        guard (200..<300).contains(probe.statusCode) else {
            throw DownloadError.serverError(status: probe.statusCode)
        }
        guard let size = probe.totalSize, size > 0 else { throw DownloadError.unknownSize }

        totalBytes = size
        validator = probe.validator

        // Resume only when the sidecar is present, readable, and still
        // describes the same remote resource. Anything else restarts at zero.
        if let sidecar = ResumeSidecar.load(from: sidecarURL),
            sidecar.matches(totalBytes: size, validator: probe.validator)
        {
            completed = sidecar.completed
        } else {
            completed = RangeSet()
            ResumeSidecar.remove(at: sidecarURL)
            try? FileManager.default.removeItem(at: SparseFile.incompleteURL(for: destinationURL))
        }

        file = try SparseFile(finalURL: destinationURL, totalBytes: size)
    }
```

- [ ] **Step 5: Make `pause()` stop the worker loop**

`shouldRetire(index:)` already returns `true` for every worker when `targetWorkerCount` is 0, so no further change is needed. Verify by reading the method.

- [ ] **Step 6: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: resume downloads from sidecar with validator guard"
```

---

### Task 11: Hostile server matrix

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/DownloadTask.swift`
- Create: `SDMKit/Tests/SDMEngineTests/HostileServerTests.swift`

**Interfaces:**
- Consumes: `FakeOrigin.Behavior`, `DownloadTask`
- Produces: `var supportsRanges: Bool { get }` on `DownloadTask`

Per spec §5.3, an origin that ignores `Range` forces the pool to a single worker. Without this, N workers each receive the whole body and stomp each other's offsets.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMEngineTests/HostileServerTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMEngine

@Test func serverIgnoringRangesForcesSingleWorker() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let payload = testPayload(5000)
    var behavior = FakeOrigin.Behavior()
    behavior.ignoresRanges = true
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: payload, behavior: behavior),
        configuration: .test(workers: 16)
    )

    let result = try await task.start()
    #expect(try Data(contentsOf: result) == payload)
    #expect(await task.supportsRanges == false)
}

@Test func serverOverstatingSizeFailsRatherThanTruncating() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    var behavior = FakeOrigin.Behavior()
    behavior.reportedSizeOverride = 9999
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: testPayload(1000), behavior: behavior),
        configuration: .test(workers: 1)
    )

    await #expect(throws: DownloadError.incompleteAfterWorkersFinished) {
        _ = try await task.start()
    }
}

@Test func midBodyDropLeavesRecoverablePartialProgress() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")

    var behavior = FakeOrigin.Behavior()
    behavior.dropAfterBytes = 500
    behavior.chunkSize = 64
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: testPayload(4000), behavior: behavior),
        configuration: .test(workers: 1)
    )

    _ = try? await task.start()
    await task.pause()

    let sidecar = ResumeSidecar.load(from: ResumeSidecar.url(for: destination))
    #expect(sidecar != nil)
    #expect(sidecar!.completed.totalBytes >= 448)
}

@Test func partialFileIsNeverRenamedIntoPlace() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")

    var behavior = FakeOrigin.Behavior()
    behavior.dropAfterBytes = 200
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: testPayload(4000), behavior: behavior),
        configuration: .test(workers: 1)
    )

    _ = try? await task.start()
    #expect(!FileManager.default.fileExists(atPath: destination.path))
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter HostileServerTests`
Expected: FAIL — `value of type 'DownloadTask' has no member 'supportsRanges'`.

- [ ] **Step 3: Track range support and clamp the pool**

Add the property alongside the others:

```swift
    private var acceptsRanges = true
```

Expose it:

```swift
    public var supportsRanges: Bool { acceptsRanges }
```

In `prepare()`, immediately after `validator = probe.validator`, add:

```swift
        // A server that ignores Range cannot be segmented: every worker would
        // receive the whole body and overwrite the others' offsets.
        acceptsRanges = probe.acceptsRanges
        if !acceptsRanges {
            configuration.workerCount = 1
            targetWorkerCount = 1
        }
```

- [ ] **Step 4: Guard the worker loop against oversized responses**

In `download(_:)`, the existing `guard offset < claim.end else { break }` already discards bytes past the claim, which is what makes a `Range`-ignoring server safe with one worker. Confirm that line is present and add this clarifying comment above it:

```swift
            // A Range-ignoring origin sends the whole body; discard anything
            // past the claim rather than writing outside it.
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: handle Range-ignoring, size-lying and connection-dropping origins"
```

---

### Task 12: Retry classification and backoff

**Files:**
- Create: `SDMKit/Sources/SDMEngine/RetryPolicy.swift`
- Create: `SDMKit/Tests/SDMEngineTests/RetryPolicyTests.swift`

**Interfaces:**
- Consumes: `TransportError`, `DownloadError`
- Produces:
  - `enum FailureKind: Equatable, Sendable` — `transient`, `permanent(reason: String)`
  - `struct RetryPolicy: Sendable` — `maxAttempts: Int`, `baseDelay: Duration`, `func classify(_ error: any Error) -> FailureKind`, `func delay(forAttempt attempt: Int) -> Duration`

Backoff is exponential with deterministic jitter derived from the attempt number, not `random()` — a flaky-looking test is worse than a slightly less uniform jitter distribution, and tests must be reproducible.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMEngineTests/RetryPolicyTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMEngine

@Test func connectionDropsAreTransient() {
    #expect(RetryPolicy().classify(TransportError.connectionDropped) == .transient)
}

@Test func serverErrorsAreTransient() {
    #expect(RetryPolicy().classify(TransportError.http(status: 503)) == .transient)
    #expect(RetryPolicy().classify(DownloadError.serverError(status: 500)) == .transient)
}

@Test func notFoundIsPermanent() {
    #expect(
        RetryPolicy().classify(DownloadError.serverError(status: 404))
            == .permanent(reason: "HTTP 404")
    )
}

@Test func forbiddenIsTransientBecauseSignedURLsExpire() {
    #expect(RetryPolicy().classify(DownloadError.serverError(status: 403)) == .transient)
}

@Test func unknownSizeIsPermanent() {
    #expect(
        RetryPolicy().classify(DownloadError.unknownSize)
            == .permanent(reason: "Server did not report a size")
    )
}

@Test func delayGrowsExponentially() {
    let policy = RetryPolicy(maxAttempts: 5, baseDelay: .milliseconds(100))
    #expect(policy.delay(forAttempt: 0) < policy.delay(forAttempt: 1))
    #expect(policy.delay(forAttempt: 1) < policy.delay(forAttempt: 2))
}

@Test func delayIsDeterministicForTheSameAttempt() {
    let policy = RetryPolicy(maxAttempts: 5, baseDelay: .milliseconds(100))
    #expect(policy.delay(forAttempt: 3) == policy.delay(forAttempt: 3))
}

@Test func delayIsCappedAtCeiling() {
    let policy = RetryPolicy(maxAttempts: 20, baseDelay: .seconds(1), maxDelay: .seconds(30))
    #expect(policy.delay(forAttempt: 19) <= .seconds(30))
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter RetryPolicyTests`
Expected: FAIL — `cannot find 'RetryPolicy' in scope`.

- [ ] **Step 3: Implement `RetryPolicy`**

`SDMKit/Sources/SDMEngine/RetryPolicy.swift`:

```swift
import Foundation

public enum FailureKind: Equatable, Sendable {
    case transient
    case permanent(reason: String)
}

public struct RetryPolicy: Sendable {
    public var maxAttempts: Int
    public var baseDelay: Duration
    public var maxDelay: Duration

    public init(
        maxAttempts: Int = 5,
        baseDelay: Duration = .seconds(1),
        maxDelay: Duration = .seconds(60)
    ) {
        precondition(maxAttempts >= 1, "maxAttempts must be at least 1")
        self.maxAttempts = maxAttempts
        self.baseDelay = baseDelay
        self.maxDelay = maxDelay
    }

    public func classify(_ error: any Error) -> FailureKind {
        if let transport = error as? TransportError {
            switch transport {
            case .connectionDropped, .malformedResponse:
                return .transient
            case .http(let status):
                return Self.classify(status: status)
            }
        }
        if let download = error as? DownloadError {
            switch download {
            case .serverError(let status):
                return Self.classify(status: status)
            case .unknownSize:
                return .permanent(reason: "Server did not report a size")
            case .incompleteAfterWorkersFinished:
                return .transient
            }
        }
        return .transient
    }

    /// 403 is transient on purpose: signed media URLs (googlevideo and
    /// friends) expire, and the correct response is to refresh and retry.
    private static func classify(status: Int) -> FailureKind {
        switch status {
        case 403, 408, 425, 429, 500...599:
            return .transient
        default:
            return .permanent(reason: "HTTP \(status)")
        }
    }

    /// Exponential backoff with deterministic jitter derived from the attempt
    /// number, so retry timing is reproducible in tests.
    public func delay(forAttempt attempt: Int) -> Duration {
        precondition(attempt >= 0, "attempt must be non-negative")
        let factor = Double(1 << Swift.min(attempt, 20))
        let jitter = 1.0 + Double((attempt &* 37) % 25) / 100.0
        let seconds = baseDelay.seconds * factor * jitter
        return Swift.min(.seconds(seconds), maxDelay)
    }
}

extension Duration {
    fileprivate var seconds: Double {
        Double(components.seconds) + Double(components.attoseconds) / 1e18
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
git commit -m "feat: add retry classification and deterministic backoff"
```

---

### Task 13: Speed telemetry

**Files:**
- Create: `SDMKit/Sources/SDMEngine/SpeedSampler.swift`
- Create: `SDMKit/Tests/SDMEngineTests/SpeedSamplerTests.swift`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `struct SpeedSampler: Sendable` — `init(historyLength: Int, smoothingFactor: Double)`, `mutating func record(bytes: Int64)`, `mutating func tick()`, `var bytesPerSecond: Double`, `var history: [Double]`

Per spec §5.4, one 1 Hz tick drives everything and aggregates are computed rather than stored. This type is the per-item sampler; package and global figures are sums over it, so there is no separate aggregate sampler to keep in sync.

`tick()` is driven by the engine's clock, so tests advance time by calling it directly — no sleeping.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMEngineTests/SpeedSamplerTests.swift`:

```swift
import Testing

@testable import SDMEngine

@Test func newSamplerReportsZero() {
    #expect(SpeedSampler().bytesPerSecond == 0)
}

@Test func firstTickReportsBytesRecordedInThatSecond() {
    var sampler = SpeedSampler(historyLength: 60, smoothingFactor: 1.0)
    sampler.record(bytes: 1000)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 1000)
}

@Test func recordAccumulatesWithinOneTick() {
    var sampler = SpeedSampler(historyLength: 60, smoothingFactor: 1.0)
    sampler.record(bytes: 400)
    sampler.record(bytes: 600)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 1000)
}

@Test func counterResetsAfterEachTick() {
    var sampler = SpeedSampler(historyLength: 60, smoothingFactor: 1.0)
    sampler.record(bytes: 1000)
    sampler.tick()
    sampler.tick()
    #expect(sampler.bytesPerSecond == 0)
}

@Test func smoothingDampensSuddenChanges() {
    var sampler = SpeedSampler(historyLength: 60, smoothingFactor: 0.5)
    sampler.record(bytes: 1000)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 500)
    sampler.record(bytes: 1000)
    sampler.tick()
    #expect(sampler.bytesPerSecond == 750)
}

@Test func historyRecordsRawSamplesInOrder() {
    var sampler = SpeedSampler(historyLength: 60, smoothingFactor: 1.0)
    sampler.record(bytes: 100)
    sampler.tick()
    sampler.record(bytes: 200)
    sampler.tick()
    #expect(sampler.history == [100, 200])
}

@Test func historyIsCappedAtItsLength() {
    var sampler = SpeedSampler(historyLength: 3, smoothingFactor: 1.0)
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
    var sampler = SpeedSampler(historyLength: 10, smoothingFactor: 1.0)
    sampler.record(bytes: 100)
    sampler.tick()
    sampler.record(bytes: 300)
    sampler.tick()
    #expect(sampler.runningAverage == 200)
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter SpeedSamplerTests`
Expected: FAIL — `cannot find 'SpeedSampler' in scope`.

- [ ] **Step 3: Implement `SpeedSampler`**

`SDMKit/Sources/SDMEngine/SpeedSampler.swift`:

```swift
/// Per-item speed measurement, driven by the engine's 1 Hz tick.
///
/// Package and global speeds are sums over these samplers rather than separate
/// state, so the three figures can never disagree. See spec §5.4.
public struct SpeedSampler: Sendable {
    private let historyLength: Int
    private let smoothingFactor: Double
    private var pendingBytes: Int64 = 0
    private var smoothed: Double = 0
    private var samples: [Double] = []

    /// - Parameter smoothingFactor: EMA weight for the newest sample, in `(0, 1]`.
    ///   `1.0` disables smoothing.
    public init(historyLength: Int = 60, smoothingFactor: Double = 0.4) {
        precondition(historyLength > 0, "historyLength must be positive")
        precondition(
            smoothingFactor > 0 && smoothingFactor <= 1,
            "smoothingFactor must be in (0, 1], got \(smoothingFactor)"
        )
        self.historyLength = historyLength
        self.smoothingFactor = smoothingFactor
    }

    /// Adds bytes transferred since the last tick.
    public mutating func record(bytes: Int64) {
        precondition(bytes >= 0, "bytes must be non-negative")
        pendingBytes += bytes
    }

    /// Closes the current one-second window.
    public mutating func tick() {
        let raw = Double(pendingBytes)
        pendingBytes = 0
        smoothed = smoothingFactor * raw + (1 - smoothingFactor) * smoothed
        samples.append(raw)
        if samples.count > historyLength {
            samples.removeFirst(samples.count - historyLength)
        }
    }

    public var bytesPerSecond: Double { smoothed }
    public var history: [Double] { samples }

    public var runningAverage: Double {
        guard !samples.isEmpty else { return 0 }
        return samples.reduce(0, +) / Double(samples.count)
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
git commit -m "feat: add SpeedSampler with EMA smoothing and ring-buffer history"
```

---

### Task 14: Scheduler ranking function

**Files:**
- Create: `SDMKit/Sources/SDMEngine/Scheduler.swift`
- Create: `SDMKit/Tests/SDMEngineTests/SchedulerTests.swift`

**Interfaces:**
- Consumes: `DownloadItem`, `DownloadPackage`, `Priority`
- Produces:
  - `struct SchedulerInput: Sendable` — `packages: [DownloadPackage]`, `runningNow: Set<UUID>`, `startedRecently: Set<UUID>`, `maxConcurrent: Int`
  - `enum Scheduler` with `static func desiredRunningSet(_ input: SchedulerInput) -> Set<UUID>`
  - `static func rank(_ packages: [DownloadPackage]) -> [DownloadItem]`

This is the whole of spec §6, and it is a **pure function** — no actors, no I/O, no clock. Every scheduling behavior in the app is testable as a table here.

Three rules, applied in order:
1. Running non-resumable items reserve their slots unconditionally (§6.3).
2. Running items inside the hysteresis window keep their slots (§6.4).
3. Remaining slots fill by rank: effective priority desc, package position asc, item position asc.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMEngineTests/SchedulerTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMEngine

private func item(
    _ name: String,
    priority: Priority? = nil,
    position: Int = 0,
    enabled: Bool = true,
    resumable: Bool = true,
    state: ItemState = .queued
) -> DownloadItem {
    DownloadItem(
        url: URL(string: "https://example.com/\(name)")!,
        filename: name,
        totalBytes: 1000,
        state: state,
        isEnabled: enabled,
        isResumable: resumable,
        priority: priority,
        position: position
    )
}

private func package(
    _ name: String,
    priority: Priority = .normal,
    position: Int = 0,
    items: [DownloadItem]
) -> DownloadPackage {
    DownloadPackage(name: name, items: items, priority: priority, position: position)
}

@Test func runningSetNeverExceedsMaxConcurrent() {
    let items = (0..<10).map { item("f\($0).bin", position: $0) }
    let input = SchedulerInput(
        packages: [package("p", items: items)],
        runningNow: [],
        startedRecently: [],
        maxConcurrent: 3
    )
    #expect(Scheduler.desiredRunningSet(input).count == 3)
}

@Test func higherPriorityWinsRegardlessOfPosition() {
    let low = item("low.bin", position: 0)
    let high = item("high.bin", priority: .highest, position: 9)
    let input = SchedulerInput(
        packages: [package("p", items: [low, high])],
        runningNow: [],
        startedRecently: [],
        maxConcurrent: 1
    )
    #expect(Scheduler.desiredRunningSet(input) == [high.id])
}

@Test func packagePriorityLiftsItsItems() {
    let normal = item("a.bin", position: 0)
    let lifted = item("b.bin", position: 0)
    let input = SchedulerInput(
        packages: [
            package("normal", priority: .normal, position: 0, items: [normal]),
            package("urgent", priority: .highest, position: 1, items: [lifted]),
        ],
        runningNow: [],
        startedRecently: [],
        maxConcurrent: 1
    )
    #expect(Scheduler.desiredRunningSet(input) == [lifted.id])
}

@Test func itemPriorityOverridesItsPackage() {
    let pinned = item("pinned.bin", priority: .highest, position: 5)
    let sibling = item("other.bin", position: 0)
    let input = SchedulerInput(
        packages: [package("low", priority: .lowest, items: [sibling, pinned])],
        runningNow: [],
        startedRecently: [],
        maxConcurrent: 1
    )
    #expect(Scheduler.desiredRunningSet(input) == [pinned.id])
}

@Test func disabledItemsAreNeverScheduled() {
    let disabled = item("off.bin", position: 0, enabled: false)
    let enabled = item("on.bin", position: 1)
    let input = SchedulerInput(
        packages: [package("p", items: [disabled, enabled])],
        runningNow: [],
        startedRecently: [],
        maxConcurrent: 5
    )
    #expect(Scheduler.desiredRunningSet(input) == [enabled.id])
}

@Test func completedAndFailedItemsAreNeverScheduled() {
    let done = item("done.bin", position: 0, state: .completed)
    let failed = item("bad.bin", position: 1, state: .failed(reason: "404"))
    let ready = item("go.bin", position: 2)
    let input = SchedulerInput(
        packages: [package("p", items: [done, failed, ready])],
        runningNow: [],
        startedRecently: [],
        maxConcurrent: 5
    )
    #expect(Scheduler.desiredRunningSet(input) == [ready.id])
}

@Test func runningNonResumableItemKeepsItsSlotAgainstHigherPriority() {
    let stubborn = item("noresume.bin", priority: .lowest, position: 9, resumable: false)
    let urgent = item("urgent.bin", priority: .highest, position: 0)
    let input = SchedulerInput(
        packages: [package("p", items: [stubborn, urgent])],
        runningNow: [stubborn.id],
        startedRecently: [],
        maxConcurrent: 1
    )
    #expect(Scheduler.desiredRunningSet(input) == [stubborn.id])
}

@Test func nonResumableItemThatIsNotRunningHasNoSpecialClaim() {
    let stubborn = item("noresume.bin", priority: .lowest, position: 9, resumable: false)
    let urgent = item("urgent.bin", priority: .highest, position: 0)
    let input = SchedulerInput(
        packages: [package("p", items: [stubborn, urgent])],
        runningNow: [],
        startedRecently: [],
        maxConcurrent: 1
    )
    #expect(Scheduler.desiredRunningSet(input) == [urgent.id])
}

@Test func hysteresisProtectsRecentlyStartedItems() {
    let fresh = item("fresh.bin", priority: .lowest, position: 9)
    let urgent = item("urgent.bin", priority: .highest, position: 0)
    let input = SchedulerInput(
        packages: [package("p", items: [fresh, urgent])],
        runningNow: [fresh.id],
        startedRecently: [fresh.id],
        maxConcurrent: 1
    )
    #expect(Scheduler.desiredRunningSet(input) == [fresh.id])
}

@Test func settledRunningItemIsPreemptedByHigherPriority() {
    let settled = item("settled.bin", priority: .lowest, position: 9)
    let urgent = item("urgent.bin", priority: .highest, position: 0)
    let input = SchedulerInput(
        packages: [package("p", items: [settled, urgent])],
        runningNow: [settled.id],
        startedRecently: [],
        maxConcurrent: 1
    )
    #expect(Scheduler.desiredRunningSet(input) == [urgent.id])
}

@Test func reservationsAreCappedByMaxConcurrent() {
    let a = item("a.bin", position: 0, resumable: false)
    let b = item("b.bin", position: 1, resumable: false)
    let c = item("c.bin", position: 2, resumable: false)
    let input = SchedulerInput(
        packages: [package("p", items: [a, b, c])],
        runningNow: [a.id, b.id, c.id],
        startedRecently: [],
        maxConcurrent: 2
    )
    #expect(Scheduler.desiredRunningSet(input).count == 2)
}

@Test func rankOrdersByPriorityThenPackageThenPosition() {
    let input = [
        package(
            "second",
            priority: .normal,
            position: 1,
            items: [item("b1.bin", position: 1), item("b0.bin", position: 0)]
        ),
        package("first", priority: .normal, position: 0, items: [item("a0.bin", position: 0)]),
        package("urgent", priority: .high, position: 2, items: [item("z.bin", position: 0)]),
    ]
    #expect(Scheduler.rank(input).map(\.filename) == ["z.bin", "a0.bin", "b0.bin", "b1.bin"])
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter SchedulerTests`
Expected: FAIL — `cannot find 'Scheduler' in scope`.

- [ ] **Step 3: Implement the scheduler**

`SDMKit/Sources/SDMEngine/Scheduler.swift`:

```swift
import Foundation
import SDMCore

public struct SchedulerInput: Sendable {
    public var packages: [DownloadPackage]
    /// Items the engine currently has running.
    public var runningNow: Set<UUID>
    /// Items started within the hysteresis window; protected from preemption.
    public var startedRecently: Set<UUID>
    public var maxConcurrent: Int

    public init(
        packages: [DownloadPackage],
        runningNow: Set<UUID>,
        startedRecently: Set<UUID>,
        maxConcurrent: Int
    ) {
        precondition(maxConcurrent >= 0, "maxConcurrent must be non-negative")
        self.packages = packages
        self.runningNow = runningNow
        self.startedRecently = startedRecently
        self.maxConcurrent = maxConcurrent
    }
}

/// The scheduling policy, expressed as a pure function so every behavior in
/// spec §6 is table-testable. Re-evaluated on every change rather than
/// maintained as a queue.
public enum Scheduler {
    /// Items eligible to run, ordered best-first.
    ///
    /// Sort key: effective priority descending, then package position, then
    /// item position.
    public static func rank(_ packages: [DownloadPackage]) -> [DownloadItem] {
        var scored: [(item: DownloadItem, priority: Priority, packagePosition: Int)] = []
        for package in packages {
            for item in package.items where isEligible(item) {
                scored.append((item, package.effectivePriority(for: item), package.position))
            }
        }
        return
            scored
            .sorted { lhs, rhs in
                if lhs.priority != rhs.priority { return lhs.priority > rhs.priority }
                if lhs.packagePosition != rhs.packagePosition {
                    return lhs.packagePosition < rhs.packagePosition
                }
                return lhs.item.position < rhs.item.position
            }
            .map(\.item)
    }

    /// The set of item IDs that should be running right now.
    ///
    /// Slots are allocated in three passes: running non-resumable items (which
    /// cannot be preempted), then running items still inside the hysteresis
    /// window, then the highest-ranked remainder.
    public static func desiredRunningSet(_ input: SchedulerInput) -> Set<UUID> {
        let ranked = rank(input.packages)
        let eligibleIDs = Set(ranked.map(\.id))
        var slots = input.maxConcurrent
        var desired: Set<UUID> = []

        func reserve(_ candidates: [DownloadItem]) {
            for candidate in candidates where slots > 0 && !desired.contains(candidate.id) {
                desired.insert(candidate.id)
                slots -= 1
            }
        }

        // Pass 1: running non-resumable items keep their slots unconditionally.
        reserve(
            ranked.filter {
                input.runningNow.contains($0.id) && !$0.isResumable
            }
        )

        // Pass 2: recently started items are protected from drag-induced churn.
        reserve(
            ranked.filter {
                input.runningNow.contains($0.id) && input.startedRecently.contains($0.id)
            }
        )

        // Pass 3: fill whatever remains by rank.
        reserve(ranked)

        return desired.intersection(eligibleIDs)
    }

    private static func isEligible(_ item: DownloadItem) -> Bool {
        guard item.isEnabled else { return false }
        switch item.state {
        case .queued, .running: return true
        case .completed, .failed: return false
        }
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
git commit -m "feat: add pure-function scheduler with preemption and hysteresis"
```

---

### Task 15: `StateStore` — debounced atomic snapshot

**Files:**
- Create: `SDMKit/Sources/SDMEngine/StateStore.swift`
- Create: `SDMKit/Tests/SDMEngineTests/StateStoreTests.swift`

**Interfaces:**
- Consumes: `DownloadPackage`
- Produces:
  - `struct PersistedState: Codable, Equatable, Sendable` — `formatVersion: Int`, `packages: [DownloadPackage]`
  - `protocol StateStore: Sendable` — `func load() -> PersistedState`, `func save(_ state: PersistedState) async`, `func flush() async`
  - `actor JSONStateStore: StateStore`
  - `actor InMemoryStateStore: StateStore` (for tests and previews)

Debouncing is expressed as "coalesce saves until `flush()`", driven by the caller's timer rather than an internal one — an internal `Task.sleep` would make every test that touches persistence slow and flaky.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMEngineTests/StateStoreTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMEngine

private func samplePackage(_ name: String) -> DownloadPackage {
    DownloadPackage(
        name: name,
        items: [
            DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
        ]
    )
}

@Test func loadingFromEmptyDirectoryReturnsEmptyState() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let store = JSONStateStore(fileURL: dir.appendingPathComponent("state.json"))
    #expect(await store.load().packages.isEmpty)
}

@Test func savedStateRoundTripsAfterFlush() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("state.json")

    let store = JSONStateStore(fileURL: url)
    await store.save(PersistedState(packages: [samplePackage("Season 1")]))
    await store.flush()

    let reopened = JSONStateStore(fileURL: url)
    #expect(await reopened.load().packages.map(\.name) == ["Season 1"])
}

@Test func saveWithoutFlushDoesNotTouchDisk() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("state.json")

    let store = JSONStateStore(fileURL: url)
    await store.save(PersistedState(packages: [samplePackage("Season 1")]))
    #expect(!FileManager.default.fileExists(atPath: url.path))
}

@Test func repeatedSavesCoalesceToTheLatestValue() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("state.json")

    let store = JSONStateStore(fileURL: url)
    await store.save(PersistedState(packages: [samplePackage("first")]))
    await store.save(PersistedState(packages: [samplePackage("second")]))
    await store.save(PersistedState(packages: [samplePackage("third")]))
    await store.flush()

    #expect(await JSONStateStore(fileURL: url).load().packages.map(\.name) == ["third"])
}

@Test func flushWithNothingPendingIsHarmless() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let store = JSONStateStore(fileURL: dir.appendingPathComponent("state.json"))
    await store.flush()
    await store.flush()
    #expect(await store.load().packages.isEmpty)
}

@Test func corruptStateFileLoadsAsEmptyRatherThanCrashing() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("state.json")
    try Data("{ not json".utf8).write(to: url)
    #expect(await JSONStateStore(fileURL: url).load().packages.isEmpty)
}

@Test func inMemoryStoreRoundTrips() async {
    let store = InMemoryStateStore()
    await store.save(PersistedState(packages: [samplePackage("Season 1")]))
    await store.flush()
    #expect(await store.load().packages.map(\.name) == ["Season 1"])
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter StateStoreTests`
Expected: FAIL — `cannot find 'JSONStateStore' in scope`.

- [ ] **Step 3: Implement the state store**

`SDMKit/Sources/SDMEngine/StateStore.swift`:

```swift
import Foundation
import SDMCore

public struct PersistedState: Codable, Equatable, Sendable {
    public static let currentFormatVersion = 1

    public var formatVersion: Int
    public var packages: [DownloadPackage]

    public init(
        formatVersion: Int = PersistedState.currentFormatVersion,
        packages: [DownloadPackage] = []
    ) {
        self.formatVersion = formatVersion
        self.packages = packages
    }
}

public protocol StateStore: Sendable {
    func load() async -> PersistedState
    func save(_ state: PersistedState) async
    /// Writes any pending state immediately. Call on quit and on a debounce timer.
    func flush() async
}

/// Durable state as a single atomically replaced JSON file.
///
/// Saves are coalesced in memory until `flush()`; the caller owns the debounce
/// timer, so no test has to wait on an internal one. See spec §4.2.
public actor JSONStateStore: StateStore {
    private let fileURL: URL
    private var pending: PersistedState?

    public init(fileURL: URL) {
        self.fileURL = fileURL
    }

    public func load() -> PersistedState {
        if let pending { return pending }
        guard let data = try? Data(contentsOf: fileURL),
            let state = try? JSONDecoder().decode(PersistedState.self, from: data),
            state.formatVersion == PersistedState.currentFormatVersion
        else { return PersistedState() }
        return state
    }

    public func save(_ state: PersistedState) {
        pending = state
    }

    public func flush() {
        guard let state = pending else { return }
        pending = nil
        guard let data = try? JSONEncoder().encode(state) else { return }
        try? FileManager.default.createDirectory(
            at: fileURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try? data.write(to: fileURL, options: .atomic)
    }
}

/// Non-persistent store for tests and SwiftUI previews.
public actor InMemoryStateStore: StateStore {
    private var state = PersistedState()

    public init() {}

    public func load() -> PersistedState { state }
    public func save(_ state: PersistedState) { self.state = state }
    public func flush() {}
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: add debounced atomic JSON state store"
```

---

### Task 16: `DownloadEngine` — orchestration and snapshots

**Files:**
- Create: `SDMKit/Sources/SDMEngine/DownloadEngine.swift`
- Create: `SDMKit/Sources/SDMEngine/EngineSnapshot.swift`
- Create: `SDMKit/Tests/SDMEngineTests/DownloadEngineTests.swift`

**Interfaces:**
- Consumes: `DownloadTask`, `Scheduler`, `SpeedSampler`, `StateStore`, `HTTPTransport`
- Produces:
  - `struct EngineSnapshot: Sendable, Equatable` — `packages: [PackageSnapshot]`, `globalBytesPerSecond: Double`, `globalHistory: [Double]`
  - `struct PackageSnapshot: Sendable, Equatable, Identifiable` and `struct ItemSnapshot: Sendable, Equatable, Identifiable`
  - `actor DownloadEngine` with `init(transport:stateStore:settings:)`, `func add(_ package: DownloadPackage) async`, `func setEnabled(_:for:) async`, `func setPriority(_:for:) async`, `func setSegmentCount(_:for:) async`, `func tick() async`, `func snapshot() async -> EngineSnapshot`, `func shutdown() async`
  - `struct EngineSettings: Sendable` — `maxConcurrent: Int`, `segmentsPerItem: Int`, `globalMaxConnections: Int`, `downloadFolder: URL`

`tick()` is called by the app once per second and drives both telemetry and rescheduling. Exposing it makes the engine fully deterministic under test — no timers inside the actor.

- [ ] **Step 1: Write the failing tests**

`SDMKit/Tests/SDMEngineTests/DownloadEngineTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMEngine

private func makeEngine(
    payload: Data,
    folder: URL,
    maxConcurrent: Int = 2,
    segments: Int = 2
) -> DownloadEngine {
    DownloadEngine(
        transport: FakeOrigin(payload: payload),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: maxConcurrent,
            segmentsPerItem: segments,
            globalMaxConnections: 32,
            downloadFolder: folder
        )
    )
}

private func packageOf(_ names: [String]) -> DownloadPackage {
    DownloadPackage(
        name: "Batch",
        items: names.enumerated().map { index, name in
            DownloadItem(
                url: URL(string: "https://example.com/\(name)")!,
                filename: name,
                position: index
            )
        }
    )
}

@Test func engineDownloadsAllItemsIntoAPackageFolder() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let payload = testPayload(3000)

    let engine = makeEngine(payload: payload, folder: dir)
    await engine.add(packageOf(["a.bin", "b.bin"]))
    try await engine.runUntilIdle()

    let packageFolder = dir.appendingPathComponent("Batch")
    #expect(try Data(contentsOf: packageFolder.appendingPathComponent("a.bin")) == payload)
    #expect(try Data(contentsOf: packageFolder.appendingPathComponent("b.bin")) == payload)
}

@Test func disabledItemsAreNotDownloaded() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let engine = makeEngine(payload: testPayload(1000), folder: dir)
    var package = packageOf(["a.bin", "b.bin"])
    package.items[1].isEnabled = false
    await engine.add(package)
    try await engine.runUntilIdle()

    let folder = dir.appendingPathComponent("Batch")
    #expect(FileManager.default.fileExists(atPath: folder.appendingPathComponent("a.bin").path))
    #expect(!FileManager.default.fileExists(atPath: folder.appendingPathComponent("b.bin").path))
}

@Test func concurrencyLimitIsRespected() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let engine = makeEngine(
        payload: testPayload(20_000),
        folder: dir,
        maxConcurrent: 2
    )
    await engine.add(packageOf(["a.bin", "b.bin", "c.bin", "d.bin"]))
    await engine.tick()

    let running = await engine.snapshot().packages
        .flatMap(\.items)
        .filter { $0.state == .running }
    #expect(running.count <= 2)

    try await engine.runUntilIdle()
}

@Test func snapshotAggregatesSpeedAcrossItems() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let engine = makeEngine(payload: testPayload(4000), folder: dir)
    await engine.add(packageOf(["a.bin", "b.bin"]))
    try await engine.runUntilIdle()
    await engine.tick()

    let snapshot = await engine.snapshot()
    let itemTotal = snapshot.packages
        .flatMap(\.items)
        .reduce(0.0) { $0 + $1.bytesPerSecond }
    #expect(abs(snapshot.globalBytesPerSecond - itemTotal) < 0.001)
}

@Test func snapshotReportsSegmentedProgressRanges() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let engine = makeEngine(payload: testPayload(2000), folder: dir)
    await engine.add(packageOf(["a.bin"]))
    try await engine.runUntilIdle()

    let item = try #require(await engine.snapshot().packages.first?.items.first)
    #expect(item.completed.ranges == [ByteRange(start: 0, end: 2000)])
    #expect(item.state == .completed)
}

@Test func shutdownFlushesStateToTheStore() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let store = InMemoryStateStore()
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: testPayload(1000)),
        stateStore: store,
        settings: EngineSettings(
            maxConcurrent: 1,
            segmentsPerItem: 1,
            globalMaxConnections: 8,
            downloadFolder: dir
        )
    )
    await engine.add(packageOf(["a.bin"]))
    await engine.shutdown()

    #expect(await store.load().packages.map(\.name) == ["Batch"])
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `swift test --package-path SDMKit --filter DownloadEngineTests`
Expected: FAIL — `cannot find 'DownloadEngine' in scope`.

- [ ] **Step 3: Implement the snapshot types**

`SDMKit/Sources/SDMEngine/EngineSnapshot.swift`:

```swift
import Foundation
import SDMCore

/// An immutable view of engine state, published to the UI. Views consume these
/// rather than touching engine actors. See spec §5.4 and §9.6.
public struct ItemSnapshot: Sendable, Equatable, Identifiable {
    public let id: UUID
    public let filename: String
    public let totalBytes: Int64?
    public let completed: RangeSet
    public let state: ItemState
    public let isEnabled: Bool
    public let isResumable: Bool
    public let activeSegments: Int
    public let configuredSegments: Int
    public let bytesPerSecond: Double
    public let speedHistory: [Double]

    public init(
        id: UUID,
        filename: String,
        totalBytes: Int64?,
        completed: RangeSet,
        state: ItemState,
        isEnabled: Bool,
        isResumable: Bool,
        activeSegments: Int,
        configuredSegments: Int,
        bytesPerSecond: Double,
        speedHistory: [Double]
    ) {
        self.id = id
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

    public var fractionCompleted: Double {
        guard let total = totalBytes, total > 0 else { return 0 }
        return Double(completed.totalBytes) / Double(total)
    }
}

public struct PackageSnapshot: Sendable, Equatable, Identifiable {
    public let id: UUID
    public let name: String
    public let priority: Priority
    public let items: [ItemSnapshot]

    public init(id: UUID, name: String, priority: Priority, items: [ItemSnapshot]) {
        self.id = id
        self.name = name
        self.priority = priority
        self.items = items
    }

    public var bytesPerSecond: Double { items.reduce(0) { $0 + $1.bytesPerSecond } }
    public var completedCount: Int { items.filter { $0.state == .completed }.count }
    public var totalBytes: Int64 { items.reduce(0) { $0 + ($1.totalBytes ?? 0) } }
}

public struct EngineSnapshot: Sendable, Equatable {
    public let packages: [PackageSnapshot]
    public let globalBytesPerSecond: Double
    public let globalHistory: [Double]

    /// Public so the app target can construct an empty starting value.
    public init(
        packages: [PackageSnapshot],
        globalBytesPerSecond: Double,
        globalHistory: [Double]
    ) {
        self.packages = packages
        self.globalBytesPerSecond = globalBytesPerSecond
        self.globalHistory = globalHistory
    }
}
```

- [ ] **Step 4: Implement `DownloadEngine`**

`SDMKit/Sources/SDMEngine/DownloadEngine.swift`:

```swift
import Foundation
import SDMCore

public struct EngineSettings: Sendable {
    public var maxConcurrent: Int
    public var segmentsPerItem: Int
    public var globalMaxConnections: Int
    public var downloadFolder: URL

    public init(
        maxConcurrent: Int,
        segmentsPerItem: Int,
        globalMaxConnections: Int,
        downloadFolder: URL
    ) {
        precondition(maxConcurrent >= 1, "maxConcurrent must be at least 1")
        precondition(segmentsPerItem >= 1, "segmentsPerItem must be at least 1")
        precondition(globalMaxConnections >= 1, "globalMaxConnections must be at least 1")
        self.maxConcurrent = maxConcurrent
        self.segmentsPerItem = segmentsPerItem
        self.globalMaxConnections = globalMaxConnections
        self.downloadFolder = downloadFolder
    }
}

/// Owns the package list, the running `DownloadTask`s, and the scheduler.
///
/// `tick()` is driven by the app once per second rather than by an internal
/// timer, so the engine is fully deterministic under test.
public actor DownloadEngine {
    private let transport: any HTTPTransport
    private let stateStore: any StateStore
    private var settings: EngineSettings

    private var packages: [DownloadPackage] = []
    private var tasks: [UUID: DownloadTask] = [:]
    private var runners: [UUID: Task<Void, Never>] = [:]
    private var samplers: [UUID: SpeedSampler] = [:]
    private var globalSampler = SpeedSampler(historyLength: 300, smoothingFactor: 0.4)
    private var segmentOverrides: [UUID: Int] = [:]

    public init(
        transport: any HTTPTransport,
        stateStore: any StateStore,
        settings: EngineSettings
    ) {
        self.transport = transport
        self.stateStore = stateStore
        self.settings = settings
    }

    public func add(_ package: DownloadPackage) async {
        packages.append(package)
        for item in package.items { samplers[item.id] = SpeedSampler() }
        await persist()
        reconcile()
    }

    public func setEnabled(_ enabled: Bool, for itemID: UUID) async {
        mutateItem(itemID) { $0.isEnabled = enabled }
        await persist()
        reconcile()
    }

    public func setPriority(_ priority: Priority?, for itemID: UUID) async {
        mutateItem(itemID) { $0.priority = priority }
        await persist()
        reconcile()
    }

    public func setSegmentCount(_ count: Int, for itemID: UUID) async {
        precondition(count >= 1, "segment count must be at least 1")
        segmentOverrides[itemID] = count
        if let task = tasks[itemID] { await task.setWorkerCount(count) }
    }

    /// One-second heartbeat: closes the speed window and reschedules.
    public func tick() {
        for id in samplers.keys { samplers[id]?.tick() }
        globalSampler.tick()
        reconcile()
    }

    public func snapshot() async -> EngineSnapshot {
        var packageSnapshots: [PackageSnapshot] = []
        for package in packages {
            var items: [ItemSnapshot] = []
            for item in package.items {
                let task = tasks[item.id]
                let completed = await task?.completedRanges ?? item.completed
                let active = await task?.activeWorkerCount ?? 0
                let sampler = samplers[item.id] ?? SpeedSampler()
                items.append(
                    ItemSnapshot(
                        id: item.id,
                        filename: item.filename,
                        totalBytes: item.totalBytes,
                        completed: completed,
                        state: item.state,
                        isEnabled: item.isEnabled,
                        isResumable: item.isResumable,
                        activeSegments: active,
                        configuredSegments: segmentCount(for: item.id),
                        bytesPerSecond: sampler.bytesPerSecond,
                        speedHistory: sampler.history
                    )
                )
            }
            packageSnapshots.append(
                PackageSnapshot(
                    id: package.id,
                    name: package.name,
                    priority: package.priority,
                    items: items
                )
            )
        }
        return EngineSnapshot(
            packages: packageSnapshots,
            globalBytesPerSecond: packageSnapshots.reduce(0) { $0 + $1.bytesPerSecond },
            globalHistory: globalSampler.history
        )
    }

    public func shutdown() async {
        for runner in runners.values { runner.cancel() }
        runners.removeAll()
        for task in tasks.values { await task.pause() }
        await persist()
        await stateStore.flush()
    }

    /// Test helper: pumps the engine until nothing is left running.
    func runUntilIdle() async throws {
        reconcile()
        while !runners.isEmpty {
            let running = runners.values.map { $0 }
            for runner in running { _ = await runner.value }
            runners = runners.filter { !$0.value.isCancelled && false }
            reconcile()
        }
    }

    private func segmentCount(for itemID: UUID) -> Int {
        segmentOverrides[itemID] ?? settings.segmentsPerItem
    }

    private func mutateItem(_ itemID: UUID, _ transform: (inout DownloadItem) -> Void) {
        for packageIndex in packages.indices {
            for itemIndex in packages[packageIndex].items.indices
            where packages[packageIndex].items[itemIndex].id == itemID {
                transform(&packages[packageIndex].items[itemIndex])
            }
        }
    }

    private func persist() async {
        await stateStore.save(PersistedState(packages: packages))
    }

    /// Applies the scheduler's desired running set: starts what should run,
    /// cancels what should not.
    private func reconcile() {
        let desired = Scheduler.desiredRunningSet(
            SchedulerInput(
                packages: packages,
                runningNow: Set(runners.keys),
                startedRecently: [],
                maxConcurrent: settings.maxConcurrent
            )
        )

        for (itemID, runner) in runners where !desired.contains(itemID) {
            runner.cancel()
            runners[itemID] = nil
            mutateItem(itemID) { $0.state = .queued }
        }

        for itemID in desired where runners[itemID] == nil {
            guard let context = context(for: itemID) else { continue }
            mutateItem(itemID) { $0.state = .running }
            runners[itemID] = Task { [weak self] in
                await self?.run(itemID: itemID, context: context)
            }
        }
    }

    private struct RunContext: Sendable {
        let sourceURL: URL
        let destinationURL: URL
        let segments: Int
    }

    private func context(for itemID: UUID) -> RunContext? {
        for package in packages {
            guard let item = package.items.first(where: { $0.id == itemID }) else { continue }
            let folder = settings.downloadFolder.appendingPathComponent(package.name)
            try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            return RunContext(
                sourceURL: item.url,
                destinationURL: folder.appendingPathComponent(item.filename),
                segments: segmentCount(for: itemID)
            )
        }
        return nil
    }

    private func run(itemID: UUID, context: RunContext) async {
        let task = DownloadTask(
            id: itemID,
            sourceURL: context.sourceURL,
            destinationURL: context.destinationURL,
            transport: transport,
            configuration: DownloadTask.Configuration(
                workerCount: context.segments,
                minChunk: 64 * 1024,
                checkpointInterval: 8 * 1024 * 1024
            )
        )
        tasks[itemID] = task

        do {
            _ = try await task.start()
            let completed = await task.completedRanges
            mutateItem(itemID) {
                $0.completed = completed
                $0.state = .completed
            }
            recordBytes(completed.totalBytes, for: itemID)
        } catch {
            let kind = RetryPolicy().classify(error)
            mutateItem(itemID) {
                if case .permanent(let reason) = kind {
                    $0.state = .failed(reason: reason)
                } else {
                    $0.state = .queued
                }
            }
        }

        runners[itemID] = nil
        await persist()
    }

    private func recordBytes(_ bytes: Int64, for itemID: UUID) {
        samplers[itemID]?.record(bytes: bytes)
        globalSampler.record(bytes: bytes)
    }
}
```

- [ ] **Step 5: Fix `runUntilIdle` if the tests hang**

The draft `runUntilIdle` above is deliberately simple. If any engine test hangs, replace its body with:

```swift
    func runUntilIdle() async throws {
        reconcile()
        for _ in 0..<1000 {
            guard let (_, runner) = runners.first else { return }
            _ = await runner.value
            reconcile()
        }
        throw DownloadError.incompleteAfterWorkersFinished
    }
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite.

- [ ] **Step 7: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add SDMKit
git commit -m "feat: add DownloadEngine orchestrating tasks, scheduler and telemetry"
```

---

### Task 17: Xcode wiring and the driver UI

**Files:**
- Delete: `SDM/Item.swift`
- Modify: `SDM/SDMApp.swift`
- Replace: `SDM/ContentView.swift`
- Create: `SDM/EngineController.swift`
- Modify: `SDMTests/SDMTests.swift`

**Interfaces:**
- Consumes: `DownloadEngine`, `EngineSnapshot`, `URLSessionTransport`, `JSONStateStore`
- Produces: a running app window listing packages and items with live progress.

This is the only task requiring Xcode's GUI — adding a local package to an app target means editing `project.pbxproj`, which is not safely scriptable.

- [ ] **Step 1: Add the local package to the app target (manual)**

1. Open `SDM.xcodeproj` in Xcode.
2. Select the **SDM** project in the navigator, then the **SDM** target.
3. Go to **General → Frameworks, Libraries, and Embedded Content**.
4. Click **+**, then **Add Other… → Add Package Dependency… → Add Local…**
5. Select the `SDMKit` folder in the repo root, click **Add Package**.
6. In the products dialog, add **SDMCore** and **SDMEngine** to the **SDM** target.
7. Build (`⌘B`) to confirm it links.

- [ ] **Step 2: Delete the SwiftData boilerplate**

Delete `SDM/Item.swift` (move to trash in Xcode so the target membership is removed too).

```bash
git rm SDM/Item.swift
```

- [ ] **Step 3: Replace `SDM/SDMApp.swift`**

```swift
import SwiftUI

@main
struct SDMApp: App {
    @State private var controller = EngineController()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(controller)
                .task { await controller.startHeartbeat() }
        }
    }
}
```

- [ ] **Step 4: Create `SDM/EngineController.swift`**

```swift
import Foundation
import Observation
import SDMCore
import SDMEngine

/// Bridges the engine actor to SwiftUI: drives the 1 Hz tick and republishes
/// snapshots on the main actor.
@MainActor
@Observable
final class EngineController {
    private(set) var snapshot = EngineSnapshot(
        packages: [],
        globalBytesPerSecond: 0,
        globalHistory: []
    )

    private let engine: DownloadEngine

    init() {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("SDM", isDirectory: true)
        let downloads = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask)[0]

        engine = DownloadEngine(
            transport: URLSessionTransport(),
            stateStore: JSONStateStore(fileURL: support.appendingPathComponent("state.json")),
            settings: EngineSettings(
                maxConcurrent: 3,
                segmentsPerItem: 8,
                globalMaxConnections: 32,
                downloadFolder: downloads
            )
        )
    }

    /// Runs the engine's 1 Hz heartbeat and refreshes the published snapshot.
    func startHeartbeat() async {
        while !Task.isCancelled {
            await engine.tick()
            snapshot = await engine.snapshot()
            try? await Task.sleep(for: .seconds(1))
        }
    }

    func addDownload(urlString: String) async {
        guard let url = URL(string: urlString), url.scheme?.hasPrefix("http") == true else {
            return
        }
        let filename = url.lastPathComponent.isEmpty ? "download.bin" : url.lastPathComponent
        let item = DownloadItem(url: url, filename: filename)
        await engine.add(DownloadPackage(name: "Manual", items: [item]))
        snapshot = await engine.snapshot()
    }

    func setEnabled(_ enabled: Bool, for itemID: UUID) async {
        await engine.setEnabled(enabled, for: itemID)
        snapshot = await engine.snapshot()
    }
}
```

- [ ] **Step 5: Replace `SDM/ContentView.swift`**

```swift
import SDMCore
import SDMEngine
import SwiftUI

struct ContentView: View {
    @Environment(EngineController.self) private var controller
    @State private var urlText = ""

    var body: some View {
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
                            ItemRow(item: item)
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
        .frame(minWidth: 640, minHeight: 420)
    }
}

private struct ItemRow: View {
    let item: ItemSnapshot

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(item.filename).lineLimit(1)
                Spacer()
                Text("\(item.activeSegments)/\(item.configuredSegments) seg")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
                Text(formatted(item.bytesPerSecond))
                    .font(.caption.monospacedDigit())
            }
            SegmentedProgressBar(completed: item.completed, total: item.totalBytes ?? 0)
                .frame(height: 6)
        }
        .padding(.vertical, 2)
    }
}

/// Renders the completed `RangeSet` directly, rasterized to the bar's pixel
/// width so it stays correct at any segment count. See spec §9.4.
struct SegmentedProgressBar: View {
    let completed: RangeSet
    let total: Int64

    var body: some View {
        Canvas { context, size in
            let background = Path(
                roundedRect: CGRect(origin: .zero, size: size),
                cornerRadius: size.height / 2
            )
            context.fill(background, with: .color(.secondary.opacity(0.25)))

            guard total > 0 else { return }
            for range in completed.ranges {
                let x = size.width * CGFloat(range.start) / CGFloat(total)
                let width = size.width * CGFloat(range.length) / CGFloat(total)
                context.fill(
                    Path(CGRect(x: x, y: 0, width: max(width, 0.5), height: size.height)),
                    with: .color(.accentColor)
                )
            }
        }
    }
}

private func formatted(_ bytesPerSecond: Double) -> String {
    let formatter = ByteCountFormatter()
    formatter.countStyle = .binary
    return formatter.string(fromByteCount: Int64(bytesPerSecond)) + "/s"
}
```

- [ ] **Step 6: Replace the boilerplate app test**

`SDMTests/SDMTests.swift`:

```swift
import Testing

@testable import SDM

@Test func appTargetLinksAgainstSDMKit() {
    _ = EngineController.self
    #expect(Bool(true))
}
```

- [ ] **Step 7: Build and run the app**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
Expected: BUILD SUCCEEDED.

Then run the app from Xcode (`⌘R`), paste a direct file URL into the field, click **Add**, and confirm: the row appears, the segmented bar fills, the segment counter shows activity, the speed figure updates, and the finished file lands in `~/Downloads/Manual/` with no `.incomplete` or `.sdmpart` left behind.

- [ ] **Step 8: Run the full test suite**

Run: `swift test --package-path SDMKit`
Expected: PASS, entire suite.

- [ ] **Step 9: Commit**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
git add -A
git commit -m "feat: wire SDMKit into the app with a driver UI"
```

---

## Phase 1 completion criteria

- [ ] `swift test --package-path SDMKit` passes with no skipped tests
- [ ] No test touches the network or sleeps on a real clock
- [ ] `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build` succeeds
- [ ] A real file downloads end-to-end through the app UI into `~/Downloads/Manual/`
- [ ] Quitting mid-download and relaunching resumes rather than restarting
- [ ] `SDM/Item.swift` and all SwiftData references are gone

## Deferred to later phases

Deliberately **not** in Phase 1, to keep it shippable:

- Per-host connection caps and the global connection ceiling actually throttling worker pools (§6.4) — `globalMaxConnections` is carried in settings but not yet enforced. **Phase 3.**
- Hysteresis wiring — `Scheduler` supports `startedRecently` and is tested, but `DownloadEngine` passes an empty set. Needs a clock. **Phase 3.**
- Full retry design — the engine now wires the *guard* from `RetryPolicy`: a per-item attempt counter, a tick-counted hold that keeps an item out of the desired running set until the computed backoff has elapsed, and a terminal `.failed(reason:)` once `maxAttempts` consecutive attempts have failed. (This paragraph previously claimed the engine did not re-attempt transient failures at all. It did — it returned them to `queued` and `tick()` re-desired them one second later, forever, which is a request storm, not an absence of retry.) Still deferred: a manual retry action in the UI, per-error-class policies, and surfacing remaining attempts. **Phase 3.**
- Signed-URL refresh on 403 (§5.3) — `RetryPolicy` already classifies 403 as transient, which is the hook, but there is no resolver to refresh from yet. **Phase 5.**
- Drag-and-drop reordering, sparklines, bandwidth graph, menu bar, notifications. **Phase 3.**
- Linkgrabber, clipboard watching. **Phase 2.**
- Theming, activation policy. **Phase 4.**
- yt-dlp resolver. **Phase 5.**
