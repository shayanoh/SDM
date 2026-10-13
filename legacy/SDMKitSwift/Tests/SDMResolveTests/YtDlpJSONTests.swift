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

@Test func deliveryTagging() {
    #expect(YtDlpParser.deliveryFor(proto: "https") == .direct)
    #expect(YtDlpParser.deliveryFor(proto: nil) == .direct)
    #expect(YtDlpParser.deliveryFor(proto: "https_native") == .direct)
    #expect(YtDlpParser.deliveryFor(proto: "m3u8_native") == .hls)
    #expect(YtDlpParser.deliveryFor(proto: "m3u8") == .hls)
    #expect(YtDlpParser.deliveryFor(proto: "http_dash_segments") == .dash)
    #expect(YtDlpParser.deliveryFor(proto: "rtmp") == nil)
    #expect(YtDlpParser.deliveryFor(proto: "ism") == nil)
}

@Test func hlsOnlyDumpMapsHlsFormatsInsteadOfRejecting() throws {
    let media = try YtDlpParser.resolvedMedia(from: fixtureDump("video_hls_only"))
    #expect(media.formats.contains { $0.id == "270" && $0.delivery == .hls })
    // The audio+video "none/none" manifest entry is still rejected.
    #expect(!media.formats.contains { $0.id == "233" })
}

@Test func codecLessTubeFormatsAreNotRejected() throws {
    // xnxx / xvideos omit vcodec/acodec even for downloadable muxed streams.
    let media = try YtDlpParser.resolvedMedia(from: fixtureDump("video_tube_no_codecs"))
    #expect(media.formats.count == 5)
    // `low` / `high`: no height, ext mp4, direct → progressive direct.
    let low = try #require(media.formats.first { $0.id == "low" })
    #expect(low.kind == .progressive)
    #expect(low.delivery == .direct)
    // hls-1080p: has height, m3u8 → progressive hls.
    let hd = try #require(media.formats.first { $0.id == "hls-1080p" })
    #expect(hd.kind == .progressive)
    #expect(hd.delivery == .hls)
}

@Test func directDumpTagsEveryFormatDirect() throws {
    let media = try YtDlpParser.resolvedMedia(from: fixtureDump("video_direct_vimeo"))
    #expect(media.formats.allSatisfy { $0.delivery == .direct })
    #expect(media.formats.count == 3)
}
