import Foundation
import Testing

@testable import SDMCore
@testable import SDMResolve

@Test func mapsMuxedVideoDumpToResolvedMediaWithDirectFormatsOnly() throws {
    let media = try YtDlpParser.resolvedMedia(from: fixtureDump("video_muxed"))
    #expect(media.videoID == "dQw4w9WgXcQ")
    #expect(media.extractor == "youtube")
    #expect(media.durationSeconds == 213.0)
    let ids = media.formats.map(\.id).sorted()
    #expect(ids == ["137", "140", "18", "248", "251", "399"])
}

@Test func classifiesStreamKinds() throws {
    let media = try YtDlpParser.resolvedMedia(from: fixtureDump("video_muxed"))
    #expect(media.formats.first { $0.id == "137" }?.kind == .videoOnly)
    #expect(media.formats.first { $0.id == "251" }?.kind == .audioOnly)
    #expect(media.formats.first { $0.id == "18" }?.kind == .progressive)
}

@Test func mapsCodecsAndContainers() throws {
    let media = try YtDlpParser.resolvedMedia(from: fixtureDump("video_muxed"))
    #expect(media.formats.first { $0.id == "399" }?.vcodec == .av1)
    #expect(media.formats.first { $0.id == "248" }?.vcodec == .vp9)
    #expect(media.formats.first { $0.id == "137" }?.vcodec == .h264)
    #expect(media.formats.first { $0.id == "251" }?.acodec == .opus)
    #expect(media.formats.first { $0.id == "140" }?.acodec == .aac)
    #expect(media.formats.first { $0.id == "248" }?.container == .webm)
}

@Test func progressiveOnlyDumpStillResolves() throws {
    let media = try YtDlpParser.resolvedMedia(from: fixtureDump("video_progressive_only"))
    #expect(media.formats.count == 1)
    #expect(media.formats[0].kind == .progressive)
}

@Test func hlsOnlyDumpThrowsUnsupported() throws {
    #expect(throws: ResolveError.unsupported) {
        try YtDlpParser.resolvedMedia(from: fixtureDump("video_hls_only"))
    }
}
