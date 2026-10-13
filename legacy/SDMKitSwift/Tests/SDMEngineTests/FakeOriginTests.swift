import Foundation
import SDMCore
import Testing

@testable import SDMEngine

private let url = URL(string: "https://example.com/file.bin")!

private func payload(_ count: Int) -> Data {
    Data((0..<count).map { UInt8($0 % 251) })
}

private func collect(_ response: RangeResponse) async throws -> Data {
    var data = Data()
    for try await chunk in response.body { data.append(chunk) }
    return data
}

@Test func fullFetchReturnsWholePayload() async throws {
    let origin = FakeOrigin(payload: payload(1000))
    let response = try await origin.fetch(RangeRequest(url: url, range: nil))
    #expect(response.statusCode == 200)
    #expect(response.totalSize == 1000)
    #expect(response.acceptsRanges)
    #expect(try await collect(response) == payload(1000))
}

@Test func rangedFetchReturnsOnlyThatSlice() async throws {
    let origin = FakeOrigin(payload: payload(1000))
    let response = try await origin.fetch(
        RangeRequest(url: url, range: ByteRange(start: 100, end: 200))
    )
    #expect(response.statusCode == 206)
    #expect(try await collect(response) == payload(1000)[100..<200])
}

@Test func originIgnoringRangesReturnsWholeBody() async throws {
    var behavior = FakeOrigin.Behavior()
    behavior.ignoresRanges = true
    let origin = FakeOrigin(payload: payload(1000), behavior: behavior)
    let response = try await origin.fetch(
        RangeRequest(url: url, range: ByteRange(start: 100, end: 200))
    )
    #expect(response.statusCode == 200)
    #expect(!response.acceptsRanges)
    #expect(try await collect(response).count == 1000)
}

@Test func originDropsConnectionAtConfiguredOffset() async throws {
    var behavior = FakeOrigin.Behavior()
    behavior.dropAfterBytes = 40
    let origin = FakeOrigin(payload: payload(1000), behavior: behavior)
    let response = try await origin.fetch(RangeRequest(url: url, range: nil))
    await #expect(throws: TransportError.connectionDropped) {
        _ = try await collect(response)
    }
}

@Test func originCanReportWrongContentLength() async throws {
    var behavior = FakeOrigin.Behavior()
    behavior.reportedSizeOverride = 5000
    let origin = FakeOrigin(payload: payload(1000), behavior: behavior)
    let response = try await origin.fetch(RangeRequest(url: url, range: nil))
    #expect(response.totalSize == 5000)
}

@Test func originCanReturnErrorStatus() async throws {
    var behavior = FakeOrigin.Behavior()
    behavior.statusOverride = 403
    let origin = FakeOrigin(payload: payload(1000), behavior: behavior)
    let response = try await origin.fetch(RangeRequest(url: url, range: nil))
    #expect(response.statusCode == 403)
}

@Test func originValidatorCanChangeBetweenRequests() async throws {
    var behavior = FakeOrigin.Behavior()
    behavior.validator = "etag-1"
    let origin = FakeOrigin(payload: payload(1000), behavior: behavior)
    let first = try await origin.fetch(RangeRequest(url: url, range: nil))
    #expect(first.validator == "etag-1")
    await origin.setValidator("etag-2")
    let second = try await origin.fetch(RangeRequest(url: url, range: nil))
    #expect(second.validator == "etag-2")
}

@Test func originRecordsRequestedRanges() async throws {
    let origin = FakeOrigin(payload: payload(1000))
    _ = try await origin.fetch(RangeRequest(url: url, range: ByteRange(start: 0, end: 10)))
    _ = try await origin.fetch(RangeRequest(url: url, range: ByteRange(start: 10, end: 20)))
    let requested = await origin.requestedRanges
    #expect(requested == [ByteRange(start: 0, end: 10), ByteRange(start: 10, end: 20)])
}
