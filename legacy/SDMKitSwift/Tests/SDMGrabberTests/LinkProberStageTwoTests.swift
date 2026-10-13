import Foundation
import SDMCore
import Testing

@testable import SDMGrabber

private let url = URL(string: "https://example.com/movie.mp4")!

@Test func sniffCapturesMagicBytesForASuccessfulLink() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.statusCode = 200
    behavior.body = Data([0x50, 0x4B, 0x03, 0x04, 0x00, 0x00])
    await origin.setBehavior(behavior, for: url)

    let link = await LinkProber(transport: origin, deepSniffEnabled: true).probe(url)
    #expect(link.sniffedSignature == .zip)
    let log = await origin.requestLog
    #expect(log.map(\.method) == [.head, .get])
}

@Test func sniffSkippedWhenDeepSniffDisabled() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.statusCode = 200
    behavior.body = Data([0x50, 0x4B, 0x03, 0x04])
    await origin.setBehavior(behavior, for: url)

    let link = await LinkProber(transport: origin, deepSniffEnabled: false).probe(url)
    #expect(link.sniffedSignature == nil)
    let log = await origin.requestLog
    #expect(log.map(\.method) == [.head])
}

@Test func sniffSkippedWhenStatusIsNotSuccessful() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.statusCode = 404
    await origin.setBehavior(behavior, for: url)

    let link = await LinkProber(transport: origin, deepSniffEnabled: true).probe(url)
    #expect(link.sniffedSignature == nil)
    let log = await origin.requestLog
    #expect(log.map(\.method) == [.head])
}
