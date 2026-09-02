import Foundation
import Testing

@testable import SDMResolve

private func makeManaged(
    bin: URL, fetcher: FakeBinaryFetcher = FakeBinaryFetcher(),
    runner: FakeProcessRunner = FakeProcessRunner(),
    assets: @escaping @Sendable () -> [VendorAsset] = { [] },
    channel: @escaping @Sendable () -> YtDlpChannel = { .stable },
    onChanged: @escaping @Sendable () async -> Void = {}
) -> ManagedBinaries {
    ManagedBinaries(
        binDirectory: bin, fetcher: fetcher, runner: runner,
        vendorAssets: assets, channel: channel, onBinariesChanged: onChanged)
}

@Test func inflatesBundledFFmpegOnce() async throws {
    let tmp = URL.tmp()
    defer { try? FileManager.default.removeItem(at: tmp) }
    let bin = tmp.appendingPathComponent("bin")
    let blob = tmp.appendingPathComponent("ffmpeg.lzfse")
    try FileManager.default.createDirectory(at: tmp, withIntermediateDirectories: true)
    let payload = Data("#!/bin/sh\necho ff\n".utf8)
    try LZFSE.compress(payload).write(to: blob)

    let mb = makeManaged(
        bin: bin, assets: { [VendorAsset(name: "ffmpeg", compressedURL: blob, version: "7.1")] })
    await mb.provisionBundledIfNeeded()

    let ff = bin.appendingPathComponent("ffmpeg")
    #expect(FileManager.default.isExecutableFile(atPath: ff.path))
    #expect(try Data(contentsOf: ff) == payload)
    #expect(await mb.manifestForTesting.ffmpegVersion == "7.1")

    // Same version → not rewritten: a local edit survives a second pass.
    try Data("touched".utf8).write(to: ff)
    await mb.provisionBundledIfNeeded()
    #expect(try Data(contentsOf: ff) == Data("touched".utf8))
}

@Test func reinflatesWhenBundledVersionBumps() async throws {
    let tmp = URL.tmp()
    defer { try? FileManager.default.removeItem(at: tmp) }
    let bin = tmp.appendingPathComponent("bin")
    try FileManager.default.createDirectory(at: tmp, withIntermediateDirectories: true)

    let v1 = tmp.appendingPathComponent("q1.lzfse")
    try LZFSE.compress(Data("one".utf8)).write(to: v1)
    let mb1 = makeManaged(
        bin: bin, assets: { [VendorAsset(name: "qjs", compressedURL: v1, version: "0.9.0")] })
    await mb1.provisionBundledIfNeeded()
    #expect(try Data(contentsOf: bin.appendingPathComponent("qjs")) == Data("one".utf8))

    let v2 = tmp.appendingPathComponent("q2.lzfse")
    try LZFSE.compress(Data("two".utf8)).write(to: v2)
    let mb2 = makeManaged(
        bin: bin, assets: { [VendorAsset(name: "qjs", compressedURL: v2, version: "0.10.0")] })
    await mb2.provisionBundledIfNeeded()
    #expect(try Data(contentsOf: bin.appendingPathComponent("qjs")) == Data("two".utf8))
    #expect(await mb2.manifestForTesting.qjsVersion == "0.10.0")
}

@Test func missingBlobIsSkippedNotFatal() async throws {
    let tmp = URL.tmp()
    defer { try? FileManager.default.removeItem(at: tmp) }
    let bin = tmp.appendingPathComponent("bin")
    let mb = makeManaged(
        bin: bin,
        assets: {
            [
                VendorAsset(
                    name: "ffmpeg",
                    compressedURL: tmp.appendingPathComponent("does-not-exist.lzfse"),
                    version: "7.1")
            ]
        })
    await mb.provisionBundledIfNeeded()
    #expect(!FileManager.default.fileExists(atPath: bin.appendingPathComponent("ffmpeg").path))
    #expect(await mb.manifestForTesting.lastError == nil)
}
