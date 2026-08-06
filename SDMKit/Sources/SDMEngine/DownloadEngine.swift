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
    /// Ticks of the 1 Hz heartbeat that must pass with no further change
    /// before durable state is written. Spec §4.2's "~2 s after the last
    /// change" debounce.
    public var persistDebounceTicks: Int

    public init(
        maxConcurrent: Int,
        segmentsPerItem: Int,
        globalMaxConnections: Int,
        maxConnectionsPerHost: Int = 8,
        downloadFolder: URL,
        checkpointIntervalBytes: Int64 = 8 * 1024 * 1024,
        persistDebounceTicks: Int = 2
    ) {
        precondition(maxConcurrent >= 1, "maxConcurrent must be at least 1")
        precondition(segmentsPerItem >= 1, "segmentsPerItem must be at least 1")
        precondition(globalMaxConnections >= 1, "globalMaxConnections must be at least 1")
        precondition(maxConnectionsPerHost >= 1, "maxConnectionsPerHost must be at least 1")
        precondition(checkpointIntervalBytes > 0, "checkpointIntervalBytes must be positive")
        precondition(persistDebounceTicks >= 1, "persistDebounceTicks must be at least 1")
        self.maxConcurrent = maxConcurrent
        self.segmentsPerItem = segmentsPerItem
        self.globalMaxConnections = globalMaxConnections
        self.maxConnectionsPerHost = maxConnectionsPerHost
        self.downloadFolder = downloadFolder
        self.checkpointIntervalBytes = checkpointIntervalBytes
        self.persistDebounceTicks = persistDebounceTicks
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
    /// A preempted runner is marked `isRetiring` and kept here until its job
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
    private struct Runner {
        let task: DownloadTask
        let job: Task<Void, Never>
        let destinationURL: URL
        var isRetiring: Bool = false
    }

    private let transport: any HTTPTransport
    private let stateStore: any StateStore
    private var settings: EngineSettings

    private var packages: [DownloadPackage] = []
    private var runners: [UUID: Runner] = [:]
    private var samplers: [UUID: SpeedSampler] = [:]
    private var globalSampler = SpeedSampler(historyLength: 300, smoothingFactor: 0.4)
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
    /// Spec §6.4: "An item started within the last ~5 s is not preempted."
    private let hysteresisWindowTicks = 5
    /// The last checkpoint failure reported by each item's task, kept after
    /// the runner retires so the reason does not vanish from the UI the
    /// instant the download stops.
    private var checkpointFailures: [UUID: String] = [:]

    public init(
        transport: any HTTPTransport,
        stateStore: any StateStore,
        settings: EngineSettings,
        retryPolicy: RetryPolicy = RetryPolicy()
    ) {
        self.transport = transport
        self.stateStore = stateStore
        self.settings = settings
        self.retryPolicy = retryPolicy
    }

    // MARK: - Mutations

    public func add(_ package: DownloadPackage) async {
        packages.append(package)
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
    /// - `.running` becomes `.queued`. Nothing in this process is actually
    ///   running it — the runner that made it `.running` died with the last
    ///   process — and leaving it `.running` would make the scheduler believe
    ///   a slot is occupied that is not, while telling the UI a download is
    ///   in flight when no worker exists.
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
                if restored[packageIndex].items[itemIndex].state == .running {
                    restored[packageIndex].items[itemIndex].state = .queued
                }
                restored[packageIndex].items[itemIndex].isResumable = nil
            }
        }

        packages.append(contentsOf: restored)
        for package in restored {
            for item in package.items where samplers[item.id] == nil {
                samplers[item.id] = SpeedSampler()
            }
        }
        await persist()
        await reconcile()
    }

    public func setEnabled(_ enabled: Bool, for itemID: UUID) async {
        mutateItem(itemID) { $0.isEnabled = enabled }
        await persist()
        await reconcile()
    }

    public func setPriority(_ priority: Priority?, for itemID: UUID) async {
        mutateItem(itemID) { $0.priority = priority }
        await persist()
        await reconcile()
    }

    public func setSegmentCount(_ count: Int, for itemID: UUID) async {
        precondition(count >= 1, "segment count must be at least 1")
        segmentOverrides[itemID] = count
        if let runner = runners[itemID], !runner.isRetiring {
            await runner.task.setWorkerCount(count)
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

    /// One-second heartbeat: folds the window's bytes into the samplers,
    /// drives the wall-clock half of the sidecar checkpoint trigger, closes
    /// the speed window, ages retry backoff, writes debounced durable state,
    /// and reschedules.
    public func tick() async {
        currentTick += 1
        for (itemID, runner) in runners where !runner.isRetiring {
            let transferred = await runner.task.completedRanges.totalBytes
            recordProgress(transferred, for: itemID)
            // Spec §4.3: "every ~8 MB per worker or every 5 s, whichever comes
            // first". `DownloadTask.record` implements the byte half; this is
            // the only caller of the wall-clock half, so without it a slow
            // origin would never checkpoint at all.
            await runner.task.checkpointTick()
            checkpointFailures[itemID] = await runner.task.lastCheckpointFailure
        }
        for itemID in Array(samplers.keys) { samplers[itemID]?.tick() }
        globalSampler.tick()

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
                if let runner = runners[item.id] {
                    completed = await runner.task.completedRanges
                    totalBytes = await runner.task.expectedTotalBytes ?? totalBytes
                    active = await runner.task.activeWorkerCount
                    checkpointFailure = await runner.task.lastCheckpointFailure
                }
                let sampler = samplers[item.id] ?? SpeedSampler()
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
                        }
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
        for runner in live { await runner.task.pause() }
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

        let allocatedSegments = ConnectionAllocator.allocate(
            demands: connectionDemands(for: desired),
            budget: ConnectionBudget(
                global: settings.globalMaxConnections, perHost: settings.maxConnectionsPerHost)
        )

        var changed = false
        var retiring: [DownloadTask] = []
        for (itemID, runner) in runners where !desired.contains(itemID) && !runner.isRetiring {
            runners[itemID]?.isRetiring = true
            runner.job.cancel()
            retiring.append(runner.task)
            mutateItem(itemID) { $0.state = .queued }
            changed = true
        }

        // Two tasks writing the same destination would interleave at absolute
        // offsets and corrupt it. Retiring runners are still in `runners`, so
        // this also covers restarting an item whose previous task has not
        // unwound yet.
        var claimed = Set(runners.values.map(\.destinationURL))
        for itemID in desired where runners[itemID] == nil {
            let runContext: RunContext
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
            guard claimed.insert(runContext.destinationURL).inserted else { continue }
            mutateItem(itemID) { $0.state = .running }
            startedAtTick[itemID] = currentTick
            changed = true
            attemptStartBytes[itemID] = completedBytes(of: itemID)
            // The one place a byte total may legitimately go backwards: a new
            // task starts over from whatever the sidecar lets it resume at.
            // Seeding the sampler baseline here — rather than letting
            // `recordProgress` rebase itself — keeps the delta strictly
            // monotonic everywhere else, so a tick that reads a stale total
            // while a job completes underneath it cannot cause the same span
            // to be counted twice across a preempt/resume cycle.
            sampledBytes[itemID] = completedBytes(of: itemID)
            let task = DownloadTask(
                id: itemID,
                sourceURL: runContext.sourceURL,
                destinationURL: runContext.destinationURL,
                transport: transport,
                configuration: DownloadTask.Configuration(
                    workerCount: allocatedSegments[itemID] ?? runContext.segments,
                    minChunk: 64 * 1024,
                    checkpointInterval: settings.checkpointIntervalBytes
                )
            )
            let job = Task { [weak self] in
                guard let self else { return }
                await self.run(itemID: itemID, task: task)
            }
            runners[itemID] = Runner(
                task: task,
                job: job,
                destinationURL: runContext.destinationURL
            )
        }

        // Re-clamp every already-running item too, not just fresh starts: a
        // sibling finishing frees budget the survivors should grow back into,
        // and a newly-added item can just as easily squeeze existing ones
        // down.
        for (itemID, runner) in runners where !runner.isRetiring && desired.contains(itemID) {
            if let allocated = allocatedSegments[itemID] {
                await runner.task.setWorkerCount(allocated)
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
                        host: item.url.host ?? "",
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
        for (itemID, runner) in runners where !runner.isRetiring {
            guard let supportsRanges = await runner.task.probedSupportsRanges else { continue }
            mutateItem(itemID) { $0.isResumable = supportsRanges }
        }
    }

    private struct RunContext: Sendable {
        let sourceURL: URL
        let destinationURL: URL
        let segments: Int
    }

    /// Resolves where an item's bytes go, creating the package folder.
    ///
    /// Throws rather than swallowing a `createDirectory` failure: a read-only
    /// volume or a permission problem is not something re-attempting fixes,
    /// and hiding it behind `try?` turned it into an undiagnosable retry loop
    /// one layer down.
    private func context(for itemID: UUID) throws -> RunContext? {
        for package in packages {
            guard let item = package.items.first(where: { $0.id == itemID }) else { continue }
            let folder = settings.downloadFolder.appendingPathComponent(package.name)
            do {
                try FileManager.default.createDirectory(
                    at: folder,
                    withIntermediateDirectories: true
                )
            } catch {
                throw EngineError.destinationFolderUnavailable(
                    path: folder.path,
                    underlying: error.localizedDescription
                )
            }
            return RunContext(
                sourceURL: item.url,
                destinationURL: folder.appendingPathComponent(item.filename),
                segments: segmentCount(for: itemID)
            )
        }
        return nil
    }

    private func run(itemID: UUID, task: DownloadTask) async {
        let state: ItemState
        do {
            _ = try await task.start()
            state = .completed
            failedAttempts[itemID] = nil
            retryHoldTicks[itemID] = nil
        } catch {
            if runners[itemID]?.isRetiring == true {
                // Preemption, not failure. `reconcile()` cancelled this job
                // and paused its task, so `start()` throwing here is the
                // expected shape of a scheduler decision — charging it to the
                // item's retry budget would eventually mark a perfectly
                // healthy download `.failed` for having been outranked too
                // often, and holding it in backoff would stop it resuming the
                // moment a slot frees up.
                state = .queued
            } else {
                let progressed =
                    await task.completedRanges.totalBytes > (attemptStartBytes[itemID] ?? 0)
                state = failureState(for: error, itemID: itemID, madeProgress: progressed)
            }
        }

        let completed = await task.completedRanges
        let totalBytes = await task.expectedTotalBytes
        // Also captured here, not only in `refreshResumability()`: a download
        // short enough to finish before any reconcile happens would otherwise
        // never record what its probe found.
        let isResumable = await task.probedSupportsRanges
        // Kept after the runner retires: "this download's resume state could
        // not be written" is exactly the thing a user must still see once the
        // download has stopped.
        checkpointFailures[itemID] = await task.lastCheckpointFailure
        finish(
            itemID: itemID,
            task: task,
            completed: completed,
            totalBytes: totalBytes,
            isResumable: isResumable,
            state: state
        )
        await persist()
    }

    /// Retires a finished runner, keeping whatever bytes it managed so a
    /// preempted or failed item resumes rather than restarts.
    private func finish(
        itemID: UUID,
        task: DownloadTask,
        completed: RangeSet,
        totalBytes: Int64?,
        isResumable: Bool?,
        state: ItemState
    ) {
        recordProgress(completed.totalBytes, for: itemID)
        // Only the runner that is still the current one may write back; a
        // superseded task must not clobber its replacement's state.
        guard runners[itemID]?.task === task else { return }
        runners[itemID] = nil
        startedAtTick[itemID] = nil
        mutateItem(itemID) {
            $0.completed = completed
            if let totalBytes { $0.totalBytes = totalBytes }
            if let isResumable { $0.isResumable = isResumable }
            $0.state = state
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
            return .failed(reason: reason)
        }

        if madeProgress { failedAttempts[itemID] = nil }
        let attempt = (failedAttempts[itemID] ?? 0) + 1
        failedAttempts[itemID] = attempt

        guard attempt < retryPolicy.maxAttempts else {
            failedAttempts[itemID] = nil
            retryHoldTicks[itemID] = nil
            return .failed(
                reason:
                    "Gave up after \(attempt) attempts: \(Self.describe(error))"
            )
        }

        // `delay(forAttempt:)` is seconds; the heartbeat is 1 Hz, so seconds
        // and ticks are the same unit. At least one tick, so a sub-second
        // backoff still costs a beat rather than re-attempting immediately.
        let seconds = retryPolicy.delay(forAttempt: attempt - 1).components.seconds
        retryHoldTicks[itemID] = Swift.max(1, Int(seconds))
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

    private func mutateItem(_ itemID: UUID, _ transform: (inout DownloadItem) -> Void) {
        for packageIndex in packages.indices {
            for itemIndex in packages[packageIndex].items.indices
            where packages[packageIndex].items[itemIndex].id == itemID {
                transform(&packages[packageIndex].items[itemIndex])
            }
        }
    }

    /// Queues the current package graph for durable storage and starts (or
    /// leaves running) the debounce window `tick()` drains.
    private func persist() async {
        await stateStore.save(PersistedState(packages: packages))
        if ticksSincePendingChange == nil { ticksSincePendingChange = 0 }
    }
}
