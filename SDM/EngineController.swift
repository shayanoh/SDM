import Foundation
import Observation
import SDMCore
import SDMEngine
import SDMResolve
import os.log

/// A package's URL and optional size, as grabbed from LinkGrabber
struct PackageUrlItem {
    let url: URL
    let size: Int64?
    let effectiveFilename: String?
}

/// Bridges the Rust download engine to SwiftUI: drives the `AppTiming.ticksPerSecond`
/// tick and republishes on the main actor at that same rate.
@MainActor
@Observable
final class EngineController {
    private(set) var snapshot = EngineSnapshot.empty
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
    /// Called after every publish that changed something — the hook for
    /// state that lives outside any view (the menu bar icon, the Touch
    /// Bar). It exists so `SDMApp.body` does not have to watch a per-tick
    /// value to refresh them: a `Scene`-level `.onChange` on one re-ran the
    /// whole scene body every tick, and with it SwiftUI re-applied the main
    /// window's toolbar — the largest main-thread cost while downloading.
    @ObservationIgnored var onPublish: (() -> Void)?
    /// Whether a downloads list is on screen — see `setListVisible(_:)`.
    /// Starts `true` so nothing is ever stale before the window first
    /// reports in.
    @ObservationIgnored private var isListVisible = true
    /// The items each details panel is currently showing, keyed by the
    /// panel's own token — see `setDetailItems(_:for:)`.
    @ObservationIgnored private var detailItemsByOwner: [UUID: Set<UUID>] = [:]
    /// The union of `detailItemsByOwner`, and the ids that last received a
    /// `detailHistory`, so deselected items can have theirs dropped.
    @ObservationIgnored private var detailItemIDs: Set<UUID> = []
    @ObservationIgnored private var itemsHoldingDetailHistory: Set<UUID> = []
    /// Set when the list structure changed while no list was visible, so
    /// the catch-up refresh knows to drop models of removed items too.
    @ObservationIgnored private var liveModelsNeedPruning = false

    /// `nonisolated` so `applicationWillTerminate` — which runs on the main
    /// thread and cannot await anything — can still reach the engine. Safe
    /// because `DownloadEngine` is a `Sendable` reference type whose state
    /// the Rust core guards itself.
    private nonisolated let engine: DownloadEngine
    private nonisolated static let log = Logger(
        subsystem: "com.shayanoh.SDM", category: "engine")
    /// The text of the last tick/snapshot error that was logged, so a core
    /// that fails on every 5 Hz tick logs once rather than five times a second.
    @ObservationIgnored private var lastLoggedTickError: String?
    /// The generation of the last published snapshot, `nil` before the first.
    /// The Rust engine returns the same snapshot (same generation) while
    /// nothing changed, which is what the old `newSnapshot != snapshot`
    /// equality check was standing in for.
    @ObservationIgnored private var publishedGeneration: UInt64?
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

        // yt-dlp / ffmpeg are looked for in the managed `bin/` directory. The
        // resolver settings start at what the Swift app's closures returned
        // on first use; `applyYtDlpSettings()` re-sends them when Settings
        // changes them.
        do {
            let config = EngineConfig(
                stateFile: support.appendingPathComponent("state.json"),
                settings: try Self.storedSettings(downloadFolder: downloads),
                binarySearchPaths: [ManagedBinariesController.binDirectory],
                cookieSource: MediaSitesSettingsStore.cookieSource,
                maxPlaylistVideos: UInt32(MediaSitesSettingsStore.maxPlaylistVideos),
                ytDlpExtraArguments: ManagedBinariesController.ytDlpExtraArguments)
            engine = try DownloadEngine(config: config)
        } catch {
            // The configuration is built from clamped app settings and fixed
            // paths, so this is a bug, not a condition the user can fix; there
            // is no engine to run without it.
            fatalError("SDM could not start its download core: \(error.localizedDescription)")
        }
    }

    private static func storedSettings(downloadFolder: URL) throws -> EngineSettings {
        try EngineSettings(
            maxConcurrent: EngineSettingsStore.maxConcurrent,
            segmentsPerItem: EngineSettingsStore.segmentsPerItem,
            globalMaxConnections: EngineSettingsStore.globalMaxConnections,
            maxConnectionsPerHost: EngineSettingsStore.maxConnectionsPerHost,
            downloadFolder: downloadFolder,
            minSegmentSizeBytes: Int64(EngineSettingsStore.minSegmentSizeMB) * 1024 * 1024)
    }

    /// Re-reads every `EngineSettingsStore` value and applies it live. Called
    /// from `SettingsView` whenever the operator changes a value.
    func applyStoredSettings() async {
        applyYtDlpSettings()
        do {
            try await engine.updateSettings(
                settings: try Self.storedSettings(downloadFolder: downloadFolder))
        } catch {
            report("apply the new settings", error)
        }
    }

    /// Sends the yt-dlp settings (cookie source, playlist cap) to the engine.
    /// The Swift core read these through closures on every use; the Rust
    /// engine holds copies, so every change must be pushed. The extra
    /// arguments are fixed at construction.
    func applyYtDlpSettings() {
        do {
            try engine.setCookieSource(source: MediaSitesSettingsStore.cookieSource)
            try engine.setMaxPlaylistVideos(
                count: UInt32(MediaSitesSettingsStore.maxPlaylistVideos))
        } catch {
            report("apply the yt-dlp settings", error)
        }
    }

    /// Points a binary (`"yt-dlp"`, `"ffmpeg"`) at a user-chosen path, or back
    /// at the default lookup with `nil`.
    func setBinaryOverride(name: String, path: URL?) {
        do {
            try engine.setBinaryOverride(name: name, path: path)
        } catch {
            report("change the \(name) location", error)
        }
    }

    /// Surfaces a failed user action: logged, and shown as a banner, because
    /// the engine used to be unable to fail and nothing else tells the user
    /// that a click did nothing.
    private func report(_ action: String, _ error: Error) {
        Self.log.error("Could not \(action, privacy: .public): \(error.localizedDescription)")
        notifications.postInfo("Could not \(action): \(error.localizedDescription)")
    }

    /// Logs a heartbeat failure once per distinct message and leaves the UI on
    /// the previous snapshot; the next tick tries again.
    private func logTickFailure(_ error: Error) {
        let message = error.localizedDescription
        guard message != lastLoggedTickError else { return }
        lastLoggedTickError = message
        Self.log.error("Engine heartbeat failed: \(message, privacy: .public)")
    }

    /// Runs one user action against the engine and republishes the snapshot
    /// whether or not it worked (a refused call may still have changed
    /// nothing, which `publish` ignores).
    private func perform(_ action: String, _ body: () async throws -> Void) async {
        do {
            try await body()
        } catch {
            report(action, error)
        }
        await republish()
    }

    private func republish() async {
        do {
            publish(try await engine.snapshot())
        } catch {
            logTickFailure(error)
        }
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
        do {
            try await engine.restore()
        } catch {
            // Without the saved list the app still works; the user just
            // starts with an empty one. Say so rather than failing silently.
            report("load your saved downloads", error)
        }
        // `restore()` always lands every non-terminal item `.stopped` — the
        // store never persists `.queued`/`.running` (see
        // `DownloadEngine.persist()`) — so "resume downloads automatically on
        // launch" is nothing more than the same `resumeAll()` Resume All
        // uses: enabled, stopped items requeue; disabled ones stay put. Off
        // by default means simply not calling it, leaving everything exactly
        // as `restore()` left it.
        if EngineSettingsStore.autoStartDownloadsOnLaunch {
            do {
                try await engine.resumeAll()
            } catch {
                report("resume your downloads", error)
            }
        }
        await republish()
        notifications.requestAuthorization()

        while !Task.isCancelled {
            do {
                try await engine.tick()
                // Asking only for a snapshot newer than the last one this
                // loop saw lets an idle engine answer without serialising the
                // whole list. It is the loop's own last snapshot, not the
                // published one: a user action can publish a newer snapshot in
                // between, and the notifications below must still see every
                // transition, starting from a first snapshot to compare with.
                let changed = try await engine.snapshotIfChanged(
                    sinceGeneration: previousSnapshot?.generation)
                lastLoggedTickError = nil
                if let newSnapshot = changed {
                    notifyChanges(from: previousSnapshot, to: newSnapshot)
                    previousSnapshot = newSnapshot
                    publish(newSnapshot)
                }
                await refreshDetailHistories()
            } catch {
                logTickFailure(error)
            }
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
        // The engine hands back the same snapshot, `generation` included,
        // while nothing changed. User actions and the heartbeat call the engine
        // concurrently, so a slower, older snapshot can resume after a newer
        // one; the generation only grows, so only a newer one is published.
        if let published = publishedGeneration, newSnapshot.generation <= published { return }
        publishedGeneration = newSnapshot.generation
        // Runs last, once everything below is in its new state.
        defer { onPublish?() }
        let priorSnapshot = snapshot
        snapshot = newSnapshot
        stats.update(from: newSnapshot)

        let structuralChanged = !Self.packagesStructurallyEqual(
            structuralPackages, newSnapshot.packages)

        // Models are brought up to date *before* `structuralPackages` is
        // reassigned below, so a row created for a newly added item finds
        // its model already there. Skipped entirely while no list is on
        // screen — see `setListVisible(_:)`.
        if isListVisible {
            refreshLiveModels(
                from: newSnapshot, previous: priorSnapshot,
                structuralChanged: structuralChanged)
        } else if structuralChanged {
            liveModelsNeedPruning = true
        }

        guard structuralChanged else { return }
        structuralPackages = newSnapshot.packages
        let completed = completedOnly(newSnapshot.packages)
        if !Self.packagesStructurallyEqual(completedPackages, completed) {
            completedPackages = completed
        }
    }

    /// Brings every item's and package's live model in line with
    /// `snapshot`, creating missing ones and — when the set of items may
    /// have changed — dropping the ones whose item or package is gone.
    ///
    /// - Parameter previous: the snapshot the models were last refreshed
    ///   from, or `nil` when they may be arbitrarily out of date (catching
    ///   up after the list was hidden). A package whose items are unchanged
    ///   since `previous` is skipped — and the engine hands back the *same*
    ///   `items` storage for an unchanged package, so that check is O(1)
    ///   rather than a walk over every item.
    private func refreshLiveModels(
        from snapshot: EngineSnapshot, previous: EngineSnapshot?, structuralChanged: Bool
    ) {
        let previousPackages = structuralChanged ? nil : previous?.packages
        for (index, package) in snapshot.packages.enumerated() {
            // With no structural change the packages line up by index. The
            // engine stamps a package with a new `revision` whenever anything
            // in it changed — items, aggregate, or the global rate its ETA
            // depends on — so an equal revision means nothing to update.
            if let previousPackages, index < previousPackages.count,
                previousPackages[index].id == package.id,
                previousPackages[index].revision == package.revision,
                packageModels[package.id] != nil
            {
                continue
            }
            for item in package.items {
                if let model = itemModels[item.id] {
                    model.update(from: item)
                } else {
                    itemModels[item.id] = ItemLiveModel(item)
                }
            }
            // The aggregate is computed once per snapshot by the core.
            if let model = packageModels[package.id] {
                model.update(package.aggregate)
            } else {
                packageModels[package.id] = PackageLiveModel(
                    id: package.id, aggregate: package.aggregate)
            }
        }

        guard structuralChanged else { return }
        let liveItemIDs = Set(snapshot.packages.flatMap { $0.items.map(\.id) })
        itemModels = itemModels.filter { liveItemIDs.contains($0.key) }
        let livePackageIDs = Set(snapshot.packages.map(\.id))
        packageModels = packageModels.filter { livePackageIDs.contains($0.key) }
    }

    /// Declares which items a details panel is showing, so their *full*
    /// speed histories are fetched from the engine each tick; every other
    /// item only ever has the short tail its snapshot carries. `owner`
    /// identifies the panel — the Downloads and Completed tabs each have
    /// one, and during a tab switch one may appear before the other has
    /// gone — and an empty set withdraws that panel's claim.
    func setDetailItems(_ itemIDs: Set<UUID>, for owner: UUID) {
        if itemIDs.isEmpty {
            detailItemsByOwner[owner] = nil
        } else {
            detailItemsByOwner[owner] = itemIDs
        }
        let union = detailItemsByOwner.values.reduce(into: Set<UUID>()) { $0.formUnion($1) }
        guard union != detailItemIDs else { return }
        detailItemIDs = union
        // Fetched straight away rather than on the next tick, so a newly
        // selected item's graph does not sit at its short history first.
        Task { await refreshDetailHistories() }
    }

    private func refreshDetailHistories() async {
        let wanted = detailItemIDs
        var histories: [UUID: [Double]] = [:]
        if !wanted.isEmpty {
            do {
                histories = try await engine.speedHistories(itemIds: wanted)
            } catch {
                // Keep the short tails already on screen.
                logTickFailure(error)
                return
            }
        }
        // The selection may have moved on while the engine was answering.
        guard wanted == detailItemIDs else { return }
        for itemID in itemsHoldingDetailHistory.subtracting(wanted) {
            itemModels[itemID]?.setDetailHistory(nil)
        }
        for itemID in wanted {
            itemModels[itemID]?.setDetailHistory(histories[itemID])
        }
        itemsHoldingDetailHistory = wanted
    }

    /// Tells the controller whether any downloads list is on screen. The
    /// per-item and per-package live models exist only to feed list rows,
    /// so with the main window closed — SDM keeps downloading from the menu
    /// bar — updating them every tick is work nobody can see. `snapshot`,
    /// `stats` and `structuralPackages` are unaffected: the menu bar icon,
    /// its popover and the Touch Bar keep reading those.
    ///
    /// Turning it back on refreshes every model from the current snapshot
    /// on the spot, so rows that reappear never show figures from before
    /// the window closed.
    func setListVisible(_ visible: Bool) {
        guard visible != isListVisible else { return }
        isListVisible = visible
        guard visible else { return }
        refreshLiveModels(
            from: snapshot, previous: nil, structuralChanged: liveModelsNeedPruning)
        liveModelsNeedPruning = false
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
            aggregate: package.aggregate)
        packageModels[package.id] = model
        return model
    }

    /// Spec §9.1: "Completed is a filtered view of the same list, not a
    /// separate store."
    private func completedOnly(_ packages: [PackageSnapshot]) -> [PackageSnapshot] {
        packages.compactMap { package in
            let completedItems = package.items.filter { $0.state == .completed }
            guard !completedItems.isEmpty else { return nil }
            // The header figures of the filtered copy sum just its items,
            // computed by the core like the engine's own packages'.
            let aggregate =
                (try? packageAggregateForItems(
                    items: completedItems, globalRecentSpeed: stats.recentGlobalSpeed))
                ?? package.aggregate
            return PackageSnapshot(
                id: package.id, name: package.name, priority: package.priority,
                items: completedItems, aggregate: aggregate, revision: package.revision)
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
            // An unchanged package keeps its `revision`; no need to walk it.
            if packageA.revision == packageB.revision { continue }
            for (itemA, itemB) in zip(packageA.items, packageB.items) {
                guard itemA.id == itemB.id, itemA.url == itemB.url,
                    itemA.filename == itemB.filename, itemA.state == itemB.state,
                    itemA.isEnabled == itemB.isEnabled, itemA.isResumable == itemB.isResumable,
                    itemA.checkpointFailure == itemB.checkpointFailure,
                    itemA.remainingAttempts == itemB.remainingAttempts,
                    itemA.fileMissing == itemB.fileMissing,
                    // The row draws these too. Left out, "Assembling…" and
                    // a retry's failure reason only appeared once something
                    // else here happened to change. (`retryHoldSeconds` is
                    // deliberately *not* here — see `ItemLiveModel`.)
                    itemA.isAssembling == itemB.isAssembling,
                    itemA.failedAttemptCount == itemB.failedAttemptCount,
                    itemA.lastFailureReason == itemB.lastFailureReason
                else { return false }
            }
        }
        return true
    }

    /// Every item of a non-empty package has completed.
    private static func isFinished(_ package: PackageSnapshot) -> Bool {
        !package.items.isEmpty && package.items.allSatisfy { $0.state == .completed }
    }

    /// Compares two heartbeats' worth of snapshot and fires exactly the
    /// notifications spec §9.8 lists, once per transition — never a repeat
    /// for a state that has already been announced.
    private func notifyChanges(from old: EngineSnapshot?, to new: EngineSnapshot) {
        guard let old, old.generation != new.generation else { return }

        // Only packages whose content changed can hold a transition, and an
        // unchanged package keeps its `revision`, so finding them costs one
        // cheap compare per package. Both sides are collected — an item moved between packages changed
        // its old package *and* its new one — so its previous state is
        // always among `changedOld` and a move never reads as "finished".
        let oldByID = Dictionary(uniqueKeysWithValues: old.packages.map { ($0.id, $0) })
        let newByID = Dictionary(uniqueKeysWithValues: new.packages.map { ($0.id, $0) })
        let changedNew = new.packages.filter { oldByID[$0.id]?.revision != $0.revision }
        guard !changedNew.isEmpty else { return }
        let changedOld = old.packages.filter { newByID[$0.id]?.revision != $0.revision }

        let oldStates = Dictionary(
            uniqueKeysWithValues: changedOld.flatMap(\.items).map { ($0.id, $0.state) })
        for item in changedNew.flatMap(\.items) {
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

        for package in changedNew {
            let isDone = Self.isFinished(package)
            guard isDone else { continue }
            let wasDone =
                oldByID[package.id].map(Self.isFinished) ?? false
            guard !wasDone else { continue }
            notifications.notifyPackageFinished(name: package.name)
        }
    }

    func setEnabled(_ enabled: Bool, for itemID: UUID) async {
        await perform("change the download") {
            try await engine.setEnabled(enabled: enabled, itemId: itemID)
        }
    }

    func startItem(_ itemID: UUID) async {
        await perform("start the download") { try await engine.startItem(itemId: itemID) }
    }

    func stopItem(_ itemID: UUID) async {
        await perform("stop the download") { try await engine.stopItem(itemId: itemID) }
    }

    func retry(_ itemID: UUID) async {
        await perform("retry the download") { try await engine.retry(itemId: itemID) }
    }

    /// Re-runs only the `ffmpeg` mux for a failed muxed item whose parts are
    /// all downloaded. Spec §9.3's "Retry mux".
    func retryMux(_ itemID: UUID) async {
        await perform("retry the mux") { try await engine.retryMux(itemId: itemID) }
    }

    func moveItem(_ itemID: UUID, toPackage packageID: UUID, atIndex index: Int? = nil) async {
        await moveItems([itemID], toPackage: packageID, atIndex: index)
    }

    func moveItems(_ itemIDs: [UUID], toPackage packageID: UUID, atIndex index: Int? = nil) async {
        await perform("move the downloads") {
            try await engine.moveItemsToPackage(
                itemIds: itemIDs, packageId: packageID, atIndex: index)
        }
    }

    func moveItems(_ itemIDs: [UUID], toNewPackageNamed packageName: String) async {
        await perform("move the downloads") {
            try await engine.moveItemsToNewPackage(itemIds: itemIDs, packageName: packageName)
        }
    }

    func reorderPackages(_ packageIDs: [UUID]) async {
        await perform("reorder the packages") {
            try await engine.reorderPackages(packageIds: packageIDs)
        }
    }

    func reorderItems(_ itemIDs: [UUID], inPackage packageID: UUID) async {
        await perform("reorder the downloads") {
            try await engine.reorderItems(itemIds: itemIDs, packageId: packageID)
        }
    }

    /// Global pause/resume: stops or (re-)queues every stoppable/resumable
    /// item in one call. Never touches any item's `isEnabled`.
    func pauseAll() async {
        await perform("pause the downloads") { try await engine.pauseAll() }
    }

    func resumeAll() async {
        await perform("resume the downloads") { try await engine.resumeAll() }
    }

    func removeItem(_ itemID: UUID, deleteFile: Bool) async {
        await perform("remove the download") {
            try await engine.removeItem(itemId: itemID, deleteFile: deleteFile)
        }
    }

    func removePackage(_ packageID: UUID, deleteFiles: Bool) async {
        await perform("remove the package") {
            try await engine.removePackage(packageId: packageID, deleteFiles: deleteFiles)
        }
    }

    func resetDownload(_ itemID: UUID) async {
        await perform("reset the download") { try await engine.resetDownload(itemId: itemID) }
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
        // One failure does not stop the rest; the first is reported.
        var firstError: Error?
        for itemID in itemIDs {
            do {
                try await engine.removeItem(itemId: itemID, deleteFile: deleteFile)
            } catch {
                firstError = firstError ?? error
            }
        }
        if let firstError { report("remove some downloads", firstError) }
        await republish()
    }

    /// Hands a grabbed package off to the download engine. Spec §7.5's "Add
    /// to downloads" / "Add and start".
    func addPackage(name: String, urlItems: [PackageUrlItem], startImmediately: Bool) async {
        guard !urlItems.isEmpty else { return }
        let items: [DownloadItem]
        do {
            items = try urlItems.map { urlItem in
                try downloadItemForUrl(
                    url: urlItem.url,
                    filename: urlItem.effectiveFilename
                        ?? (urlItem.url.lastPathComponent.isEmpty
                            ? "download" : urlItem.url.lastPathComponent),
                    totalBytes: urlItem.size,
                    state: startImmediately ? .queued : .stopped)
            }
        } catch {
            report("add the download", error)
            return
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
        await perform("add the download") {
            try await engine.add(
                package: try downloadPackageNew(name: name, items: staged, note: note))
        }
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
            // Nothing useful to do with a failure at quit; the state file was
            // flushed by the debounce, and the process is ending either way.
            try? await engine.shutdown()
            done.signal()
        }
        _ = done.wait(timeout: .now() + timeout)
    }
}
