import Foundation
import SDMCore

public struct EngineSettings: Sendable {
    public var maxConcurrent: Int
    public var segmentsPerItem: Int
    public var globalMaxConnections: Int
    /// Spec §6.4 and §12 (default 8): enforced together with
    /// `globalMaxConnections` by `ConnectionAllocator`, largest pool
    /// yielding first.
    public var maxConnectionsPerHost: Int
    public var downloadFolder: URL
    /// Bytes written per worker between sidecar checkpoints (spec §4.3's byte
    /// half). Settable so a test can drive the checkpoint path without moving
    /// eight megabytes.
    public var checkpointIntervalBytes: Int64
    /// Heartbeat ticks that must pass with no further change before durable
    /// state is written. Spec §4.2's "~2 s after the last change" debounce,
    /// expressed in ticks at `AppTiming.ticksPerSecond`.
    public var persistDebounceTicks: Int
    /// The floor `DownloadTask.stealableRemainder()` enforces when splitting a
    /// busy worker's claim to hand half to an idle one: a split only happens
    /// when both resulting halves would still be at least this large. An
    /// unclaimed free gap is always handed out whole regardless of size — this
    /// only bounds *splitting*, not claiming. Settings UI default is 10 MB,
    /// exposed there in 1...100 MB.
    public var minSegmentSizeBytes: Int64

    public init(
        maxConcurrent: Int,
        segmentsPerItem: Int,
        globalMaxConnections: Int,
        maxConnectionsPerHost: Int = 8,
        downloadFolder: URL,
        checkpointIntervalBytes: Int64 = 8 * 1024 * 1024,
        persistDebounceTicks: Int = AppTiming.ticksPerSecond * 2,
        minSegmentSizeBytes: Int64 = 10 * 1024 * 1024
    ) {
        precondition(maxConcurrent >= 1, "maxConcurrent must be at least 1")
        precondition(segmentsPerItem >= 1, "segmentsPerItem must be at least 1")
        precondition(globalMaxConnections >= 1, "globalMaxConnections must be at least 1")
        precondition(maxConnectionsPerHost >= 1, "maxConnectionsPerHost must be at least 1")
        precondition(checkpointIntervalBytes > 0, "checkpointIntervalBytes must be positive")
        precondition(persistDebounceTicks >= 1, "persistDebounceTicks must be at least 1")
        precondition(minSegmentSizeBytes > 0, "minSegmentSizeBytes must be positive")
        self.maxConcurrent = maxConcurrent
        self.segmentsPerItem = segmentsPerItem
        self.globalMaxConnections = globalMaxConnections
        self.maxConnectionsPerHost = maxConnectionsPerHost
        self.downloadFolder = downloadFolder
        self.checkpointIntervalBytes = checkpointIntervalBytes
        self.persistDebounceTicks = persistDebounceTicks
        self.minSegmentSizeBytes = minSegmentSizeBytes
    }
}

/// Failures the engine itself produces, as opposed to the transport or the
/// per-download task. Routed through `RetryPolicy.classify` like any other
/// error so they reach a terminal `.failed(reason:)` instead of a retry loop.
public enum EngineError: Error, Equatable {
    /// The per-package output folder could not be created — permissions, a
    /// read-only volume, a file sitting where the folder should be.
    case destinationFolderUnavailable(path: String, underlying: String)
}

/// Owns the package list, the running `DownloadTask`s, and the scheduler.
///
/// `tick()` is driven by the app once per second rather than by an internal
/// timer, so the engine is fully deterministic under test. It is also what
/// picks up newly runnable work: a finished download does not reschedule from
/// inside its own completion path, because a transient failure would then
/// re-attempt itself with no gap at all. Re-attempts are governed by
/// `RetryPolicy` — a per-item attempt counter, tick-counted backoff before an
/// item may be re-desired, and a terminal `.failed(reason:)` once
/// `maxAttempts` consecutive attempts have failed.
public actor DownloadEngine {
    /// One in-flight item: the task doing the work and the job awaiting it.
    ///
    /// A preempted or user-stopped runner is marked retiring and kept here until its job
    /// has actually unwound. That is what stops a second `DownloadTask` from
    /// being pointed at a destination file the first one has not finished
    /// with.
    ///
    /// It does *not* bound `runners.count` by `maxConcurrent`. A retiring
    /// runner is still reported to the scheduler in `runningNow`, but
    /// `runningNow` only feeds passes 1 and 2, and a resumable (or not yet
    /// probed) item is eligible for neither — pass 1 covers only
    /// `isResumable == false`, and pass 2 is empty while hysteresis is
    /// unwired. So a retiring resumable item can be outranked, leaving
    /// `runners` transiently holding the retiring runner *plus* a full
    /// complement of new ones. That is benign: a retiring runner has already
    /// been paused, so its worker target is zero and its descriptor is
    /// closed, and it issues no further requests. What is bounded at all
    /// times is the number of items in state `.running`, since preemption
    /// sets `.queued` synchronously.
    /// One component of an in-flight item. A generic HTTP item has one; a
    /// muxed YouTube item has two; an HLS/DASH item has one `.wholesale`
    /// worker. Parent spec §7.1 / `2026-09-03-multi-site-resolver-design.md` §6.7.
    private enum ComponentWorker {
        case segmented(DownloadTask)
        case wholesale(WholesaleComponentTask)
    }

    private struct ComponentRun {
        /// Index into `DownloadItem.components` — stable across the run.
        let componentIndex: Int
        var worker: ComponentWorker
        let destinationURL: URL

        /// The `DownloadTask`, when this is a segmented component (the
        /// `403`-refresh path only applies there).
        var segmentedTask: DownloadTask? {
            if case .segmented(let task) = worker { return task }
            return nil
        }

        func start() async throws -> URL {
            switch worker {
            case .segmented(let task): return try await task.start()
            case .wholesale(let task): return try await task.start()
            }
        }

        func pause() async {
            switch worker {
            case .segmented(let task): await task.pause()
            case .wholesale(let task): await task.pause()
            }
        }

        var completedRanges: RangeSet {
            get async {
                switch worker {
                case .segmented(let task): return await task.completedRanges
                case .wholesale(let task): return await task.completedRanges
                }
            }
        }

        var expectedTotalBytes: Int64? {
            get async {
                switch worker {
                case .segmented(let task): return await task.expectedTotalBytes
                case .wholesale(let task): return await task.expectedTotalBytes
                }
            }
        }

        var activeWorkerCount: Int {
            get async {
                switch worker {
                case .segmented(let task): return await task.activeWorkerCount
                case .wholesale(let task): return await task.activeWorkerCount
                }
            }
        }

        var probedSupportsRanges: Bool? {
            get async {
                switch worker {
                case .segmented(let task): return await task.probedSupportsRanges
                case .wholesale(let task): return await task.probedSupportsRanges
                }
            }
        }

        var lastCheckpointFailure: String? {
            get async {
                switch worker {
                case .segmented(let task): return await task.lastCheckpointFailure
                case .wholesale(let task): return await task.lastCheckpointFailure
                }
            }
        }

        var isAssembling: Bool {
            get async {
                if case .wholesale(let task) = worker { return await task.isAssembling }
                return false
            }
        }

        func checkpointTick() async {
            if case .segmented(let task) = worker { await task.checkpointTick() }
        }

        func setWorkerCount(_ count: Int) async {
            if case .segmented(let task) = worker { await task.setWorkerCount(count) }
        }
    }

    /// One in-flight item: every component's task plus the single job
    /// awaiting them and running assembly.
    private struct Runner {
        var components: [ComponentRun]
        let job: Task<Void, Never>
        /// Identifies this specific run, so a superseded job's late
        /// `finishItem` cannot clobber its replacement.
        let runID: UUID
        var retireIntent: RetireIntent = .none

        var allDestinations: [URL] { components.map(\.destinationURL) }

        func pauseAllComponents() async {
            for component in components { await component.pause() }
        }
    }

    /// Why a runner is being torn down, decided at the moment it is marked
    /// retiring and consulted later, once `run()`'s job actually finishes, to
    /// pick the item's landing state. The two outcomes are genuinely
    /// different: a preempted item must come back `.queued` — it is still
    /// desired, just outranked for a slot right now — while a user-stopped
    /// item must land `.stopped` and stay there until explicitly started
    /// again.
    private enum RetireIntent: Sendable {
        case none
        case preempted
        case userStopped
    }

    private let transport: any HTTPTransport
    private let stateStore: any StateStore
    private let resolver: (any LinkResolver)?
    private let muxer: (any Muxer)?
    private let wholesaleDownloader: (any WholesaleDownloader)?
    private var settings: EngineSettings
    /// Items currently in the `ffmpeg` assembly step. Their `state` stays
    /// `.running`; the snapshot exposes this as `isAssembling`.
    private var assembling: Set<UUID> = []

    private var packages: [DownloadPackage] = []
    private var runners: [UUID: Runner] = [:]
    private var samplers: [UUID: SpeedSampler] = [:]
    private var globalSampler = SpeedSampler(
        historyLength: AppTiming.ticksPerSecond * 300, averagingWindowSeconds: 2)
    private var segmentOverrides: [UUID: Int] = [:]
    /// Byte totals already folded into the samplers, so progress read at 1 Hz
    /// is turned into per-window deltas exactly once.
    private var sampledBytes: [UUID: Int64] = [:]
    private var isShutDown = false
    private var hasRestored = false

    private let retryPolicy: RetryPolicy
    /// Consecutive failed attempts per item, reset by any attempt that made
    /// real progress and by success.
    private var failedAttempts: [UUID: Int] = [:]
    /// Ticks of backoff an item still owes before it may be re-desired.
    /// Counted in heartbeat ticks rather than measured against a clock, which
    /// is the same idiom `checkpointTick()` uses and keeps the engine
    /// deterministic under test.
    private var retryHoldTicks: [UUID: Int] = [:]
    /// The most recent transient failure's message per item, surfaced in the
    /// snapshot only while `failedAttempts` still holds the item (i.e. it is
    /// retrying, not yet terminal and not recovered).
    private var lastFailure: [UUID: String] = [:]
    /// Bytes an attempt started from, so a failure that nevertheless moved
    /// bytes can clear the attempt counter.
    private var attemptStartBytes: [UUID: Int64] = [:]
    /// Ticks elapsed since the first unflushed change, or `nil` when the store
    /// has nothing pending. Spec §4.2's debounce.
    private var ticksSincePendingChange: Int?
    /// The heartbeat tick each currently-tracked runner started on, so
    /// `reconcile()` can tell whether it is still inside spec §6.4's
    /// hysteresis window. Cleared when the runner finishes, in `finish()`.
    private var startedAtTick: [UUID: Int] = [:]
    /// Ticks elapsed since the engine started, driven by `tick()`. Counted
    /// rather than measured against a clock, matching every other
    /// tick-driven mechanism here (`retryHoldTicks`, checkpoint staleness).
    private var currentTick: Int = 0
    /// Spec §6.4: "An item started within the last ~5 s is not preempted,"
    /// expressed in ticks at `AppTiming.ticksPerSecond` rather than a raw
    /// tick count, so raising the heartbeat rate does not shrink the window.
    private let hysteresisWindowTicks = AppTiming.ticksPerSecond * 5
    /// The last checkpoint failure reported by each item's task, kept after
    /// the runner retires so the reason does not vanish from the UI the
    /// instant the download stops.
    private var checkpointFailures: [UUID: String] = [:]

    public init(
        transport: any HTTPTransport,
        stateStore: any StateStore,
        settings: EngineSettings,
        retryPolicy: RetryPolicy = RetryPolicy(),
        resolver: (any LinkResolver)? = nil,
        muxer: (any Muxer)? = nil,
        wholesaleDownloader: (any WholesaleDownloader)? = nil
    ) {
        self.transport = transport
        self.stateStore = stateStore
        self.settings = settings
        self.retryPolicy = retryPolicy
        self.resolver = resolver
        self.muxer = muxer
        self.wholesaleDownloader = wholesaleDownloader
    }

    // MARK: - Mutations

    public func add(_ package: DownloadPackage) async {
        if let originalPackageIndex = packages.firstIndex(where: { $0.name == package.name }) {
            packages[originalPackageIndex].items.append(contentsOf: package.items)
        } else {
            packages.append(package)
        }
        renumberAllPositions()
        for item in package.items where samplers[item.id] == nil {
            samplers[item.id] = SpeedSampler()
        }
        await persist()
        await reconcile()
    }

    /// Loads whatever `stateStore` has durably and installs it as the
    /// starting package list. Call once, before the first `tick()`.
    ///
    /// Idempotent via `hasRestored` rather than "call only when `packages` is
    /// empty": a second call (e.g. `EngineController`'s heartbeat re-firing
    /// after a window close/reopen that didn't end the process) is a no-op
    /// rather than dropping whatever `add()` may have contributed in the
    /// meantime. The expected call order is `restore()` once, at the head of
    /// the heartbeat, before any UI-driven `add()` can occur — calling
    /// `add()` first is not a case this method defends against, since the
    /// store itself only ever holds the latest full snapshot `persist()` last
    /// wrote, and an `add()`-triggered `persist()` before `restore()` has run
    /// would already have overwritten whatever `restore()` was going to read.
    ///
    /// Two adjustments are made to what comes back from the store, because
    /// both fields describe the *previous* process's runtime state, not
    /// anything true of this one:
    ///
    /// - `.running`/`.queued` becomes `.stopped`. `persist()` already writes
    ///   every non-terminal item as `.stopped` (see its doc comment), so this
    ///   only matters for a file from an older build or one hand-seeded by a
    ///   test — but the invariant "the store never says something is in
    ///   flight or about to run" should hold regardless of how the file got
    ///   here. Nothing in this process is actually running it: the runner
    ///   that made it `.running` died with the last process, and leaving it
    ///   `.running`/`.queued` would make the scheduler believe a slot is
    ///   occupied that is not, or auto-pick it up before the operator's
    ///   "resume on launch" setting has had a say.
    /// - `isResumable` resets to `nil`. It records what the *previous*
    ///   process's probe found; this process has probed nothing yet. The
    ///   byte-level safety net is `DownloadTask.prepare()`'s validator check
    ///   against the `.sdmpart` sidecar, which runs unconditionally on every
    ///   restart regardless of this flag — so resetting it costs nothing
    ///   there. What it does buy: a stale persisted `false` would otherwise
    ///   hand the item spec §6.3's unconditional-claim rule before this
    ///   process has verified anything, making it unpreemptible on faith.
    ///   `nil` puts it through the same probe-then-decide path a freshly
    ///   grabbed item takes.
    public func restore() async {
        guard !hasRestored else { return }
        hasRestored = true

        var restored = await stateStore.load().packages
        for packageIndex in restored.indices {
            for itemIndex in restored[packageIndex].items.indices {
                switch restored[packageIndex].items[itemIndex].state {
                case .running, .queued: restored[packageIndex].items[itemIndex].state = .stopped
                case .stopped, .completed, .failed: break
                }
                restored[packageIndex].items[itemIndex].isResumable = nil
            }
        }

        packages.append(contentsOf: restored)
        // Self-heals any position/array-order drift already sitting in a
        // persisted file from before this normalization existed — see
        // `renumberAllPositions()`.
        renumberAllPositions()
        for package in restored {
            for item in package.items where samplers[item.id] == nil {
                samplers[item.id] = SpeedSampler()
            }
        }
        await persist()
        await reconcile()
    }

    /// Applies new settings live. Spec §10.2's "changing the mode applies
    /// immediately" precedent extends to every engine setting: the very next
    /// `reconcile()` this call triggers picks up the new
    /// `maxConcurrent`/connection ceilings with no restart needed.
    public func updateSettings(_ newSettings: EngineSettings) async {
        settings = newSettings
        await reconcile()
    }

    /// User-initiated Disable/Enable. Purely the `isEnabled` axis — see
    /// `ItemState`'s doc comment for why this is independent of `state`.
    ///
    /// Disabling an item that is currently `.queued` or `.running` also stops
    /// it (mirrors `stopItem`'s effect on `state`), since "disabled" must mean
    /// "not running" immediately, not just "not eligible to be picked up
    /// next." Enabling deliberately does *not* resume it — re-enabling only
    /// makes the item startable again; the user (or Resume All) still has to
    /// start it explicitly. That asymmetry is the point: it is what makes
    /// Start disabled-but-visible rather than silently racing a re-enable.
    public func setEnabled(_ enabled: Bool, for itemID: UUID) async {
        mutateItem(itemID) { $0.isEnabled = enabled }
        if !enabled {
            switch itemState(for: itemID) {
            case .running:
                retireRunnerNonBlocking(itemID)
                mutateItem(itemID) { $0.state = .stopped }
            case .queued:
                mutateItem(itemID) { $0.state = .stopped }
            default:
                break
            }
        }
        await persist()
        await reconcile()
    }

    /// User-initiated Start: moves a `.stopped`, enabled item into `.queued`
    /// so the scheduler can pick it up. A no-op on anything else — a disabled
    /// item cannot be started (the UI disables the affordance too, this is
    /// the backstop), and starting a `.queued`/`.running` item has nothing to
    /// do.
    public func startItem(_ itemID: UUID) async {
        guard itemState(for: itemID) == .stopped, isItemEnabled(itemID) else { return }
        mutateItem(itemID) { $0.state = .queued }
        await persist()
        await reconcile()
    }

    /// User-initiated Stop: moves a `.queued` or `.running` item to
    /// `.stopped`, stopping its runner first if it has one. Does not touch
    /// `isEnabled` — a stopped-but-enabled item is exactly what Resume All
    /// looks for. A no-op on `.completed`/`.failed`/already-`.stopped` items.
    public func stopItem(_ itemID: UUID) async {
        switch itemState(for: itemID) {
        case .running:
            retireRunnerNonBlocking(itemID)
            mutateItem(itemID) { $0.state = .stopped }
            await persist()
            await reconcile()
        case .queued:
            mutateItem(itemID) { $0.state = .stopped }
            await persist()
            await reconcile()
        default:
            return
        }
    }

    public func setPriority(_ priority: Priority?, for itemID: UUID) async {
        mutateItem(itemID) { $0.priority = priority }
        await persist()
        await reconcile()
    }

    /// Applies a new item order within one package. Spec §9.3: "Dropping
    /// between rows reorders," and reordering writes the position field the
    /// scheduler's rank already reads — no separate re-scheduling step.
    public func reorderItems(_ itemIDs: [UUID], inPackage packageID: UUID) async {
        guard let packageIndex = packages.firstIndex(where: { $0.id == packageID }) else { return }
        var byID = Dictionary(
            uniqueKeysWithValues: packages[packageIndex].items.map { ($0.id, $0) })
        var reordered: [DownloadItem] = []
        for (position, id) in itemIDs.enumerated() {
            guard var item = byID.removeValue(forKey: id) else { continue }
            item.position = position
            reordered.append(item)
        }
        reordered.append(contentsOf: byID.values)
        packages[packageIndex].items = reordered
        await persist()
        await reconcile()
    }

    /// Moves an item to a position — possibly within its own package
    public func moveItem(_ itemID: UUID, toPackage packageID: UUID, atIndex index: Int? = nil) async
    {
        await moveItems([itemID], toPackage: packageID, atIndex: index)
    }

    /// Moves items into a new package
    public func moveItems(_ itemIDs: [UUID], toNewPackageNamed packageName: String) async {
        guard !itemIDs.isEmpty else { return }
        let newPackage = DownloadPackage(name: packageName)
        packages.append(newPackage)
        await moveItems(itemIDs, toPackage: newPackage.id, atIndex: 0)
    }

    /// `moveItem`'s multi-select counterpart: moves an ordered batch of
    /// items to a destination package in one atomic step, rather than one
    /// `moveItem` call per item (which would re-run `reconcile()`/`persist()`
    /// once per item and let each move's index arithmetic see the others'
    /// already-applied shifts instead of the pre-drop layout the operator
    /// actually dragged from).
    ///
    /// `itemIDs` is trusted to already be in the exact relative order they
    /// should land in — this does not re-sort them. The caller
    /// (`PackagesListView`'s drop handling) is the one place that knows
    /// "list order" as opposed to whatever order a multi-selection drag
    /// session happens to hand back, which is unspecified.
    ///
    /// All-or-nothing, generalizing `moveItem`'s single-item refusal: if any
    /// named item does not exist or is `.running`, the whole batch is
    /// refused rather than silently moving a confusing subset.
    ///
    /// `atIndex` positions the *whole batch* as one contiguous run within
    /// the destination package's item list, indexed against that list
    /// before any of the batch is removed from wherever it currently sits —
    /// same "insert before whatever is currently at this index" semantics
    /// as `moveItem`'s `atIndex`. `nil` appends the batch at the end.
    public func moveItems(_ itemIDs: [UUID], toPackage packageID: UUID, atIndex index: Int? = nil)
        async
    {
        guard !itemIDs.isEmpty,
            let destinationIndex = packages.firstIndex(where: { $0.id == packageID })
        else { return }

        var sources: [UUID: (packageIndex: Int, itemIndex: Int)] = [:]
        for itemID in itemIDs {
            guard let source = location(of: itemID),
                packages[source.packageIndex].items[source.itemIndex].state != .running
            else { return }
            sources[itemID] = source
        }

        // Snapshot the moved items themselves before any removal below
        // touches the arrays they currently sit in.
        let movedItems = itemIDs.map {
            packages[sources[$0]!.packageIndex].items[sources[$0]!.itemIndex]
        }

        // How many of the batch sit ahead of `index` within the destination
        // package itself — same "removing it first shifts everything after
        // it down by one" adjustment `moveItem` made for a single item,
        // generalized to however many of the batch are also sourced from
        // the destination.
        var insertionIndex = index ?? packages[destinationIndex].items.count
        if let index {
            let precedingWithinDestination = itemIDs.filter {
                sources[$0]!.packageIndex == destinationIndex && sources[$0]!.itemIndex < index
            }.count
            insertionIndex = index - precedingWithinDestination
        }

        let destinationFolder = settings.downloadFolder.appendingPathComponent(
            packages[destinationIndex].name)
        for (itemID, item) in zip(itemIDs, movedItems) {
            let source = sources[itemID]!
            guard source.packageIndex != destinationIndex else { continue }
            let sourceFolder = settings.downloadFolder.appendingPathComponent(
                packages[source.packageIndex].name)
            relocateItemFiles(item, from: sourceFolder, to: destinationFolder)
        }

        // Removed highest-index-first within each source package so an
        // earlier removal never invalidates a later one's stored index.
        let byPackage = Dictionary(grouping: itemIDs) { sources[$0]!.packageIndex }
        for (packageIndex, ids) in byPackage {
            for itemIndex in ids.map({ sources[$0]!.itemIndex }).sorted(by: >) {
                packages[packageIndex].items.remove(at: itemIndex)
            }
        }

        insertionIndex = min(max(insertionIndex, 0), packages[destinationIndex].items.count)
        packages[destinationIndex].items.insert(contentsOf: movedItems, at: insertionIndex)
        renumberPositions(inPackageAt: destinationIndex)

        // Descending so removing an emptied source package doesn't shift
        // the index of another not-yet-processed one still pending below
        // it in this same loop.
        var emptySourceFolders: [URL] = []
        for packageIndex in byPackage.keys.sorted(by: >) where packageIndex != destinationIndex {
            if packages[packageIndex].items.isEmpty {
                emptySourceFolders.append(
                    settings.downloadFolder.appendingPathComponent(packages[packageIndex].name))
                packages.remove(at: packageIndex)
            } else {
                renumberPositions(inPackageAt: packageIndex)
            }
        }
        for folder in emptySourceFolders {
            removeFolderIfEmpty(folder)
        }

        await persist()
        await reconcile()
    }

    /// Renumbers a package's items to a contiguous `0..<count` sequence
    /// matching their array order — the same normalization `reorderItems`
    /// already applies to a full reordering, needed here too since
    /// `Scheduler` ranks directly on `item.position` (see `Scheduler.swift`),
    /// so an inserted item's position must not collide with or fall out of
    /// order against its new siblings'.
    private func renumberPositions(inPackageAt packageIndex: Int) {
        for index in packages[packageIndex].items.indices {
            packages[packageIndex].items[index].position = index
        }
    }

    /// Renumbers every package's position, and every item's position within
    /// it, to match current array order — `packages` array order is what
    /// `snapshot()` actually displays, but `Scheduler.rank` sorts by the
    /// `position` field, not array order, so the two must never drift apart.
    /// `moveItem`/`reorderItems`/`reorderPackages` keep that invariant by
    /// construction (they renumber after rearranging), but `add(_:)` and
    /// `restore()` hand `packages`/`items` a caller-supplied struct whose
    /// `position` defaults to `0` regardless of where it lands in the array
    /// — trusting that value let a freshly added package silently collide
    /// with (or outrank) an existing one that already had a real position,
    /// scheduling it out of the order actually shown in the list.
    private func renumberAllPositions() {
        for packageIndex in packages.indices {
            packages[packageIndex].position = packageIndex
            renumberPositions(inPackageAt: packageIndex)
        }
    }

    /// Moves whichever of an item's on-disk files currently exist — the
    /// finished file, the `.incomplete` sparse file, the `.sdmpart` resume
    /// sidecar — from one package folder to another. Best-effort per file,
    /// same as `trashIfExists`/`removeFolderIfEmpty` elsewhere in this type:
    /// a missing source file (the common case — most of these three exist
    /// for any given item, not all of them) is simply skipped rather than
    /// treated as an error.
    private func relocateItemFiles(
        _ item: DownloadItem, from sourceFolder: URL, to destinationFolder: URL
    ) {
        try? FileManager.default.createDirectory(
            at: destinationFolder, withIntermediateDirectories: true)
        let sources = itemArtefactURLs(item, in: sourceFolder)
        let destinations = itemArtefactURLs(item, in: destinationFolder)
        for (from, to) in zip(sources, destinations) {
            guard FileManager.default.fileExists(atPath: from.path) else { continue }
            try? FileManager.default.moveItem(at: from, to: to)
        }
    }

    private func location(of itemID: UUID) -> (packageIndex: Int, itemIndex: Int)? {
        for packageIndex in packages.indices {
            if let itemIndex = packages[packageIndex].items.firstIndex(where: { $0.id == itemID }) {
                return (packageIndex, itemIndex)
            }
        }
        return nil
    }

    /// Applies a new package order. Same idiom as `reorderItems`: any id not
    /// present in `packageIDs` keeps its relative order, appended at the end.
    public func reorderPackages(_ packageIDs: [UUID]) async {
        var byID = Dictionary(uniqueKeysWithValues: packages.map { ($0.id, $0) })
        var reordered: [DownloadPackage] = []
        for (position, id) in packageIDs.enumerated() {
            guard var package = byID.removeValue(forKey: id) else { continue }
            package.position = position
            reordered.append(package)
        }
        reordered.append(contentsOf: byID.values)
        packages = reordered
        await persist()
    }

    /// Global pause: exactly "select every item and click Stop," nothing
    /// more — same `stopItem` call, same eligibility, per item. Never touches
    /// `isEnabled`.
    public func pauseAll() async {
        for itemID in packages.flatMap(\.items).map(\.id) {
            await stopItem(itemID)
        }
    }

    /// Global resume: exactly "select every item and click Start," nothing
    /// more — same `startItem` call, same eligibility (enabled and
    /// `.stopped`), per item. A disabled item is left alone, same as it
    /// would be by an individual Start.
    public func resumeAll() async {
        for itemID in packages.flatMap(\.items).map(\.id) {
            await startItem(itemID)
        }
    }

    /// Removes one item: stops its runner if it is currently running, always
    /// drops its resume sidecar (a sidecar with no owning list entry is just
    /// an orphan), and — only when `deleteFile` is true — trashes whatever
    /// bytes it wrote (both the in-progress `.incomplete` file and, if it had
    /// already finished, the final file) and, if that was the last item in
    /// its package, trashes the now-empty package folder too.
    ///
    /// `deleteFile == false` is deliberately non-destructive: it only edits
    /// the list, so "remove from list" can never surprise someone by taking
    /// their downloaded bytes with it.
    public func removeItem(_ itemID: UUID, deleteFile: Bool) async {
        guard let loc = location(of: itemID) else { return }
        let package = packages[loc.packageIndex]
        let item = package.items[loc.itemIndex]
        let folder = settings.downloadFolder.appendingPathComponent(package.name)

        await stopRunnerIfRunning(itemID)

        // Sidecars go regardless of `deleteFile` (an orphan sidecar is
        // useless); bytes only when asked.
        for url in itemArtefactURLs(item, in: folder) {
            if url.pathExtension == "sdmpart" {
                ResumeSidecar.remove(at: url)
            } else if deleteFile {
                trashIfExists(url)
            }
        }

        guard let currentLoc = location(of: itemID) else { return }
        packages[currentLoc.packageIndex].items.remove(at: currentLoc.itemIndex)
        let packageBecameEmpty = packages[currentLoc.packageIndex].items.isEmpty
        if packageBecameEmpty {
            packages.remove(at: currentLoc.packageIndex)
        }
        if deleteFile && packageBecameEmpty {
            removeFolderIfEmpty(folder)
        }

        clearItemBookkeeping(itemID)
        await persist()
        await reconcile()
    }

    /// Removes an entire package. `deleteFiles` trashes the whole package
    /// folder in one move rather than per-item — the folder holds every
    /// item's bytes and sidecar together, so this is both simpler and atomic
    /// compared to removing them one at a time.
    public func removePackage(_ packageID: UUID, deleteFiles: Bool) async {
        guard let packageIndex = packages.firstIndex(where: { $0.id == packageID }) else { return }
        let package = packages[packageIndex]
        for item in package.items {
            await stopRunnerIfRunning(item.id)
        }

        let folder = settings.downloadFolder.appendingPathComponent(package.name)
        if deleteFiles {
            for item in package.items {
                for url in itemArtefactURLs(item, in: folder) where url.pathExtension == "sdmpart" {
                    ResumeSidecar.remove(at: url)
                }
            }
            trashIfExists(folder)
        }

        for item in package.items { clearItemBookkeeping(item.id) }
        packages.removeAll { $0.id == packageID }
        await persist()
        await reconcile()
    }

    /// Restarts a download from zero: stops it if running, discards its
    /// sidecar and any partial bytes, and resets it — leaving it exactly
    /// where it was in the schedule rather than starting it.
    ///
    /// A reset must not itself start a download: `.queued` stays `.queued`
    /// and `.stopped` stays `.stopped` (an enabled item was previously
    /// unconditionally re-queued here, which auto-started it the moment its
    /// slot came up — surprising for a reset, which is about discarding
    /// progress, not scheduling). A disabled item still lands `.stopped`
    /// regardless of what it was reset from, matching `setEnabled`'s own
    /// "disabled means not running" rule. `.running` is stopped first as
    /// always, then treated as `.queued` — the UI is expected to disable the
    /// Reset action on a running item in the first place (see
    /// `PackagesListView.canReset`), so this is a backstop, not the normal
    /// path.
    public func resetDownload(_ itemID: UUID) async {
        guard let loc = location(of: itemID) else { return }
        let package = packages[loc.packageIndex]
        let item = package.items[loc.itemIndex]
        let folder = settings.downloadFolder.appendingPathComponent(package.name)
        let priorState = item.state

        await stopRunnerIfRunning(itemID)

        // A reset discards *all* progress: every component's part file,
        // `.incomplete`, and `.sdmpart`, plus any finalized output. Missing
        // one (the old code only touched the output filename) left stale
        // parts that made the next resume fail with "file already exists".
        for url in itemArtefactURLs(item, in: folder) {
            if url.pathExtension == "sdmpart" {
                ResumeSidecar.remove(at: url)
            } else {
                trashIfExists(url)
            }
        }

        failedAttempts[itemID] = nil
        retryHoldTicks[itemID] = nil
        lastFailure[itemID] = nil
        sampledBytes[itemID] = nil
        checkpointFailures[itemID] = nil
        attemptStartBytes[itemID] = nil

        mutateItem(itemID) {
            for index in $0.components.indices {
                $0.components[index].completed = RangeSet()
                $0.components[index].totalBytes = nil
                $0.components[index].isResumable = nil
                $0.components[index].validator = nil
            }
            if $0.isEnabled {
                switch priorState {
                case .queued, .running: $0.state = .queued
                case .stopped, .completed, .failed: $0.state = .stopped
                }
            } else {
                $0.state = .stopped
            }
        }
        await persist()
        await reconcile()
    }

    /// Cancels and awaits an item's runner if it has one, mirroring
    /// `shutdown()`'s stop sequence for a single item: cancel the job, pause
    /// the task so its file descriptor closes, then await the job so
    /// `finish()` has already run and `runners[itemID]` is cleared before the
    /// caller touches disk.
    ///
    /// Blocking here is deliberate and safe for this method's callers
    /// (`removeItem`, `removePackage`, `resetDownload`): they are about to
    /// touch the same file on disk and must not race the worker that has it
    /// open, and each already overwrites the item's landing state (or
    /// removes the item outright) immediately after, so whatever `run()`
    /// lands on in the meantime is transient. Marking `.preempted` (rather
    /// than leaving `.none`) matters independently of that overwrite though:
    /// without it, the cancellation was charged to the item's retry budget —
    /// `run()`'s catch block would classify it as a real failure and could
    /// leave the item silently held in `retryHoldTicks` backoff, invisible in
    /// `state` but enough to make a later restart sit `.queued` and never
    /// actually get picked up until the backoff aged out on its own.
    ///
    /// User-facing Stop/Disable do **not** go through this method — see
    /// `retireRunnerNonBlocking`.
    private func stopRunnerIfRunning(_ itemID: UUID) async {
        guard let runner = runners[itemID] else { return }
        runners[itemID]?.retireIntent = .preempted
        runner.job.cancel()
        await runner.pauseAllComponents()
        _ = await runner.job.value
    }

    /// Cancels a running item's job **without** waiting for it to actually
    /// finish unwinding — the counterpart to `stopRunnerIfRunning` for
    /// user-facing Stop/Disable.
    ///
    /// Blocking there (as `stopRunnerIfRunning` does) would hang the caller
    /// for as long as the worker's in-flight read takes to resolve — against
    /// a origin that is merely slow, or a `.probe`-phase test gate that has
    /// not opened yet, that is indefinite. That defeats the point of a
    /// user-facing Stop, which must return immediately even against a hung
    /// read. The runner stays in `runners` (now marked `.userStopped`) until
    /// `run()` completes on its own schedule; its catch block then lands the
    /// item at `.stopped` — see `finish()`'s caller. Until that happens, the
    /// item cannot be restarted (`reconcile()` only starts an item once
    /// `runners[itemID]` is `nil`), the same transient unavailability a
    /// preempted-but-not-yet-unwound item already has.
    private func retireRunnerNonBlocking(_ itemID: UUID) {
        guard let runner = runners[itemID], runner.retireIntent == .none else { return }
        runners[itemID]?.retireIntent = .userStopped
        runner.job.cancel()
        let components = runner.components
        Task { for component in components { await component.pause() } }
    }

    /// Every on-disk artefact an item can leave in its package folder: the
    /// final output file, and — for a multi-component item — each component's
    /// part file. For each, the base URL plus its `.incomplete` sparse file
    /// and its `.sdmpart` resume sidecar. A one-component HTTP item's part
    /// filename equals its output filename, so this collapses to the old
    /// single-file set for it.
    private func itemArtefactURLs(_ item: DownloadItem, in folder: URL) -> [URL] {
        var bases = [folder.appendingPathComponent(item.outputFilename)]
        for component in item.components {
            bases.append(folder.appendingPathComponent(component.partFilename))
        }
        var urls: [URL] = []
        for base in bases {
            urls.append(base)
            urls.append(SparseFile.incompleteURL(for: base))
            urls.append(ResumeSidecar.url(for: base))
        }
        return urls
    }

    private func trashIfExists(_ url: URL) {
        guard FileManager.default.fileExists(atPath: url.path) else { return }
        try? FileManager.default.trashItem(at: url, resultingItemURL: nil)
    }

    private func removeFolderIfEmpty(_ folder: URL) {
        guard
            let contents = try? FileManager.default.contentsOfDirectory(atPath: folder.path),
            contents.isEmpty
        else { return }
        try? FileManager.default.trashItem(at: folder, resultingItemURL: nil)
    }

    private func clearItemBookkeeping(_ itemID: UUID) {
        failedAttempts[itemID] = nil
        retryHoldTicks[itemID] = nil
        lastFailure[itemID] = nil
        samplers[itemID] = nil
        sampledBytes[itemID] = nil
        checkpointFailures[itemID] = nil
        segmentOverrides[itemID] = nil
        attemptStartBytes[itemID] = nil
        startedAtTick[itemID] = nil
    }

    public func setSegmentCount(_ count: Int, for itemID: UUID) async {
        precondition(count >= 1, "segment count must be at least 1")
        segmentOverrides[itemID] = count
        if let runner = runners[itemID], runner.retireIntent == .none {
            let perComponent = Swift.max(1, count / runner.components.count)
            for componentRun in runner.components {
                await componentRun.setWorkerCount(perComponent)
            }
        }
    }

    /// Manually retries a terminally `.failed` item. Spec §6.4 promised "a
    /// terminal `failed` state surfaced with a reason and a manual retry
    /// action" — this is that action.
    ///
    /// A no-op for any other state: `Scheduler.isEligible` excludes `.failed`
    /// unconditionally, so `.queued` is what makes the item reachable again,
    /// not merely re-enabling it. Guarding on state rather than acting
    /// unconditionally means a button wired to every row cannot accidentally
    /// hand a healthy download a fresh retry budget it never asked for.
    public func retry(_ itemID: UUID) async {
        guard case .failed = itemState(for: itemID) else { return }
        mutateItem(itemID) {
            $0.state = .queued
            $0.isEnabled = true
        }
        failedAttempts[itemID] = nil
        retryHoldTicks[itemID] = nil
        await persist()
        await reconcile()
    }

    private func itemState(for itemID: UUID) -> ItemState? {
        for package in packages {
            if let item = package.items.first(where: { $0.id == itemID }) { return item.state }
        }
        return nil
    }

    private func isItemEnabled(_ itemID: UUID) -> Bool {
        for package in packages {
            if let item = package.items.first(where: { $0.id == itemID }) { return item.isEnabled }
        }
        return false
    }

    /// One-second heartbeat: folds the window's bytes into the samplers,
    /// drives the wall-clock half of the sidecar checkpoint trigger, closes
    /// the speed window, ages retry backoff, writes debounced durable state,
    /// and reschedules.
    public func tick() async {
        currentTick += 1
        for (itemID, runner) in runners where runner.retireIntent == .none {
            var transferred: Int64 = 0
            for componentRun in runner.components {
                transferred += await componentRun.completedRanges.totalBytes
                // Spec §4.3: "every ~8 MB per worker or every 5 s, whichever
                // comes first". `DownloadTask.record` implements the byte
                // half; this is the only caller of the wall-clock half.
                await componentRun.checkpointTick()
                if let failure = await componentRun.lastCheckpointFailure {
                    checkpointFailures[itemID] = failure
                }
            }
            recordProgress(transferred, for: itemID)
        }
        // Spec's "immediately drop to zero when not downloading": a sampler
        // whose item is not `.running` gets `idle()`, not `tick()`, so it
        // reports zero on the very next read instead of decaying toward it.
        // A sampler that still has bytes recorded but not yet folded into a
        // window gets one real `.tick()` regardless of the item's current
        // state, so a pause/completion-moment burst — or a download that
        // goes `.queued` → `.running` → `.completed` entirely between two
        // heartbeats — still lands in history exactly once instead of being
        // silently dropped.
        var anyItemActive = false
        for itemID in Array(samplers.keys) {
            let isRunning = itemState(for: itemID) == .running
            let hasPendingBytes = samplers[itemID]?.hasPendingBytes ?? false
            if isRunning || hasPendingBytes {
                samplers[itemID]?.tick()
                anyItemActive = true
            } else {
                samplers[itemID]?.idle()
            }
        }
        // Mirrors the per-item branch above: without it, `globalSampler`
        // mutated `history` every tick forever (even appending zeros),
        // which kept `EngineSnapshot.globalHistory` changing identity and
        // forced `BandwidthGraph` to fully re-render at the tick rate with
        // no downloads running at all.
        if anyItemActive || globalSampler.hasPendingBytes {
            globalSampler.tick()
        } else {
            globalSampler.idle()
        }

        for (itemID, remaining) in retryHoldTicks {
            if remaining <= 1 {
                retryHoldTicks[itemID] = nil
            } else {
                retryHoldTicks[itemID] = remaining - 1
            }
        }

        await flushIfDebounceElapsed()
        await reconcile()
    }

    /// Spec §4.2: durable state is written "~2 s after the last change".
    ///
    /// `save()` only stores into the store's in-memory `pending`; `flush()` is
    /// the only thing that writes `state.json`. Before this existed the sole
    /// caller of `flush()` anywhere was `shutdown()`, which in the ordinary
    /// quit path never ran — so nothing was ever written, `.incomplete` files
    /// survived with no record of the item owning them, and the next launch
    /// restored an empty list.
    ///
    /// Counted from the *first* unflushed change rather than the last, so a
    /// download that changes something every tick still gets written every two
    /// seconds instead of being starved forever. Cheap when nothing is dirty:
    /// `ticksSincePendingChange` is nil and this returns without touching the
    /// store.
    private func flushIfDebounceElapsed() async {
        guard let elapsed = ticksSincePendingChange else { return }
        let next = elapsed + 1
        guard next >= settings.persistDebounceTicks else {
            ticksSincePendingChange = next
            return
        }
        ticksSincePendingChange = nil
        await stateStore.flush()
    }

    public func snapshot() async -> EngineSnapshot {
        var packageSnapshots: [PackageSnapshot] = []
        for package in packages {
            var items: [ItemSnapshot] = []
            for item in package.items {
                var completed = item.completed
                var totalBytes = item.totalBytes
                var active = 0
                var checkpointFailure = checkpointFailures[item.id]
                var wholesalePostProcessing = false
                if let runner = runners[item.id] {
                    // Per-component sizes: a running component's live probed
                    // size when it has one, the stored size otherwise. Bases
                    // and the item total are both derived from this one array
                    // so the concatenated progress and the total can't drift
                    // apart.
                    var sizes: [Int64?] = item.components.map(\.totalBytes)
                    var liveRanges: [Int: RangeSet] = [:]
                    for componentRun in runner.components {
                        let index = componentRun.componentIndex
                        guard index < sizes.count else { continue }
                        if let live = await componentRun.expectedTotalBytes {
                            sizes[index] = live
                        }
                        liveRanges[index] = await componentRun.completedRanges
                        active += await componentRun.activeWorkerCount
                        if let failure = await componentRun.lastCheckpointFailure {
                            checkpointFailure = failure
                        }
                        if await componentRun.isAssembling { wholesalePostProcessing = true }
                    }

                    if sizes.allSatisfy({ $0 != nil }) {
                        let known = sizes.map { $0! }
                        totalBytes = known.reduce(0, +)
                        var bases: [Int64] = []
                        var running: Int64 = 0
                        for size in known {
                            bases.append(running)
                            running += size
                        }
                        var union = RangeSet()
                        for index in item.components.indices {
                            let component = liveRanges[index] ?? item.components[index].completed
                            for range in component.ranges {
                                union.insert(
                                    ByteRange(
                                        start: range.start + bases[index],
                                        end: range.end + bases[index]))
                            }
                        }
                        completed = union
                    } else {
                        // At least one size still unknown — best-effort: sum
                        // the live per-component progress without shifting
                        // (correct for a one-component item, an approximation
                        // otherwise until the probes land).
                        var union = RangeSet()
                        for index in item.components.indices {
                            let component = liveRanges[index] ?? item.components[index].completed
                            for range in component.ranges { union.insert(range) }
                        }
                        completed = union
                    }
                }
                let sampler = samplers[item.id] ?? SpeedSampler()
                // Spec-adjacent UI need: a `.completed` item whose file has
                // since been moved or deleted outside SDM should say so
                // rather than silently claiming to be done. Scoped to
                // `.completed` — any other state's file legitimately may not
                // exist yet.
                var fileMissing = false
                if item.state == .completed {
                    let destination = settings.downloadFolder
                        .appendingPathComponent(package.name)
                        .appendingPathComponent(item.filename)
                    fileMissing = !FileManager.default.fileExists(atPath: destination.path)
                }
                items.append(
                    ItemSnapshot(
                        id: item.id,
                        url: item.url,
                        filename: item.filename,
                        totalBytes: totalBytes,
                        completed: completed,
                        state: item.state,
                        isEnabled: item.isEnabled,
                        isResumable: item.isResumable,
                        activeSegments: active,
                        configuredSegments: segmentCount(for: item.id),
                        bytesPerSecond: sampler.bytesPerSecond,
                        speedHistory: sampler.history,
                        checkpointFailure: checkpointFailure,
                        remainingAttempts: failedAttempts[item.id].map {
                            retryPolicy.maxAttempts - $0
                        },
                        failedAttemptCount: failedAttempts[item.id],
                        // Only meaningful while the item is still retrying —
                        // `failedAttempts` is the gate, and it is cleared on
                        // success, progress, and terminal failure alike.
                        lastFailureReason: failedAttempts[item.id] != nil
                            ? lastFailure[item.id] : nil,
                        retryHoldSeconds: retryHoldTicks[item.id].map {
                            Int(
                                ($0 + AppTiming.ticksPerSecond - 1)
                                    / AppTiming.ticksPerSecond)
                        },
                        fileMissing: fileMissing,
                        isAssembling: assembling.contains(item.id) || wholesalePostProcessing,
                        assembly: item.assembly,
                        partFilenames: item.components.map(\.partFilename)
                    )
                )
            }
            packageSnapshots.append(
                PackageSnapshot(
                    id: package.id,
                    name: package.name,
                    priority: package.priority,
                    items: items
                )
            )
        }
        return EngineSnapshot(
            packages: packageSnapshots,
            // Spec §5.4: the global figure is the sum of the package figures,
            // which are sums of the item figures, so the three can never
            // disagree. `globalSampler` supplies only the graph history.
            globalBytesPerSecond: packageSnapshots.reduce(0) { $0 + $1.bytesPerSecond },
            globalHistory: globalSampler.history
        )
    }

    /// Stops everything and writes durable state. Jobs are awaited rather than
    /// merely cancelled, so no file descriptor outlives the call and each
    /// item's final state reaches the snapshot before it is flushed.
    ///
    /// Idempotent. The app now shuts down from two places — the heartbeat's
    /// `.task` continuation and `applicationWillTerminate` — and on a ⌘Q both
    /// can fire. Guarding here rather than only at the call site makes "call
    /// it once" a property of the engine instead of an invariant every caller
    /// has to uphold.
    public func shutdown() async {
        guard !isShutDown else { return }
        isShutDown = true
        let live = runners.values.map { $0 }
        for runner in live { runner.job.cancel() }
        for runner in live { await runner.pauseAllComponents() }
        for runner in live { _ = await runner.job.value }
        await persist()
        ticksSincePendingChange = nil
        await stateStore.flush()
    }

    /// Test helper: pumps the engine until nothing is left running.
    ///
    /// Does not tick, so retry backoff never ages: a transiently failing item
    /// is held out of the desired set and this returns rather than looping.
    /// Still bounded, as a backstop against any future path that could keep
    /// re-desiring an item without a tick.
    func runUntilIdle() async throws {
        await reconcile()
        for _ in 0..<1000 {
            guard let runner = runners.values.first else { return }
            _ = await runner.job.value
            await reconcile()
        }
        throw DownloadError.incompleteAfterWorkersFinished
    }

    // MARK: - Scheduling

    /// Applies the scheduler's desired running set: starts what should run,
    /// retires what should not.
    ///
    /// The desired set is computed and applied to `runners` without any
    /// suspension in between, so two overlapping calls can never both decide
    /// to start the same item. `startedRecently` is derived from
    /// `startedAtTick` and the tick-counted hysteresis window (spec §6.4),
    /// rather than a wall clock — see `currentTick`.
    ///
    /// `refreshResumability()` runs *before* that block, and does suspend.
    /// That is safe for a different reason than the block itself: a call that
    /// suspends there and finds another `reconcile()` has run to completion in
    /// the meantime simply recomputes from the state that call left behind and
    /// applies it, which is indistinguishable from the two having run in
    /// sequence. What must not happen — and does not — is a suspension between
    /// deciding the desired set and acting on it.
    private func reconcile() async {
        guard !isShutDown else { return }

        await refreshResumability()

        let desired = Scheduler.desiredRunningSet(
            SchedulerInput(
                packages: schedulablePackages(),
                runningNow: Set(runners.keys),
                startedRecently: Set(
                    startedAtTick.filter { currentTick - $0.value < hysteresisWindowTicks }.keys
                ),
                maxConcurrent: settings.maxConcurrent
            )
        )

        #if SDM_ENGINE_LOGGING
            let runningNow = Set(runners.keys)
            if desired != runningNow {
                let starting = desired.subtracting(runningNow)
                let stopping = runningNow.subtracting(desired)
                engineLog.debug(
                    "[scheduler] assigning: +\(starting.count, privacy: .public) -\(stopping.count, privacy: .public) (desired=\(desired.count, privacy: .public) running=\(runningNow.count, privacy: .public))"
                )
            }
        #endif

        let allocatedSegments = ConnectionAllocator.allocate(
            demands: connectionDemands(for: desired),
            budget: ConnectionBudget(
                global: settings.globalMaxConnections, perHost: settings.maxConnectionsPerHost)
        )

        var changed = false
        var retiring: [ComponentRun] = []
        for (itemID, runner) in runners
        where !desired.contains(itemID) && runner.retireIntent == .none {
            runners[itemID]?.retireIntent = .preempted
            runner.job.cancel()
            retiring.append(contentsOf: runner.components)
            mutateItem(itemID) { $0.state = .queued }
            changed = true
        }

        // Two tasks writing the same destination would interleave at absolute
        // offsets and corrupt it. Retiring runners are still in `runners`, so
        // this also covers restarting an item whose previous task has not
        // unwound yet.
        var claimed = Set(runners.values.flatMap(\.allDestinations))
        for itemID in desired where runners[itemID] == nil {
            let runContext: ItemRunContext
            do {
                guard let resolved = try context(for: itemID) else { continue }
                runContext = resolved
            } catch {
                // Creating the package folder failed. Left as `try?` this was
                // invisible: `SparseFile` then failed to open, the failure
                // classified transient, and the item re-attempted once a
                // second forever with nothing anywhere saying why.
                mutateItem(itemID) { $0.state = failureState(for: error, itemID: itemID) }
                failedAttempts[itemID] = nil
                changed = true
                continue
            }
            let destinations = runContext.contexts.map(\.destinationURL)
            guard destinations.allSatisfy({ !claimed.contains($0) }) else { continue }
            for destination in destinations { claimed.insert(destination) }
            mutateItem(itemID) { $0.state = .running }
            startedAtTick[itemID] = currentTick
            changed = true

            let completedTotalBytes = completedRanges(of: itemID).totalBytes
            attemptStartBytes[itemID] = completedTotalBytes
            // The one place a byte total may legitimately go backwards: a new
            // task starts over from whatever the sidecar lets it resume at.
            // Seeding the sampler baseline here — rather than letting
            // `recordProgress` rebase itself — keeps the delta strictly
            // monotonic everywhere else.
            sampledBytes[itemID] = completedTotalBytes

            let totalSegments = allocatedSegments[itemID] ?? runContext.segments
            let perComponent = Swift.max(1, totalSegments / runContext.contexts.count)
            let itemPageURL = itemSourceURL(itemID: itemID)
            let componentRuns: [ComponentRun] = runContext.contexts.map { componentContext in
                let worker: ComponentWorker
                if case .wholesale(let selector) = componentContext.origin,
                    let wholesaleDownloader
                {
                    worker = .wholesale(
                        WholesaleComponentTask(
                            itemID: itemID,
                            pageURL: itemPageURL ?? componentContext.sourceURL,
                            formatSelector: selector,
                            destinationURL: componentContext.destinationURL,
                            downloader: wholesaleDownloader))
                } else {
                    worker = .segmented(
                        DownloadTask(
                            id: itemID,
                            sourceURL: componentContext.sourceURL,
                            destinationURL: componentContext.destinationURL,
                            transport: transport,
                            configuration: DownloadTask.Configuration(
                                workerCount: perComponent,
                                minChunk: settings.minSegmentSizeBytes,
                                checkpointInterval: settings.checkpointIntervalBytes,
                                cachedCompleted: componentContext.cachedCompleted,
                                refreshableFormatID: componentContext.refreshableFormatID)))
                }
                return ComponentRun(
                    componentIndex: componentContext.componentIndex, worker: worker,
                    destinationURL: componentContext.destinationURL)
            }
            let runID = UUID()
            let job = Task { [weak self] in
                guard let self else { return }
                await self.runItem(itemID: itemID, runID: runID)
            }
            runners[itemID] = Runner(components: componentRuns, job: job, runID: runID)
        }

        // Re-clamp every already-running item too, not just fresh starts: a
        // sibling finishing frees budget the survivors should grow back into,
        // and a newly-added item can just as easily squeeze existing ones
        // down.
        for (itemID, runner) in runners
        where runner.retireIntent == .none && desired.contains(itemID) {
            if let allocated = allocatedSegments[itemID] {
                let perComponent = Swift.max(1, allocated / runner.components.count)
                for componentRun in runner.components {
                    await componentRun.setWorkerCount(perComponent)
                }
            }
        }

        for task in retiring { await task.pause() }
        if changed { await persist() }
    }

    /// Connection demand for a set of items: their real host and their
    /// requested (not yet budget-capped) worker count.
    private func connectionDemands(for itemIDs: Set<UUID>) -> [ConnectionDemand] {
        var demands: [ConnectionDemand] = []
        for package in packages {
            for item in package.items where itemIDs.contains(item.id) {
                demands.append(
                    ConnectionDemand(
                        id: item.id,
                        // The host actually connected to — the first
                        // component's stream URL, not the grabbed source URL
                        // (which for a YouTube item is `youtube.com`, where
                        // nothing is downloaded from).
                        host: item.components.first?.url.host ?? "",
                        desiredSegments: segmentCount(for: item.id)
                    )
                )
            }
        }
        return demands
    }

    /// The package graph as the scheduler should see it right now.
    ///
    /// An item serving retry backoff is presented as disabled, so it drops out
    /// of `rank` entirely and its slot goes to the next item by rank rather
    /// than being held empty. Doing it here — rather than subtracting from the
    /// scheduler's answer afterwards — keeps `Scheduler.desiredRunningSet` a
    /// pure function of its input, which is what makes spec §6 table-testable.
    ///
    private func schedulablePackages() -> [DownloadPackage] {
        guard !retryHoldTicks.isEmpty else { return packages }
        var held = packages
        for packageIndex in held.indices {
            for itemIndex in held[packageIndex].items.indices
            where retryHoldTicks[held[packageIndex].items[itemIndex].id] != nil {
                held[packageIndex].items[itemIndex].isEnabled = false
            }
        }
        return held
    }

    /// Mirrors each running task's probe result onto its item.
    ///
    /// The scheduler reads `isResumable` straight off `packages`, so the
    /// answer has to live there rather than in a side table. This is the
    /// earliest point the engine can observe it without blocking the
    /// decision block below: a task reports `nil` until its probe lands, and
    /// the true value from then on. The `nil → false` transition is the one
    /// that matters — it is what stops a genuinely non-resumable download
    /// from being preempted and losing every byte it has — and running this
    /// at the head of every `reconcile()` means the very scheduling decision
    /// that could preempt it is already looking at the real value.
    private func refreshResumability() async {
        for (itemID, runner) in runners where runner.retireIntent == .none {
            for componentRun in runner.components {
                guard let supportsRanges = await componentRun.probedSupportsRanges
                else { continue }
                mutateItem(itemID) { item in
                    if componentRun.componentIndex < item.components.count {
                        item.components[componentRun.componentIndex].isResumable = supportsRanges
                    }
                }
            }
        }
    }

    private struct ComponentContext: Sendable {
        let componentIndex: Int
        let sourceURL: URL
        let destinationURL: URL
        let cachedCompleted: RangeSet
        let refreshableFormatID: String?
        let origin: ComponentOrigin
    }

    private struct ItemRunContext: Sendable {
        let contexts: [ComponentContext]
        let segments: Int
    }

    /// Resolves where each not-yet-complete component of an item writes,
    /// creating the package folder.
    ///
    /// Throws rather than swallowing a `createDirectory` failure: a read-only
    /// volume or a permission problem is not something re-attempting fixes,
    /// and hiding it behind `try?` turned it into an undiagnosable retry loop
    /// one layer down.
    private func context(for itemID: UUID) throws -> ItemRunContext? {
        for package in packages {
            guard let item = package.items.first(where: { $0.id == itemID }) else { continue }
            let folder = settings.downloadFolder.appendingPathComponent(package.name)
            do {
                try FileManager.default.createDirectory(
                    at: folder, withIntermediateDirectories: true)
            } catch {
                throw EngineError.destinationFolderUnavailable(
                    path: folder.path, underlying: error.localizedDescription)
            }
            var contexts: [ComponentContext] = []
            for (index, component) in item.components.enumerated() where !component.isComplete {
                let formatID: String?
                if case .resolved(let id) = component.origin {
                    formatID = id
                } else {
                    formatID = nil
                }
                // A wholesale component never resumes — it re-runs yt-dlp
                // from zero — so its cached progress is always discarded.
                let cached: RangeSet
                if case .wholesale = component.origin {
                    cached = RangeSet()
                } else {
                    cached = component.completed
                }
                contexts.append(
                    ComponentContext(
                        componentIndex: index,
                        sourceURL: component.url,
                        destinationURL: folder.appendingPathComponent(component.partFilename),
                        cachedCompleted: cached,
                        refreshableFormatID: formatID,
                        origin: component.origin))
            }
            // Every component already complete → still hand back one context
            // for component 0 so the run finalizes/assembles rather than
            // stalling. (`isComplete` on a component needs a known size; an
            // unprobed component is not complete, so this only bites a truly
            // finished item that lost its runner.)
            if contexts.isEmpty {
                contexts.append(
                    ComponentContext(
                        componentIndex: 0, sourceURL: item.components[0].url,
                        destinationURL: folder.appendingPathComponent(
                            item.components[0].partFilename),
                        cachedCompleted: item.components[0].completed,
                        refreshableFormatID: nil,
                        origin: item.components[0].origin))
            }
            return ItemRunContext(contexts: contexts, segments: segmentCount(for: itemID))
        }
        return nil
    }

    /// Runs every component task of an item in parallel, then assembles.
    /// Replaces the single-task `run`; a one-component item is the N=1 case.
    private func runItem(itemID: UUID, runID: UUID) async {
        guard let components = runners[itemID]?.components else { return }

        // Fan out. A component throwing `urlExpired` is refreshed and
        // restarted in place (Task 6); any other throw is the first error we
        // keep, and the siblings are paused.
        var firstError: (any Error)?
        await withTaskGroup(of: (any Error)?.self) { group in
            for componentRun in components {
                group.addTask { [weak self] in
                    guard let self else { return nil }
                    return await self.runComponent(
                        itemID: itemID, runID: runID, componentRun: componentRun)
                }
            }
            for await error in group where error != nil {
                if firstError == nil {
                    firstError = error
                    // Stop the siblings — their partial bytes stay on disk.
                    if let live = runners[itemID]?.components {
                        for sibling in live { await sibling.pause() }
                    }
                }
            }
        }

        let retireIntent = runners[itemID]?.retireIntent ?? .none
        let state: ItemState?
        switch (retireIntent, firstError) {
        case (.preempted, _):
            state = .queued
        case (.userStopped, _):
            state = nil
        case (.none, .some(let error)):
            let progressed =
                completedRanges(of: itemID).totalBytes > (attemptStartBytes[itemID] ?? 0)
            state = failureState(for: error, itemID: itemID, madeProgress: progressed)
        case (.none, .none):
            state = await assemble(itemID: itemID)
            if state == .completed {
                failedAttempts[itemID] = nil
                retryHoldTicks[itemID] = nil
            }
        }

        await finishItem(itemID: itemID, runID: runID, state: state)
        await persist()
    }

    /// Runs one component's task to completion. Returns the thrown error, or
    /// `nil` on success. A `403`/`410` on a `.resolved` component is
    /// refreshed via the injected resolver and the task is restarted in
    /// place against its existing bytes; anything else propagates. Parent
    /// spec §7.3.
    private func runComponent(
        itemID: UUID, runID: UUID, componentRun: ComponentRun
    ) async -> (any Error)? {
        let index = componentRun.componentIndex

        // A wholesale component re-resolves internally on every run — the
        // `403` → refresh loop below does not apply. `.cancelled` reaches
        // `runItem` as the pause/stop signal via the retire-intent branch.
        guard let task = componentRun.segmentedTask else {
            do {
                _ = try await componentRun.start()
                return nil
            } catch WholesaleError.cancelled {
                return CancellationError()
            } catch {
                return error
            }
        }

        var runningTask = task
        while true {
            do {
                _ = try await runningTask.start()
                return nil
            } catch let DownloadError.urlExpired(formatID) {
                guard let resolver,
                    let component = itemComponent(itemID: itemID, index: index),
                    case .resolved = component.origin,
                    let sourceURL = itemSourceURL(itemID: itemID)
                else { return DownloadError.serverError(status: 403) }

                // A refresh consumes one attempt against the existing cap, so
                // a genuinely broken video still terminates.
                let attempt = (failedAttempts[itemID] ?? 0) + 1
                failedAttempts[itemID] = attempt
                guard attempt < retryPolicy.maxAttempts else {
                    failedAttempts[itemID] = nil
                    return DownloadError.refreshFailed(
                        reason: "Gave up refreshing the download URL after \(attempt) attempts")
                }

                let refreshed: RefreshedFormat
                do {
                    refreshed = try await resolver.refresh(
                        sourceURL: sourceURL, formatID: formatID)
                } catch {
                    return DownloadError.refreshFailed(
                        reason: "Could not refresh the URL: \(error)")
                }
                guard refreshed.formatID == formatID else {
                    return DownloadError.refreshFailed(reason: "The video's format changed")
                }
                if let known = component.totalBytes, let fresh = refreshed.filesize, fresh != known
                {
                    return DownloadError.refreshFailed(reason: "The video's size changed")
                }

                mutateItem(itemID) { item in
                    if index < item.components.count { item.components[index].url = refreshed.url }
                }
                let perComponent = Swift.max(
                    1, segmentCount(for: itemID) / (runners[itemID]?.components.count ?? 1))
                let newTask = DownloadTask(
                    id: itemID, sourceURL: refreshed.url,
                    destinationURL: componentRun.destinationURL, transport: transport,
                    configuration: DownloadTask.Configuration(
                        workerCount: perComponent, minChunk: settings.minSegmentSizeBytes,
                        checkpointInterval: settings.checkpointIntervalBytes,
                        cachedCompleted: component.completed, refreshableFormatID: formatID))
                if let slot = runners[itemID]?.components.firstIndex(where: {
                    $0.componentIndex == index
                }) {
                    runners[itemID]?.components[slot].worker = .segmented(newTask)
                }
                runningTask = newTask
                continue
            } catch {
                return error
            }
        }
    }

    private func itemComponent(itemID: UUID, index: Int) -> FileComponent? {
        for package in packages {
            guard let item = package.items.first(where: { $0.id == itemID }) else { continue }
            return index < item.components.count ? item.components[index] : nil
        }
        return nil
    }

    private func itemSourceURL(itemID: UUID) -> URL? {
        for package in packages {
            if let item = package.items.first(where: { $0.id == itemID }) { return item.sourceURL }
        }
        return nil
    }

    /// Assembly step after every component completed. `.none` → each task
    /// already finalized its own file, nothing to do. `.mux` → run `ffmpeg
    /// -c copy` over the finalized parts into the output container, then
    /// delete the parts. Parent spec §7.2.
    private func assemble(itemID: UUID) async -> ItemState {
        guard let loc = location(of: itemID) else { return .completed }
        let package = packages[loc.packageIndex]
        let item = package.items[loc.itemIndex]
        if item.assembly == .mux {
            return await runMux(item: item, packageName: package.name)
        }
        // No mux, but a single component whose finalized part file is not
        // already the output name (a progressive YouTube stream lands as
        // `<title> [id].f18.mp4`, the item's output is `<title> [id].mp4`).
        // Rename it so the item's destination matches what is on disk.
        if item.components.count == 1, item.components[0].partFilename != item.outputFilename {
            let folder = settings.downloadFolder.appendingPathComponent(package.name)
            let from = folder.appendingPathComponent(item.components[0].partFilename)
            let to = folder.appendingPathComponent(item.outputFilename)
            if FileManager.default.fileExists(atPath: from.path) {
                try? FileManager.default.removeItem(at: to)
                do {
                    try FileManager.default.moveItem(at: from, to: to)
                } catch {
                    return .failed(reason: "Could not name the file: \(error.localizedDescription)")
                }
            }
            ResumeSidecar.remove(at: ResumeSidecar.url(for: from))
        }
        return .completed
    }

    /// Runs the mux for a `.mux` item whose components are all downloaded.
    /// Shared by `assemble()` and `retryMux()`.
    private func runMux(item: DownloadItem, packageName: String) async -> ItemState {
        guard let muxer else {
            return .failed(reason: "ffmpeg is not configured")
        }
        guard item.components.count == 2 else {
            return .failed(reason: "a muxed item needs exactly two components")
        }
        let folder = settings.downloadFolder.appendingPathComponent(packageName)
        let videoPart = folder.appendingPathComponent(item.components[0].partFilename)
        let audioPart = folder.appendingPathComponent(item.components[1].partFilename)
        let output = folder.appendingPathComponent(item.outputFilename)
        let container = MediaContainer.fromFileExtension(
            (item.outputFilename as NSString).pathExtension)

        assembling.insert(item.id)
        defer { assembling.remove(item.id) }
        do {
            try await muxer.mux(
                videoPart: videoPart, audioPart: audioPart, into: output, container: container)
        } catch {
            let tail: String
            if case MuxError.ffmpegFailed(let stderrTail) = error {
                tail = stderrTail
            } else {
                tail = "\(error)"
            }
            return .failed(reason: "mux failed: \(tail)")
        }
        try? FileManager.default.removeItem(at: videoPart)
        try? FileManager.default.removeItem(at: audioPart)
        for component in item.components {
            ResumeSidecar.remove(
                at: ResumeSidecar.url(for: folder.appendingPathComponent(component.partFilename)))
        }
        return .completed
    }

    /// Re-runs only the mux step for a `.failed` item whose components are
    /// all downloaded — the "Retry mux" action. Parent spec §7.2 / §9.3.
    public func retryMux(_ itemID: UUID) async {
        guard let loc = location(of: itemID) else { return }
        let package = packages[loc.packageIndex]
        let item = package.items[loc.itemIndex]
        guard item.assembly == .mux, case .failed = item.state,
            item.components.allSatisfy(\.isComplete)
        else { return }
        let state = await runMux(item: item, packageName: package.name)
        mutateItem(itemID) { $0.state = state }
        await persist()
    }

    /// Retires a finished runner, writing each component's final bytes back
    /// onto `DownloadItem.components[k]` so a preempted or failed item
    /// resumes rather than restarts.
    private func finishItem(itemID: UUID, runID: UUID, state: ItemState?) async {
        guard let runner = runners[itemID], runner.runID == runID else { return }

        var totalCompleted: Int64 = 0
        var perComponent:
            [(index: Int, completed: RangeSet, totalBytes: Int64?, isResumable: Bool?)] =
                []
        for componentRun in runner.components {
            let completed = await componentRun.completedRanges
            let totalBytes = await componentRun.expectedTotalBytes
            let isResumable = await componentRun.probedSupportsRanges
            if let failure = await componentRun.lastCheckpointFailure {
                checkpointFailures[itemID] = failure
            }
            totalCompleted += completed.totalBytes
            perComponent.append((componentRun.componentIndex, completed, totalBytes, isResumable))
        }

        recordProgress(totalCompleted, for: itemID)

        // Superseded run — a newer one has replaced it; do not write back.
        guard runners[itemID]?.runID == runID else { return }
        runners[itemID] = nil
        startedAtTick[itemID] = nil

        mutateItem(itemID) { item in
            for entry in perComponent where entry.index < item.components.count {
                // Same non-resumable-restart display rule as before, per
                // component: a confirmed non-resumable component that is not
                // landing `.completed` shows zero rather than a partial that
                // its next attempt will discard.
                let display: RangeSet =
                    (entry.isResumable == false && state != .completed)
                    ? RangeSet() : entry.completed
                item.components[entry.index].completed = display
                if let totalBytes = entry.totalBytes {
                    item.components[entry.index].totalBytes = totalBytes
                }
                if let isResumable = entry.isResumable {
                    item.components[entry.index].isResumable = isResumable
                }
            }
            if let state { item.state = state }
        }
    }

    // MARK: - Failure handling

    /// Turns a thrown error into the item's next state, applying spec §6.4's
    /// backoff and attempt cap.
    ///
    /// Before this existed, `run()` returned every transient failure to
    /// `.queued` and `tick()` re-desired it one second later, forever: against
    /// an origin that 500s, truncates, or drops the connection that is an
    /// unbounded request storm at one attempt per second per item. `run()` and
    /// `reconcile()` both route through here so a folder that cannot be
    /// created is capped the same way a hostile origin is.
    ///
    /// A permanent failure is terminal immediately. A transient one holds the
    /// item out of the desired running set for `RetryPolicy.delay` — counted
    /// in heartbeat ticks, no clock — and becomes terminal once `maxAttempts`
    /// consecutive attempts have failed. An attempt that moved real bytes
    /// clears the counter first: a download that is genuinely progressing,
    /// however unreliably, should not exhaust a budget meant for one that
    /// cannot start at all.
    private func failureState(
        for error: any Error,
        itemID: UUID,
        madeProgress: Bool = false
    ) -> ItemState {
        if case .permanent(let reason) = retryPolicy.classify(error) {
            failedAttempts[itemID] = nil
            retryHoldTicks[itemID] = nil
            lastFailure[itemID] = nil
            return .failed(reason: reason)
        }

        if madeProgress { failedAttempts[itemID] = nil }
        let attempt = (failedAttempts[itemID] ?? 0) + 1
        failedAttempts[itemID] = attempt
        lastFailure[itemID] = Self.describe(error)

        guard attempt < retryPolicy.maxAttempts else {
            failedAttempts[itemID] = nil
            retryHoldTicks[itemID] = nil
            lastFailure[itemID] = nil
            return .failed(
                reason:
                    "Gave up after \(attempt) attempts: \(Self.describe(error))"
            )
        }

        // `delay(forAttempt:)` is seconds; scale by the heartbeat rate to
        // get ticks. At least one tick, so a sub-second backoff still costs
        // a beat rather than re-attempting immediately.
        let seconds = retryPolicy.delay(forAttempt: attempt - 1).components.seconds
        retryHoldTicks[itemID] = Swift.max(1, Int(seconds) * AppTiming.ticksPerSecond)
        return .queued
    }

    private static func describe(_ error: any Error) -> String {
        if let download = error as? DownloadError { return "\(download)" }
        if let transport = error as? TransportError { return "\(transport)" }
        return error.localizedDescription
    }

    // MARK: - Telemetry

    /// Folds a monotonically growing byte total into the per-item and global
    /// samplers as a delta, so the same bytes are never counted twice.
    ///
    /// A total lower than the baseline is ignored rather than rebased onto.
    /// `tick()` suspends while reading a task's total, and the job can finish
    /// during that suspension — `finish()` then records the final figure, and
    /// `tick()` resumes holding a smaller, stale one. Rebasing down there
    /// would let a preempted item's later resume re-record the span between
    /// the two. The baseline is instead seeded deliberately in `reconcile()`,
    /// the only place progress can legitimately go backwards.
    private func recordProgress(_ transferred: Int64, for itemID: UUID) {
        let previous = sampledBytes[itemID] ?? 0
        guard transferred > previous else { return }
        sampledBytes[itemID] = transferred
        samplers[itemID]?.record(bytes: transferred - previous)
        globalSampler.record(bytes: transferred - previous)
    }

    // MARK: - Helpers

    private func segmentCount(for itemID: UUID) -> Int {
        segmentOverrides[itemID] ?? settings.segmentsPerItem
    }

    /// What a fresh `DownloadTask` for this item is expected to resume from.
    private func completedBytes(of itemID: UUID) -> Int64 {
        for package in packages {
            if let item = package.items.first(where: { $0.id == itemID }) {
                return item.completed.totalBytes
            }
        }
        return 0
    }

    private func completedRanges(of itemID: UUID) -> RangeSet {
        for package in packages {
            if let item = package.items.first(where: { $0.id == itemID }) {
                return item.completed
            }
        }
        return RangeSet()
    }

    private func mutateItem(_ itemID: UUID, _ transform: (inout DownloadItem) -> Void) {
        for packageIndex in packages.indices {
            for itemIndex in packages[packageIndex].items.indices
            where packages[packageIndex].items[itemIndex].id == itemID {
                #if SDM_ENGINE_LOGGING
                    let before = packages[packageIndex].items[itemIndex].state
                #endif
                transform(&packages[packageIndex].items[itemIndex])
                #if SDM_ENGINE_LOGGING
                    let item = packages[packageIndex].items[itemIndex]
                    if item.state != before {
                        engineLog.debug(
                            "\(itemTag(itemID, filename: item.filename), privacy: .public) state: \(String(describing: before), privacy: .public) -> \(String(describing: item.state), privacy: .public)"
                        )
                    }
                #endif
            }
        }
    }

    /// Queues the current package graph for durable storage and starts (or
    /// leaves running) the debounce window `tick()` drains.
    ///
    /// `.queued`/`.running` are squashed to `.stopped` in what actually gets
    /// written — nothing "in flight" or "about to run" is ever true of a
    /// process that isn't currently alive to act on it, so the durable
    /// snapshot should not claim otherwise. This is also what makes restart
    /// behavior fall out of existing primitives with no dedicated mechanism:
    /// every restored item lands `.stopped` (or terminal), and "resume
    /// downloads automatically on launch" is then just `resumeAll()` called
    /// once after `restore()` — see `EngineController`. Live in-memory
    /// `packages` is untouched; only the copy handed to `stateStore` is
    /// transformed.
    private func persist() async {
        var forStorage = packages
        for packageIndex in forStorage.indices {
            for itemIndex in forStorage[packageIndex].items.indices {
                switch forStorage[packageIndex].items[itemIndex].state {
                case .queued, .running: forStorage[packageIndex].items[itemIndex].state = .stopped
                case .stopped, .completed, .failed: break
                }
            }
        }
        await stateStore.save(PersistedState(packages: forStorage))
        if ticksSincePendingChange == nil { ticksSincePendingChange = 0 }
    }
}
