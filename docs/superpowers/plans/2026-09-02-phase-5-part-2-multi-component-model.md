# Phase 5 Part 2 — Multi-Component Model & Migration: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rework `DownloadItem` so one item owns `[FileComponent]` (≥1), with the old top-level `url`/`filename`/`totalBytes`/`completed`/`validator`/`isResumable` becoming concatenated computed accessors — **behavior-preserving for the one-component case** — plus a one-shot v1→v2 durable-state migration and `DownloadPackage.note`.

**Architecture:** `FileComponent` is a new `SDMCore` value type carrying what used to be per-item download state. `DownloadItem` stores `components`, `outputFilename`, and `assembly`; a convenience initializer with the old signature wraps a single URL into a one-element `components` array so every existing call site keeps compiling. The old properties stay as `var` computed forwarders (get = concatenation/aggregation, set = write-through to the sole component, `precondition`-guarded to one component). `DownloadItem` gets a custom `Codable` decoder that accepts both the legacy flat shape and the new nested one, and `PersistedState` accepts `formatVersion <= 2`. The engine, snapshot, scheduler, and app are **not modified** — they read and write `DownloadItem` through the same property names as before. Parallel component downloads, muxing, URL refresh, and sidecar v2 are Part 3; grabber and UI are Part 4.

**Tech Stack:** Swift 6 language mode (strict concurrency), Swift Testing (`@Test`/`#expect`), local SPM package `SDMKit`.

**Spec:** `docs/superpowers/specs/2026-09-02-phase-5-youtube-resolver-design.md` — Part 2 implements §5 (Core model: multi-component items) and §5.4 (`DownloadPackage.note`), plus the §7.4 / §11 v1→v2 migration shim as it applies to `PersistedState` (the `ResumeSidecar` half is Part 3).

## Global Constraints

- **Swift 6 language mode, strict concurrency.** Every type crossing an `async` boundary is `Sendable`. Parent spec §2.
- **macOS 15.0 baseline.**
- **Behavior-preserving.** The full pre-existing test suite (**348 tests**) MUST stay green after every task. A one-component `DownloadItem` must behave byte-for-byte as it did before this plan.
- **No test may touch the network or sleep on a real clock.** Parent spec §11.1.
- **Format Swift with `./format.sh` before every commit; lint clean with `./lint.sh`.**
- **Tests:** `cd SDMKit && swift test`; single test `cd SDMKit && swift test --filter '<Type>/<name>'` — always confirm the reported count is non-zero (project `swift-testing-filter-gotcha`).
- Invariant: `DownloadItem.components.count >= 1`, enforced in every initializer with `precondition`.
- `DownloadItem` Codable stays round-trippable: encoding then decoding yields an equal value, for both a fresh v2 item and a migrated v1 item.

---

## File Structure

**`SDMKit/Sources/SDMCore/` (changes):**

| File | Change |
|---|---|
| `FileComponent.swift` *(new)* | `FileComponent`, `ComponentOrigin`, `Assembly` value types |
| `DownloadItem.swift` | Rework: stored `components`/`outputFilename`/`assembly`; old props → computed forwarders; convenience init; custom `Codable` |
| `DownloadPackage.swift` | Add `note: String?` |

**`SDMKit/Sources/SDMEngine/StateStore.swift`:** `PersistedState.currentFormatVersion = 2`; `load()` accepts `formatVersion <= 2`.

**`SDMKit/Tests/` (new/changed):**

| File | Covers |
|---|---|
| `SDMCoreTests/FileComponentTests.swift` *(new)* | component value types, `Assembly` |
| `SDMCoreTests/DownloadItemTests.swift` *(new)* | convenience init, concatenated accessors, write-through, Codable round-trip, v1 decode |
| `SDMCoreTests/DomainModelTests.swift` | add `DownloadPackage.note` assertions if the file already tests `DownloadPackage` (append, don't restructure) |
| `SDMEngineTests/DurableStateTests.swift` | add: a hand-written v1 `PersistedState` JSON loads and migrates |

No engine or app source file changes. If the compiler reports a break in `SDMEngine`/`SDM` after Task 2, that is a signal the computed-forwarder surface is incomplete — fix the forwarder, not the call site (see Task 2 step 6).

---

## Task 1: `FileComponent`, `ComponentOrigin`, `Assembly`

**Files:**
- Create: `SDMKit/Sources/SDMCore/FileComponent.swift`
- Test: `SDMKit/Tests/SDMCoreTests/FileComponentTests.swift`

**Interfaces:**
- Consumes: `RangeSet` (existing).
- Produces:
  - `enum ComponentOrigin: Equatable, Codable, Sendable, Hashable` — `case http`; `case resolved(extractor: String, videoID: String, formatID: String)`
  - `enum Assembly: String, Equatable, Codable, Sendable` — `case none`, `case mux`
  - `struct ComponentError: Equatable, Codable, Sendable` — `var message: String`; `init(_ message: String)`
  - `struct FileComponent: Identifiable, Equatable, Codable, Sendable`:
    - `let id: UUID`
    - `var url: URL`
    - `var partFilename: String`
    - `var totalBytes: Int64?`
    - `var completed: RangeSet`
    - `var validator: String?`
    - `var origin: ComponentOrigin`
    - `var isResumable: Bool?`
    - `var lastError: ComponentError?`
    - `init(id: UUID = UUID(), url: URL, partFilename: String, totalBytes: Int64? = nil, completed: RangeSet = RangeSet(), validator: String? = nil, origin: ComponentOrigin = .http, isResumable: Bool? = nil, lastError: ComponentError? = nil)` — `precondition(!partFilename.isEmpty)`
    - `var isComplete: Bool` — `totalBytes` present and `completed.isComplete(total:)`
    - `var fractionCompleted: Double` — `0` when size unknown/zero, else `completed.totalBytes / totalBytes`

- [ ] **Step 1: Write the failing test**

```swift
// SDMKit/Tests/SDMCoreTests/FileComponentTests.swift
import Foundation
import Testing

@testable import SDMCore

private func url(_ s: String) -> URL { URL(string: s)! }

@Test func componentReportsCompletionAgainstItsOwnSize() {
    var c = FileComponent(url: url("https://x/v"), partFilename: "v.f137.mp4", totalBytes: 100)
    #expect(c.isComplete == false)
    #expect(c.fractionCompleted == 0)
    c.completed.insert(ByteRange(start: 0, end: 100))
    #expect(c.isComplete)
    #expect(c.fractionCompleted == 1.0)
}

@Test func componentWithUnknownSizeIsNeverComplete() {
    var c = FileComponent(url: url("https://x/v"), partFilename: "v.mp4")
    c.completed.insert(ByteRange(start: 0, end: 500))
    #expect(c.isComplete == false)
    #expect(c.fractionCompleted == 0)
}

@Test func componentOriginRoundTripsThroughCodable() throws {
    let origin = ComponentOrigin.resolved(extractor: "youtube", videoID: "abc", formatID: "137")
    let data = try JSONEncoder().encode(origin)
    #expect(try JSONDecoder().decode(ComponentOrigin.self, from: data) == origin)
}

@Test func fileComponentRoundTripsThroughCodable() throws {
    var c = FileComponent(
        url: url("https://x/v"), partFilename: "v.f251.webm", totalBytes: 4096,
        origin: .resolved(extractor: "youtube", videoID: "abc", formatID: "251"))
    c.completed.insert(ByteRange(start: 0, end: 1024))
    c.lastError = ComponentError("connection reset")
    let data = try JSONEncoder().encode(c)
    #expect(try JSONDecoder().decode(FileComponent.self, from: data) == c)
}

@Test func assemblyIsCodableAsAString() throws {
    #expect(try JSONDecoder().decode(Assembly.self, from: Data("\"mux\"".utf8)) == .mux)
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd SDMKit && swift test --filter 'FileComponentTests'`
Expected: FAIL — `cannot find 'FileComponent' in scope`.

- [ ] **Step 3: Write the implementation**

```swift
// SDMKit/Sources/SDMCore/FileComponent.swift
import Foundation

/// Where a component's bytes come from, and what is needed to refresh its
/// URL if it expires mid-download. Parent spec §5.1.
public enum ComponentOrigin: Equatable, Codable, Sendable, Hashable {
    case http
    case resolved(extractor: String, videoID: String, formatID: String)
}

/// What has to happen once every component of an item is fully downloaded.
public enum Assembly: String, Equatable, Codable, Sendable {
    /// One component whose container is already final — just rename it.
    case none
    /// Combine the components with `ffmpeg -c copy`.
    case mux
}

public struct ComponentError: Equatable, Codable, Sendable {
    public var message: String
    public init(_ message: String) { self.message = message }
}

/// One separately-downloaded file backing a `DownloadItem`. A generic HTTP
/// download has exactly one; a muxed YouTube download has two (video +
/// audio). Parent spec §5.1.
public struct FileComponent: Identifiable, Equatable, Codable, Sendable {
    public let id: UUID
    public var url: URL
    public var partFilename: String
    public var totalBytes: Int64?
    public var completed: RangeSet
    public var validator: String?
    public var origin: ComponentOrigin
    public var isResumable: Bool?
    public var lastError: ComponentError?

    public init(
        id: UUID = UUID(),
        url: URL,
        partFilename: String,
        totalBytes: Int64? = nil,
        completed: RangeSet = RangeSet(),
        validator: String? = nil,
        origin: ComponentOrigin = .http,
        isResumable: Bool? = nil,
        lastError: ComponentError? = nil
    ) {
        precondition(!partFilename.isEmpty, "partFilename must not be empty")
        self.id = id
        self.url = url
        self.partFilename = partFilename
        self.totalBytes = totalBytes
        self.completed = completed
        self.validator = validator
        self.origin = origin
        self.isResumable = isResumable
        self.lastError = lastError
    }

    public var isComplete: Bool {
        guard let totalBytes else { return false }
        return completed.isComplete(total: totalBytes)
    }

    public var fractionCompleted: Double {
        guard let totalBytes, totalBytes > 0 else { return 0 }
        return Double(completed.totalBytes) / Double(totalBytes)
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd SDMKit && swift test --filter 'FileComponentTests'`
Expected: PASS, 5 tests.

- [ ] **Step 5: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDMKit/Sources/SDMCore/FileComponent.swift SDMKit/Tests/SDMCoreTests/FileComponentTests.swift
git commit -m "feat(core): FileComponent, ComponentOrigin, Assembly value types"
```

---

## Task 2: Rework `DownloadItem` around `components`

**Files:**
- Modify: `SDMKit/Sources/SDMCore/DownloadItem.swift` (full rewrite)
- Test: `SDMKit/Tests/SDMCoreTests/DownloadItemTests.swift` *(new)*

**Interfaces:**
- Consumes: `FileComponent`, `ComponentOrigin`, `Assembly` (Task 1); `RangeSet`, `ByteRange`, `ItemState`, `Priority` (existing).
- Produces (`DownloadItem`):
  - Stored: `let id: UUID`, `var components: [FileComponent]`, `var outputFilename: String`, `var assembly: Assembly`, `var state: ItemState`, `var isEnabled: Bool`, `var priority: Priority?`, `var position: Int`
  - **Designated init:** `init(id: UUID = UUID(), components: [FileComponent], outputFilename: String, assembly: Assembly = .none, state: ItemState = .queued, isEnabled: Bool = true, priority: Priority? = nil, position: Int = 0)` — `precondition(!components.isEmpty)`, `precondition(!outputFilename.isEmpty)`
  - **Convenience init (unchanged old signature):** `init(id: UUID = UUID(), url: URL, filename: String, totalBytes: Int64? = nil, completed: RangeSet = RangeSet(), state: ItemState = .queued, isEnabled: Bool = true, isResumable: Bool? = nil, priority: Priority? = nil, position: Int = 0, validator: String? = nil)` — builds `components: [FileComponent(url: url, partFilename: filename, totalBytes: totalBytes, completed: completed, validator: validator, origin: .http, isResumable: isResumable)]`, `outputFilename: filename`, `assembly: .none`
  - Computed forwarders (get **and** set unless noted):
    - `var url: URL { get { components[0].url } }` — get only
    - `var filename: String { get { outputFilename } }` — get only
    - `var validator: String? { get { components[0].validator } set { for i in components.indices { components[i].validator = newValue } } }`
    - `var isResumable: Bool? { get; set }` — get: `false` if any component `== false`; else `nil` if any is `nil`; else `true`. set: assign to every component.
    - `var totalBytes: Int64? { get; set }` — get: `nil` if any component size is `nil`, else the sum. set: `precondition(components.count == 1)`, `components[0].totalBytes = newValue`
    - `var completed: RangeSet { get; set }` — get: concatenated (component *k*'s ranges shifted by `Σ_{i<k} size_i`; if any size unknown, only component 0 contributes, unshifted). set: `precondition(components.count == 1)`, `components[0].completed = newValue`
  - Computed read-only:
    - `var fractionCompleted: Double` — `completedBytes / totalBytes` (0 when `totalBytes` nil/0), where `completedBytes` = `components.reduce(0) { $0 + $1.completed.totalBytes }`
    - `var isComplete: Bool` — every component `isComplete`
    - `var componentBaseOffsets: [Int64]` — running prefix sums of component sizes; `[0]` when any size unknown (used by the snapshot later; defined here so the concat logic has one home)
  - Custom `Codable`: `init(from:)` decodes the new shape when a `components` key is present, else the legacy flat shape (`url`, `filename`, `totalBytes`, `completed`, `state`, `isEnabled`, `isResumable`, `priority`, `position`, `validator`) via the convenience init. `encode(to:)` always writes the new shape.

**Concatenation rule (single source of truth):** if **all** component `totalBytes` are non-nil, `completed` get = union of `component[k].completed` each `+ base_k`; otherwise `completed` get = `components[0].completed` verbatim. For `components.count == 1` this is exactly `components[0].completed`, so a one-component item is unchanged.

- [ ] **Step 1: Write the failing test**

```swift
// SDMKit/Tests/SDMCoreTests/DownloadItemTests.swift
import Foundation
import Testing

@testable import SDMCore

private func u(_ s: String) -> URL { URL(string: s)! }

@Test func convenienceInitBuildsAOneComponentHttpItem() {
    let item = DownloadItem(url: u("https://x/f.bin"), filename: "f.bin", totalBytes: 1000)
    #expect(item.components.count == 1)
    #expect(item.components[0].origin == .http)
    #expect(item.components[0].partFilename == "f.bin")
    #expect(item.outputFilename == "f.bin")
    #expect(item.assembly == .none)
    #expect(item.url == u("https://x/f.bin"))
    #expect(item.filename == "f.bin")
    #expect(item.totalBytes == 1000)
}

@Test func oneComponentAccessorsAreByteForByteUnchanged() {
    var item = DownloadItem(url: u("https://x/f.bin"), filename: "f.bin", totalBytes: 1000)
    item.completed = RangeSet([ByteRange(start: 0, end: 400)])
    #expect(item.completed.totalBytes == 400)
    #expect(item.fractionCompleted == 0.4)
    #expect(item.isComplete == false)
    item.isResumable = true
    #expect(item.isResumable == true)
    #expect(item.components[0].isResumable == true)
    item.totalBytes = 400
    #expect(item.isComplete)
}

@Test func concatenatedCompletedShiftsLaterComponentsByPriorSizes() {
    let video = FileComponent(
        url: u("https://gv/v"), partFilename: "t.f137.mp4", totalBytes: 100,
        completed: RangeSet([ByteRange(start: 0, end: 60)]), origin: .http)
    let audio = FileComponent(
        url: u("https://gv/a"), partFilename: "t.f251.webm", totalBytes: 20,
        completed: RangeSet([ByteRange(start: 0, end: 10)]), origin: .http)
    let item = DownloadItem(components: [video, audio], outputFilename: "t.mp4", assembly: .mux)
    #expect(item.totalBytes == 120)
    // item-space: [0,60) from video + [100,110) from audio
    #expect(item.completed.ranges == [ByteRange(start: 0, end: 60), ByteRange(start: 100, end: 110)])
    #expect(item.fractionCompleted == 70.0 / 120.0)
    #expect(item.isComplete == false)
}

@Test func isResumableIsFalseIfAnyComponentIsFalse() {
    var a = FileComponent(url: u("https://x/a"), partFilename: "a", isResumable: true)
    let b = FileComponent(url: u("https://x/b"), partFilename: "b", isResumable: false)
    var item = DownloadItem(components: [a, b], outputFilename: "out.mp4", assembly: .mux)
    #expect(item.isResumable == false)
    a.isResumable = nil
    item = DownloadItem(components: [a, b], outputFilename: "out.mp4", assembly: .mux)
    #expect(item.isResumable == false)
    let c = FileComponent(url: u("https://x/c"), partFilename: "c", isResumable: nil)
    let d = FileComponent(url: u("https://x/d"), partFilename: "d", isResumable: true)
    item = DownloadItem(components: [c, d], outputFilename: "out.mp4", assembly: .mux)
    #expect(item.isResumable == nil)
}

@Test func newShapeCodableRoundTrips() throws {
    let item = DownloadItem(
        components: [
            FileComponent(
                url: u("https://gv/v"), partFilename: "t.f137.mp4", totalBytes: 100,
                origin: .resolved(extractor: "youtube", videoID: "abc", formatID: "137")),
            FileComponent(
                url: u("https://gv/a"), partFilename: "t.f251.webm", totalBytes: 20,
                origin: .resolved(extractor: "youtube", videoID: "abc", formatID: "251")),
        ], outputFilename: "t.mp4", assembly: .mux, state: .stopped)
    let data = try JSONEncoder().encode(item)
    #expect(try JSONDecoder().decode(DownloadItem.self, from: data) == item)
}

@Test func legacyFlatCodableDecodesToOneComponentItem() throws {
    let legacy = """
        {
          "id": "1B4E28BA-2FA1-11D2-883F-0016D3CCA427",
          "url": "https://x/f.bin",
          "filename": "f.bin",
          "totalBytes": 2048,
          "completed": { "ranges": [ { "start": 0, "end": 1024 } ] },
          "state": "queued",
          "isEnabled": true,
          "isResumable": true,
          "position": 3
        }
        """
    let item = try JSONDecoder().decode(DownloadItem.self, from: Data(legacy.utf8))
    #expect(item.components.count == 1)
    #expect(item.components[0].origin == .http)
    #expect(item.url == u("https://x/f.bin"))
    #expect(item.outputFilename == "f.bin")
    #expect(item.totalBytes == 2048)
    #expect(item.completed.totalBytes == 1024)
    #expect(item.isResumable == true)
    #expect(item.position == 3)
}
```

> Verify the `RangeSet` Codable shape in the legacy fixture against
> `SDMKit/Sources/SDMCore/RangeSet.swift` before running — it encodes its
> `ranges` array; if the key or `ByteRange` field names differ, match them.

- [ ] **Step 2: Run test to verify it fails**

Run: `cd SDMKit && swift test --filter 'DownloadItemTests'`
Expected: FAIL — build error (new initializer/properties absent).

- [ ] **Step 3: Rewrite `DownloadItem.swift`**

```swift
// SDMKit/Sources/SDMCore/DownloadItem.swift
import Foundation

public struct DownloadItem: Identifiable, Equatable, Sendable {
    public let id: UUID
    /// Invariant: at least one. A generic HTTP download has exactly one;
    /// a muxed YouTube download has two (video + audio). Parent spec §5.
    public var components: [FileComponent]
    /// The final file once assembly (if any) completes.
    public var outputFilename: String
    public var assembly: Assembly
    public var state: ItemState
    /// Purely user-managed: "never start this, no matter what." Independent
    /// of `state` — see `ItemState`. Only Disable/Enable changes it.
    public var isEnabled: Bool
    public var priority: Priority?
    /// Position within the owning package. Lower sorts earlier.
    public var position: Int

    public init(
        id: UUID = UUID(),
        components: [FileComponent],
        outputFilename: String,
        assembly: Assembly = .none,
        state: ItemState = .queued,
        isEnabled: Bool = true,
        priority: Priority? = nil,
        position: Int = 0
    ) {
        precondition(!components.isEmpty, "a DownloadItem needs at least one component")
        precondition(!outputFilename.isEmpty, "outputFilename must not be empty")
        self.id = id
        self.components = components
        self.outputFilename = outputFilename
        self.assembly = assembly
        self.state = state
        self.isEnabled = isEnabled
        self.priority = priority
        self.position = position
    }

    /// The pre-Part-5 signature. Wraps a single URL into a one-component
    /// HTTP item so every existing call site keeps compiling unchanged.
    public init(
        id: UUID = UUID(),
        url: URL,
        filename: String,
        totalBytes: Int64? = nil,
        completed: RangeSet = RangeSet(),
        state: ItemState = .queued,
        isEnabled: Bool = true,
        isResumable: Bool? = nil,
        priority: Priority? = nil,
        position: Int = 0,
        validator: String? = nil
    ) {
        self.init(
            id: id,
            components: [
                FileComponent(
                    url: url, partFilename: filename, totalBytes: totalBytes,
                    completed: completed, validator: validator, origin: .http,
                    isResumable: isResumable)
            ],
            outputFilename: filename,
            assembly: .none,
            state: state,
            isEnabled: isEnabled,
            priority: priority,
            position: position)
    }

    // MARK: - Concatenated accessors (one-component: identical to before)

    public var url: URL { components[0].url }
    public var filename: String { outputFilename }

    public var validator: String? {
        get { components[0].validator }
        set { for index in components.indices { components[index].validator = newValue } }
    }

    public var isResumable: Bool? {
        get {
            if components.contains(where: { $0.isResumable == false }) { return false }
            if components.contains(where: { $0.isResumable == nil }) { return nil }
            return true
        }
        set { for index in components.indices { components[index].isResumable = newValue } }
    }

    public var totalBytes: Int64? {
        get {
            guard components.allSatisfy({ $0.totalBytes != nil }) else { return nil }
            return components.reduce(0) { $0 + ($1.totalBytes ?? 0) }
        }
        set {
            precondition(components.count == 1, "set totalBytes only on a one-component item")
            components[0].totalBytes = newValue
        }
    }

    /// Item-space completed ranges: each component's ranges shifted by the
    /// sum of the sizes before it. Falls back to component 0 alone while any
    /// size is still unknown.
    public var completed: RangeSet {
        get {
            let bases = componentBaseOffsets
            guard bases.count == components.count else { return components[0].completed }
            var union = RangeSet()
            for (index, component) in components.enumerated() {
                for range in component.completed.ranges {
                    union.insert(
                        ByteRange(start: range.start + bases[index], end: range.end + bases[index]))
                }
            }
            return union
        }
        set {
            precondition(components.count == 1, "set completed only on a one-component item")
            components[0].completed = newValue
        }
    }

    /// Running prefix sums of component sizes, or `[]` when any size is
    /// unknown (so callers fall back rather than misplace ranges).
    public var componentBaseOffsets: [Int64] {
        guard components.allSatisfy({ $0.totalBytes != nil }) else { return [] }
        var bases: [Int64] = []
        var running: Int64 = 0
        for component in components {
            bases.append(running)
            running += component.totalBytes ?? 0
        }
        return bases
    }

    public var fractionCompleted: Double {
        guard let total = totalBytes, total > 0 else { return 0 }
        let done = components.reduce(Int64(0)) { $0 + $1.completed.totalBytes }
        return Double(done) / Double(total)
    }

    public var isComplete: Bool {
        components.allSatisfy(\.isComplete)
    }
}

// MARK: - Codable (accepts the legacy flat shape and the new nested one)

extension DownloadItem: Codable {
    private enum CodingKeys: String, CodingKey {
        case id, components, outputFilename, assembly, state, isEnabled, priority, position
        // legacy-only keys
        case url, filename, totalBytes, completed, isResumable, validator
    }

    public init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let id = try container.decode(UUID.self, forKey: .id)
        let state = try container.decodeIfPresent(ItemState.self, forKey: .state) ?? .queued
        let isEnabled = try container.decodeIfPresent(Bool.self, forKey: .isEnabled) ?? true
        let priority = try container.decodeIfPresent(Priority.self, forKey: .priority)
        let position = try container.decodeIfPresent(Int.self, forKey: .position) ?? 0

        if container.contains(.components) {
            let components = try container.decode([FileComponent].self, forKey: .components)
            let outputFilename = try container.decode(String.self, forKey: .outputFilename)
            let assembly = try container.decodeIfPresent(Assembly.self, forKey: .assembly) ?? .none
            self.init(
                id: id, components: components, outputFilename: outputFilename,
                assembly: assembly, state: state, isEnabled: isEnabled,
                priority: priority, position: position)
        } else {
            let url = try container.decode(URL.self, forKey: .url)
            let filename = try container.decode(String.self, forKey: .filename)
            let totalBytes = try container.decodeIfPresent(Int64.self, forKey: .totalBytes)
            let completed =
                try container.decodeIfPresent(RangeSet.self, forKey: .completed) ?? RangeSet()
            let isResumable = try container.decodeIfPresent(Bool.self, forKey: .isResumable)
            let validator = try container.decodeIfPresent(String.self, forKey: .validator)
            self.init(
                id: id, url: url, filename: filename, totalBytes: totalBytes,
                completed: completed, state: state, isEnabled: isEnabled,
                isResumable: isResumable, priority: priority, position: position,
                validator: validator)
        }
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(id, forKey: .id)
        try container.encode(components, forKey: .components)
        try container.encode(outputFilename, forKey: .outputFilename)
        try container.encode(assembly, forKey: .assembly)
        try container.encode(state, forKey: .state)
        try container.encode(isEnabled, forKey: .isEnabled)
        try container.encodeIfPresent(priority, forKey: .priority)
        try container.encode(position, forKey: .position)
    }
}
```

- [ ] **Step 4: Run the new test**

Run: `cd SDMKit && swift test --filter 'DownloadItemTests'`
Expected: PASS, 6 tests. Fix `RangeSet` Codable key mismatches in the legacy fixture if the decode test fails on shape.

- [ ] **Step 5: Build the whole package**

Run: `cd SDMKit && swift build`
Expected: SUCCESS. If `SDMEngine` fails to compile:
- A **read** of `item.url` / `item.filename` / `item.totalBytes` / `item.completed` / `item.isResumable` / `item.validator` should still work — if one doesn't, the computed getter is missing or misnamed; add it.
- A **write** `$0.completed = …` / `$0.totalBytes = …` / `$0.isResumable = …` / `$0.validator = …` inside `mutateItem` should still work via the setters. `$0.completed`/`$0.totalBytes` setters `precondition(components.count == 1)` — that always holds in Part 2 (nothing builds a multi-component item yet).
- Do **not** change engine call sites. If a genuine new access is needed, that belongs in Part 3.

- [ ] **Step 6: Run the full suite**

Run: `cd SDMKit && swift test`
Expected: **all pre-existing tests still pass** (348) plus the new `FileComponentTests` (5) and `DownloadItemTests` (6). Investigate any pre-existing failure before continuing — the rework must be behavior-preserving.

- [ ] **Step 7: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDMKit/Sources/SDMCore/DownloadItem.swift SDMKit/Tests/SDMCoreTests/DownloadItemTests.swift
git commit -m "feat(core): DownloadItem owns [FileComponent]; old accessors become concatenated forwarders"
```

---

## Task 3: `DownloadPackage.note`

**Files:**
- Modify: `SDMKit/Sources/SDMCore/DownloadPackage.swift`
- Test: `SDMKit/Tests/SDMCoreTests/DomainModelTests.swift` (append)

**Interfaces:**
- Produces: `DownloadPackage.note: String?` — optional line shown under the package name (parent spec §5.4: `"50 of 320 videos"` for a truncated playlist). Defaults to `nil`. Added to the memberwise init as a trailing defaulted parameter so existing call sites are unaffected. Codable: `decodeIfPresent`, so old snapshots decode with `note == nil`.

- [ ] **Step 1: Write the failing test** (append to `DomainModelTests.swift`)

```swift
@Test func downloadPackageNoteDefaultsToNilAndRoundTrips() throws {
    let plain = DownloadPackage(name: "P", items: [])
    #expect(plain.note == nil)

    var annotated = DownloadPackage(name: "Playlist", items: [], note: "50 of 320 videos")
    #expect(annotated.note == "50 of 320 videos")
    let data = try JSONEncoder().encode(annotated)
    #expect(try JSONDecoder().decode(DownloadPackage.self, from: data) == annotated)

    // Old snapshot without the key still decodes.
    let legacy = Data(#"{"id":"1B4E28BA-2FA1-11D2-883F-0016D3CCA427","name":"P","items":[],"priority":"normal","position":0}"#.utf8)
    annotated = try JSONDecoder().decode(DownloadPackage.self, from: legacy)
    #expect(annotated.note == nil)
}
```

> Check `Priority`'s Codable form before running — if `Priority` does not
> encode as the string `"normal"`, adjust the legacy fixture to match its
> actual encoding (or drop the `priority` key and rely on a default if the
> decoder provides one).

- [ ] **Step 2: Run test to verify it fails**

Run: `cd SDMKit && swift test --filter 'DomainModelTests/downloadPackageNoteDefaultsToNilAndRoundTrips'`
Expected: FAIL — `extra argument 'note' in call` / no member `note`.

- [ ] **Step 3: Write the implementation**

In `DownloadPackage.swift`, add the stored property after `position`:
```swift
    /// An optional line shown under the package name — e.g. a truncated
    /// playlist's "50 of 320 videos". Parent spec §5.4. `nil` for ordinary
    /// packages.
    public var note: String?
```
Add `note: String? = nil` as the last parameter of `init`, and `self.note = note` in the body.
Add explicit `Codable` handling only if `DownloadPackage` has a custom coder; if it uses the synthesized one, `note` as `String?` already `decodeIfPresent`s — verify by running the legacy-decode assertion.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd SDMKit && swift test --filter 'DomainModelTests/downloadPackageNoteDefaultsToNilAndRoundTrips'`
Expected: PASS, 1 test.

- [ ] **Step 5: Full suite, format, lint, commit**

```bash
cd SDMKit && swift test    # expect the full green suite + 1
cd .. && ./format.sh && ./lint.sh
git add SDMKit/Sources/SDMCore/DownloadPackage.swift SDMKit/Tests/SDMCoreTests/DomainModelTests.swift
git commit -m "feat(core): DownloadPackage.note for truncated-playlist annotations"
```

---

## Task 4: `PersistedState` v1 → v2 migration

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/StateStore.swift`
- Test: `SDMKit/Tests/SDMEngineTests/DurableStateTests.swift` (append)

**Interfaces:**
- Consumes: `DownloadItem`'s dual-shape `Codable` (Task 2).
- Produces:
  - `PersistedState.currentFormatVersion == 2`
  - `JSONStateStore.load()` accepts any file whose `formatVersion` is `1...2`. A v1 file's items decode through `DownloadItem`'s legacy branch automatically; the loaded `PersistedState` is returned with whatever `formatVersion` it had, and the next `save()`/`flush()` writes it back as v2 (because `PersistedState.init` defaults `formatVersion` to `currentFormatVersion` and the engine constructs fresh `PersistedState(packages:)` values — confirm this in `DownloadEngine.persist()`; if `persist()` preserves a loaded `formatVersion`, add `state.formatVersion = PersistedState.currentFormatVersion` in `load()` before returning).

- [ ] **Step 1: Write the failing test** (append to `DurableStateTests.swift`)

```swift
@Test func loadsAndMigratesAV1SnapshotToMultiComponentItems() async throws {
    let dir = try makeScratchDirectory()
    let fileURL = dir.appendingPathComponent("state.json")
    let v1 = """
        {
          "formatVersion": 1,
          "packages": [
            {
              "id": "1B4E28BA-2FA1-11D2-883F-0016D3CCA427",
              "name": "Old Package",
              "priority": "normal",
              "position": 0,
              "items": [
                {
                  "id": "2B4E28BA-2FA1-11D2-883F-0016D3CCA427",
                  "url": "https://example.com/big.iso",
                  "filename": "big.iso",
                  "totalBytes": 5000,
                  "completed": { "ranges": [ { "start": 0, "end": 2000 } ] },
                  "state": "stopped",
                  "isEnabled": true,
                  "isResumable": true,
                  "position": 0
                }
              ]
            }
          ]
        }
        """
    try Data(v1.utf8).write(to: fileURL)

    let store = JSONStateStore(fileURL: fileURL)
    let loaded = await store.load()
    #expect(loaded.packages.count == 1)
    let item = try #require(loaded.packages.first?.items.first)
    #expect(item.components.count == 1)
    #expect(item.components[0].origin == .http)
    #expect(item.url == URL(string: "https://example.com/big.iso")!)
    #expect(item.completed.totalBytes == 2000)

    // Re-saving writes v2.
    await store.save(loaded)
    await store.flush()
    let reread = try JSONDecoder().decode(
        PersistedState.self, from: Data(contentsOf: fileURL))
    #expect(reread.formatVersion == 2)
    #expect(reread.packages.first?.items.first?.components.count == 1)
}
```

> `makeScratchDirectory()` already exists in `SDMEngineTests/TestSupport.swift`.
> Match the `priority` encoding to `Priority`'s real Codable form (see Task 3
> note).

- [ ] **Step 2: Run test to verify it fails**

Run: `cd SDMKit && swift test --filter 'DurableStateTests/loadsAndMigratesAV1SnapshotToMultiComponentItems'`
Expected: FAIL — `load()` rejects `formatVersion == 1` and returns an empty `PersistedState`, so `loaded.packages` is empty.

- [ ] **Step 3: Write the implementation**

In `StateStore.swift`:
```swift
    public static let currentFormatVersion = 2
```
In `JSONStateStore.load()`, replace the version guard:
```swift
        guard let data = try? Data(contentsOf: fileURL),
            var state = try? JSONDecoder().decode(PersistedState.self, from: data),
            (1...PersistedState.currentFormatVersion).contains(state.formatVersion),
            state.isValid
        else { return PersistedState() }
        state.formatVersion = PersistedState.currentFormatVersion
        return state
```
(`state` becomes `var` and its `formatVersion` is normalized before return, so a later `save`/`flush` cannot round-trip a stale `1`.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cd SDMKit && swift test --filter 'DurableStateTests'`
Expected: PASS — the new test plus every existing `DurableStateTests` case.

- [ ] **Step 5: Full suite, format, lint, commit**

```bash
cd SDMKit && swift test    # full green suite + new test
cd .. && ./format.sh && ./lint.sh
git add SDMKit/Sources/SDMEngine/StateStore.swift SDMKit/Tests/SDMEngineTests/DurableStateTests.swift
git commit -m "feat(engine): migrate v1 durable state to the multi-component model on load"
```

---

## Task 5: Round-trip guard for a live engine restart

**Files:**
- Test: `SDMKit/Tests/SDMEngineTests/DurableStateTests.swift` (append) — or `ResumeTests.swift` if that file owns restart coverage; pick whichever already reconstructs an engine from disk.

**Interfaces:**
- Consumes: `DownloadEngine`, `JSONStateStore`, `FakeOrigin` (existing test infra).

This task adds **no production code** — it proves the Part 2 rework did not disturb the persist → restore cycle the engine actually runs, beyond the unit-level Codable tests.

- [ ] **Step 1: Write the test**

```swift
@Test func engineRestoresAMultiComponentModelItemAndFinishesIt() async throws {
    let dir = try makeScratchDirectory()
    let payload = testPayload(4096)
    let origin = FakeOrigin(payload: payload)
    let fileURL = dir.appendingPathComponent("state.json")
    let settings = EngineSettings(
        maxConcurrent: 2, segmentsPerItem: 4, globalMaxConnections: 16,
        downloadFolder: dir)

    // Session 1: add an item, let it run to completion, shut down.
    do {
        let store = JSONStateStore(fileURL: fileURL)
        let engine = DownloadEngine(
            transport: origin, stateStore: store, settings: settings)
        await engine.add(
            DownloadPackage(
                name: "Pkg",
                items: [DownloadItem(url: origin.url, filename: "out.bin", state: .queued)]))
        try await engine.runUntilIdle()
        await engine.shutdown()
    }

    // Session 2: a brand-new engine restores from the v2 file just written.
    let store2 = JSONStateStore(fileURL: fileURL)
    let engine2 = DownloadEngine(transport: origin, stateStore: store2, settings: settings)
    await engine2.restore()
    let snapshot = await engine2.snapshot()
    let item = try #require(snapshot.packages.first?.items.first)
    #expect(item.state == .completed)

    // The persisted file is v2 and nested.
    let reread = try JSONDecoder().decode(
        PersistedState.self, from: Data(contentsOf: fileURL))
    #expect(reread.formatVersion == 2)
    #expect(reread.packages.first?.items.first?.components.count == 1)
}
```

> Match `FakeOrigin`'s real initializer and `url` accessor to
> `SDMKit/Sources/SDMEngine/FakeOrigin.swift`; adjust `EngineSettings(...)` to
> its current signature (see `DownloadEngine.swift` — `checkpointIntervalBytes`,
> `persistDebounceTicks`, `minSegmentSizeBytes` all have defaults). If
> `runUntilIdle()` is not accessible from the test module, drive the engine
> with the same `tick()` / `reconcile` pattern the neighbouring restart test
> uses.

- [ ] **Step 2: Run it**

Run: `cd SDMKit && swift test --filter 'DurableStateTests/engineRestoresAMultiComponentModelItemAndFinishesIt'`
Expected: PASS, 1 test.

- [ ] **Step 3: Full suite, format, commit**

```bash
cd SDMKit && swift test
cd .. && ./format.sh && ./lint.sh
git add SDMKit/Tests/SDMEngineTests/
git commit -m "test(engine): end-to-end persist/restore round-trip on the v2 model"
```

---

## Self-Review

**1. Spec coverage (Part 2 scope = parent spec §5 + §5.4 + the `PersistedState` half of §7.4/§11):**

| Spec item | Task |
|---|---|
| `FileComponent` with `url`/`partFilename`/`totalBytes`/`completed`/`validator`/`origin`/`isResumable`/`lastError` (§5.1) | Task 1 |
| `ComponentOrigin` `.http` / `.resolved(extractor,videoID,formatID)` (§5.1) | Task 1 |
| `DownloadItem` owns `components` (≥1), `outputFilename`, `assembly` (§5.2) | Task 2 |
| Generic HTTP download = one component; behaviour unchanged (§5.2) | Task 2 (convenience init + forwarders) + Task 5 (proof) |
| Item-space concatenation is derived, not stored (§5.3) | Task 2 (`completed` getter, `componentBaseOffsets`) |
| `isResumable` three-state across components (§7.1 rule, used later) | Task 2 |
| `DownloadPackage.note`, persisted (§5.4) | Task 3 |
| v1 → v2 one-shot migration shim for `state.json` (§7.4, §11) | Tasks 2 + 4 |

**Deferred to Part 3:** per-component worker pools, `assembling` state, `Muxer`, 403 → `refresh` wiring in `DownloadTask`, `ResumeSidecar` v2 + its migration, `ItemSnapshot.components` / concatenated snapshot fields, `DownloadEngine.init(resolver:)`.
**Deferred to Part 4:** `GrabberRow`/`MediaRow`, playlist expansion, handoff building multi-component items, format-picker UI, Settings screens, wiring `YtDlpResolver` into `EngineController`/`GrabberController`.

**2. Placeholder scan:** No "TBD"/"handle errors"/"similar to". Every step has real code. Three steps carry a "verify X against the existing file" note (RangeSet Codable shape, Priority Codable form, FakeOrigin/EngineSettings signatures) — these are verification instructions with a concrete fallback, not placeholders.

**3. Type consistency:** `FileComponent` field names defined Task 1 are used verbatim in Task 2's forwarders and Codable. `ComponentOrigin.resolved(extractor:videoID:formatID:)` labels match parent spec §5.1 and Part 1's `LinkResolver.refresh` parameter names. `Assembly` cases `.none`/`.mux` consistent. `DownloadItem` designated vs. convenience init signatures are fixed in Task 2 and not altered later. `PersistedState.currentFormatVersion` bumped once (Task 4). The `completed`/`totalBytes` setters' `precondition(components.count == 1)` is safe because no task in Parts 1–2 constructs a multi-component item that the engine then mutates — the multi-component `DownloadItem` tests in Task 2 only read.

**Known follow-up for Part 3, not a gap here:** once the engine writes progress per-component, the `completed`/`totalBytes` **setters** on `DownloadItem` must be replaced with per-component writes in `DownloadEngine.finish()` / `resetDownload()`; the `precondition` will start firing the moment a two-component item reaches `finish()`, which is the intended tripwire.

---

## Execution Handoff

Executing inline in this session via superpowers:executing-plans, on a fresh branch (no worktree), immediately after writing.
