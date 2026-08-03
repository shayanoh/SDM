import Foundation
import SDMCore
import Testing

@testable import SDMEngine

private func sampleSidecar() -> ResumeSidecar {
    ResumeSidecar(
        sourceURL: URL(string: "https://example.com/a.bin")!,
        totalBytes: 1000,
        validator: "etag-1",
        completed: RangeSet([ByteRange(start: 0, end: 250)])
    )
}

@Test func sidecarURLUsesSdmpartExtension() {
    let url = URL(fileURLWithPath: "/tmp/movie.mp4")
    #expect(ResumeSidecar.url(for: url).lastPathComponent == "movie.mp4.sdmpart")
}

@Test func sidecarRoundTripsThroughDisk() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("a.bin.sdmpart")

    try sampleSidecar().save(to: url)
    #expect(ResumeSidecar.load(from: url) == sampleSidecar())
}

@Test func loadingMissingSidecarReturnsNil() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    #expect(ResumeSidecar.load(from: dir.appendingPathComponent("nope.sdmpart")) == nil)
}

@Test func loadingCorruptSidecarReturnsNil() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("a.bin.sdmpart")
    try Data("not json at all".utf8).write(to: url)
    #expect(ResumeSidecar.load(from: url) == nil)
}

@Test func loadingFutureFormatVersionReturnsNil() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("a.bin.sdmpart")
    var sidecar = sampleSidecar()
    sidecar.formatVersion = 999
    try sidecar.save(to: url)
    #expect(ResumeSidecar.load(from: url) == nil)
}

@Test func savingOverwritesPreviousContent() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("a.bin.sdmpart")

    try sampleSidecar().save(to: url)
    var updated = sampleSidecar()
    updated.completed.insert(ByteRange(start: 500, end: 900))
    try updated.save(to: url)

    #expect(ResumeSidecar.load(from: url)?.completed.totalBytes == 650)
}

@Test func matchesRejectsChangedValidator() {
    let sidecar = sampleSidecar()
    #expect(sidecar.matches(totalBytes: 1000, validator: "etag-1"))
    #expect(!sidecar.matches(totalBytes: 1000, validator: "etag-2"))
    #expect(!sidecar.matches(totalBytes: 2000, validator: "etag-1"))
}

@Test func matchesAcceptsAbsentValidatorOnBothSides() {
    let sidecar = ResumeSidecar(
        sourceURL: URL(string: "https://example.com/a.bin")!,
        totalBytes: 1000,
        validator: nil,
        completed: RangeSet()
    )
    #expect(sidecar.matches(totalBytes: 1000, validator: nil))
}
