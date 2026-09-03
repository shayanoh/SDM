import Foundation
import Testing

@testable import SDMCore
@testable import SDMEngine

@Test func itemMetadataIsSurfacedInTheSnapshot() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let engine = DownloadEngine(
        transport: FakeOrigin(payload: Data()),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir))

    var withMeta = DownloadItem(url: testSourceURL, filename: "a.mkv")
    withMeta.metadata = "1080p · WEB-DL · H.264"
    let withoutMeta = DownloadItem(url: testSourceURL, filename: "b.zip")
    await engine.add(DownloadPackage(name: "P", items: [withMeta, withoutMeta]))

    #expect(await snapshotItem(withMeta.id, in: engine)?.metadata == "1080p · WEB-DL · H.264")
    #expect(await snapshotItem(withoutMeta.id, in: engine)?.metadata == nil)
}
