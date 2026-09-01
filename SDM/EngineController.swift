import Foundation
import Observation
import SDMCore
import SDMEngine
import SDMResolve

/// The "hot" per-item fields that change every heartbeat tick while a
/// download is active — split out from `ItemSnapshot` so a row can read live
/// numbers without forcing `PackagesListView`'s `List` to receive a new
/// `packages` array identity on every tick. See
/// `EngineController.structuralPackages`/`itemTelemetry` for why.
struct ItemTelemetry: Equatable {
    var completed: RangeSet
    var totalBytes: Int64?
    var activeSegments: Int
    var configuredSegments: Int
    var bytesPerSecond: Double
    var speedHistory: [Double]
}

/// A package's URL and optional size, as grabbed from LinkGrabber
struct PackageUrlItem {
    let url: URL
    let size: Int64?
    let effectiveFilename: String?
}

/// Bridges the engine actor to SwiftUI: drives the `AppTiming.ticksPerSecond`
/// tick and republishes on the main actor at that same rate.
@MainActor
@Observable
final class EngineController {
    private(set) var snapshot = EngineSnapshot(
        packages: [],
        globalBytesPerSecond: 0,
        globalHistory: []
    )
    /// A `packages` array that only changes identity when something beyond
    /// pure byte/speed telemetry actually changed (add/remove/reorder, a
    /// state transition, enable/disable, resumability learned, etc.) — see
    /// `packagesStructurallyEqual`. `PackagesListView` reads this instead of
    /// `snapshot.packages` for its `List`'s data source.
    ///
    /// Reassigning `snapshot` itself every tick (still done, for
    /// `globalBytesPerSecond`/`globalHistory`/notifications/anything else
    /// that wants a fully live view) is what originally caused this: every
    /// reassignment fed `List` a brand-new `packages` value, forcing it to
    /// re-diff its entire structure five times a second even when nothing
    /// but a byte count changed inside an otherwise-identical row — which
    /// is what was interrupting an in-flight drag-and-drop reorder (AppKit
    /// resets a table's drag session when its data source reloads
    /// mid-drag). A mouse-event-based "pause publishing during a drag"
    /// mitigation was tried and abandoned: `NSDraggingSession`'s own
    /// tracking loop does not reliably deliver events to a local `NSEvent`
    /// monitor, so the resume signal it depended on was itself unreliable.
    /// This fixes the actual cause instead of working around a symptom.
    private(set) var structuralPackages: [PackageSnapshot] = []
    /// Live per-item telemetry, keyed by item ID, reassigned every tick
    /// regardless of `structuralPackages`. A row (`ItemRow`, the package
    /// header, the bottom bar) reads its own entry directly via
    /// `@Environment(EngineController.self)`, which is what lets it update
    /// at full tick rate without `PackagesListView.body` — and hence its
    /// `List`'s structure — re-evaluating at all.
    private(set) var itemTelemetry: [UUID: ItemTelemetry] = [:]

    /// `nonisolated` so `applicationWillTerminate` — which runs on the main
    /// thread and cannot await anything — can still reach the engine. Safe
    /// because `DownloadEngine` is an actor, and therefore `Sendable`.
    private nonisolated let engine: DownloadEngine
    /// Retains the heartbeat's own `Task`, started once and independent of
    /// any SwiftUI view's `.task` — see `startHeartbeatIfNeeded()`.
    private var heartbeatTask: Task<Void, Never>?
    private let notifications: NotificationManager
    private var previousSnapshot: EngineSnapshot?
    private let downloadFolder: URL

    init(notificationManager: NotificationManager) {
        notifications = notificationManager
        let support = FileManager.default.urls(
            for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("SDM", isDirectory: true)
        let downloads = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask)[0]
        downloadFolder = downloads

        // yt-dlp / ffmpeg are located once and shared by the resolver and the
        // muxer. Cookie source and playlist cap are Part 4 Settings — literals
        // for now.
        let processRunner = SystemProcessRunner()
        let binaryLocator = BinaryLocator()
        engine = DownloadEngine(
            transport: URLSessionTransport(),
            stateStore: JSONStateStore(fileURL: support.appendingPathComponent("state.json")),
            settings: EngineSettings(
                maxConcurrent: EngineSettingsStore.maxConcurrent,
                segmentsPerItem: EngineSettingsStore.segmentsPerItem,
                globalMaxConnections: EngineSettingsStore.globalMaxConnections,
                maxConnectionsPerHost: EngineSettingsStore.maxConnectionsPerHost,
                downloadFolder: downloads,
                minSegmentSizeBytes: Int64(EngineSettingsStore.minSegmentSizeMB) * 1024 * 1024
            ),
            resolver: YtDlpResolver(
                runner: processRunner, locator: binaryLocator,
                cookieSource: { .none },  // Part 4: read from Settings
                maxPlaylistVideos: { 50 }  // Part 4: read from Settings
            ),
            muxer: FFmpegMuxer(runner: processRunner, locator: binaryLocator)
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
                downloadFolder: downloadFolder,
                minSegmentSizeBytes: Int64(EngineSettingsStore.minSegmentSizeMB) * 1024 * 1024
            )
        )
    }

    /// Starts the heartbeat exactly once for the process's lifetime, in a
    /// `Task` this controller owns directly rather than one hung off a
    /// SwiftUI view's `.task`.
    ///
    /// SDM keeps downloading (and the menu bar ring keeps updating) with the
    /// main window closed — closing it only switches the dock/menu-bar
    /// activation policy, per `ActivationPolicyController`; it does not quit.
    /// A `.task` attached to the window's content view is cancelled by
    /// SwiftUI the moment that view leaves the hierarchy, i.e. on every
    /// window close, so the heartbeat must not live there: cancellation used
    /// to call `engine.shutdown()`, which cancels every running job and
    /// permanently flips `DownloadEngine.isShutDown` — reopening the window
    /// restarted the loop, but `reconcile()` had become a no-op forever,
    /// so everything sat `.queued` and nothing ever ran again.
    ///
    /// Real shutdown only needs to happen once, at actual process
    /// termination, and `AppDelegate.applicationWillTerminate` already
    /// covers that reliably (see its doc comment in `SDMApp.swift`) — so
    /// this task simply never calls `engine.shutdown()` itself.
    func startHeartbeatIfNeeded() {
        guard heartbeatTask == nil else { return }
        heartbeatTask = Task { await self.runHeartbeat() }
    }

    /// Loads durable state and runs the engine's `AppTiming.ticksPerSecond`
    /// Hz heartbeat, refreshing the published snapshot, for as long as the
    /// process lives. See `startHeartbeatIfNeeded()`, its only caller.
    ///
    /// `restore()` is `await`-only I/O against an injected `stateStore`,
    /// which is why it happens here rather than in `init()` — `init()`
    /// cannot `await`, and doing that I/O implicitly inside an initializer
    /// would hide it from callers and from tests that construct a
    /// controller without wanting disk access yet.
    private func runHeartbeat() async {
        await engine.restore()
        // `restore()` always lands every non-terminal item `.stopped` — the
        // store never persists `.queued`/`.running` (see
        // `DownloadEngine.persist()`) — so "resume downloads automatically on
        // launch" is nothing more than the same `resumeAll()` Resume All
        // uses: enabled, stopped items requeue; disabled ones stay put. Off
        // by default means simply not calling it, leaving everything exactly
        // as `restore()` left it.
        if EngineSettingsStore.autoStartDownloadsOnLaunch {
            await engine.resumeAll()
        }
        publish(await engine.snapshot())
        notifications.requestAuthorization()

        while !Task.isCancelled {
            await engine.tick()
            let newSnapshot = await engine.snapshot()
            notifyChanges(from: previousSnapshot, to: newSnapshot)
            previousSnapshot = newSnapshot
            publish(newSnapshot)
            try? await Task.sleep(for: .seconds(1.0 / Double(AppTiming.ticksPerSecond)))
        }
    }

    /// The one place `snapshot`/`structuralPackages`/`itemTelemetry` are
    /// updated together, so every call site (the tick loop, and every direct
    /// user-action method below) keeps the three in sync the same way.
    private func publish(_ newSnapshot: EngineSnapshot) {
        // `@Observable` notifies on *reassignment*, not on value change —
        // gated the same way `itemTelemetry`/`structuralPackages` below are,
        // otherwise every view reading `snapshot.*` (chiefly `BandwidthGraph`
        // via `globalHistory`) re-evaluates at full tick rate forever, even
        // once `SpeedSampler.idle()` has made the underlying data static.
        if newSnapshot != snapshot {
            snapshot = newSnapshot
        }

        // `@Observable` notifies on *reassignment*, not on value change — an
        // unconditional `itemTelemetry = newTelemetry` here was invalidating
        // every row that reads it on every tick even when nothing in it
        // actually differed (confirmed live: with zero downloads running,
        // every `ItemRow` was still re-rendering every tick). Gate the
        // reassignment the same way `structuralPackages` already is.
        let newTelemetry = Self.telemetry(from: newSnapshot)
        let telemetryChanged = newTelemetry != itemTelemetry
        if telemetryChanged {
            itemTelemetry = newTelemetry
        }

        let structuralChanged = !Self.packagesStructurallyEqual(
            structuralPackages, newSnapshot.packages)
        if structuralChanged {
            structuralPackages = newSnapshot.packages
        }
    }

    private static func telemetry(from snapshot: EngineSnapshot) -> [UUID: ItemTelemetry] {
        var result: [UUID: ItemTelemetry] = [:]
        for item in snapshot.packages.flatMap(\.items) {
            result[item.id] = ItemTelemetry(
                completed: item.completed,
                totalBytes: item.totalBytes,
                activeSegments: item.activeSegments,
                configuredSegments: item.configuredSegments,
                bytesPerSecond: item.bytesPerSecond,
                speedHistory: item.speedHistory
            )
        }
        return result
    }

    /// Everything about a package/item *except* the fields already carried
    /// by `ItemTelemetry` — i.e. everything a `List` actually needs to
    /// re-diff its structure for: identity, order, name/filename, and any
    /// state a row's non-telemetry chrome (icons, badges, strikethrough)
    /// depends on.
    private static func packagesStructurallyEqual(
        _ a: [PackageSnapshot], _ b: [PackageSnapshot]
    ) -> Bool {
        guard a.count == b.count else { return false }
        for (packageA, packageB) in zip(a, b) {
            guard packageA.id == packageB.id, packageA.name == packageB.name,
                packageA.priority == packageB.priority,
                packageA.items.count == packageB.items.count
            else { return false }
            for (itemA, itemB) in zip(packageA.items, packageB.items) {
                guard itemA.id == itemB.id, itemA.url == itemB.url,
                    itemA.filename == itemB.filename, itemA.state == itemB.state,
                    itemA.isEnabled == itemB.isEnabled, itemA.isResumable == itemB.isResumable,
                    itemA.checkpointFailure == itemB.checkpointFailure,
                    itemA.remainingAttempts == itemB.remainingAttempts,
                    itemA.fileMissing == itemB.fileMissing
                else { return false }
            }
        }
        return true
    }

    /// Compares two heartbeats' worth of snapshot and fires exactly the
    /// notifications spec §9.8 lists, once per transition — never a repeat
    /// for a state that has already been announced.
    private func notifyChanges(from old: EngineSnapshot?, to new: EngineSnapshot) {
        guard let old else { return }
        let oldStates = Dictionary(
            uniqueKeysWithValues: old.packages.flatMap(\.items).map { ($0.id, $0.state) })
        for item in new.packages.flatMap(\.items) {
            guard oldStates[item.id] != item.state else { continue }
            switch item.state {
            case .completed:
                notifications.notifyDownloadFinished(filename: item.filename)
            case .failed(let reason):
                notifications.notifyDownloadFailed(filename: item.filename, reason: reason)
            default:
                break
            }
        }

        let oldPackagesDone = Dictionary(
            uniqueKeysWithValues: old.packages.map {
                ($0.id, !$0.items.isEmpty && $0.completedCount == $0.items.count)
            })
        for package in new.packages {
            let isDone = !package.items.isEmpty && package.completedCount == package.items.count
            guard isDone, oldPackagesDone[package.id] != true else { continue }
            notifications.notifyPackageFinished(name: package.name)
        }
    }

    func setEnabled(_ enabled: Bool, for itemID: UUID) async {
        await engine.setEnabled(enabled, for: itemID)
        publish(await engine.snapshot())
    }

    func startItem(_ itemID: UUID) async {
        await engine.startItem(itemID)
        publish(await engine.snapshot())
    }

    func stopItem(_ itemID: UUID) async {
        await engine.stopItem(itemID)
        publish(await engine.snapshot())
    }

    func retry(_ itemID: UUID) async {
        await engine.retry(itemID)
        publish(await engine.snapshot())
    }

    func moveItem(_ itemID: UUID, toPackage packageID: UUID, atIndex index: Int? = nil) async {
        await moveItems([itemID], toPackage: packageID, atIndex: index)
    }

    func moveItems(_ itemIDs: [UUID], toPackage packageID: UUID, atIndex index: Int? = nil) async {
        await engine.moveItems(itemIDs, toPackage: packageID, atIndex: index)
        publish(await engine.snapshot())
    }

    func moveItems(_ itemIDs: [UUID], toNewPackageNamed packageName: String) async {
        await engine.moveItems(itemIDs, toNewPackageNamed: packageName)
        publish(await engine.snapshot())
    }

    func reorderPackages(_ packageIDs: [UUID]) async {
        await engine.reorderPackages(packageIDs)
        publish(await engine.snapshot())
    }

    func reorderItems(_ itemIDs: [UUID], inPackage packageID: UUID) async {
        await engine.reorderItems(itemIDs, inPackage: packageID)
        publish(await engine.snapshot())
    }

    /// Global pause/resume: stops or (re-)queues every stoppable/resumable
    /// item in one call. Never touches any item's `isEnabled`.
    func pauseAll() async {
        await engine.pauseAll()
        publish(await engine.snapshot())
    }

    func resumeAll() async {
        await engine.resumeAll()
        publish(await engine.snapshot())
    }

    func removeItem(_ itemID: UUID, deleteFile: Bool) async {
        await engine.removeItem(itemID, deleteFile: deleteFile)
        publish(await engine.snapshot())
    }

    func removePackage(_ packageID: UUID, deleteFiles: Bool) async {
        await engine.removePackage(packageID, deleteFiles: deleteFiles)
        publish(await engine.snapshot())
    }

    func resetDownload(_ itemID: UUID) async {
        await engine.resetDownload(itemID)
        publish(await engine.snapshot())
    }

    /// Where a completed item's file actually lives on disk — the same path
    /// `DownloadEngine` resolves internally, reconstructed here since
    /// `ItemSnapshot`/`PackageSnapshot` don't carry a destination URL of
    /// their own. Used to open a finished download from the list.
    func destinationURL(for item: ItemSnapshot, inPackage package: PackageSnapshot) -> URL {
        destinationPackageUrl(for: package).appendingPathComponent(item.filename)
    }

    /// The container folder for the download item
    func destinationPackageUrl(for package: PackageSnapshot) -> URL {
        downloadFolder.appendingPathComponent(package.name)
    }

    /// Batch variant for multi-selection delete: each item is stopped and
    /// removed in turn, then the snapshot is republished once at the end
    /// rather than once per item.
    func removeItems(_ itemIDs: [UUID], deleteFile: Bool) async {
        for itemID in itemIDs {
            await engine.removeItem(itemID, deleteFile: deleteFile)
        }
        publish(await engine.snapshot())
    }

    /// Hands a grabbed package off to the download engine. Spec §7.5's "Add
    /// to downloads" / "Add and start".
    func addPackage(name: String, urlItems: [PackageUrlItem], startImmediately: Bool) async {
        guard !urlItems.isEmpty else { return }
        // `startImmediately` is a scheduling choice (queued vs. stopped), not
        // a disable — a freshly grabbed item is always enabled; see
        // `ItemState`'s doc comment for why the two axes stay independent.
        let items = urlItems.map { urlItem in
            DownloadItem(
                url: urlItem.url,
                filename: urlItem.effectiveFilename
                    ?? (urlItem.url.lastPathComponent.isEmpty
                        ? "download" : urlItem.url.lastPathComponent),
                totalBytes: urlItem.size,
                state: startImmediately ? .queued : .stopped
            )
        }
        await engine.add(DownloadPackage(name: name, items: items))
        publish(await engine.snapshot())
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
