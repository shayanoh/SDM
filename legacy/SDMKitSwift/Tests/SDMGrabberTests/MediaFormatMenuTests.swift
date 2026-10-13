import Foundation
import Testing

@testable import SDMCore
@testable import SDMGrabber

private func media(_ formats: [MediaFormat]) -> ResolvedMedia {
    ResolvedMedia(
        extractor: "youtube", videoID: "abc", title: "T", durationSeconds: 60, formats: formats)
}

@Test func matchingOptionsComeFirstRankedByResolutionThenCodec() {
    let m = media([
        vf("v720av1", 720, .av1, .webm),
        vf("v1080vp9", 1080, .vp9, .webm),
        vf("v2160av1", 2160, .av1, .mp4),
        af("a", .opus, .webm),
    ])
    let opts = MediaFormatMenu.options(for: m, preferences: .default)
    #expect(opts.first?.choice.video?.id == "v1080vp9")
    #expect(opts.map(\.matchesPreferences) == [true, true, false])
    #expect(opts.last?.choice.video?.id == "v2160av1")
}

@Test func progressiveFormatsAppearWithNoAudioPairing() {
    let prog = MediaFormat(
        id: "18", kind: .progressive, height: 360, width: 640, vcodec: .h264, acodec: .aac,
        container: .mp4, filesize: 5_000_000, filesizeApprox: nil, tbr: nil,
        url: URL(string: "https://gv/18")!)
    let opts = MediaFormatMenu.options(for: media([prog]), preferences: .default)
    #expect(opts.count == 1)
    #expect(opts[0].choice.audio == nil)
    #expect(opts[0].label.contains("360p"))
}

@Test func aVideoOnlyOptionWithNoEligibleAudioStillShowsMarkedNonMatching() {
    var prefs = QualityPreferences.default
    prefs.audioCodecs = [.opus]
    let m = media([vf("v", 1080, .h264, .mp4), af("aac140", .aac, .m4a)])
    let opts = MediaFormatMenu.options(for: m, preferences: prefs)
    #expect(opts.count == 1)
    #expect(opts[0].matchesPreferences == false)
    #expect(opts[0].choice.audio?.id == "aac140")
}

@Test func hlsProgressiveFormatsAreOnlyStreamedRowsNeverDirect() {
    // Twitch: every quality is a progressive `m3u8_native` format. None of
    // them may show up as a direct-download row whose URL is the .m3u8.
    func hlsProg(_ id: String, _ h: Int) -> MediaFormat {
        MediaFormat(
            id: id, kind: .progressive, height: h, width: nil, vcodec: .h264,
            acodec: .aac, container: .mp4, filesize: nil, filesizeApprox: nil, tbr: Double(h),
            url: URL(string: "https://x/\(id)/index.m3u8")!, delivery: .hls)
    }
    let m = media([hlsProg("1080p60", 1080), hlsProg("480p", 480), hlsProg("160p", 160)])
    let opts = MediaFormatMenu.options(for: m, preferences: .default)

    #expect(opts.count == 3)
    #expect(opts.allSatisfy { $0.isWholesale })
    #expect(opts.allSatisfy { $0.choice.wholesaleSelector != nil })
    // The selector is yt-dlp's format id, not a URL.
    #expect(opts.contains { $0.choice.wholesaleSelector == "1080p60" })
}

@Test func hlsVariantsAppearAsStreamedOptionsAfterDirectOnes() {
    let hls1080 = MediaFormat(
        id: "270", kind: .videoOnly, height: 1080, width: nil, vcodec: .h264, acodec: nil,
        container: .mp4, filesize: nil, filesizeApprox: nil, tbr: 3000,
        url: URL(string: "https://x/270.m3u8")!, delivery: .hls)
    let m = media([vf("v", 720, .h264, .mp4), af("a", .aac, .m4a), hls1080])
    let opts = MediaFormatMenu.options(for: m, preferences: .default)
    #expect(opts.first?.isWholesale == false)
    #expect(opts.last?.isWholesale == true)
    #expect(opts.last?.choice.wholesaleSelector == "270+ba/b")
    #expect(opts.last?.label.contains("streamed") == true)
}

@Test func labelMarksApproximateSizes() {
    let v = MediaFormat(
        id: "v", kind: .videoOnly, height: 1080, width: 1920, vcodec: .av1, acodec: nil,
        container: .webm, filesize: nil, filesizeApprox: 80_000_000, tbr: 3000,
        url: URL(string: "https://gv/v")!)
    let opts = MediaFormatMenu.options(
        for: media([v, af("a", .opus, .webm)]), preferences: .default)
    #expect(opts[0].label.contains("~"))
}
