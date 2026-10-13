import Foundation
import Testing

@testable import SDMRust

// What happens to a URL string that crosses the boundary through Foundation.
// The core keeps the string it is given as the link's identity; Swift hands it
// back as `URL.absoluteString`. These tests record, against the real generated
// bindings, which strings survive that trip byte for byte and which Foundation
// rewrites (it percent-encodes spaces, non-ASCII, brackets and stray `%`, and
// converts an international host to punycode).

/// Strings Foundation leaves alone: the trip through Swift is the identity.
private let preserved = [
    "https://example.com/a%20b.bin",
    "https://example.com/%C3%BC.bin",
    "https://xn--exmple-cua.com/x",
    "https://EXAMPLE.com/X",
    "https://example.com:443/x",
    "https://example.com./x",
    "https://example.com/x#frag",
    "https://example.com/dir/",
    "https://example.com/x?",
    "https://example.com/a%5B1%5D.bin",
]

/// Strings Foundation rewrites, with what it makes of them.
private let rewritten: [(String, String)] = [
    ("https://example.com/a b.bin", "https://example.com/a%20b.bin"),
    ("https://example.com/ü.bin", "https://example.com/%C3%BC.bin"),
    ("https://exämple.com/x", "https://xn--exmple-cua.com/x"),
    ("https://example.com/x#frag ment", "https://example.com/x#frag%20ment"),
    ("https://example.com/a[1].bin", "https://example.com/a%5B1%5D.bin"),
    ("https://example.com/x?q=a b", "https://example.com/x?q=a%20b"),
    ("https://example.com/x?q=ü", "https://example.com/x?q=%C3%BC"),
    ("https://example.com/a%zz", "https://example.com/a%25zz"),
    // A literal %20 next to a character that needs encoding is encoded again.
    ("https://example.com/a b%20c ü", "https://example.com/a%20b%2520c%20%C3%BC"),
]

@Test func preservedStringsComeBackToRustByteForByte() throws {
    for text in preserved {
        // Rust holds `text` -> Swift URL -> back into Rust.
        let url = try sourceUrlParse(text: text)
        #expect(url.absoluteString == text, "\(text)")
        #expect(try sourceUrlText(url: url) == text, "\(text)")
        // Swift builds the URL itself.
        let built = try #require(URL(string: text))
        #expect(try sourceUrlText(url: built) == text, "\(text)")
    }
}

@Test func foundationRewritesAreStableSoTheSwiftRouteDeduplicates() throws {
    for (original, encoded) in rewritten {
        // One trip through Foundation yields its form...
        let url = try sourceUrlParse(text: original)
        #expect(url.absoluteString == encoded, "\(original)")
        #expect(try sourceUrlText(url: url) == encoded, "\(original)")
        // ...and that form is a fixed point: a second trip changes nothing,
        // so a link that entered the core as a Swift URL compares equal to
        // itself however often it crosses (the known-download comparison).
        let again = try sourceUrlParse(text: encoded)
        #expect(try sourceUrlText(url: again) == encoded, "\(encoded)")
        let built = try #require(URL(string: original))
        #expect(try sourceUrlText(url: built) == encoded, "\(original)")
    }
}

/// The text the link grabber extracts is handed to Swift first (`extractUrls`
/// returns `URL`s) and ingested as URLs, so the identity the core keeps is
/// the Foundation form from the start.
@Test func extractedLinksCarryTheFoundationForm() throws {
    let urls = try extractUrls(text: "see https://example.com/caf\u{e9}/a.zip please")
    let url = try #require(urls.first)
    #expect(try sourceUrlText(url: url) == url.absoluteString)
    #expect(!url.absoluteString.contains("\u{e9}"))
}

/// The stand-in for an address Foundation rejects must not come back as data.
@Test func theUnusablePlaceholderIsRefusedOnTheWayIn() throws {
    let placeholder = try #require(URL(string: "about:invalid"))
    #expect(throws: CoreError.self) { _ = try sourceUrlText(url: placeholder) }
    #expect(throws: CoreError.self) {
        _ = try downloadItemForUrl(
            url: placeholder, filename: "x.bin", totalBytes: nil, state: .queued)
    }
}
