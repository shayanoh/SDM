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
