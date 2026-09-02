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
    if case .resolved(_, let videoID, let formatID) = items[0].components[0].origin {
        #expect(videoID == "abc")
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
