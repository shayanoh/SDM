import Foundation
import Observation
import SDMCore
import SDMEngine
import SDMResolve

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
    /// notifications and anything else that wants a fully live view) is
    /// what originally caused this: every
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
    /// `structuralPackages` filtered down to `.completed` items — the
    /// Completed tab's data source. Stored, and reassigned only when the
    /// filtered result itself structurally changes, for the same reason
    /// `structuralPackages` is: computing it inside `MainWindowView.body`
    /// handed the Completed tab's `List` a brand-new array on every body
    /// evaluation.
    private(set) var completedPackages: [PackageSnapshot] = []
    /// Whole-engine counts and the global speed figures, updated once per
    /// tick. A `let`: readers observe the individual properties on it.
    let stats = EngineStats()
    /// Live per-item / per-package figures, one stable `@Observable` object
    /// each — see `ItemLiveModel`. Deliberately *not* observed themselves:
    /// a row looks its model up once and then observes that object, so
    /// adding or removing an entry must not invalidate every other row.
    /// `structuralPackages` changing is what tells views the set of rows
    /// changed.
    @ObservationIgnored private var itemModels: [UUID: ItemLiveModel] = [:]
    @ObservationIgnored private var packageModels: [UUID: PackageLiveModel] = [:]

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

    init(
        notificationManager: NotificationManager,
        binaryLocator: BinaryLocator = BinaryLocator(searchPaths: [
            ManagedBinariesController.binDirectory
        ])
    ) {
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
                cookieSource: { MediaSitesSettingsStore.cookieSource },
                maxPlaylistVideos: { MediaSitesSettingsStore.maxPlaylistVideos },
                extraArguments: { ManagedBinariesController.ytDlpExtraArguments }
            ),
            muxer: FFmpegMuxer(runner: processRunner, locator: binaryLocator),
            wholesaleDownloader: YtDlpWholesaleDownloader(
                runner: processRunner, locator: binaryLocator,
                cookieSource: { MediaSitesSettingsStore.cookieSource },
                extraArguments: { ManagedBinariesController.ytDlpExtraArguments }
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

    /// The one place `snapshot`, `structuralPackages`, `completedPackages`,
    /// `stats` and the live models are updated together, so every call site
    /// (the tick loop, and every direct user-action method below) keeps them
    /// in sync the same way.
    private func publish(_ newSnapshot: EngineSnapshot) {
        // Everything below is derived from the snapshot, so an unchanged one
        // means there is nothing to do — and `@Observable` notifies on
        // *reassignment*, not on value change, so assigning it anyway would
        // re-evaluate every reader at full tick rate with nothing running.
        guard newSnapshot != snapshot else { return }
        snapshot = newSnapshot
        stats.update(from: newSnapshot)

        let structuralChanged = !Self.packagesStructurallyEqual(
            structuralPackages, newSnapshot.packages)
        let globalRecentSpeed = stats.recentGlobalSpeed

        // Models are brought up to date *before* `structuralPackages` is
        // reassigned below, so a row created for a newly added item finds
        // its model already there.
        for package in newSnapshot.packages {
            var isStale = structuralChanged
            var hasRunning = false
            for item in package.items {
                if let model = itemModels[item.id] {
                    if model.update(from: item) { isStale = true }
                } else {
                    itemModels[item.id] = ItemLiveModel(item)
                    isStale = true
                }
                if item.state == .running { hasRunning = true }
            }
            // A package with nothing running and no changed member has the
            // same aggregate it had last tick. One with a running member is
            // always recomputed: its ETA also depends on the global rate.
            if let model = packageModels[package.id] {
                if isStale || hasRunning {
                    model.update(
                        PackageAggregate(
                            items: package.items, globalRecentSpeed: globalRecentSpeed))
                }
            } else {
                packageModels[package.id] = PackageLiveModel(
                    id: package.id,
                    aggregate: PackageAggregate(
                        items: package.items, globalRecentSpeed: globalRecentSpeed))
            }
        }

        guard structuralChanged else { return }
        let liveItemIDs = Set(newSnapshot.packages.flatMap { $0.items.map(\.id) })
        itemModels = itemModels.filter { liveItemIDs.contains($0.key) }
        let livePackageIDs = Set(newSnapshot.packages.map(\.id))
        packageModels = packageModels.filter { livePackageIDs.contains($0.key) }

        structuralPackages = newSnapshot.packages
        let completed = Self.completedOnly(newSnapshot.packages)
        if !Self.packagesStructurallyEqual(completedPackages, completed) {
            completedPackages = completed
        }
    }

    /// The live model for `item`. `publish` creates one for every item
    /// before the row that wants it can exist; creating it here on a miss
    /// is only a backstop so a row never has to handle "no model".
    func liveModel(for item: ItemSnapshot) -> ItemLiveModel {
        if let model = itemModels[item.id] { return model }
        let model = ItemLiveModel(item)
        itemModels[item.id] = model
        return model
    }

    /// The live aggregate model for `package`. Same backstop as
    /// `liveModel(for:)` above.
    func liveModel(for package: PackageSnapshot) -> PackageLiveModel {
        if let model = packageModels[package.id] { return model }
        let model = PackageLiveModel(
            id: package.id,
            aggregate: PackageAggregate(items: package.items, globalRecentSpeed: 0))
        packageModels[package.id] = model
        return model
    }

    /// Spec §9.1: "Completed is a filtered view of the same list, not a
    /// separate store."
    private static func completedOnly(_ packages: [PackageSnapshot]) -> [PackageSnapshot] {
        packages.compactMap { package in
            let completedItems = package.items.filter { $0.state == .completed }
            guard !completedItems.isEmpty else { return nil }
            return PackageSnapshot(
                id: package.id, name: package.name, priority: package.priority,
                items: completedItems)
        }
    }

    /// Everything about a package/item *except* the fields already carried
    /// by `ItemLiveModel` — i.e. everything a `List` actually needs to
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

    /// Re-runs only the `ffmpeg` mux for a failed muxed item whose parts are
    /// all downloaded. Spec §9.3's "Retry mux".
    func retryMux(_ itemID: UUID) async {
        await engine.retryMux(itemID)
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
        await addItems(name: name, note: nil, items: items, startImmediately: startImmediately)
    }

    /// Hands pre-built `DownloadItem`s (possibly multi-component, from the
    /// media handoff) to the engine under a named, optionally-annotated
    /// package. `startImmediately` is a scheduling choice (queued vs.
    /// stopped) — a freshly grabbed item is always enabled.
    func addItems(
        name: String, note: String?, items: [DownloadItem], startImmediately: Bool
    ) async {
        guard !items.isEmpty else { return }
        let staged = items.map { item -> DownloadItem in
            var copy = item
            copy.state = startImmediately ? .queued : .stopped
            copy.isEnabled = true
            return copy
        }
        await engine.add(DownloadPackage(name: name, items: staged, note: note))
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
