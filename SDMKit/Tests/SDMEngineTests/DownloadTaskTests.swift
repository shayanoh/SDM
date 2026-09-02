import Foundation
import SDMCore
import Testing

@testable import SDMEngine

@Test func singleWorkerDownloadsCompleteFile() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let payload = testPayload(4000)
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: payload),
        configuration: .test(workers: 1)
    )

    let result = try await task.start()
    #expect(try Data(contentsOf: result) == payload)
}

@Test func completedFileHasNoIncompleteOrSidecarLeftBehind() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")

    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: testPayload(1000)),
        configuration: .test(workers: 1)
    )
    _ = try await task.start()

    let remaining = try FileManager.default.contentsOfDirectory(atPath: dir.path)
    #expect(remaining == ["out.bin"])
}

@Test func rangeSetCoversWholeFileAfterCompletion() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: testPayload(1000)),
        configuration: .test(workers: 1)
    )
    _ = try await task.start()

    let completed = await task.completedRanges
    #expect(completed.ranges == [ByteRange(start: 0, end: 1000)])
}

@Test func truncatedBodyFailsWithoutRetryingForever() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    // A single worker's claim is always the whole free gap now, so with a
    // 100-byte payload the truncation point below lands inside that one
    // claim rather than at its edge.
    var behavior = FakeOrigin.Behavior()
    behavior.truncateAfterBytes = 50
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: testPayload(100), behavior: behavior),
        configuration: .test(workers: 1)
    )

    await #expect(throws: DownloadError.truncatedResponse(expected: 100, received: 50)) {
        _ = try await task.start()
    }

    // The short read must not have lost the bytes it did receive.
    let completed = await task.completedRanges
    #expect(completed.ranges == [ByteRange(start: 0, end: 50)])
}

@Test func midStreamFailureLeavesIncompleteFileAndSidecarResumable() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")

    var behavior = FakeOrigin.Behavior()
    // chunkSize defaults to 64 and checkpointInterval is 128, so a
    // checkpoint fires (at 128 bytes) before the drop at 512 — the sidecar
    // is guaranteed to exist when the connection dies.
    behavior.dropAfterBytes = 512
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: destination,
        transport: FakeOrigin(payload: testPayload(4000), behavior: behavior),
        configuration: .test(workers: 1)
    )

    await #expect(throws: TransportError.connectionDropped) {
        _ = try await task.start()
    }

    let incompleteURL = SparseFile.incompleteURL(for: destination)
    let sidecarURL = ResumeSidecar.url(for: destination)
    #expect(FileManager.default.fileExists(atPath: incompleteURL.path))
    #expect(FileManager.default.fileExists(atPath: sidecarURL.path))
}

@Test func errorStatusFailsTheDownload() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    var behavior = FakeOrigin.Behavior()
    behavior.statusOverride = 404
    let task = DownloadTask(
        id: UUID(),
        sourceURL: testSourceURL,
        destinationURL: dir.appendingPathComponent("out.bin"),
        transport: FakeOrigin(payload: testPayload(100), behavior: behavior),
        configuration: .test(workers: 1)
    )

    await #expect(throws: DownloadError.serverError(status: 404)) {
        _ = try await task.start()
    }
}

@Test func expiredUrlOnARefreshableComponentThrowsUrlExpired() async throws {
    var b = FakeOrigin.Behavior()
    b.statusOverride = 403
    let origin = FakeOrigin(payload: testPayload(2000), behavior: b)
    let dir = try makeScratchDirectory()
    let task = DownloadTask(
        id: UUID(), sourceURL: URL(string: "https://gv/v137")!,
        destinationURL: dir.appendingPathComponent("v.f137.mp4"),
        transport: origin,
        configuration: DownloadTask.Configuration(
            workerCount: 1, minChunk: 64, checkpointInterval: 128,
            cachedCompleted: nil, refreshableFormatID: "137"))
    await #expect(throws: DownloadError.urlExpired(formatID: "137")) { _ = try await task.start() }
}

@Test func expiredUrlWithoutARefreshableFormatIsAPlainServerError() async throws {
    var b = FakeOrigin.Behavior()
    b.statusOverride = 403
    let origin = FakeOrigin(payload: testPayload(2000), behavior: b)
    let dir = try makeScratchDirectory()
    let task = DownloadTask(
        id: UUID(), sourceURL: URL(string: "https://x/f")!,
        destinationURL: dir.appendingPathComponent("f.bin"),
        transport: origin,
        configuration: DownloadTask.Configuration(
            workerCount: 1, minChunk: 64, checkpointInterval: 128, cachedCompleted: nil))
    await #expect(throws: DownloadError.serverError(status: 403)) { _ = try await task.start() }
}
