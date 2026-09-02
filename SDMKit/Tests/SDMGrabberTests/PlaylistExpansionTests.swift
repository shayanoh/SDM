import Foundation
import Testing

@testable import SDMCore
@testable import SDMGrabber

private func playlistResolver(title: String, count: Int, totalAvailable: Int) -> FakeLinkResolver {
    let entries = (1...count).map {
        ResolvedMedia(
            extractor: "youtube", videoID: "vid\($0)", title: "Episode \($0)",
            durationSeconds: nil, formats: [])
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
