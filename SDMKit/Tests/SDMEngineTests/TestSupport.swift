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

/// Freezes every non-probe body fetch until `open()` is called, so a test can
/// inspect `DownloadTask.activeWorkerCount` while every claimed worker is
/// still in flight. `FakeOrigin` has no artificial delay, so an ungated
/// transfer can complete before an assertion runs — this is the fix.
actor WorkerGate {
    private var isOpen = false
    private var waiters: [CheckedContinuation<Void, Never>] = []

    func waitUntilOpen() async {
        if isOpen { return }
        await withCheckedContinuation { waiters.append($0) }
    }

    func open() {
        isOpen = true
        let pending = waiters
        waiters.removeAll()
        for waiter in pending { waiter.resume() }
    }
}

struct WorkerGatedOrigin: HTTPTransport {
    let payload: Data
    let gate: WorkerGate

    func fetch(_ request: RangeRequest) async throws -> RangeResponse {
        let total = Int64(payload.count)
        let range = request.range ?? ByteRange(start: 0, end: total)
        let isProbe = range.start == 0 && range.end == 1
        guard !isProbe else {
            return RangeResponse(
                statusCode: 206,
                totalSize: total,
                acceptsRanges: true,
                validator: "etag-gated",
                body: AsyncThrowingStream { $0.finish() }
            )
        }

        let lower = Int(Swift.min(range.start, total))
        let upper = Int(Swift.min(range.end, total))
        let slice = payload.subdata(in: lower..<upper)
        let gate = self.gate
        let body = AsyncThrowingStream<Data, any Error> { continuation in
            Task {
                await gate.waitUntilOpen()
                continuation.yield(slice)
                continuation.finish()
            }
        }
        return RangeResponse(
            statusCode: 206,
            totalSize: total,
            acceptsRanges: true,
            validator: "etag-gated",
            body: body
        )
    }
}

func snapshotItem(_ id: UUID, in engine: DownloadEngine) async -> ItemSnapshot? {
    await engine.snapshot().packages.flatMap(\.items).first { $0.id == id }
}
