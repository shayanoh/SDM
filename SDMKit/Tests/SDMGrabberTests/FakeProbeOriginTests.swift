import Foundation
import SDMCore
import Testing

@testable import SDMGrabber

private let url = URL(string: "https://example.com/file.zip")!

@Test func headRequestReturnsConfiguredHeaders() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.statusCode = 200
    behavior.headers = ["content-length": "1000", "content-type": "application/zip"]
    await origin.setBehavior(behavior, for: url)

    let response = try await origin.send(ProbeRequest(url: url, method: .head))
    #expect(response.statusCode == 200)
    #expect(response.headers["content-length"] == "1000")
    #expect(response.body.isEmpty)
}

@Test func getRequestWithRangeReturnsSlice() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.body = Data((0..<100).map { UInt8($0) })
    await origin.setBehavior(behavior, for: url)

    let response = try await origin.send(
        ProbeRequest(url: url, method: .get, range: ByteRange(start: 10, end: 20))
    )
    #expect(response.body == Data((10..<20).map { UInt8($0) }))
}

@Test func rejectsHeadThrowsMalformedResponse() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.rejectsHead = true
    await origin.setBehavior(behavior, for: url)

    await #expect(throws: ProbeError.malformedResponse) {
        _ = try await origin.send(ProbeRequest(url: url, method: .head))
    }
}

@Test func defaultBehaviorIs404ForUnconfiguredURL() async throws {
    let origin = FakeProbeOrigin()
    let response = try await origin.send(ProbeRequest(url: url, method: .head))
    #expect(response.statusCode == 404)
}

@Test func requestLogRecordsEachRequestInOrder() async throws {
    let origin = FakeProbeOrigin()
    _ = try await origin.send(ProbeRequest(url: url, method: .head))
    _ = try await origin.send(
        ProbeRequest(url: url, method: .get, range: ByteRange(start: 0, end: 1)))
    let log = await origin.requestLog
    #expect(log.map(\.method) == [.head, .get])
}

@Test func holdsUntilReleasedBlocksUntilReleaseCalled() async throws {
    let origin = FakeProbeOrigin()
    var behavior = FakeProbeOrigin.Behavior()
    behavior.holdsUntilReleased = true
    await origin.setBehavior(behavior, for: url)

    let task = Task { try await origin.send(ProbeRequest(url: url, method: .head)) }
    while await origin.requestLog.isEmpty { await Task.yield() }
    // Still holding: the send call has registered but not returned.
    await origin.release(url)
    _ = try await task.value
}
