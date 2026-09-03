import Foundation
import Testing

@testable import SDMCore
@testable import SDMGrabber

private func playlistResolver(title: String, count: Int, totalAvailable: Int) -> FakeLinkResolver {
    let entries = (1...count).map {
        ResolvedMedia(
            extractor: "youtube", videoID: "vid\($0)", title: "Episode \($0)",
            durationSeconds: nil, formats: [],
            sourceURL: URL(string: "https://www.youtube.com/watch?v=vid\($0)"))
    }
    return FakeLinkResolver { url in
        if url.absoluteString.contains("list=") || url.path.hasPrefix("/playlist") {
            return .playlist(title: title, entries: entries, totalAvailable: totalAvailable)
        }
        let id =
            URLComponents(url: url, resolvingAgainstBaseURL: false)?
            .queryItems?.first(where: { $0.name == "v" })?.value ?? "?"
        return singleMedia(
            videoID: id, title: "Episode",
            formats: [vf("137", 720, .h264, .mp4), af("140", .aac, .m4a)])
    }
}

private func session(_ resolver: FakeLinkResolver) -> GrabberSession {
    GrabberSession(
        prober: LinkProber(transport: FakeProbeOrigin(), deepSniffEnabled: false),
        resolver: resolver)
}

@Test func aPlaylistUrlExpandsIntoOneRowPerEntryInOnePackage() async {
    let s = session(playlistResolver(title: "My Series", count: 5, totalAvailable: 12))
    await s.ingest(urls: [URL(string: "https://www.youtube.com/playlist?list=PL123")!])
    let snap = await s.snapshot()
    #expect(snap.mediaRows.count == 5)
    #expect(Set(snap.mediaRows.compactMap(\.playlistGroup)).count == 1)
    #expect(snap.packages.count == 1)
    #expect(snap.packages[0].name == "My Series")
    #expect(snap.packages[0].note == "5 of 12 videos")
    #expect(snap.packages[0].linkIDs.count == 5)
    #expect(snap.mediaRows.allSatisfy { $0.state == .resolved })
}

@Test func playlistEntryRowsUseTheirOwnSourceURL() async {
    let entries = [
        ResolvedMedia(
            extractor: "", videoID: "t1", title: "Track 1", durationSeconds: nil, formats: [],
            sourceURL: URL(string: "https://soundcloud.com/artist/track-1")),
        ResolvedMedia(
            extractor: "", videoID: "t2", title: "Track 2", durationSeconds: nil, formats: [],
            sourceURL: URL(string: "https://soundcloud.com/artist/track-2")),
    ]
    let resolver = FakeLinkResolver { url in
        if url.path.contains("/sets/") {
            return .playlist(title: "Set", entries: entries, totalAvailable: 2)
        }
        return singleMedia(
            videoID: url.lastPathComponent, title: url.lastPathComponent,
            formats: [vf("137", 720, .h264, .mp4), af("140", .aac, .m4a)])
    }
    resolver.handledHosts = ["soundcloud.com"]
    let s = session(resolver)
    await s.ingest(urls: [URL(string: "https://soundcloud.com/artist/sets/mix")!])
    let rows = await s.snapshot().mediaRows
    #expect(rows.count == 2)
    #expect(
        Set(rows.map(\.sourceURL)) == [
            URL(string: "https://soundcloud.com/artist/track-1")!,
            URL(string: "https://soundcloud.com/artist/track-2")!,
        ])
}

@Test func aFullyListedPlaylistHasNoTruncationNote() async {
    let s = session(playlistResolver(title: "Short", count: 3, totalAvailable: 3))
    await s.ingest(urls: [URL(string: "https://www.youtube.com/playlist?list=X")!])
    #expect(await s.snapshot().packages[0].note == nil)
}

@Test func removingAPlaylistPackageClearsItsNote() async {
    let s = session(playlistResolver(title: "Gone", count: 4, totalAvailable: 9))
    await s.ingest(urls: [URL(string: "https://www.youtube.com/playlist?list=Y")!])
    let packageID = await s.snapshot().packages[0].id
    await s.removePackage(packageID)
    #expect(await s.snapshot().packages.isEmpty)
    #expect(await s.snapshot().mediaRows.isEmpty)
}

/// Resolver that records the peak number of `resolve` calls in flight.
private final class ConcurrencyTrackingResolver: LinkResolver, @unchecked Sendable {
    private let lock = NSLock()
    private var inFlight = 0
    private(set) var peak = 0
    let entries: [ResolvedMedia]

    init(entries: [ResolvedMedia]) { self.entries = entries }

    func canHandle(_ url: URL) -> Bool { url.host?.contains("youtube") ?? false }
    func refresh(sourceURL: URL, formatID: String) async throws -> RefreshedFormat {
        RefreshedFormat(url: url, filesize: nil, formatID: formatID)
    }
    private var url: URL { URL(string: "https://gv/x")! }

    func resolve(_ url: URL) async throws -> ResolvedTarget {
        if url.absoluteString.contains("list=") {
            return .playlist(title: "P", entries: entries, totalAvailable: entries.count)
        }
        lock.withLock {
            inFlight += 1
            peak = max(peak, inFlight)
        }
        try? await Task.sleep(for: .milliseconds(30))
        lock.withLock { inFlight -= 1 }
        return singleMedia(
            videoID: "v", title: "E", formats: [vf("137", 720, .h264, .mp4), af("140", .aac, .m4a)])
    }
}

@Test func playlistEntryResolvesAreCappedAtMaxConcurrentResolves() async {
    let entries = (1...20).map {
        ResolvedMedia(
            extractor: "youtube", videoID: "vid\($0)", title: "E\($0)", durationSeconds: nil,
            formats: [], sourceURL: URL(string: "https://www.youtube.com/watch?v=vid\($0)"))
    }
    let resolver = ConcurrencyTrackingResolver(entries: entries)
    let session = GrabberSession(
        prober: LinkProber(transport: FakeProbeOrigin(), deepSniffEnabled: false),
        budget: GrabberSession.Budget(globalMaxConcurrentProbes: 32, maxConcurrentResolves: 3),
        resolver: resolver)
    await session.ingest(urls: [URL(string: "https://www.youtube.com/playlist?list=PLx")!])
    #expect(await session.snapshot().mediaRows.count == 20)
    #expect(resolver.peak <= 3)
    #expect(resolver.peak >= 2)  // it did actually parallelize some
}

@Test func theSamePlaylistCanBeAddedAgainAfterItsEntriesWereHandedOff() async {
    let resolver = playlistResolver(title: "Repeat", count: 4, totalAvailable: 4)
    let session = session(resolver)
    let url = URL(string: "https://www.youtube.com/playlist?list=PLrepeat")!

    await session.ingest(urls: [url])
    #expect(await session.snapshot().mediaRows.count == 4)

    // Simulate the handoff removing every entry row.
    for row in await session.snapshot().mediaRows { await session.removeLink(row.id) }
    #expect(await session.snapshot().mediaRows.isEmpty)

    // The same playlist URL must ingest again — the origin URL was left in
    // `seenURLs` before this fix and silently deduped.
    await session.ingest(urls: [url])
    #expect(await session.snapshot().mediaRows.count == 4)
}

/// Resolver whose per-entry `resolve` blocks until released, so a playlist
/// expansion can be caught mid-flight.
private final class GatedPlaylistResolver: LinkResolver, @unchecked Sendable {
    let entries: [ResolvedMedia]
    private let lock = NSLock()
    private var started = 0
    private var released = false

    init(count: Int) {
        entries = (1...count).map {
            ResolvedMedia(
                extractor: "youtube", videoID: "vid\($0)", title: "E\($0)", durationSeconds: nil,
                formats: [], sourceURL: URL(string: "https://www.youtube.com/watch?v=vid\($0)"))
        }
    }

    func canHandle(_ url: URL) -> Bool { url.host?.contains("youtube") ?? false }
    func refresh(sourceURL: URL, formatID: String) async throws -> RefreshedFormat {
        RefreshedFormat(url: URL(string: "https://gv/x")!, filesize: nil, formatID: formatID)
    }
    var startedCount: Int { lock.withLock { started } }
    func releaseAll() { lock.withLock { released = true } }

    func resolve(_ url: URL) async throws -> ResolvedTarget {
        if url.absoluteString.contains("list=") {
            return .playlist(title: "P", entries: entries, totalAvailable: entries.count)
        }
        lock.withLock { started += 1 }
        while !lock.withLock({ released }) { try await Task.sleep(for: .milliseconds(5)) }
        return singleMedia(
            videoID: "v", title: "E", formats: [vf("137", 720, .h264, .mp4), af("140", .aac, .m4a)])
    }
}

@Test func cancelChecksStopsPlaylistEntriesStillResolving() async {
    let resolver = GatedPlaylistResolver(count: 12)
    let session = GrabberSession(
        prober: LinkProber(transport: FakeProbeOrigin(), deepSniffEnabled: false),
        budget: GrabberSession.Budget(maxConcurrentResolves: 2),
        resolver: resolver)

    let ingestTask = Task {
        await session.ingest(urls: [URL(string: "https://www.youtube.com/playlist?list=PLx")!])
    }
    while resolver.startedCount == 0 { await Task.yield() }

    await session.cancelChecks()
    resolver.releaseAll()
    await ingestTask.value

    let snap = await session.snapshot()
    #expect(snap.mediaRows.count == 12)
    #expect(snap.mediaRows.contains { $0.state == .failed("Check cancelled") })
    #expect(snap.isChecking == false)
    #expect(snap.recheckableCount >= 1)
}

@Test func recheckReResolvesFailedMediaRows() async {
    final class Counter: @unchecked Sendable { var n = 0 }
    let counter = Counter()
    let resolver = FakeLinkResolver { _ in
        counter.n += 1
        if counter.n == 1 { throw ResolveError.ytDlpFailed(stderrTail: "temporary blip") }
        return singleMedia(
            videoID: "v", title: "Now works",
            formats: [vf("137", 720, .h264, .mp4), af("140", .aac, .m4a)])
    }
    let session = GrabberSession(
        prober: LinkProber(transport: FakeProbeOrigin(), deepSniffEnabled: false),
        resolver: resolver)
    await session.ingest(urls: [URL(string: "https://www.youtube.com/watch?v=abc")!])
    guard case .failed = await session.snapshot().mediaRows[0].state else {
        Issue.record("expected first attempt to fail")
        return
    }
    #expect(await session.snapshot().recheckableCount == 1)

    await session.recheckFailed()
    #expect(await session.snapshot().mediaRows[0].state == .resolved)
    #expect(await session.snapshot().recheckableCount == 0)
}
