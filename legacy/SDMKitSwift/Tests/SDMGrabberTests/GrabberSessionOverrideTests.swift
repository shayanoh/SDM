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

@Test func removingAPackageRemovesEveryMemberLinkAndAllowsReingesting() async throws {
    let (session, _) = makeSession()
    let urlA = URL(string: "https://example.com/Show.S01E01.mkv")!
    let urlB = URL(string: "https://example.com/Show.S01E02.mkv")!
    let urlC = URL(string: "https://other.example.com/unrelated.bin")!
    await session.ingest(urls: [urlA, urlB, urlC])
    #expect(await session.snapshot().totalCount == 3)

    let packages = await session.snapshot().packages
    let showPackage = try #require(packages.first { $0.linkIDs.count == 2 })

    await session.removePackage(showPackage.id)

    let snapshot = await session.snapshot()
    #expect(snapshot.totalCount == 1)
    #expect(snapshot.packages.count == 1)
    #expect(!snapshot.packages.contains { $0.id == showPackage.id })

    // Removed links are gone from `seenURLs` too, so they can be re-grabbed.
    await session.ingest(urls: [urlA])
    #expect(await session.snapshot().totalCount == 2)
}

@Test func clearRemovesEveryLinkAndPackage() async throws {
    let (session, _) = makeSession()
    await session.ingest(
        urls: [
            URL(string: "https://example.com/a.zip")!,
            URL(string: "https://other.example.com/b.zip")!,
        ])
    #expect(await session.snapshot().totalCount == 2)

    await session.clear()

    let snapshot = await session.snapshot()
    #expect(snapshot.totalCount == 0)
    #expect(snapshot.packages.isEmpty)

    // Cleared links can be re-grabbed, same as any removed link.
    await session.ingest(urls: [URL(string: "https://example.com/a.zip")!])
    #expect(await session.snapshot().totalCount == 1)
}

@Test func movingALinkOverridesAutomaticClustering() async throws {
    let (session, _) = makeSession()
    let urlA = URL(string: "https://a.example.com/alpha.bin")!
    let urlB = URL(string: "https://b.example.com/beta.bin")!
    await session.ingest(urls: [urlA, urlB])

    let snapshotBefore = await session.snapshot()
    #expect(snapshotBefore.packages.count == 2)
    let packageAID = try #require(snapshotBefore.packages.first?.id)
    let idB = try #require(snapshotBefore.packages.last?.linkIDs.first)

    await session.moveLink(idB, toPackage: packageAID)

    let snapshotAfter = await session.snapshot()
    #expect(snapshotAfter.packages.count == 1)
    #expect(snapshotAfter.packages.first?.linkIDs.count == 2)
}
