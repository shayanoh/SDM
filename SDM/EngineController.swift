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
    /// Guards against a second `shutdown()` call, since SwiftUI's
    /// `WindowGroup` does not guarantee the process exits when its window
    /// closes: the `@State` controller can survive a close/reopen and
    /// `.task { startHeartbeat() }` can re-fire. `DownloadEngine.shutdown()`
    /// is not documented as safe to call twice, so this makes "call it once"
    /// an explicit invariant here rather than an accident of the engine's
    /// current implementation.
    private var hasShutDown = false

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

    /// Loads durable state and runs the engine's 1 Hz heartbeat, refreshing
    /// the published snapshot.
    ///
    /// `restore()` runs first so packages persisted by a previous launch
    /// exist before anything is scheduled; it is `await`-only I/O against an
    /// injected `stateStore`, which is why it happens here rather than in
    /// `init()` — `init()` cannot `await`, and doing that I/O implicitly
    /// inside an initializer would hide it from callers and from tests that
    /// construct a controller without wanting disk access yet. `restore()`
    /// itself is idempotent, so a second `startHeartbeat()` call restores
    /// nothing further.
    ///
    /// Exits its loop as soon as the enclosing `.task` is cancelled (window
    /// close / app termination), then shuts the engine down so its durable
    /// state actually reaches disk — `DownloadEngine.tick()` only queues a
    /// save in memory; `shutdown()` is the only public call that flushes it.
    /// The engine reference itself is retained by this controller regardless,
    /// so a redundant `tick()`/`snapshot()` call after shutdown is harmless
    /// (`reconcile()` is a no-op once shut down). `hasShutDown` stops that
    /// final call itself from firing twice, in case the window closes and
    /// reopens without the process exiting.
    func startHeartbeat() async {
        await engine.restore()
        snapshot = await engine.snapshot()

        while !Task.isCancelled {
            await engine.tick()
            snapshot = await engine.snapshot()
            try? await Task.sleep(for: .seconds(1))
        }

        guard !hasShutDown else { return }
        hasShutDown = true
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
