import Foundation
import SDMCore
import Testing

@testable import SDMEngine

// The partial file is flushed to the device about once a minute, not on every
// checkpoint, so a sidecar records both what was written and what was
// flushed. These pin which of the two a resume trusts.

private let fullRange = RangeSet([ByteRange(start: 0, end: 600)])
private let flushedRange = RangeSet([ByteRange(start: 0, end: 200)])

private func sidecar(durable: RangeSet?, boot: String?) -> ResumeSidecar {
    ResumeSidecar(
        sourceURL: testSourceURL, resolvedURL: nil, totalBytes: 1000, validator: nil,
        completed: fullRange, durableCompleted: durable, bootSessionID: boot)
}

@Test func sidecarWithoutADurableSetIsTrustedWhole() {
    // Written by a version that flushed before every sidecar.
    #expect(sidecar(durable: nil, boot: nil).resumableRanges(bootSessionID: "now") == fullRange)
}

@Test func sidecarFromTheSameBootSessionIsTrustedWhole() {
    let written = sidecar(durable: flushedRange, boot: "boot-1")
    #expect(written.resumableRanges(bootSessionID: "boot-1") == fullRange)
}

@Test func sidecarFromAnEarlierBootSessionIsTrustedOnlyAsFarAsItWasFlushed() {
    let written = sidecar(durable: flushedRange, boot: "boot-1")
    #expect(written.resumableRanges(bootSessionID: "boot-2") == flushedRange)
}

@Test func sidecarIsTrustedOnlyAsFarAsFlushedWhenTheBootSessionIsUnknown() {
    #expect(
        sidecar(durable: flushedRange, boot: "boot-1").resumableRanges(bootSessionID: nil)
            == flushedRange)
    #expect(
        sidecar(durable: flushedRange, boot: nil).resumableRanges(bootSessionID: "boot-1")
            == flushedRange)
}

@Test func sidecarWrittenBeforeTheDurableFieldsExistedStillLoads() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("old.sdmpart")
    let json = """
        {"formatVersion":1,"sourceURL":"https://example.com/file.bin","totalBytes":1000,
         "validator":"v","completed":{"ranges":[{"start":0,"end":600}]}}
        """
    try Data(json.utf8).write(to: url)

    let loaded = try #require(ResumeSidecar.load(from: url))
    #expect(loaded.durableCompleted == nil)
    #expect(loaded.resumableRanges(bootSessionID: "now") == fullRange)
}

// MARK: - DownloadTask

/// Lays down an interrupted download by hand: an `.incomplete` file holding
/// only `onDisk` bytes of the payload (zeros elsewhere), and a sidecar that
/// claims `written` was written but only `flushed` was flushed.
private func stageInterruptedDownload(
    payload: Data, destination: URL, onDisk: Range<Int>, written: Range<Int>, flushed: Range<Int>,
    boot: String
) throws {
    var bytes = Data(count: payload.count)
    bytes.replaceSubrange(onDisk, with: payload.subdata(in: onDisk))
    try bytes.write(to: SparseFile.incompleteURL(for: destination))
    try ResumeSidecar(
        sourceURL: testSourceURL, resolvedURL: nil, totalBytes: Int64(payload.count),
        validator: nil,
        completed: RangeSet([
            ByteRange(start: Int64(written.lowerBound), end: Int64(written.upperBound))
        ]),
        durableCompleted: RangeSet([
            ByteRange(start: Int64(flushed.lowerBound), end: Int64(flushed.upperBound))
        ]),
        bootSessionID: boot
    ).save(to: ResumeSidecar.url(for: destination))
}

private func resume(payload: Data, destination: URL, boot: String) async throws
    -> (result: URL, requested: [ByteRange])
{
    let origin = FakeOrigin(payload: payload)
    var configuration = DownloadTask.Configuration.test(workers: 1)
    configuration.bootSessionID = boot
    let task = DownloadTask(
        id: UUID(), sourceURL: testSourceURL, destinationURL: destination, transport: origin,
        configuration: configuration)
    let result = try await task.start()
    return (result, await origin.requestedRanges)
}

@Test func resumeAfterARestartRefetchesBytesThatWereNeverFlushed() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(8000)
    // A power cut: the sidecar says 6000 bytes were written, but only the
    // 2000 that had been flushed actually reached the disk.
    try stageInterruptedDownload(
        payload: payload, destination: destination, onDisk: 0..<2000, written: 0..<6000,
        flushed: 0..<2000, boot: "before-the-power-cut")

    let resumed = try await resume(payload: payload, destination: destination, boot: "after")

    // Byte identity is the point: trusting the 6000 would have left zeros.
    #expect(try Data(contentsOf: resumed.result) == payload)
    #expect(resumed.requested.contains { $0.start == 2000 })
}

@Test func resumeInTheSameBootSessionKeepsEverythingThatWasWritten() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(8000)
    // A crash or force quit: unflushed bytes are still there.
    try stageInterruptedDownload(
        payload: payload, destination: destination, onDisk: 0..<6000, written: 0..<6000,
        flushed: 0..<2000, boot: "same")

    let resumed = try await resume(payload: payload, destination: destination, boot: "same")

    #expect(try Data(contentsOf: resumed.result) == payload)
    // Apart from the one-byte probe, nothing before 6000 is asked for again.
    #expect(!resumed.requested.contains { $0.length > 1 && $0.start < 6000 })
}

@Test func aFailedRunFlushesSoItsSidecarVouchesForEverythingWritten() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let destination = dir.appendingPathComponent("out.bin")
    let payload = testPayload(8000)
    let origin = FakeOrigin(payload: payload)
    var behavior = FakeOrigin.Behavior()
    behavior.dropAfterBytes = 3000
    await origin.setBehavior(behavior)
    let task = DownloadTask(
        id: UUID(), sourceURL: testSourceURL, destinationURL: destination, transport: origin,
        configuration: .test(workers: 1))
    _ = try? await task.start()

    // No minute has passed, so none of the byte-triggered checkpoints
    // flushed — the closing one on the failure path is what does.
    let saved = try #require(ResumeSidecar.load(from: ResumeSidecar.url(for: destination)))
    #expect(saved.completed.totalBytes == 3000)
    #expect(saved.durableCompleted == saved.completed)
    #expect(saved.bootSessionID == ResumeSidecar.currentBootSessionID)
}
