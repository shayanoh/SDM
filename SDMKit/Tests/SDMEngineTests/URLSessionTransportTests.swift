import Foundation
import SDMCore
import Testing

@testable import SDMEngine

/// `URLSessionTransport.fetch` itself cannot be exercised here — no test may
/// touch the network. What *can* be tested is every decision the transport
/// makes at the HTTP boundary, which is why those decisions live in `static`
/// functions over values rather than inline in the streaming path.
/// `HTTPURLResponse` is constructible directly from header fields, so response
/// interpretation is fully covered without a socket.

private func response(
    status: Int = 200,
    headers: [String: String] = [:]
) -> HTTPURLResponse {
    HTTPURLResponse(
        url: testSourceURL,
        statusCode: status,
        httpVersion: "HTTP/1.1",
        headerFields: headers
    )!
}

// MARK: - Range header construction

@Test func rangeHeaderConvertsHalfOpenToInclusive() {
    #expect(
        URLSessionTransport.rangeHeaderValue(for: ByteRange(start: 0, end: 1)) == "bytes=0-0"
    )
    #expect(
        URLSessionTransport.rangeHeaderValue(for: ByteRange(start: 100, end: 200))
            == "bytes=100-199"
    )
    // Int64 offsets: a range past 4 GB must not be truncated to 32 bits.
    #expect(
        URLSessionTransport.rangeHeaderValue(
            for: ByteRange(start: 5_000_000_000, end: 5_000_001_000)
        ) == "bytes=5000000000-5000000999"
    )
}

@Test func noRangeHeaderForWholeResourceOrEmptyRange() {
    #expect(URLSessionTransport.rangeHeaderValue(for: nil) == nil)
    // An empty claim would otherwise become "bytes=10-9", which is malformed.
    #expect(URLSessionTransport.rangeHeaderValue(for: ByteRange(start: 10, end: 10)) == nil)
}

@Test func requestCarriesTheRangeHeaderAndBypassesTheCache() {
    let request = URLSessionTransport.makeURLRequest(
        for: RangeRequest(url: testSourceURL, range: ByteRange(start: 8, end: 16))
    )
    #expect(request.value(forHTTPHeaderField: "Range") == "bytes=8-15")
    #expect(request.cachePolicy == .reloadIgnoringLocalCacheData)
    #expect(request.url == testSourceURL)

    let whole = URLSessionTransport.makeURLRequest(for: RangeRequest(url: testSourceURL))
    #expect(whole.value(forHTTPHeaderField: "Range") == nil)
}

// MARK: - Total size selection

@Test func contentRangeTotalWinsOverContentLength() {
    let http = response(
        status: 206,
        headers: [
            "Content-Length": "1024",
            "Content-Range": "bytes 0-1023/1048576",
        ]
    )
    #expect(URLSessionTransport.totalSize(from: http) == 1_048_576)
}

@Test func contentLengthIsUsedWhenNoContentRangeIsOffered() {
    #expect(
        URLSessionTransport.totalSize(from: response(headers: ["Content-Length": "4096"]))
            == 4096)
}

@Test func unknownTotalSizeIsReportedAsNil() {
    // No length at all — a chunked response.
    #expect(URLSessionTransport.totalSize(from: response()) == nil)
    // A wildcard Content-Range total falls back to Content-Length, which is
    // also absent here.
    #expect(
        URLSessionTransport.totalSize(from: response(headers: ["Content-Range": "bytes 0-9/*"]))
            == nil)
}

@Test func wildcardContentRangeFallsBackToContentLength() {
    let http = response(
        status: 206,
        headers: ["Content-Range": "bytes 0-9/*", "Content-Length": "10"]
    )
    #expect(URLSessionTransport.totalSize(from: http) == 10)
}

@Test func multiGigabyteTotalSizeSurvivesAsInt64() {
    let http = response(
        status: 206,
        headers: ["Content-Range": "bytes 0-0/9663676416"]
    )
    #expect(URLSessionTransport.totalSize(from: http) == 9_663_676_416)
}

// MARK: - Resumability and validator

@Test func partialContentImpliesRangeSupport() {
    #expect(URLSessionTransport.acceptsRanges(from: response(status: 206)))
}

@Test func acceptRangesHeaderImpliesRangeSupportOnA200() {
    #expect(URLSessionTransport.acceptsRanges(from: response(headers: ["Accept-Ranges": "bytes"])))
    #expect(URLSessionTransport.acceptsRanges(from: response(headers: ["Accept-Ranges": "Bytes"])))
    #expect(!URLSessionTransport.acceptsRanges(from: response(headers: ["Accept-Ranges": "none"])))
    #expect(!URLSessionTransport.acceptsRanges(from: response()))
}

@Test func etagIsPreferredOverLastModifiedAsTheValidator() {
    let both = response(headers: [
        "ETag": "\"abc\"", "Last-Modified": "Mon, 03 Aug 2026 00:00:00 GMT",
    ])
    #expect(URLSessionTransport.validator(from: both) == "\"abc\"")

    let onlyDate = response(headers: ["Last-Modified": "Mon, 03 Aug 2026 00:00:00 GMT"])
    #expect(URLSessionTransport.validator(from: onlyDate) == "Mon, 03 Aug 2026 00:00:00 GMT")

    #expect(URLSessionTransport.validator(from: response()) == nil)
}

// MARK: - Session configuration

@Test func defaultConfigurationNeverTouchesTheSharedCache() {
    let configuration = URLSessionTransport.defaultConfiguration
    #expect(configuration.requestCachePolicy == .reloadIgnoringLocalCacheData)
    #expect(configuration.urlCache == nil)
}
