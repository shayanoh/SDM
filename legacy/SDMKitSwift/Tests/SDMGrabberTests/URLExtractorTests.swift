import Foundation
import Testing

@testable import SDMGrabber

@Test func extractsPlainURLFromProse() {
    let text = "Check this out: https://example.com/movie.mp4 nice right"
    #expect(
        URLExtractor.extractLinks(from: text) == [URL(string: "https://example.com/movie.mp4")!])
}

@Test func extractsMultipleDistinctURLsInOrder() {
    let text = "https://a.example.com/1.zip and https://b.example.com/2.zip"
    #expect(
        URLExtractor.extractLinks(from: text) == [
            URL(string: "https://a.example.com/1.zip")!,
            URL(string: "https://b.example.com/2.zip")!,
        ]
    )
}

@Test func dedupesRepeatedURL() {
    let text = "https://a.example.com/1.zip mirror: https://a.example.com/1.zip"
    #expect(URLExtractor.extractLinks(from: text) == [URL(string: "https://a.example.com/1.zip")!])
}

@Test func ignoresNonHTTPSchemes() {
    let text = "ftp://old.example.com/file.zip or mailto:me@example.com"
    #expect(URLExtractor.extractLinks(from: text).isEmpty)
}

@Test func handlesURLOnItsOwnLineWithinAParagraph() {
    let text = """
        Here's the download link:
        https://cdn.example.com/season1/show.s01e01.mkv
        Let me know if it works.
        """
    #expect(
        URLExtractor.extractLinks(from: text) == [
            URL(string: "https://cdn.example.com/season1/show.s01e01.mkv")!
        ]
    )
}

@Test func returnsEmptyForTextWithNoLinks() {
    #expect(URLExtractor.extractLinks(from: "just some text, nothing to see").isEmpty)
}

@Test func ignoresBareHostURLsWithNoPathOrQuery() {
    let text = """
        7.3.2.example.org
        https://something.com/
        http://another.com
        visit https://real.example/file.zip for the download
        https://cdn.example.com/?token=abc123
        https://www.youtube.com/@SomeCreator
        """
    #expect(
        URLExtractor.extractLinks(from: text) == [
            URL(string: "https://real.example/file.zip")!,
            URL(string: "https://cdn.example.com/?token=abc123")!,
            URL(string: "https://www.youtube.com/@SomeCreator")!,
        ])
}

@Test func isGrabbableRules() {
    #expect(!URLExtractor.isGrabbable(URL(string: "https://something.com")!))
    #expect(!URLExtractor.isGrabbable(URL(string: "https://something.com/")!))
    #expect(!URLExtractor.isGrabbable(URL(string: "http://7.3.2.example.org")!))
    #expect(URLExtractor.isGrabbable(URL(string: "https://x.com/f.mp4")!))
    #expect(URLExtractor.isGrabbable(URL(string: "https://x.com/?id=1")!))
    #expect(URLExtractor.isGrabbable(URL(string: "https://www.youtube.com/watch?v=abc")!))
}
