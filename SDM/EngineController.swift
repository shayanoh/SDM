import Foundation
import Observation
import SDMCore
import SDMEngine

/// Bridges the engine actor to SwiftUI: drives the 1 Hz tick and republishes
/// snapshots on the main actor.
@MainActor
@Observable
final class EngineController {
    private(set) var snapshot = EngineSnapshot(
        packages: [],
        globalBytesPerSecond: 0,
        globalHistory: []
    )

    private let engine: DownloadEngine

    init() {
        let support = FileManager.default.urls(
            for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("SDM", isDirectory: true)
        let downloads = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask)[0]

        engine = DownloadEngine(
            transport: URLSessionTransport(),
            stateStore: JSONStateStore(fileURL: support.appendingPathComponent("state.json")),
            settings: EngineSettings(
                maxConcurrent: 3,
                segmentsPerItem: 8,
                globalMaxConnections: 32,
                downloadFolder: downloads
            )
        )
    }

    /// Runs the engine's 1 Hz heartbeat and refreshes the published snapshot.
    ///
    /// Exits its loop as soon as the enclosing `.task` is cancelled (window
    /// close / app termination), then shuts the engine down so its durable
    /// state actually reaches disk — `DownloadEngine.tick()` only queues a
    /// save in memory; `shutdown()` is the only public call that flushes it.
    /// The engine reference itself is retained by this controller regardless,
    /// so a redundant `tick()`/`snapshot()` call after shutdown is harmless
    /// (`reconcile()` is a no-op once shut down).
    func startHeartbeat() async {
        while !Task.isCancelled {
            await engine.tick()
            snapshot = await engine.snapshot()
            try? await Task.sleep(for: .seconds(1))
        }
        await engine.shutdown()
    }

    func addDownload(urlString: String) async {
        guard let url = URL(string: urlString), url.scheme?.hasPrefix("http") == true else {
            return
        }
        let filename = url.lastPathComponent.isEmpty ? "download.bin" : url.lastPathComponent
        let item = DownloadItem(url: url, filename: filename)
        await engine.add(DownloadPackage(name: "Manual", items: [item]))
        snapshot = await engine.snapshot()
    }

    func setEnabled(_ enabled: Bool, for itemID: UUID) async {
        await engine.setEnabled(enabled, for: itemID)
        snapshot = await engine.snapshot()
    }
}
