import Foundation
import Testing

@testable import SDMGrabber

@Test func detectsZipMagicBytes() {
    #expect(FileSignature.detect(in: Data([0x50, 0x4B, 0x03, 0x04])) == .zip)
}

@Test func detectsRarMagicBytes() {
    #expect(FileSignature.detect(in: Data([0x52, 0x61, 0x72, 0x21])) == .rar)
}

@Test func detectsMp4ByFtypBoxAtOffsetFour() {
    var bytes = Data([0x00, 0x00, 0x00, 0x18])
    bytes.append(contentsOf: "ftypmp42".utf8)
    #expect(FileSignature.detect(in: bytes) == .mp4)
}

@Test func detectsHTMLByDoctype() {
    let data = Data("<!DOCTYPE html><html><body>Not found</body></html>".utf8)
    #expect(FileSignature.detect(in: data) == .html)
}

@Test func unknownForUnrecognizedBytes() {
    #expect(FileSignature.detect(in: Data([0x01, 0x02, 0x03])) == .unknown)
}

@Test func matchesAcceptsExpectedExtensionPairs() {
    #expect(FileSignature.zip.matches(extension: "zip"))
    #expect(FileSignature.mp4.matches(extension: "mp4"))
    #expect(FileSignature.mp4.matches(extension: "m4v"))
}

@Test func matchesRejectsContradictingExtension() {
    #expect(!FileSignature.html.matches(extension: "mp4"))
    #expect(!FileSignature.zip.matches(extension: "pdf"))
}

@Test func unknownSignatureNeverContradictsAnExtension() {
    // Insufficient data to sniff should never itself trigger a "faulty" verdict.
    #expect(FileSignature.unknown.matches(extension: "mp4"))
}
