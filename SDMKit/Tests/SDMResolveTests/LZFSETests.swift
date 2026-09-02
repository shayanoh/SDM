import Foundation
import Testing

@testable import SDMResolve

@Test func lzfseRoundTrips() throws {
    let original = Data((0..<200_000).map { UInt8($0 % 251) })
    let compressed = try LZFSE.compress(original)
    #expect(compressed.count < original.count)
    #expect(try LZFSE.decompress(compressed) == original)
}

@Test func lzfseRoundTripsShellScript() throws {
    let original = Data("#!/bin/sh\necho hello\n".utf8)
    #expect(try LZFSE.decompress(LZFSE.compress(original)) == original)
}

@Test func lzfseRejectsGarbage() {
    #expect(throws: (any Error).self) {
        try LZFSE.decompress(Data([1, 2, 3, 4, 5, 6, 7, 8]))
    }
}
