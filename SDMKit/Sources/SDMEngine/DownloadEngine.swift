import Foundation
import SDMCore

public struct EngineSettings: Sendable {
    public var maxConcurrent: Int
    public var segmentsPerItem: Int
    /// Carried for the UI and for Phase 3; not enforced here yet.
    public var globalMaxConnections: Int
    public var downloadFolder: URL

    public init(
        maxConcurrent: Int,
        segmentsPerItem: Int,
        globalMaxConnections: Int,
        downloadFolder: URL
    ) {
        precondition(maxConcurrent >= 1, "maxConcurrent must be at least 1")
        precondition(segmentsPerItem >= 1, "segmentsPerItem must be at least 1")
        precondition(globalMaxConnections >= 1, "globalMaxConnections must be at least 1")
        self.maxConcurrent = maxConcurrent
        self.segmentsPerItem = segmentsPerItem
        self.globalMaxConnections = globalMaxConnections
        self.downloadFolder = downloadFolder
    }
}

/// Owns the package list, the running `DownloadTask`s, and the scheduler.
///
/// `tick()` is driven by the app once per second rather than by an internal
/// timer, so the engine is fully deterministic under test. It is also what
/// picks up newly runnable work: a finished download does not reschedule from
/// inside its own completion path, because a transient failure would then
/// re-attempt itself in a hot loop, and Phase 1 deliberately does not retry.
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

    public init(
        transport: any HTTPTransport,
        stateStore: any StateStore,
        settings: EngineSettings
    ) {
        self.transport = transport
        self.stateStore = stateStore
        self.settings = settings
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

    /// One-second heartbeat: folds the window's bytes into the samplers,
    /// drives the wall-clock half of the sidecar checkpoint trigger, closes
    /// the speed window, and reschedules.
    public func tick() async {
        for (itemID, runner) in runners where !runner.isRetiring {
            let transferred = await runner.task.completedRanges.totalBytes
            recordProgress(transferred, for: itemID)
            // Spec §4.3: "every ~8 MB per worker or every 5 s, whichever comes
            // first". `DownloadTask.record` implements the byte half; this is
            // the only caller of the wall-clock half, so without it a slow
            // origin would never checkpoint at all.
            await runner.task.checkpointTick()
        }
        for itemID in Array(samplers.keys) { samplers[itemID]?.tick() }
        globalSampler.tick()
        await reconcile()
    }

    public func snapshot() async -> EngineSnapshot {
        var packageSnapshots: [PackageSnapshot] = []
        for package in packages {
            var items: [ItemSnapshot] = []
            for item in package.items {
                var completed = item.completed
                var totalBytes = item.totalBytes
                var active = 0
                if let runner = runners[item.id] {
                    completed = await runner.task.completedRanges
                    totalBytes = await runner.task.expectedTotalBytes ?? totalBytes
                    active = await runner.task.activeWorkerCount
                }
                let sampler = samplers[item.id] ?? SpeedSampler()
                items.append(
                    ItemSnapshot(
                        id: item.id,
                        filename: item.filename,
                        totalBytes: totalBytes,
                        completed: completed,
                        state: item.state,
                        isEnabled: item.isEnabled,
                        isResumable: item.isResumable,
                        activeSegments: active,
                        configuredSegments: segmentCount(for: item.id),
                        bytesPerSecond: sampler.bytesPerSecond,
                        speedHistory: sampler.history
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
    public func shutdown() async {
        isShutDown = true
        let live = runners.values.map { $0 }
        for runner in live { runner.job.cancel() }
        for runner in live { await runner.task.pause() }
        for runner in live { _ = await runner.job.value }
        await persist()
        await stateStore.flush()
    }

    /// Test helper: pumps the engine until nothing is left running.
    ///
    /// Bounded rather than open-ended. Phase 1 does not retry, but a
    /// transiently failing item still returns to `queued` and stays eligible,
    /// so an unbounded loop would spin forever on an origin that never
    /// succeeds instead of failing the test.
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
    /// to start the same item. `startedRecently` is empty: hysteresis needs a
    /// clock and is deferred to Phase 3.
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
                packages: packages,
                runningNow: Set(runners.keys),
                startedRecently: [],
                maxConcurrent: settings.maxConcurrent
            )
        )

        var retiring: [DownloadTask] = []
        for (itemID, runner) in runners where !desired.contains(itemID) && !runner.isRetiring {
            runners[itemID]?.isRetiring = true
            runner.job.cancel()
            retiring.append(runner.task)
            mutateItem(itemID) { $0.state = .queued }
        }

        // Two tasks writing the same destination would interleave at absolute
        // offsets and corrupt it. Retiring runners are still in `runners`, so
        // this also covers restarting an item whose previous task has not
        // unwound yet.
        var claimed = Set(runners.values.map(\.destinationURL))
        for itemID in desired where runners[itemID] == nil {
            guard let context = context(for: itemID) else { continue }
            guard claimed.insert(context.destinationURL).inserted else { continue }
            mutateItem(itemID) { $0.state = .running }
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
                sourceURL: context.sourceURL,
                destinationURL: context.destinationURL,
                transport: transport,
                configuration: DownloadTask.Configuration(
                    workerCount: context.segments,
                    minChunk: 64 * 1024,
                    checkpointInterval: 8 * 1024 * 1024
                )
            )
            let job = Task { [weak self] in
                guard let self else { return }
                await self.run(itemID: itemID, task: task)
            }
            runners[itemID] = Runner(
                task: task,
                job: job,
                destinationURL: context.destinationURL
            )
        }

        for task in retiring { await task.pause() }
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

    private func context(for itemID: UUID) -> RunContext? {
        for package in packages {
            guard let item = package.items.first(where: { $0.id == itemID }) else { continue }
            let folder = settings.downloadFolder.appendingPathComponent(package.name)
            try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
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
        } catch {
            // Phase 1 does not retry. A transient failure returns the item to
            // `queued`; the next `tick()` decides whether to attempt it again.
            if case .permanent(let reason) = RetryPolicy().classify(error) {
                state = .failed(reason: reason)
            } else {
                state = .queued
            }
        }

        let completed = await task.completedRanges
        let totalBytes = await task.expectedTotalBytes
        // Also captured here, not only in `refreshResumability()`: a download
        // short enough to finish before any reconcile happens would otherwise
        // never record what its probe found.
        let isResumable = await task.probedSupportsRanges
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
        mutateItem(itemID) {
            $0.completed = completed
            if let totalBytes { $0.totalBytes = totalBytes }
            if let isResumable { $0.isResumable = isResumable }
            $0.state = state
        }
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

    private func persist() async {
        await stateStore.save(PersistedState(packages: packages))
    }
}
