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

    /// `nonisolated` so `applicationWillTerminate` — which runs on the main
    /// thread and cannot await anything — can still reach the engine. Safe
    /// because `DownloadEngine` is an actor, and therefore `Sendable`.
    private nonisolated let engine: DownloadEngine
    /// Guards against a second `shutdown()` call, since SwiftUI's
    /// `WindowGroup` does not guarantee the process exits when its window
    /// closes: the `@State` controller can survive a close/reopen and
    /// `.task { startHeartbeat() }` can re-fire.
    ///
    /// `DownloadEngine.shutdown()` is now itself idempotent, which is what
    /// makes the two shutdown paths (this one and the terminate hook) safe to
    /// compose — they can and on ⌘Q do both fire. This flag stays as the
    /// main-actor-side half of that: it stops the heartbeat path from
    /// re-entering at all.
    private var hasShutDown = false
    private let downloadFolder: URL

    init() {
        let support = FileManager.default.urls(
            for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("SDM", isDirectory: true)
        let downloads = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask)[0]
        downloadFolder = downloads

        engine = DownloadEngine(
            transport: URLSessionTransport(),
            stateStore: JSONStateStore(fileURL: support.appendingPathComponent("state.json")),
            settings: EngineSettings(
                maxConcurrent: EngineSettingsStore.maxConcurrent,
                segmentsPerItem: EngineSettingsStore.segmentsPerItem,
                globalMaxConnections: EngineSettingsStore.globalMaxConnections,
                maxConnectionsPerHost: EngineSettingsStore.maxConnectionsPerHost,
                downloadFolder: downloads
            )
        )
    }

    /// Re-reads every `EngineSettingsStore` value and applies it live. Called
    /// from `SettingsView` whenever the operator changes a value.
    func applyStoredSettings() async {
        await engine.updateSettings(
            EngineSettings(
                maxConcurrent: EngineSettingsStore.maxConcurrent,
                segmentsPerItem: EngineSettingsStore.segmentsPerItem,
                globalMaxConnections: EngineSettingsStore.globalMaxConnections,
                maxConnectionsPerHost: EngineSettingsStore.maxConnectionsPerHost,
                downloadFolder: downloadFolder
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

    func retry(_ itemID: UUID) async {
        await engine.retry(itemID)
        snapshot = await engine.snapshot()
    }

    /// Hands a grabbed package off to the download engine. Spec §7.5's "Add
    /// to downloads" / "Add and start".
    func addPackage(name: String, urls: [URL], startImmediately: Bool) async {
        guard !urls.isEmpty else { return }
        let items = urls.map { url in
            DownloadItem(
                url: url,
                filename: url.lastPathComponent.isEmpty ? "download" : url.lastPathComponent,
                isEnabled: startImmediately
            )
        }
        await engine.add(DownloadPackage(name: name, items: items))
        snapshot = await engine.snapshot()
    }

    /// Shuts the engine down from `applicationWillTerminate`, blocking the
    /// main thread until durable state has actually reached disk.
    ///
    /// This exists because the heartbeat's shutdown never ran on a normal
    /// quit. `startHeartbeat()` only reaches its `shutdown()` call after the
    /// `while !Task.isCancelled` loop exits, i.e. only if the cancelled
    /// `.task` continuation gets scheduled before the process dies. On ⌘Q it
    /// does not. Combined with `flush()` having been the sole writer of
    /// `state.json`, that meant nothing was ever written in the ordinary case:
    /// `.incomplete` files survived with no record of the item owning them and
    /// the next launch restored an empty list.
    ///
    /// Blocking is the point — `applicationWillTerminate` returning ends the
    /// process, so an unawaited `Task` would be killed mid-write. It is
    /// deliberately `nonisolated` and goes straight to the engine rather than
    /// through `self`: hopping to the main actor from a blocked main thread
    /// would deadlock. The engine's own `shutdown()` is idempotent, so this
    /// composes with the heartbeat path rather than double-shutting-down.
    ///
    /// The timeout is a backstop, not an expectation: `shutdown()` awaits
    /// every runner's job, and a wedged origin should still not hold the quit
    /// forever.
    nonisolated func shutdownBlocking(timeout: TimeInterval = 5) {
        let engine = self.engine
        let done = DispatchSemaphore(value: 0)
        Task.detached(priority: .userInitiated) {
            await engine.shutdown()
            done.signal()
        }
        _ = done.wait(timeout: .now() + timeout)
    }
}
