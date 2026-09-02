import Foundation
import Testing

@testable import SDMCore
@testable import SDMEngine

/// Hands back a scripted `RefreshedFormat` (or throws) — the engine's only
/// use of `LinkResolver` is `refresh`.
final class FakeResolver: LinkResolver, @unchecked Sendable {
    var refreshResult: @Sendable (String) throws -> RefreshedFormat
    init(_ f: @escaping @Sendable (String) throws -> RefreshedFormat) { refreshResult = f }
    func canHandle(_ url: URL) -> Bool { false }
    func resolve(_ url: URL) async throws -> ResolvedTarget { throw ResolveError.unsupported }
    func refresh(
        extractor: String, videoID: String, formatID: String
    ) async throws -> RefreshedFormat {
        try refreshResult(formatID)
    }
}

private func resolvedItem(_ url: URL, totalBytes: Int64?) -> DownloadItem {
    DownloadItem(
        components: [
            FileComponent(
                url: url, partFilename: "clip.f137.mp4", totalBytes: totalBytes,
                origin: .resolved(extractor: "youtube", videoID: "abc", formatID: "137"))
        ], outputFilename: "clip.mp4", assembly: .none, state: .queued)
}

@Test func expiredComponentUrlIsRefreshedAndTheDownloadFinishes() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let payload = testPayload(8000)

    var staleBehavior = FakeOrigin.Behavior()
    staleBehavior.statusOverride = 403
    let router = TwoHostRouter(
        videoHost: "stale.example", videoPayload: payload, videoBehavior: staleBehavior,
        audioHost: "fresh.example", audioPayload: payload)
    let resolver = FakeResolver { formatID in
        RefreshedFormat(
            url: URL(string: "https://fresh.example/f137")!, filesize: nil, formatID: formatID)
    }
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 2, globalMaxConnections: 8, downloadFolder: dir),
        resolver: resolver)
    await engine.add(
        DownloadPackage(
            name: "P",
            items: [resolvedItem(URL(string: "https://stale.example/f137")!, totalBytes: nil)]))
    try await engine.runUntilIdle()

    let snap = try #require(await engine.snapshot().packages.first?.items.first)
    #expect(snap.state == .completed)
    #expect(try Data(contentsOf: dir.appendingPathComponent("P/clip.f137.mp4")) == payload)
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
            items: [resolvedItem(URL(string: "https://stale.example/f137")!, totalBytes: 8000)]))
    try await engine.runUntilIdle()
    let snap = try #require(await engine.snapshot().packages.first?.items.first)
    if case .failed(let reason) = snap.state {
        #expect(reason.contains("size"))
    } else {
        Issue.record("expected .failed, got \(snap.state)")
    }
}

@Test func expiredComponentWithNoResolverFailsRatherThanLooping() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    var staleBehavior = FakeOrigin.Behavior()
    staleBehavior.statusOverride = 403
    let router = TwoHostRouter(
        videoHost: "stale.example", videoPayload: testPayload(8000), videoBehavior: staleBehavior,
        audioHost: "fresh.example", audioPayload: testPayload(8000))
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 2, globalMaxConnections: 8, downloadFolder: dir))
    await engine.add(
        DownloadPackage(
            name: "P",
            items: [resolvedItem(URL(string: "https://stale.example/f137")!, totalBytes: nil)]))
    // No resolver: urlExpired → serverError(403) → transient backoff, held
    // out of the desired set. runUntilIdle returns without looping.
    try await engine.runUntilIdle()
    let snap = try #require(await engine.snapshot().packages.first?.items.first)
    #expect(snap.state != .completed)
}
