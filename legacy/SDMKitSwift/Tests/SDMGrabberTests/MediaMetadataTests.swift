import Foundation
import Testing

@testable import SDMCore
@testable import SDMGrabber

private func vfmt(
    _ id: String, _ h: Int, _ v: VideoCodec, _ c: MediaContainer, tbr: Double = 0,
    delivery: MediaDelivery = .direct, acodec: AudioCodec? = nil
) -> MediaFormat {
    MediaFormat(
        id: id, kind: acodec == nil ? .videoOnly : .progressive, height: h, width: h * 16 / 9,
        vcodec: v, acodec: acodec, container: c, filesize: nil, filesizeApprox: nil,
        tbr: tbr == 0 ? nil : tbr, url: URL(string: "https://x/\(id)")!, delivery: delivery)
}

private func afmt(_ id: String, _ a: AudioCodec, tbr: Double = 128) -> MediaFormat {
    MediaFormat(
        id: id, kind: .audioOnly, height: nil, width: nil, vcodec: nil, acodec: a,
        container: .m4a, filesize: nil, filesizeApprox: nil, tbr: tbr,
        url: URL(string: "https://x/\(id)")!)
}

private func media(_ formats: [MediaFormat], extractor: String = "youtube", duration: Double? = nil)
    -> ResolvedMedia
{
    ResolvedMedia(
        extractor: extractor, videoID: "abc", title: "Clip", durationSeconds: duration,
        formats: formats)
}

@Test func describesADirectMuxDownload() {
    let v = vfmt("137", 1080, .h264, .mp4, tbr: 4000)
    let a = afmt("140", .aac, tbr: 130)
    let choice = FormatChoice(video: v, audio: a, outputContainer: .mp4, estimatedBytes: nil)
    let s = MediaMetadata.describe(
        choice: choice, media: media([v, a], extractor: "youtube", duration: 213))
    #expect(s == "Direct · 1080p · h264 · aac · mp4 · 4.1 Mbps · 3:33 · youtube")
}

@Test func describesAStreamingWholesaleDownload() {
    let v = vfmt("270", 1080, .h264, .mp4, tbr: 2760, delivery: .hls)
    let choice = FormatChoice(
        video: v, audio: nil, outputContainer: .mp4, estimatedBytes: nil,
        wholesaleSelector: "bv*+ba/b")
    let s = MediaMetadata.describe(
        choice: choice, media: media([v], extractor: "twitch", duration: 3600))
    #expect(s == "Streaming · 1080p · h264 · mp4 · 2.8 Mbps · 1:00:00 · twitch")
}

@Test func omitsUnknownFieldsAndGenericExtractor() {
    let v = MediaFormat(
        id: "x", kind: .progressive, height: 720, width: nil, vcodec: nil, acodec: nil,
        container: .mp4, filesize: nil, filesizeApprox: nil, tbr: nil,
        url: URL(string: "https://x/x")!, delivery: .direct)
    let choice = FormatChoice(video: v, audio: nil, outputContainer: .mp4, estimatedBytes: nil)
    let s = MediaMetadata.describe(choice: choice, media: media([v], extractor: "generic"))
    #expect(s == "Direct · 720p · mp4")
}
