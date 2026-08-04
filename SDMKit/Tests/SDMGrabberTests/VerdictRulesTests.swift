import Foundation
import Testing

@testable import SDMGrabber

private func link(
    statusCode: Int? = 200,
    contentLength: Int64? = 5_000_000,
    contentType: String? = "video/mp4",
    filename: String = "movie.mp4",
    signature: FileSignature? = nil,
    transportFailed: Bool = false,
    originalHost: String = "example.com",
    finalHost: String = "example.com",
    finalPath: String = "/movie.mp4"
) -> ProbedLink {
    var probed = ProbedLink(
        originalURL: URL(string: "https://\(originalHost)/movie.mp4")!,
        finalURL: URL(string: "https://\(finalHost)\(finalPath)")!,
        stage: .done,
        statusCode: statusCode,
        contentLength: contentLength,
        contentType: contentType,
        suggestedFilename: filename,
        sniffedSignature: signature,
        transportFailed: transportFailed
    )
    probed.suggestedFilename = filename
    return probed
}

@Test func onlineForAPlausibleSuccessfulResponse() {
    #expect(VerdictRules.evaluate(link()) == .online)
}

@Test func offlineForNon2xxStatus() {
    #expect(VerdictRules.evaluate(link(statusCode: 404)) == .offline)
}

@Test func faultyForHTMLBodyOnAMediaExtension() {
    #expect(
        VerdictRules.evaluate(link(contentType: "text/html")) == .faulty(reason: "html, not mp4")
    )
}

@Test func faultyForImplausiblySmallSize() {
    #expect(
        VerdictRules.evaluate(link(contentLength: 100, filename: "archive.zip"))
            == .faulty(reason: "too small to be a real zip")
    )
}

@Test func faultyForSignatureContradictingExtension() {
    #expect(
        VerdictRules.evaluate(link(signature: .html))
            == .faulty(reason: "file signature does not match .mp4")
    )
}

@Test func checkFailedWhenTheTransportNeverConnected() {
    #expect(VerdictRules.evaluate(link(transportFailed: true)) == .checkFailed)
}

@Test func checkFailedWhenStatusWasNeverCaptured() {
    #expect(VerdictRules.evaluate(link(statusCode: nil)) == .checkFailed)
}

@Test func faultyForRedirectToASuspiciousPathOnADifferentHost() {
    let verdict = VerdictRules.evaluate(
        link(
            contentType: "application/octet-stream",
            finalHost: "sketchy-cdn.example",
            finalPath: "/link-expired"
        )
    )
    #expect(verdict == .faulty(reason: "redirected to sketchy-cdn.example"))
}

@Test func onlineForASmallNonMediaFile() {
    #expect(
        VerdictRules.evaluate(
            link(contentLength: 50, contentType: "text/plain", filename: "notes.txt")
        ) == .online
    )
}
