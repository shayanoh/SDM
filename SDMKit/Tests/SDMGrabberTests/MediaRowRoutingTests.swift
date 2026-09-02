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
