import Foundation
import Testing

@testable import SDMEngine

@Test func identifiesYtDlpScratchSiblings() {
    let out = "Clip [abc123].mp4"
    let stem = "Clip [abc123]"
    let scratch = [
        "Clip [abc123].mp4.ytdl",
        "Clip [abc123].mp4.part",
        "Clip [abc123].mp4.part-Frag17",
        "Clip [abc123].mp4-Frag25",
        "Clip [abc123].mp4-Frag412",
        "Clip [abc123].mp4.frag0",
        "Clip [abc123].f137.mp4",
        "Clip [abc123].f137.mp4.ytdl",
        "Clip [abc123].f234.webm-Frag3",
        "Clip [abc123].temp.mp4",
    ]
    for name in scratch {
        #expect(YtDlpArtifacts.isScratch(name, output: out, stem: stem), "\(name)")
    }
}

@Test func doesNotSweepUnrelatedFiles() {
    let out = "Clip [abc123].mp4"
    let stem = "Clip [abc123]"
    for name in [
        "Clip [abc123].mp4",  // the output itself
        "Clip [abc123].final.mp4",  // not a .f<digit> stream file
        "Other Video [xyz].mp4",
        "Clip [abc123].en.srt",
        "poster.jpg",
    ] {
        #expect(!YtDlpArtifacts.isScratch(name, output: out, stem: stem), "\(name)")
    }
}

@Test func scratchFilesScansTheFolder() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let fm = FileManager.default
    for name in [
        "V.mp4", "V.mp4.ytdl", "V.mp4-Frag9", "V.f137.mp4", "V.f137.mp4.part",
        "unrelated.txt",
    ] {
        fm.createFile(atPath: dir.appendingPathComponent(name).path, contents: Data())
    }
    let found = Set(
        YtDlpArtifacts.scratchFiles(in: dir, outputFilename: "V.mp4").map(\.lastPathComponent))
    #expect(found == ["V.mp4.ytdl", "V.mp4-Frag9", "V.f137.mp4", "V.f137.mp4.part"])
}
