import Foundation
import SDMCore
import Testing

@testable import SDMGrabber

private let url = URL(string: "https://example.com/file.zip")!

@Test func requestUsesHeadMethodWithNoRangeHeader() {
    let request = URLSessionProbeTransport.makeURLRequest(
        for: ProbeRequest(url: url, method: .head)
    )
    #expect(request.httpMethod == "HEAD")
    #expect(request.value(forHTTPHeaderField: "Range") == nil)
}

@Test func requestUsesGetMethodWithRangeHeaderWhenProvided() {
    let request = URLSessionProbeTransport.makeURLRequest(
        for: ProbeRequest(url: url, method: .get, range: ByteRange(start: 0, end: 65536))
    )
    #expect(request.httpMethod == "GET")
    #expect(request.value(forHTTPHeaderField: "Range") == "bytes=0-65535")
}

@Test func requestCarriesNoRangeHeaderForNilRange() {
    let request = URLSessionProbeTransport.makeURLRequest(for: ProbeRequest(url: url, method: .get))
    #expect(request.value(forHTTPHeaderField: "Range") == nil)
}

@Test func headersAreLowercasedForCaseInsensitiveLookup() {
    let response = HTTPURLResponse(
        url: url,
        statusCode: 200,
        httpVersion: "HTTP/1.1",
        headerFields: ["Content-Type": "application/zip", "ETag": "\"abc\""]
    )!
    let headers = URLSessionProbeTransport.lowercasedHeaders(from: response)
    #expect(headers["content-type"] == "application/zip")
    #expect(headers["etag"] == "\"abc\"")
}

@Test func classifyMapsTimeoutAndDNSErrorCodes() {
    #expect(URLSessionProbeTransport.classify(.timedOut) == .timedOut)
    #expect(URLSessionProbeTransport.classify(.cannotFindHost) == .dnsFailure)
    #expect(URLSessionProbeTransport.classify(.dnsLookupFailed) == .dnsFailure)
}

@Test func classifyReturnsNilForUnrecognizedErrorCodes() {
    #expect(URLSessionProbeTransport.classify(.notConnectedToInternet) == nil)
}
