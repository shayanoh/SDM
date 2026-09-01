import Foundation
import Testing

@testable import SDMCore

@Test func videoCodecPriorityOrdersAv1FirstH264Last() {
    #expect(VideoCodec.av1.rank < VideoCodec.vp9.rank)
    #expect(VideoCodec.vp9.rank < VideoCodec.h264.rank)
    #expect(VideoCodec.h264.rank < VideoCodec.other("theora").rank)
}

@Test func containerPriorityOrdersMp4BeforeWebm() {
    #expect(MediaContainer.mp4.rank < MediaContainer.webm.rank)
    #expect(MediaContainer.webm.rank < MediaContainer.other("mkv").rank)
}

@Test func audioCodecPriorityOrdersOpusBeforeAac() {
    #expect(AudioCodec.opus.rank < AudioCodec.aac.rank)
}

@Test func filesizeEffectivePrefersExactThenApproxThenNil() {
    let exact = MediaFormat(
        id: "137", kind: .videoOnly, height: 1080, width: 1920, vcodec: .h264,
        acodec: nil, container: .mp4, filesize: 100, filesizeApprox: 90, tbr: 4000,
        url: URL(string: "https://x/1")!)
    #expect(exact.filesizeEffective == 100)
    #expect(exact.isApproximateSize == false)

    let approx = MediaFormat(
        id: "248", kind: .videoOnly, height: 1080, width: 1920, vcodec: .vp9,
        acodec: nil, container: .webm, filesize: nil, filesizeApprox: 90, tbr: 3800,
        url: URL(string: "https://x/2")!)
    #expect(approx.filesizeEffective == 90)
    #expect(approx.isApproximateSize == true)

    let unknown = MediaFormat(
        id: "600", kind: .videoOnly, height: 1080, width: 1920, vcodec: .av1,
        acodec: nil, container: .mp4, filesize: nil, filesizeApprox: nil, tbr: nil,
        url: URL(string: "https://x/3")!)
    #expect(unknown.filesizeEffective == nil)
}

@Test func mediaContainerFileExtension() {
    #expect(MediaContainer.mp4.fileExtension == "mp4")
    #expect(MediaContainer.other("mkv").fileExtension == "mkv")
}
