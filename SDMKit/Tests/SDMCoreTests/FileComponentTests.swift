import Foundation
import Testing

@testable import SDMCore

private func url(_ s: String) -> URL { URL(string: s)! }

@Test func componentReportsCompletionAgainstItsOwnSize() {
    var c = FileComponent(url: url("https://x/v"), partFilename: "v.f137.mp4", totalBytes: 100)
    #expect(c.isComplete == false)
    #expect(c.fractionCompleted == 0)
    c.completed.insert(ByteRange(start: 0, end: 100))
    #expect(c.isComplete)
    #expect(c.fractionCompleted == 1.0)
}

@Test func componentWithUnknownSizeIsNeverComplete() {
    var c = FileComponent(url: url("https://x/v"), partFilename: "v.mp4")
    c.completed.insert(ByteRange(start: 0, end: 500))
    #expect(c.isComplete == false)
    #expect(c.fractionCompleted == 0)
}

@Test func componentOriginRoundTripsThroughCodable() throws {
    for origin in [ComponentOrigin.http, .resolved(formatID: "137")] {
        let data = try JSONEncoder().encode(origin)
        #expect(try JSONDecoder().decode(ComponentOrigin.self, from: data) == origin)
    }
}

@Test func legacyResolvedOriginDropsExtractorAndVideoIdKeepsFormatId() throws {
    // Old synthesized shape from state.json written before the slim.
    let legacy = Data(
        #"{"resolved":{"extractor":"youtube","videoID":"abc","formatID":"137"}}"#.utf8)
    #expect(
        try JSONDecoder().decode(ComponentOrigin.self, from: legacy) == .resolved(formatID: "137"))

    let legacyHttp = Data(#"{"http":{}}"#.utf8)
    #expect(try JSONDecoder().decode(ComponentOrigin.self, from: legacyHttp) == .http)
}

@Test func fileComponentRoundTripsThroughCodable() throws {
    var c = FileComponent(
        url: url("https://x/v"), partFilename: "v.f251.webm", totalBytes: 4096,
        origin: .resolved(formatID: "251"))
    c.completed.insert(ByteRange(start: 0, end: 1024))
    c.lastError = ComponentError("connection reset")
    let data = try JSONEncoder().encode(c)
    #expect(try JSONDecoder().decode(FileComponent.self, from: data) == c)
}

@Test func assemblyIsCodableAsAString() throws {
    #expect(try JSONDecoder().decode(Assembly.self, from: Data("\"mux\"".utf8)) == .mux)
}
