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
