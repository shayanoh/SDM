import Foundation
import Testing

@testable import SDMResolve

@Test func fakeFetcherReplaysAndThrows() async throws {
    let f = FakeBinaryFetcher()
    f.setResponse(.success(Data("hi".utf8)), for: URL(string: "https://x/a")!)
    f.setResponse(.failure(BinaryFetchError.transport("offline")), for: URL(string: "https://x/b")!)

    #expect(try await f.data(from: URL(string: "https://x/a")!) == Data("hi".utf8))
    await #expect(throws: BinaryFetchError.transport("offline")) {
        try await f.data(from: URL(string: "https://x/b")!)
    }
    #expect(f.requestCount == 2)
}

@Test func fakeFetcherUnknownURLThrows404() async {
    let f = FakeBinaryFetcher()
    await #expect(throws: BinaryFetchError.http(404)) {
        try await f.data(from: URL(string: "https://x/missing")!)
    }
}
