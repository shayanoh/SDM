import Foundation
import SDMCore

public enum DownloadError: Error, Equatable {
    case unknownSize
    case serverError(status: Int)
    case incompleteAfterWorkersFinished
    /// The origin's response body ended before the claimed range was fully
    /// delivered, without the transport throwing — a clean short read. Bytes
    /// already written for the claim remain on disk and recorded in
    /// `completedRanges`; only the unwritten remainder is lost.
    case truncatedResponse(expected: Int64, received: Int64)
}

/// Downloads one file using a pool of ephemeral workers that claim gaps from a
/// shared `RangeSet`. See spec §5.
public actor DownloadTask {
    public struct Configuration: Sendable {
        public var workerCount: Int
        public var minChunk: Int64
        /// Bytes written per worker between sidecar checkpoints.
        public var checkpointInterval: Int64

        public init(workerCount: Int, minChunk: Int64, checkpointInterval: Int64) {
            precondition(workerCount >= 1, "workerCount must be at least 1")
            precondition(minChunk > 0, "minChunk must be positive")
            precondition(checkpointInterval > 0, "checkpointInterval must be positive")
            self.workerCount = workerCount
            self.minChunk = minChunk
            self.checkpointInterval = checkpointInterval
        }
    }

    public let id: UUID
    public let sourceURL: URL
    public let destinationURL: URL

    private let transport: any HTTPTransport
    private var configuration: Configuration

    private var completed = RangeSet()
    private var reserved: [UUID: ByteRange] = [:]
    private var totalBytes: Int64 = 0
    private var validator: String?
    private var file: SparseFile?
    private var bytesSinceCheckpoint: Int64 = 0

    /// The number of workers the pool is currently trying to keep running.
    private var targetWorkerCount: Int = 1
    /// Indices of workers currently inside `workerLoop`.
    private var liveWorkerIndices: Set<Int> = []
    /// Monotonic source of worker indices; never reused.
    private var nextWorkerIndex: Int = 0

    public init(
        id: UUID,
        sourceURL: URL,
        destinationURL: URL,
        transport: any HTTPTransport,
        configuration: Configuration
    ) {
        self.id = id
        self.sourceURL = sourceURL
        self.destinationURL = destinationURL
        self.transport = transport
        self.configuration = configuration
    }

    public var completedRanges: RangeSet { completed }
    public var activeWorkerCount: Int { reserved.count }

    private var sidecarURL: URL { ResumeSidecar.url(for: destinationURL) }

    /// Probes the origin, then runs the worker pool until the file is complete.
    public func start() async throws -> URL {
        try await prepare()

        do {
            try await runWorkers()
            guard completed.isComplete(total: totalBytes) else {
                throw DownloadError.incompleteAfterWorkersFinished
            }
        } catch {
            // Close the descriptor on any failure path so a caller that
            // retains a failed task (e.g. to inspect completedRanges before
            // retrying) doesn't leak the fd until the task deallocates.
            // `.incomplete` and the sidecar are left in place — a failed
            // download must stay resumable. SparseFile.close() is
            // idempotent, so this can't fight with Task 10's pause().
            file?.close()
            throw error
        }

        guard let file else { throw DownloadError.incompleteAfterWorkersFinished }
        let result = try file.finalize()
        self.file = nil
        ResumeSidecar.remove(at: sidecarURL)
        return result
    }

    /// Probes the resource and opens the destination file.
    private func prepare() async throws {
        let probe = try await transport.fetch(
            RangeRequest(url: sourceURL, range: ByteRange(start: 0, end: 1))
        )
        guard (200..<300).contains(probe.statusCode) else {
            throw DownloadError.serverError(status: probe.statusCode)
        }
        guard let size = probe.totalSize, size > 0 else { throw DownloadError.unknownSize }

        totalBytes = size
        validator = probe.validator
        file = try SparseFile(finalURL: destinationURL, totalBytes: size)
    }

    private func runWorkers() async throws {
        targetWorkerCount = configuration.workerCount

        try await withThrowingTaskGroup(of: Void.self) { group in
            var spawned = 0
            while spawned < targetWorkerCount {
                spawnWorker(into: &group)
                spawned += 1
            }

            // Drain finished workers, spawning replacements when the target
            // rises above the number still running.
            while try await group.next() != nil {
                while liveWorkerIndices.count < targetWorkerCount, hasWorkRemaining() {
                    spawnWorker(into: &group)
                }
            }
        }
    }

    private func spawnWorker(into group: inout ThrowingTaskGroup<Void, any Error>) {
        let index = nextWorkerIndex
        nextWorkerIndex += 1
        liveWorkerIndices.insert(index)
        group.addTask { [weak self] in
            guard let self else { return }
            try await self.workerLoop(index: index)
        }
    }

    private func hasWorkRemaining() -> Bool {
        completed.nextClaim(
            total: totalBytes,
            reserved: Array(reserved.values),
            minChunk: configuration.minChunk
        ) != nil
    }

    /// One worker: claim a gap, stream it to disk, repeat until the file is
    /// done or this worker is retired by a lowered target count.
    private func workerLoop(index: Int) async throws {
        let workerID = UUID()
        defer {
            reserved[workerID] = nil
            liveWorkerIndices.remove(index)
        }

        while !shouldRetire(index: index) {
            guard let claim = claimNext(for: workerID) else { return }
            try await download(claim)
            reserved[workerID] = nil
        }
    }

    /// Retires the highest-indexed workers first, so lowering the target from
    /// 100 to 3 keeps three long-lived workers rather than churning all of them.
    private func shouldRetire(index: Int) -> Bool {
        let rank = liveWorkerIndices.sorted().firstIndex(of: index) ?? 0
        return rank >= targetWorkerCount
    }

    /// Changes the number of concurrent workers mid-download.
    ///
    /// Raising it spawns more as slots free; lowering it retires surplus
    /// workers after they finish their current claim. Their partial progress is
    /// already in `completed`, so survivors simply pick up the resulting gaps.
    public func setWorkerCount(_ count: Int) {
        precondition(count >= 1, "workerCount must be at least 1")
        targetWorkerCount = count
        configuration.workerCount = count
    }

    private func claimNext(for workerID: UUID) -> ByteRange? {
        guard
            let claim = completed.nextClaim(
                total: totalBytes,
                reserved: Array(reserved.values),
                minChunk: configuration.minChunk
            )
        else { return nil }
        reserved[workerID] = claim
        return claim
    }

    private func download(_ claim: ByteRange) async throws {
        let response = try await transport.fetch(RangeRequest(url: sourceURL, range: claim))
        guard (200..<300).contains(response.statusCode) else {
            throw DownloadError.serverError(status: response.statusCode)
        }

        var offset = claim.start
        for try await chunk in response.body {
            guard offset < claim.end else { break }
            let writable = Swift.min(Int64(chunk.count), claim.end - offset)
            let slice = chunk.prefix(Int(writable))
            try file?.write(Data(slice), at: offset)
            record(ByteRange(start: offset, end: offset + writable))
            offset += writable
        }

        // The stream ended cleanly (no throw) but didn't cover the whole
        // claim — a truncated body. Bytes written so far are already on
        // disk and folded into `completed` above; make the short read a
        // defined failure instead of silently re-handing the same gap back
        // to `nextClaim` forever.
        guard offset >= claim.end else {
            throw DownloadError.truncatedResponse(
                expected: claim.end - claim.start,
                received: offset - claim.start
            )
        }
    }

    /// Folds a freshly written range into the completed set, checkpointing
    /// the sidecar every `checkpointInterval` bytes.
    private func record(_ range: ByteRange) {
        completed.insert(range)
        bytesSinceCheckpoint += range.length
        if bytesSinceCheckpoint >= configuration.checkpointInterval {
            checkpoint()
        }
    }

    private func checkpoint() {
        bytesSinceCheckpoint = 0
        let sidecar = ResumeSidecar(
            sourceURL: sourceURL,
            totalBytes: totalBytes,
            validator: validator,
            completed: completed
        )
        try? sidecar.save(to: sidecarURL)
    }
}
