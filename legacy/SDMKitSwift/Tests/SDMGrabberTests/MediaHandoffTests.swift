import Foundation
import Testing

@testable import SDMCore
@testable import SDMGrabber

private func resolvedRow(mux: Bool) -> MediaRow {
    let v = vf("137", 1080, .h264, .mp4)
    let a = af("140", .aac, .m4a)
    var row = MediaRow(sourceURL: URL(string: "https://youtu.be/abc")!)
    row.media = ResolvedMedia(
        extractor: "youtube", videoID: "abc", title: "Clip", durationSeconds: 10, formats: [v, a])
    row.choice =
        mux
        ? FormatChoice(video: v, audio: a, outputContainer: .mp4, estimatedBytes: 1100)
        : FormatChoice(video: v, audio: nil, outputContainer: .mp4, estimatedBytes: 1000)
    row.state = .resolved
    return row
}

@Test func aMuxRowBecomesATwoComponentItem() {
    let (items, held) = MediaHandoff.build(httpLinks: [], mediaRows: [resolvedRow(mux: true)])
    #expect(held == 0)
    #expect(items.count == 1)
    #expect(items[0].components.count == 2)
    #expect(items[0].assembly == .mux)
    #expect(items[0].outputFilename == "Clip [abc].mp4")
    // The item's URL is the grabbed YouTube URL, not a googlevideo stream.
    #expect(items[0].url == URL(string: "https://youtu.be/abc")!)
    #expect(items[0].components[0].url.absoluteString.hasPrefix("https://gv/"))
    if case .resolved(let formatID) = items[0].components[0].origin {
        #expect(formatID == "137")
    } else {
        Issue.record("component 0 should be .resolved")
    }
    #expect(items[0].components[1].partFilename.contains(".f140."))
}

@Test func aProgressiveRowBecomesAOneComponentItemNamedItsFinalName() {
    let (items, _) = MediaHandoff.build(httpLinks: [], mediaRows: [resolvedRow(mux: false)])
    #expect(items[0].components.count == 1)
    #expect(items[0].assembly == .none)
    // No `.fNNN` for a single stream — the part file is the output file, so
    // nothing has to rename it and the item's destination matches disk.
    #expect(items[0].components[0].partFilename == items[0].outputFilename)
    #expect(items[0].outputFilename == "Clip [abc].mp4")
}

@Test func aWholesaleRowBecomesOneNonResumableComponent() {
    let hlsFmt = MediaFormat(
        id: "270", kind: .videoOnly, height: 1080, width: nil, vcodec: .h264, acodec: nil,
        container: .mp4, filesize: nil, filesizeApprox: nil, tbr: nil,
        url: URL(string: "https://x/m.m3u8")!, delivery: .hls)
    var row = MediaRow(sourceURL: URL(string: "https://www.tiktok.com/@a/video/1")!)
    row.media = ResolvedMedia(
        extractor: "tiktok", videoID: "1", title: "Clip", durationSeconds: 10, formats: [hlsFmt])
    row.choice = FormatChoice(
        video: hlsFmt, audio: nil, outputContainer: .mp4, estimatedBytes: nil,
        wholesaleSelector: "bv*+ba/b")
    row.state = .resolved

    let (items, held) = MediaHandoff.build(httpLinks: [], mediaRows: [row])
    #expect(held == 0)
    #expect(items.count == 1)
    #expect(items[0].components.count == 1)
    #expect(items[0].assembly == .none)
    #expect(items[0].components[0].isResumable == false)
    #expect(items[0].components[0].partFilename == items[0].outputFilename)
    #expect(items[0].url == URL(string: "https://www.tiktok.com/@a/video/1")!)
    if case .wholesale(let sel) = items[0].components[0].origin {
        #expect(sel == "bv*+ba/b")
    } else {
        Issue.record("expected .wholesale origin")
    }
}

@Test func mediaItemsCarryAMediaInfoString() {
    let (items, _) = MediaHandoff.build(httpLinks: [], mediaRows: [resolvedRow(mux: true)])
    #expect(items[0].metadata?.hasPrefix("Direct · 1080p · h264 · aac · mp4") == true)
}

@Test func httpItemsCarryReleaseTagsFromTheirFilename() {
    let tagged = ProbedLink(
        originalURL: URL(string: "https://x/The.Show.S01E01.1080p.WEB-DL.H.264-GRP.mkv")!,
        stage: .done, statusCode: 200, contentLength: 5000, verdict: .online)
    let plain = ProbedLink(
        originalURL: URL(string: "https://x/archive.zip")!, stage: .done, statusCode: 200,
        contentLength: 100, verdict: .online)
    let (items, _) = MediaHandoff.build(httpLinks: [tagged, plain], mediaRows: [])
    #expect(items[0].metadata == "1080p · WEB-DL · H.264")
    #expect(items[1].metadata == nil)
}

@Test func unselectedRowsAreHeldBackButHttpSiblingsGoThrough() {
    var unselected = MediaRow(sourceURL: URL(string: "https://youtu.be/x")!)
    unselected.state = .unselected
    let http = ProbedLink(
        originalURL: URL(string: "https://example.com/f.zip")!, stage: .done, statusCode: 200,
        contentLength: 500, verdict: .online)
    let (items, held) = MediaHandoff.build(httpLinks: [http], mediaRows: [unselected])
    #expect(held == 1)
    #expect(items.count == 1)
    #expect(items[0].components[0].origin == .http)
}
