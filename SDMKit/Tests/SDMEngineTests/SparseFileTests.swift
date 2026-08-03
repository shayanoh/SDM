import Foundation
import Testing

@testable import SDMEngine

private func makeScratchDirectory() throws -> URL {
    let url = FileManager.default.temporaryDirectory
        .appendingPathComponent("sdm-tests-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

@Test func createsIncompleteFileWithSuffix() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let final = dir.appendingPathComponent("movie.mp4")

    let file = try SparseFile(finalURL: final, totalBytes: 100)
    defer { file.close() }

    let incomplete = dir.appendingPathComponent("movie.mp4.incomplete")
    #expect(FileManager.default.fileExists(atPath: incomplete.path))
    #expect(!FileManager.default.fileExists(atPath: final.path))
}

@Test func writesLandAtTheirAbsoluteOffsets() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let final = dir.appendingPathComponent("out.bin")

    let file = try SparseFile(finalURL: final, totalBytes: 10)
    try file.write(Data([9, 9]), at: 8)
    try file.write(Data([1, 2, 3]), at: 0)
    let result = try file.finalize()

    #expect(try Data(contentsOf: result) == Data([1, 2, 3, 0, 0, 0, 0, 0, 9, 9]))
}

@Test func finalizeRenamesOffTheIncompleteSuffix() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let final = dir.appendingPathComponent("movie.mp4")

    let file = try SparseFile(finalURL: final, totalBytes: 4)
    try file.write(Data([1, 2, 3, 4]), at: 0)
    let result = try file.finalize()

    #expect(result == final)
    #expect(FileManager.default.fileExists(atPath: final.path))
    #expect(!FileManager.default.fileExists(atPath: final.path + ".incomplete"))
}

@Test func reopeningPreservesExistingBytes() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let final = dir.appendingPathComponent("out.bin")

    let first = try SparseFile(finalURL: final, totalBytes: 6)
    try first.write(Data([1, 2, 3]), at: 0)
    try first.sync()
    first.close()

    let second = try SparseFile(finalURL: final, totalBytes: 6)
    try second.write(Data([4, 5, 6]), at: 3)
    let result = try second.finalize()

    #expect(try Data(contentsOf: result) == Data([1, 2, 3, 4, 5, 6]))
}

@Test func incompleteURLAppendsSuffix() {
    let url = URL(fileURLWithPath: "/tmp/a.bin")
    #expect(SparseFile.incompleteURL(for: url).lastPathComponent == "a.bin.incomplete")
}
