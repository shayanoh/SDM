import Foundation
import SDMCore

@testable import SDMEngine

/// Deterministic pseudo-random payload; index-derived so any byte can be
/// verified independently of how it was fetched.
func testPayload(_ count: Int) -> Data {
    Data((0..<count).map { UInt8(($0 &* 31 &+ 7) % 251) })
}

func makeScratchDirectory() throws -> URL {
    let url = FileManager.default.temporaryDirectory
        .appendingPathComponent("sdm-tests-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

let testSourceURL = URL(string: "https://example.com/file.bin")!

extension DownloadTask.Configuration {
    static func test(workers: Int, minChunk: Int64 = 64) -> DownloadTask.Configuration {
        DownloadTask.Configuration(
            workerCount: workers,
            minChunk: minChunk,
            checkpointInterval: 128
        )
    }
}
