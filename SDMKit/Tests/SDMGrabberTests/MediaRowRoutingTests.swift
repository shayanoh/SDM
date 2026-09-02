import Foundation
import Testing

@testable import SDMCore
@testable import SDMGrabber

@Test func mediaRowFallsBackToUrlTitleBeforeResolving() {
    let row = MediaRow(sourceURL: URL(string: "https://youtu.be/dQw4w9WgXcQ")!)
    #expect(row.state == .resolving)
    #expect(row.title == "dQw4w9WgXcQ")
    #expect(row.displayFilename == "dQw4w9WgXcQ")
    #expect(row.combinedBytes == nil)
}

@Test func mediaRowDisplayFilenameUsesTitleVideoIdAndContainer() {
    let v = MediaFormat(
        id: "137", kind: .videoOnly, height: 1080, width: 1920, vcodec: .h264, acodec: nil,
        container: .mp4, filesize: 100, filesizeApprox: nil, tbr: nil,
        url: URL(string: "https://gv/v")!)
    let a = MediaFormat(
        id: "140", kind: .audioOnly, height: nil, width: nil, vcodec: nil, acodec: .aac,
        container: .m4a, filesize: 10, filesizeApprox: nil, tbr: nil,
        url: URL(string: "https://gv/a")!)
    var row = MediaRow(sourceURL: URL(string: "https://youtu.be/abc")!)
    row.media = ResolvedMedia(
        extractor: "youtube", videoID: "abc", title: "Rick Astley / Never Gonna",
        durationSeconds: 213, formats: [v, a])
    row.choice = FormatChoice(video: v, audio: a, outputContainer: .mp4, estimatedBytes: 110)
    row.state = .resolved
    #expect(row.displayFilename == "Rick Astley Never Gonna [abc].mp4")
    #expect(row.combinedBytes == 110)
}

@Test func packageCandidateNoteDefaultsNil() {
    #expect(PackageCandidate(name: "P", linkIDs: []).note == nil)
    #expect(
        PackageCandidate(name: "P", linkIDs: [], note: "50 of 320 videos").note
            == "50 of 320 videos")
}

// MARK: - Routing (Task 2)

private func makeSession(_ resolver: FakeLinkResolver) -> GrabberSession {
    GrabberSession(
        prober: LinkProber(transport: FakeProbeOrigin(), deepSniffEnabled: false),
        resolver: resolver)
}

@Test func aYouTubeUrlBecomesAResolvedMediaRow() async {
    let resolver = FakeLinkResolver { _ in
        singleMedia(
            videoID: "abc", title: "Song",
            formats: [vf("137", 1080, .h264, .mp4), af("140", .aac, .m4a)])
    }
    let session = makeSession(resolver)
    await session.ingest(urls: [URL(string: "https://youtu.be/abc")!])
    let snap = await session.snapshot()
    #expect(snap.links.isEmpty)
    #expect(snap.mediaRows.count == 1)
    #expect(snap.mediaRows[0].state == .resolved)
    #expect(snap.mediaRows[0].choice?.video?.id == "137")
    #expect(snap.mediaRows[0].choice?.audio?.id == "140")
}

@Test func aNonYouTubeUrlStaysOnTheHttpProbePath() async {
    let resolver = FakeLinkResolver { _ in
        .single(
            ResolvedMedia(
                extractor: "x", videoID: "x", title: "x", durationSeconds: nil, formats: []))
    }
    let session = makeSession(resolver)
    await session.ingest(urls: [URL(string: "https://example.com/a.mp4")!])
    let snap = await session.snapshot()
    #expect(snap.mediaRows.isEmpty)
    #expect(snap.links.count == 1)
}

@Test func autoPickReturningNilLeavesTheRowUnselected() async {
    let resolver = FakeLinkResolver { _ in
        singleMedia(videoID: "abc", title: "Song", formats: [vf("137", 1080, .h264, .mp4)])
    }
    let session = makeSession(resolver)
    await session.ingest(urls: [URL(string: "https://youtu.be/abc")!])
    #expect(await session.snapshot().mediaRows[0].state == .unselected)
}

@Test func anUnsupportedResolveMarksTheRowUnsupported() async {
    let session = makeSession(FakeLinkResolver { _ in throw ResolveError.unsupported })
    await session.ingest(urls: [URL(string: "https://youtu.be/live")!])
    #expect(await session.snapshot().mediaRows[0].state == .unsupported)
}

@Test func aMissingBinaryMarksTheRowNeedsYtDlp() async {
    let session = makeSession(FakeLinkResolver { _ in throw ResolveError.binaryMissing })
    await session.ingest(urls: [URL(string: "https://youtu.be/x")!])
    #expect(await session.snapshot().mediaRows[0].state == .needsYtDlp)
}

@Test func aMuxChoiceWithNoFfmpegMarksTheRowNeedsFfmpeg() async {
    let resolver = FakeLinkResolver { _ in
        singleMedia(
            videoID: "abc", title: "Song",
            formats: [vf("137", 1080, .h264, .mp4), af("140", .aac, .m4a)])
    }
    let session = GrabberSession(
        prober: LinkProber(transport: FakeProbeOrigin(), deepSniffEnabled: false),
        resolver: resolver, ffmpegAvailable: { false })
    await session.ingest(urls: [URL(string: "https://youtu.be/abc")!])
    #expect(await session.snapshot().mediaRows[0].state == .needsFfmpeg)
}

@Test func manuallyChoosingAFormatResolvesAnUnselectedRow() async {
    let resolver = FakeLinkResolver { _ in
        singleMedia(videoID: "abc", title: "Song", formats: [vf("137", 1080, .h264, .mp4)])
    }
    let session = makeSession(resolver)
    await session.ingest(urls: [URL(string: "https://youtu.be/abc")!])
    let id = await session.snapshot().mediaRows[0].id
    await session.setFormatChoice(
        FormatChoice(
            video: vf("137", 1080, .h264, .mp4), audio: nil, outputContainer: .mp4,
            estimatedBytes: 1000), for: id)
    #expect(await session.snapshot().mediaRows[0].state == .resolved)
}
