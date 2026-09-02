import Foundation
import Testing

@testable import SDMGrabber

@Test func ingestExtractsAndProbesLinks() async throws {
    let origin = FakeProbeOrigin()
    let url = URL(string: "https://a.example.com/movie.mp4")!
    var behavior = FakeProbeOrigin.Behavior()
    behavior.statusCode = 200
    behavior.headers = ["content-type": "video/mp4", "content-length": "5000000"]
    await origin.setBehavior(behavior, for: url)

    let session = GrabberSession(prober: LinkProber(transport: origin, deepSniffEnabled: false))
    await session.ingest(text: "check out https://a.example.com/movie.mp4 now")

    let snapshot = await session.snapshot()
    #expect(snapshot.totalCount == 1)
    #expect(snapshot.checkedCount == 1)
    #expect(snapshot.links.first?.verdict == .online)
}

@Test func ingestDedupesARepeatedURLAcrossCalls() async throws {
    let origin = FakeProbeOrigin()
    let url = URL(string: "https://a.example.com/movie.mp4")!
    let session = GrabberSession(prober: LinkProber(transport: origin, deepSniffEnabled: false))

    await session.ingest(urls: [url])
    await session.ingest(urls: [url])

    let snapshot = await session.snapshot()
    #expect(snapshot.totalCount == 1)
}

@Test func ingestClustersRelatedLinksIntoOnePackage() async throws {
    let origin = FakeProbeOrigin()
    let urls = [
        URL(string: "https://tv.example.com/season1/Show.S01E01.1080p.mkv")!,
        URL(string: "https://tv.example.com/season1/Show.S01E02.1080p.mkv")!,
    ]
    var behavior = FakeProbeOrigin.Behavior()
    behavior.statusCode = 200
    for url in urls { await origin.setBehavior(behavior, for: url) }

    let session = GrabberSession(prober: LinkProber(transport: origin, deepSniffEnabled: false))
    await session.ingest(urls: urls)

    let snapshot = await session.snapshot()
    #expect(snapshot.packages.count == 1)
    #expect(snapshot.packages.first?.linkIDs.count == 2)
}

@Test func probingRespectsTheGlobalConcurrencyBudget() async throws {
    let origin = FakeProbeOrigin()
    let urlA = URL(string: "https://a.example.com/1.bin")!
    let urlB = URL(string: "https://a.example.com/2.bin")!
    var held = FakeProbeOrigin.Behavior()
    held.holdsUntilReleased = true
    await origin.setBehavior(held, for: urlA)
    await origin.setBehavior(held, for: urlB)

    let session = GrabberSession(
        prober: LinkProber(transport: origin, deepSniffEnabled: false),
        budget: GrabberSession.Budget(globalMaxConcurrentProbes: 1, maxConcurrentPerHost: 4)
    )

    let ingestTask = Task { await session.ingest(urls: [urlA, urlB]) }
    while await origin.requestLog.isEmpty { await Task.yield() }
    let midFlight = await session.snapshot()
    #expect(midFlight.links.filter { $0.stage == .probing }.count == 1)
    #expect(midFlight.links.filter { $0.stage == .queued }.count == 1)

    await origin.release(urlA)
    await origin.release(urlB)
    await ingestTask.value

    let final = await session.snapshot()
    #expect(final.links.allSatisfy { $0.stage == .done })
}

@Test func probingRespectsThePerHostBudgetIndependentlyOfTheGlobalBudget() async throws {
    let origin = FakeProbeOrigin()
    let hostAFirst = URL(string: "https://a.example.com/1.bin")!
    let hostASecond = URL(string: "https://a.example.com/2.bin")!
    let hostB = URL(string: "https://b.example.com/1.bin")!
    var held = FakeProbeOrigin.Behavior()
    held.holdsUntilReleased = true
    for url in [hostAFirst, hostASecond, hostB] { await origin.setBehavior(held, for: url) }

    let session = GrabberSession(
        prober: LinkProber(transport: origin, deepSniffEnabled: false),
        budget: GrabberSession.Budget(globalMaxConcurrentProbes: 8, maxConcurrentPerHost: 1)
    )

    let ingestTask = Task { await session.ingest(urls: [hostAFirst, hostASecond, hostB]) }
    while await origin.requestLog.count < 2 { await Task.yield() }
    let midFlight = await session.snapshot()
    #expect(midFlight.links.filter { $0.stage == .probing }.count == 2)
    #expect(midFlight.links.filter { $0.stage == .queued }.count == 1)

    for url in [hostAFirst, hostASecond, hostB] { await origin.release(url) }
    await ingestTask.value
}

@Test func recheckRetriesTheGivenLinksRegardlessOfVerdict() async throws {
    let origin = FakeProbeOrigin()
    let urlA = URL(string: "https://a.example.com/a.mp4")!
    let urlB = URL(string: "https://a.example.com/b.mp4")!
    var gone = FakeProbeOrigin.Behavior()
    gone.statusCode = 404
    await origin.setBehavior(gone, for: urlA)
    await origin.setBehavior(gone, for: urlB)

    let session = GrabberSession(prober: LinkProber(transport: origin, deepSniffEnabled: false))
    await session.ingest(urls: [urlA, urlB])
    #expect(await session.snapshot().links.allSatisfy { $0.verdict == .offline })

    var ok = FakeProbeOrigin.Behavior()
    ok.statusCode = 200
    ok.headers = ["content-length": "5000000"]
    await origin.setBehavior(ok, for: urlA)

    let idA = try #require(await session.snapshot().links.first { $0.originalURL == urlA }?.id)
    await session.recheck(ids: [idA])

    let links = await session.snapshot().links
    #expect(links.first { $0.originalURL == urlA }?.verdict == .online)
    #expect(links.first { $0.originalURL == urlB }?.verdict == .offline)
}

@Test func pruneOlderThanRemovesRowsPastTheCutoffAndKeepsFresherOnes() async throws {
    let origin = FakeProbeOrigin()
    let session = GrabberSession(prober: LinkProber(transport: origin, deepSniffEnabled: false))

    await session.ingest(urls: [URL(string: "https://a.example/old.zip")!])
    try await Task.sleep(for: .milliseconds(30))
    // Everything ingested from here on is strictly newer than `cutoff`.
    let cutoff = Date()
    await session.ingest(urls: [URL(string: "https://b.example/new.zip")!])

    // A generous max age: nothing is old enough yet.
    await session.pruneOlderThan(3600, now: cutoff)
    #expect(await session.snapshot().links.count == 2)

    // maxAge 0 with `now == cutoff`: rows added before `cutoff` go, the one
    // added after stays.
    await session.pruneOlderThan(0, now: cutoff)
    #expect(
        await session.snapshot().links.map(\.originalURL.absoluteString)
            == ["https://b.example/new.zip"])
}

@Test func pruneOlderThanIsANoOpForANeverExpiredList() async {
    let session = GrabberSession(
        prober: LinkProber(transport: FakeProbeOrigin(), deepSniffEnabled: false))
    await session.ingest(urls: [URL(string: "https://x.example/f.zip")!])
    await session.pruneOlderThan(3600)
    #expect(await session.snapshot().links.count == 1)
}
