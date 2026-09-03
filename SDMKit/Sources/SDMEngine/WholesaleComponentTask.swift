import Foundation
import SDMCore

/// Drives a `WholesaleDownloader` (yt-dlp as a downloader) for one
/// HLS/DASH-only component, exposing the slice of `DownloadTask`'s
/// interface the engine's `Runner` touches. Non-resumable: progress is a
/// *synthesized* contiguous `RangeSet` derived from the downloader's
/// reports, and any failure or pause discards the partial output. Parent
/// spec `2026-09-03-multi-site-resolver-design.md` §6.7.
actor WholesaleComponentTask {
    let destinationURL: URL

    private let itemID: UUID
    private let pageURL: URL
    private let formatSelector: String
    private let downloader: any WholesaleDownloader

    /// Written by the downloader's `onProgress` (a sync callback off an
    /// arbitrary thread), read by the engine via the forwarders below.
    private let state = ProgressState()
    private var job: Task<Void, any Error>?
    private var isCancelled = false

    init(
        itemID: UUID, pageURL: URL, formatSelector: String, destinationURL: URL,
        downloader: any WholesaleDownloader
    ) {
        self.itemID = itemID
        self.pageURL = pageURL
        self.formatSelector = formatSelector
        self.destinationURL = destinationURL
        self.downloader = downloader
    }

    // MARK: - DownloadTask-shaped interface

    func start() async throws -> URL {
        if isCancelled { throw CancellationError() }
        let state = self.state
        let job = Task { [downloader, pageURL, formatSelector, destinationURL] in
            try await downloader.download(
                pageURL: pageURL, formatSelector: formatSelector, destination: destinationURL
            ) { progress in
                state.apply(progress)
            }
        }
        self.job = job
        do {
            try await job.value
        } catch {
            cleanupPartialOutput()
            state.reset()
            if error is CancellationError { throw WholesaleError.cancelled }
            throw error
        }
        guard FileManager.default.fileExists(atPath: destinationURL.path) else {
            state.reset()
            throw WholesaleError.failed(stderrTail: "yt-dlp produced no output file")
        }
        return destinationURL
    }

    func pause() {
        isCancelled = true
        job?.cancel()
        cleanupPartialOutput()
        state.reset()
    }

    /// Synthesized `[0, downloadedBytes)` — the one place a `RangeSet` is
    /// derived from an external reporter rather than from bytes SDM wrote.
    var completedRanges: RangeSet {
        var set = RangeSet()
        let done = state.downloadedBytesClamped
        if done > 0 { set.insert(ByteRange(start: 0, end: done)) }
        return set
    }

    var expectedTotalBytes: Int64? { state.totalBytes }
    var activeWorkerCount: Int { state.isActive ? 1 : 0 }
    var peakWorkerCount: Int { 1 }
    /// Never resumable — the scheduler must reserve this item's slot.
    var probedSupportsRanges: Bool? { false }
    var lastCheckpointFailure: String? { nil }
    var isAssembling: Bool { state.phase == .postProcessing }

    func checkpointTick() {}
    func setWorkerCount(_ count: Int) {}

    // MARK: - Cleanup

    private func cleanupPartialOutput() {
        let fm = FileManager.default
        try? fm.removeItem(at: destinationURL)
        try? fm.removeItem(at: destinationURL.appendingPathExtension("part"))
        let ytdlpTemp = destinationURL.deletingLastPathComponent()
            .appendingPathComponent(destinationURL.lastPathComponent + ".ytdl")
        try? fm.removeItem(at: ytdlpTemp)
    }
}

/// Thread-safe holder for the latest wholesale progress snapshot.
private final class ProgressState: @unchecked Sendable {
    private let lock = NSLock()
    private var downloaded: Int64 = 0
    private var total: Int64?
    private var phaseValue: WholesaleProgress.Phase = .downloading
    private var active = false

    func apply(_ progress: WholesaleProgress) {
        lock.withLock {
            active = true
            phaseValue = progress.phase
            if let t = progress.totalBytes { total = t }
            if let d = progress.downloadedBytes {
                downloaded = max(downloaded, d)
            } else if let fraction = progress.fraction, let t = total {
                downloaded = max(downloaded, Int64(fraction * Double(t)))
            }
        }
    }

    func reset() {
        lock.withLock {
            downloaded = 0
            total = nil
            phaseValue = .downloading
            active = false
        }
    }

    var downloadedBytesClamped: Int64 {
        lock.withLock {
            guard let total else { return downloaded }
            return Swift.min(downloaded, total)
        }
    }
    var totalBytes: Int64? { lock.withLock { total } }
    var phase: WholesaleProgress.Phase { lock.withLock { phaseValue } }
    var isActive: Bool { lock.withLock { active } }
}
