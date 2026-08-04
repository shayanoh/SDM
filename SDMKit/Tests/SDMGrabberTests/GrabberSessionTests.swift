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
