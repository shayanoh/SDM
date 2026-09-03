import Foundation
import Testing

@testable import SDMCore

private func u(_ s: String) -> URL { URL(string: s)! }

@Test func convenienceInitBuildsAOneComponentHttpItem() {
    let item = DownloadItem(url: u("https://x/f.bin"), filename: "f.bin", totalBytes: 1000)
    #expect(item.components.count == 1)
    #expect(item.components[0].origin == .http)
    #expect(item.components[0].partFilename == "f.bin")
    #expect(item.outputFilename == "f.bin")
    #expect(item.assembly == .none)
    #expect(item.url == u("https://x/f.bin"))
    #expect(item.filename == "f.bin")
    #expect(item.totalBytes == 1000)
}

@Test func oneComponentAccessorsAreByteForByteUnchanged() {
    var item = DownloadItem(url: u("https://x/f.bin"), filename: "f.bin", totalBytes: 1000)
    item.completed = RangeSet([ByteRange(start: 0, end: 400)])
    #expect(item.completed.totalBytes == 400)
    #expect(item.fractionCompleted == 0.4)
    #expect(item.isComplete == false)
    item.isResumable = true
    #expect(item.isResumable == true)
    #expect(item.components[0].isResumable == true)
    item.totalBytes = 400
    #expect(item.isComplete)
}

@Test func concatenatedCompletedShiftsLaterComponentsByPriorSizes() {
    let video = FileComponent(
        url: u("https://gv/v"), partFilename: "t.f137.mp4", totalBytes: 100,
        completed: RangeSet([ByteRange(start: 0, end: 60)]), origin: .http)
    let audio = FileComponent(
        url: u("https://gv/a"), partFilename: "t.f251.webm", totalBytes: 20,
        completed: RangeSet([ByteRange(start: 0, end: 10)]), origin: .http)
    let item = DownloadItem(components: [video, audio], outputFilename: "t.mp4", assembly: .mux)
    #expect(item.totalBytes == 120)
    #expect(
        item.completed.ranges == [ByteRange(start: 0, end: 60), ByteRange(start: 100, end: 110)])
    #expect(item.fractionCompleted == 70.0 / 120.0)
    #expect(item.isComplete == false)
}

@Test func isResumableIsFalseIfAnyComponentIsFalse() {
    var a = FileComponent(url: u("https://x/a"), partFilename: "a", isResumable: true)
    let b = FileComponent(url: u("https://x/b"), partFilename: "b", isResumable: false)
    var item = DownloadItem(components: [a, b], outputFilename: "out.mp4", assembly: .mux)
    #expect(item.isResumable == false)
    a.isResumable = nil
    item = DownloadItem(components: [a, b], outputFilename: "out.mp4", assembly: .mux)
    #expect(item.isResumable == false)
    let c = FileComponent(url: u("https://x/c"), partFilename: "c", isResumable: nil)
    let d = FileComponent(url: u("https://x/d"), partFilename: "d", isResumable: true)
    item = DownloadItem(components: [c, d], outputFilename: "out.mp4", assembly: .mux)
    #expect(item.isResumable == nil)
}

@Test func newShapeCodableRoundTrips() throws {
    let item = DownloadItem(
        components: [
            FileComponent(
                url: u("https://gv/v"), partFilename: "t.f137.mp4", totalBytes: 100,
                origin: .resolved(formatID: "137")),
            FileComponent(
                url: u("https://gv/a"), partFilename: "t.f251.webm", totalBytes: 20,
                origin: .resolved(formatID: "251")),
        ], outputFilename: "t.mp4", assembly: .mux, state: .stopped,
        metadata: "Streaming · 1080p · h264 · mp4 · twitch")
    let data = try JSONEncoder().encode(item)
    let decoded = try JSONDecoder().decode(DownloadItem.self, from: data)
    #expect(decoded == item)
    #expect(decoded.metadata == "Streaming · 1080p · h264 · mp4 · twitch")
}

@Test func metadataDefaultsToNilAndSurvivesAbsence() throws {
    let item = DownloadItem(url: u("https://x/f.bin"), filename: "f.bin")
    #expect(item.metadata == nil)
    let data = try JSONEncoder().encode(item)
    #expect(try JSONDecoder().decode(DownloadItem.self, from: data).metadata == nil)
}

@Test func legacyFlatCodableDecodesToOneComponentItem() throws {
    let legacy = """
        {
          "id": "1B4E28BA-2FA1-11D2-883F-0016D3CCA427",
          "url": "https://x/f.bin",
          "filename": "f.bin",
          "totalBytes": 2048,
          "completed": { "ranges": [ { "start": 0, "end": 1024 } ] },
          "state": { "stopped": {} },
          "isEnabled": true,
          "isResumable": true,
          "position": 3
        }
        """
    let item = try JSONDecoder().decode(DownloadItem.self, from: Data(legacy.utf8))
    #expect(item.components.count == 1)
    #expect(item.components[0].origin == .http)
    #expect(item.url == u("https://x/f.bin"))
    #expect(item.outputFilename == "f.bin")
    #expect(item.totalBytes == 2048)
    #expect(item.completed.totalBytes == 1024)
    #expect(item.isResumable == true)
    #expect(item.state == .stopped)
    #expect(item.position == 3)
}

@Test func sourceUrlDefaultsToComponentZeroButCanBeOverridden() {
    let http = DownloadItem(url: u("https://x/f.bin"), filename: "f.bin")
    #expect(http.url == u("https://x/f.bin"))
    #expect(http.sourceURL == http.components[0].url)

    let media = DownloadItem(
        components: [
            FileComponent(url: u("https://gv/v"), partFilename: "t.f137.mp4"),
            FileComponent(url: u("https://gv/a"), partFilename: "t.f251.webm"),
        ], outputFilename: "t.mp4", sourceURL: u("https://youtu.be/abc"), assembly: .mux)
    #expect(media.url == u("https://youtu.be/abc"))
    #expect(media.components[0].url == u("https://gv/v"))

    let data = try! JSONEncoder().encode(media)
    #expect(
        try! JSONDecoder().decode(DownloadItem.self, from: data).url == u("https://youtu.be/abc"))
}

@Test func legacyItemWithoutSourceUrlFallsBackToTheFlatUrl() throws {
    let legacy = """
        {"id":"1B4E28BA-2FA1-11D2-883F-0016D3CCA427","url":"https://x/f.bin","filename":"f.bin",
         "state":{"stopped":{}},"isEnabled":true,"position":0}
        """
    let item = try JSONDecoder().decode(DownloadItem.self, from: Data(legacy.utf8))
    #expect(item.url == u("https://x/f.bin"))
    #expect(item.sourceURL == u("https://x/f.bin"))
}
