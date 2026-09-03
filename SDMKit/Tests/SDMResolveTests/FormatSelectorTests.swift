import Foundation
import Testing

@testable import SDMCore
@testable import SDMResolve

private func vf(
    _ id: String, _ height: Int, _ vcodec: VideoCodec, _ container: MediaContainer,
    size: Int64 = 1000, tbr: Double = 1000, progressive: Bool = false, acodec: AudioCodec? = nil
) -> MediaFormat {
    MediaFormat(
        id: id, kind: progressive ? .progressive : .videoOnly, height: height,
        width: height * 16 / 9, vcodec: vcodec, acodec: acodec, container: container,
        filesize: size, filesizeApprox: nil, tbr: tbr, url: URL(string: "https://gv/\(id)")!)
}

private func af(
    _ id: String, _ acodec: AudioCodec, _ container: MediaContainer,
    size: Int64 = 100, tbr: Double = 128
) -> MediaFormat {
    MediaFormat(
        id: id, kind: .audioOnly, height: nil, width: nil, vcodec: nil, acodec: acodec,
        container: container, filesize: size, filesizeApprox: nil, tbr: tbr,
        url: URL(string: "https://gv/\(id)")!)
}

private func media(_ formats: [MediaFormat]) -> ResolvedMedia {
    ResolvedMedia(
        extractor: "youtube", videoID: "abc", title: "T", durationSeconds: 10, formats: formats)
}

private func hls(_ id: String, _ h: Int, tbr: Double = 0) -> MediaFormat {
    MediaFormat(
        id: id, kind: .videoOnly, height: h, width: nil, vcodec: .h264, acodec: nil,
        container: .mp4, filesize: nil, filesizeApprox: nil, tbr: tbr == 0 ? Double(h) : tbr,
        url: URL(string: "https://x/\(id).m3u8")!, delivery: .hls)
}

@Test func directFormatsWinOverHls() {
    let m = media([
        vf("137", 720, .h264, .mp4), af("140", .aac, .m4a), hls("270", 1080),
    ])
    let choice = FormatSelector.pick(m, .default)
    #expect(choice?.isWholesale == false)
    #expect(choice?.video?.id == "137")
}

@Test func hlsOnlyMediaYieldsWholesaleChoice() {
    let m = media([hls("270", 1080), hls("232", 720)])
    let choice = FormatSelector.pick(m, .default)
    #expect(choice?.isWholesale == true)
    #expect(choice?.wholesaleSelector?.contains("height<=1080") == true)
    #expect(choice?.outputContainer == .mp4)
    #expect(choice?.video?.id == "270")
}

@Test func hlsAboveTheCapStillDownloads() {
    let m = media([hls("2160", 2160)])
    let choice = FormatSelector.pick(m, .default)
    #expect(choice?.isWholesale == true)
}

@Test func nothingUsableYieldsNil() {
    #expect(FormatSelector.pick(media([]), .default) == nil)
}

@Test func picksHighestResolutionThenCodecThenContainer() {
    let m = media([
        vf("v720av1", 720, .av1, .webm),
        vf("v1080vp9", 1080, .vp9, .webm),
        vf("v1080h264", 1080, .h264, .mp4),
        af("a", .opus, .webm),
    ])
    let choice = FormatSelector.pick(m, .default)
    #expect(choice?.video?.id == "v1080vp9")
    #expect(choice?.audio?.id == "a")
    #expect(choice?.outputContainer == .webm)
    #expect(choice?.requiresMux == true)
}

@Test func progressiveFormatNeedsNoAudioAndNoMux() {
    let m = media([vf("prog", 720, .h264, .mp4, progressive: true, acodec: .aac)])
    let choice = FormatSelector.pick(m, .default)
    #expect(choice?.video?.id == "prog")
    #expect(choice?.audio == nil)
    #expect(choice?.requiresMux == false)
    #expect(choice?.outputContainer == .mp4)
}

@Test func maxHeightCapDropsHigherFormats() {
    var prefs = QualityPreferences.default
    prefs.maxHeight = 720
    let m = media([
        vf("v2160", 2160, .av1, .mp4), vf("v720", 720, .h264, .mp4), af("a", .aac, .m4a),
    ])
    #expect(FormatSelector.pick(m, prefs)?.video?.id == "v720")
}

@Test func codecAllowlistExcludingTopFormat() {
    var prefs = QualityPreferences.default
    prefs.videoCodecs = [.h264]
    let m = media([vf("av1", 1080, .av1, .mp4), vf("h264", 1080, .h264, .mp4), af("a", .aac, .m4a)])
    #expect(FormatSelector.pick(m, prefs)?.video?.id == "h264")
}

@Test func videoOnlyWithNoEligibleAudioReturnsNil() {
    var prefs = QualityPreferences.default
    prefs.audioCodecs = [.opus]
    let m = media([vf("v", 1080, .av1, .webm), af("a", .aac, .m4a)])
    #expect(FormatSelector.pick(m, prefs) == nil)
}

@Test func noEligibleVideoButVideosExistReturnsNil() {
    var prefs = QualityPreferences.default
    prefs.maxHeight = 144
    let m = media([vf("v1080", 1080, .av1, .webm), af("a", .opus, .webm)])
    #expect(FormatSelector.pick(m, prefs) == nil)
}

@Test func audioOnlyMediaPicksBestAudio() {
    let m = media([af("aac", .aac, .m4a), af("opus", .opus, .webm)])
    let choice = FormatSelector.pick(m, .default)
    #expect(choice?.video == nil)
    #expect(choice?.audio?.id == "opus")
    #expect(choice?.outputContainer == .webm)
}

@Test func estimatedBytesNilWhenAComponentSizeUnknown() {
    let vNoSize = MediaFormat(
        id: "v", kind: .videoOnly, height: 1080, width: 1920, vcodec: .av1, acodec: nil,
        container: .webm, filesize: nil, filesizeApprox: nil, tbr: nil,
        url: URL(string: "https://gv/v")!)
    let m = ResolvedMedia(
        extractor: "youtube", videoID: "abc", title: "T", durationSeconds: 10,
        formats: [vNoSize, af("a", .opus, .webm)])
    #expect(FormatSelector.pick(m, .default)?.estimatedBytes == nil)
}
