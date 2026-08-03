import Foundation
import SDMCore

public enum DownloadError: Error, Equatable {
    case unknownSize
    case serverError(status: Int)
    case incompleteAfterWorkersFinished
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
        try await runWorkers()

        guard completed.isComplete(total: totalBytes) else {
            throw DownloadError.incompleteAfterWorkersFinished
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
        try await withThrowingTaskGroup(of: Void.self) { group in
            for _ in 0..<configuration.workerCount {
                group.addTask { [weak self] in
                    guard let self else { return }
                    try await self.workerLoop()
                }
            }
            try await group.waitForAll()
        }
    }

    /// One worker: claim a gap, stream it to disk, repeat until nothing is left.
    private func workerLoop() async throws {
        let workerID = UUID()
        defer { reserved[workerID] = nil }

        while let claim = claimNext(for: workerID) {
            try await download(claim)
            reserved[workerID] = nil
        }
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
