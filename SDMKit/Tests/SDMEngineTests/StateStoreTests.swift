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

// MARK: - flush() failure handling (fix round 1, Finding 1)
//
// The brief's Step 3 code cleared `pending` before the encode/write were
// known to succeed, so a write failure (disk full, permission denied, a
// future unencodable field) silently discarded the save with no retry.
// `flush()` now only clears `pending` once the write has actually
// succeeded; on any failure the save stays queued so the *next* `flush()`
// call retries it.

@Test func flushPreservesPendingStateWhenTheWriteFails() async throws {
    let dir = try makeScratchDirectory()
    defer {
        // Restore write permission before cleanup — removing a file
        // requires write permission on its parent directory.
        try? FileManager.default.setAttributes(
            [.posixPermissions: 0o755], ofItemAtPath: dir.path)
        try? FileManager.default.removeItem(at: dir)
    }
    let url = dir.appendingPathComponent("state.json")

    // Strip write permission from the directory so the atomic write (which
    // creates a temp file alongside the destination) fails with EACCES.
    try FileManager.default.setAttributes([.posixPermissions: 0o500], ofItemAtPath: dir.path)

    let store = JSONStateStore(fileURL: url)
    await store.save(PersistedState(packages: [samplePackage("first")]))
    await store.flush()

    // The write failed; nothing should have landed on disk, and the save
    // must not have been discarded.
    #expect(!FileManager.default.fileExists(atPath: url.path))

    // Restore permission and flush again: the pending save from before
    // must still be there, waiting to retry.
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: dir.path)
    await store.flush()

    #expect(await JSONStateStore(fileURL: url).load().packages.map(\.name) == ["first"])
}

// MARK: - Round-trip fidelity (fix round 1, gap)
//
// Prior tests only ever asserted on `packages.map(\.name)`. Spec §4.2 says
// the snapshot carries packages, items, URLs, sizes, priorities, ordering,
// and enabled flags — this test saves a graph exercising all of those
// (non-trivial `RangeSet` progress, a mix of inherited vs. explicitly-set
// priority, and a deliberately non-identity ordering) and checks it comes
// back byte-for-byte equal, not just name-equal.

@Test func roundTripPreservesFullPackageGraphThroughTheStore() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("state.json")

    var partiallyDownloaded = DownloadItem(
        url: URL(string: "https://example.com/movie.mkv")!,
        filename: "movie.mkv",
        totalBytes: 1_000,
        isEnabled: true,
        isResumable: true,
        priority: nil,  // inherits the package's priority
        position: 1,
        validator: "etag-abc123"
    )
    partiallyDownloaded.completed.insert(ByteRange(start: 0, end: 100))
    partiallyDownloaded.completed.insert(ByteRange(start: 500, end: 750))
    partiallyDownloaded.state = .running

    let disabledItem = DownloadItem(
        url: URL(string: "https://example.com/subs.srt")!,
        filename: "subs.srt",
        totalBytes: nil,
        isEnabled: false,
        isResumable: false,
        priority: .highest,  // explicitly overrides the package's priority
        position: 0
    )

    let failedItem = DownloadItem(
        url: URL(string: "https://example.com/cover.jpg")!,
        filename: "cover.jpg",
        state: .failed(reason: "404 Not Found"),
        priority: .lowest,
        position: 2
    )

    let package = DownloadPackage(
        name: "Season 1",
        // Deliberately not stored in position order, to prove ordering is
        // preserved as array order, not resorted on load.
        items: [partiallyDownloaded, disabledItem, failedItem],
        priority: .high,
        position: 3
    )

    let store = JSONStateStore(fileURL: url)
    let saved = PersistedState(packages: [package])
    await store.save(saved)
    await store.flush()

    let reloaded = await JSONStateStore(fileURL: url).load()
    #expect(reloaded == saved)

    // Spell out the properties the equality check above is standing in
    // for, so a future change to `Equatable` synthesis can't quietly
    // weaken this test.
    let reloadedPackage = try #require(reloaded.packages.first)
    #expect(reloadedPackage.priority == .high)
    #expect(reloadedPackage.position == 3)
    #expect(reloadedPackage.items.map(\.filename) == ["movie.mkv", "subs.srt", "cover.jpg"])
    #expect(reloadedPackage.items.map(\.position) == [1, 0, 2])
    #expect(reloadedPackage.items[0].priority == nil)
    #expect(reloadedPackage.items[1].priority == .highest)
    #expect(
        reloadedPackage.items[0].completed.ranges == [
            ByteRange(start: 0, end: 100), ByteRange(start: 500, end: 750),
        ])
    #expect(reloadedPackage.items[0].state == .running)
    #expect(reloadedPackage.items[1].isEnabled == false)
    #expect(reloadedPackage.items[2].state == .failed(reason: "404 Not Found"))
    #expect(reloadedPackage.items[0].validator == "etag-abc123")
}

// MARK: - Corrupt-snapshot validation (Task 15 addition)
//
// `Codable`'s synthesized `init(from:)` does not route through
// `DownloadPackage.init` / `DownloadItem.init`, so their `precondition`
// guards never fire on a hand-edited or corrupt snapshot. Rejecting such a
// snapshot is this store's job. "Reject" is defined to mean the same thing
// as every other malformed-file case here (missing file, unreadable file,
// garbage bytes): `load()` returns an empty `PersistedState`, never a
// value containing the invalid model.

/// Builds real, schema-correct JSON for a valid snapshot (by round-tripping
/// through `JSONEncoder`), then corrupts one field by direct string
/// substitution. This avoids hand-guessing the synthesized `Codable` layout
/// for `Priority` / `ItemState` while still exercising a hand-edited file,
/// exactly the failure mode the addition targets.
private func encodedJSON(for state: PersistedState) throws -> String {
    let data = try JSONEncoder().encode(state)
    return String(decoding: data, as: UTF8.self)
}

@Test func snapshotWithEmptyPackageNameLoadsAsEmptyRatherThanInvalidModel() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("state.json")

    var json = try encodedJSON(for: PersistedState(packages: [samplePackage("Season 1")]))
    #expect(json.contains("\"Season 1\""))
    json = json.replacingOccurrences(of: "\"Season 1\"", with: "\"\"")
    try Data(json.utf8).write(to: url)

    #expect(await JSONStateStore(fileURL: url).load().packages.isEmpty)
}

@Test func snapshotWithEmptyItemFilenameLoadsAsEmptyRatherThanInvalidModel() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("state.json")

    var json = try encodedJSON(for: PersistedState(packages: [samplePackage("Season 1")]))
    #expect(json.contains("\"a.bin\""))
    json = json.replacingOccurrences(of: "\"a.bin\"", with: "\"\"")
    try Data(json.utf8).write(to: url)

    #expect(await JSONStateStore(fileURL: url).load().packages.isEmpty)
}

/// Wire format, discovered directly (not assumed) by encoding a fully
/// populated `PersistedState` with `JSONEncoder(outputFormatting:
/// [.prettyPrinted, .sortedKeys])` and inspecting the result:
///
/// ```json
/// {
///   "formatVersion" : 1,
///   "packages" : [
///     {
///       "id" : "<uuid>",
///       "name" : "Season 1",
///       "position" : 0,
///       "priority" : 2,
///       "items" : [
///         {
///           "id" : "<uuid>",
///           "url" : "https://example.com/a.bin",
///           "filename" : "a.bin",
///           "totalBytes" : 100,
///           "completed" : { "ranges" : [ { "start" : 0, "end" : 10 } ] },
///           "state" : { "queued" : {} },
///           "isEnabled" : true,
///           "isResumable" : true,
///           "priority" : 3,
///           "position" : 1
///         }
///       ]
///     }
///   ]
/// }
/// ```
///
/// Key points pinned down by the probe, not guessed:
/// - `Priority` is its raw `Int` (0...4), not a string.
/// - `ItemState` (an enum with an associated-value case) is a single-key
///   object per case, e.g. `{"queued": {}}` / `{"failed": {"reason": "..."}}`.
/// - `RangeSet` is `{"ranges": [{"start", "end"}]}`.
/// - `Optional` fields (`totalBytes`, `validator`, item `priority` when
///   `nil`) are omitted entirely rather than encoded as `null`.
///
/// This test types that format out literally — independent of whatever
/// `JSONEncoder` happens to produce today — so a future change to the
/// encoder that still round-trips through itself but silently breaks
/// decoding of *existing on-disk snapshots* would be caught here.
@Test func handWrittenSnapshotWithEmptyNestedItemFilenameLoadsAsEmpty() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("state.json")

    // Deliberately: keys in a different order than the encoder emits them,
    // an item with no totalBytes/validator/priority (all omitted, not
    // null), and the violation (empty filename) nested inside an otherwise
    // entirely valid package.
    let json = """
        {
          "packages": [
            {
              "priority": 2,
              "name": "Season 1",
              "id": "8B687F15-C739-421D-9E0F-1D2EFD8AC989",
              "position": 0,
              "items": [
                {
                  "state": { "queued": {} },
                  "url": "https://example.com/a.bin",
                  "isEnabled": true,
                  "filename": "",
                  "isResumable": false,
                  "id": "37B1C7A5-EDCF-4DA5-8849-FED5AC7AF237",
                  "completed": { "ranges": [] },
                  "position": 0
                }
              ]
            }
          ],
          "formatVersion": 1
        }
        """
    try Data(json.utf8).write(to: url)

    #expect(await JSONStateStore(fileURL: url).load().packages.isEmpty)
}
