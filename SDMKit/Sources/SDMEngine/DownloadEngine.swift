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
    /// with, and it keeps the item counted against `maxConcurrent` for as
    /// long as it really holds a slot.
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
    private func reconcile() async {
        guard !isShutDown else { return }

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
        finish(
            itemID: itemID, task: task, completed: completed, totalBytes: totalBytes, state: state)
        await persist()
    }

    /// Retires a finished runner, keeping whatever bytes it managed so a
    /// preempted or failed item resumes rather than restarts.
    private func finish(
        itemID: UUID,
        task: DownloadTask,
        completed: RangeSet,
        totalBytes: Int64?,
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
            $0.state = state
        }
    }

    // MARK: - Telemetry

    /// Folds a monotonically growing byte total into the per-item and global
    /// samplers as a delta, so the same bytes are never counted twice.
    private func recordProgress(_ transferred: Int64, for itemID: UUID) {
        let previous = sampledBytes[itemID] ?? 0
        guard transferred > previous else {
            // A restart that could not resume goes backwards; rebase rather
            // than record a negative delta.
            if transferred < previous { sampledBytes[itemID] = transferred }
            return
        }
        sampledBytes[itemID] = transferred
        samplers[itemID]?.record(bytes: transferred - previous)
        globalSampler.record(bytes: transferred - previous)
    }

    // MARK: - Helpers

    private func segmentCount(for itemID: UUID) -> Int {
        segmentOverrides[itemID] ?? settings.segmentsPerItem
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
