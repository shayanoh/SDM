import Foundation
import Testing

@testable import SDMGrabber

private func makeSession() -> (GrabberSession, FakeProbeOrigin) {
    let origin = FakeProbeOrigin()
    let session = GrabberSession(prober: LinkProber(transport: origin, deepSniffEnabled: false))
    return (session, origin)
}

@Test func settingKnownDownloadURLsMarksExistingLinksAsDuplicates() async throws {
    let (session, _) = makeSession()
    let url = URL(string: "https://a.example.com/movie.mp4")!
    await session.ingest(urls: [url])
    #expect(await session.snapshot().links.first?.isDuplicate == false)

    await session.setKnownDownloadURLs([url])
    #expect(await session.snapshot().links.first?.isDuplicate == true)
}

@Test func ingestMarksDuplicateImmediatelyWhenAlreadyKnown() async throws {
    let (session, _) = makeSession()
    let url = URL(string: "https://a.example.com/movie.mp4")!
    await session.setKnownDownloadURLs([url])
    await session.ingest(urls: [url])
    #expect(await session.snapshot().links.first?.isDuplicate == true)
}

@Test func removingALinkAllowsItToBeReingested() async throws {
    let (session, _) = makeSession()
    let url = URL(string: "https://a.example.com/movie.mp4")!
    await session.ingest(urls: [url])
    let firstID = try #require(await session.snapshot().links.first?.id)

    await session.removeLink(firstID)
    #expect(await session.snapshot().totalCount == 0)

    await session.ingest(urls: [url])
    #expect(await session.snapshot().totalCount == 1)
    #expect(await session.snapshot().links.first?.id != firstID)
}

@Test func movingALinkOverridesAutomaticClustering() async throws {
    let (session, _) = makeSession()
    let urlA = URL(string: "https://a.example.com/alpha.bin")!
    let urlB = URL(string: "https://b.example.com/beta.bin")!
    await session.ingest(urls: [urlA, urlB])

    let snapshotBefore = await session.snapshot()
    #expect(snapshotBefore.packages.count == 2)
    let packageAName = try #require(snapshotBefore.packages.first?.name)
    let idB = try #require(snapshotBefore.packages.last?.linkIDs.first)

    await session.moveLink(idB, toPackageNamed: packageAName)

    let snapshotAfter = await session.snapshot()
    #expect(snapshotAfter.packages.count == 1)
    #expect(snapshotAfter.packages.first?.linkIDs.count == 2)
}
