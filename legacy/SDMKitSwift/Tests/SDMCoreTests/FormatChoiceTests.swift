import Foundation
import Testing

@testable import SDMCore

@Test func directChoiceIsNotWholesale() {
    let c = FormatChoice(
        video: nil, audio: nil, outputContainer: .mp4, estimatedBytes: nil)
    #expect(!c.isWholesale)
}

@Test func wholesaleChoiceReportsItselfAndSkipsMux() {
    let v = MediaFormat(
        id: "270", kind: .videoOnly, height: 1080, width: nil, vcodec: .h264,
        acodec: nil, container: .mp4, filesize: nil, filesizeApprox: nil, tbr: nil,
        url: URL(string: "https://x/m.m3u8")!, delivery: .hls)
    let a = MediaFormat(
        id: "233", kind: .audioOnly, height: nil, width: nil, vcodec: nil,
        acodec: .aac, container: .m4a, filesize: nil, filesizeApprox: nil, tbr: nil,
        url: URL(string: "https://x/a.m3u8")!, delivery: .hls)
    let c = FormatChoice(
        video: v, audio: a, outputContainer: .mp4, estimatedBytes: nil,
        wholesaleSelector: "bv*+ba/b")
    #expect(c.isWholesale)
    #expect(!c.requiresMux)
}

@Test func wholesaleSelectorRoundTripsThroughCodable() throws {
    let c = FormatChoice(
        video: nil, audio: nil, outputContainer: .webm, estimatedBytes: 42,
        wholesaleSelector: "233+ba/b")
    let data = try JSONEncoder().encode(c)
    #expect(try JSONDecoder().decode(FormatChoice.self, from: data) == c)
}
