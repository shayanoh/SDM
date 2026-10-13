import Foundation
import SDMCore
import Testing

@testable import SDMGrabber

private let url = URL(string: "https://example.com/movie.mp4")!

@Test func probeCapturesHeadResponseFields() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.statusCode = 200
    behavior.headers = [
        "content-length": "5000000",
        "content-type": "video/mp4; charset=binary",
        "etag": "\"abc123\"",
        "content-disposition": "attachment; filename=\"Real Movie.mp4\"",
    ]
    await origin.setBehavior(behavior, for: url)

    let link = await LinkProber(transport: origin, deepSniffEnabled: false).probe(url)
    #expect(link.statusCode == 200)
    #expect(link.contentLength == 5_000_000)
    #expect(link.contentType == "video/mp4")
    #expect(link.validator == "\"abc123\"")
    #expect(link.suggestedFilename == "Real Movie.mp4")
    #expect(link.acceptsRanges == false)
    #expect(link.stage == .done)
    #expect(link.transportFailed == false)
}

@Test func probeFallsBackToRangedGetWhenHeadIsRejected() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.rejectsHead = true
    behavior.statusCode = 206
    await origin.setBehavior(behavior, for: url)

    let link = await LinkProber(transport: origin, deepSniffEnabled: false).probe(url)
    #expect(link.acceptsRanges == true)
    let log = await origin.requestLog
    #expect(log.map(\.method) == [.head, .get])
}

@Test func probeCapturesTotalFromContentRangeOnFallbackGet() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.rejectsHead = true
    behavior.statusCode = 206
    behavior.headers = ["content-range": "bytes 0-0/123456", "content-length": "1"]
    await origin.setBehavior(behavior, for: url)

    let link = await LinkProber(transport: origin, deepSniffEnabled: false).probe(url)
    #expect(link.contentLength == 123456)
}

@Test func probePreservesStatusCodeVerbatim() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.statusCode = 404
    await origin.setBehavior(behavior, for: url)

    let link = await LinkProber(transport: origin, deepSniffEnabled: false).probe(url)
    #expect(link.statusCode == 404)
    #expect(link.stage == .done)
}

@Test func probeMarksTransportFailureWhenBothAttemptsThrow() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.error = .timedOut
    await origin.setBehavior(behavior, for: url)

    let link = await LinkProber(transport: origin, deepSniffEnabled: false).probe(url)
    #expect(link.transportFailed == true)
    #expect(link.statusCode == nil)
    #expect(link.stage == .done)
}
