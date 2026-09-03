import Foundation
import SDMCore

/// Drives a `WholesaleDownloader` (yt-dlp as a downloader) for one
/// HLS/DASH-only component, exposing the slice of `DownloadTask`'s
/// interface the engine's `Runner` touches. Progress is a *synthesized*
/// contiguous `RangeSet` derived from the downloader's reports.
///
/// Resume: a pause / preempt keeps the yt-dlp scratch (`.part` / `.ytdl`
/// / fragments) on disk so the next run's `--continue` picks up from the
/// last completed fragment. A hard (non-cancellation) failure sweeps the
/// scratch so the next attempt starts clean. The component only reports
/// itself resumable once it has seen a *fragmented* progress report —
/// confirmation that yt-dlp's native (resumable) downloader is in use.
/// Parent specs `2026-09-03-multi-site-resolver-design.md` §6.7 and
/// `2026-09-03-wholesale-resume-design.md`.
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
            // Cancellation is a pause / preempt: keep every scratch file and
            // the synthesized progress so the next run resumes via
            // `--continue`. A real failure sweeps the scratch so the retry
            // starts clean.
            if error is CancellationError { throw WholesaleError.cancelled }
            cleanupPartialOutput()
            state.reset()
            throw error
        }
        guard FileManager.default.fileExists(atPath: destinationURL.path) else {
            cleanupPartialOutput()
            state.reset()
            throw WholesaleError.failed(stderrTail: "yt-dlp produced no output file")
        }
        // Success — remove any fragment / metadata siblings yt-dlp left, but
        // keep the finished output file itself.
        cleanupScratchOnly()
        return destinationURL
    }

    func pause() {
        isCancelled = true
        job?.cancel()
        // Deliberately keeps the yt-dlp scratch and the synthesized progress:
        // the next run resumes from the last completed fragment.
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
    /// `false` until a fragmented progress report confirms yt-dlp's native
    /// (resumable) downloader is in use — so the scheduler reserves this
    /// item's slot and never preempts it until resume is known to work.
    /// `true` afterward: the item is preemptible like any resumable
    /// download, and a pause / preempt keeps its fragments for `--continue`.
    var probedSupportsRanges: Bool? { state.sawFragmentedProgress ? true : false }
    var lastCheckpointFailure: String? { nil }
    var isAssembling: Bool { state.phase == .postProcessing }

    func checkpointTick() {}
    func setWorkerCount(_ count: Int) {}

    // MARK: - Cleanup

    private func cleanupPartialOutput() {
        let fm = FileManager.default
        let folder = destinationURL.deletingLastPathComponent()
        // `destinationURL` for a wholesale component is the item's final
        // output path, so sweep the output plus every yt-dlp scratch sibling
        // (`.ytdl`, `.part*`, `.fragN`, `Clip.fNNN.*`, `Clip.temp.*`).
        for url in YtDlpArtifacts.allFiles(
            in: folder, outputFilename: destinationURL.lastPathComponent)
        {
            try? fm.removeItem(at: url)
        }
    }

    /// Scratch siblings only — the finished output file stays.
    private func cleanupScratchOnly() {
        let fm = FileManager.default
        let folder = destinationURL.deletingLastPathComponent()
        for url in YtDlpArtifacts.scratchFiles(
            in: folder, outputFilename: destinationURL.lastPathComponent)
        {
            try? fm.removeItem(at: url)
        }
    }
}

/// Thread-safe holder for the latest wholesale progress snapshot.
private final class ProgressState: @unchecked Sendable {
    private let lock = NSLock()
    private var downloaded: Int64 = 0
    private var total: Int64?
    private var phaseValue: WholesaleProgress.Phase = .downloading
    private var active = false
    private var sawFragmented = false

    func apply(_ progress: WholesaleProgress) {
        lock.withLock {
            active = true
            phaseValue = progress.phase
            if progress.isFragmented { sawFragmented = true }

            if let d = progress.downloadedBytes {
                downloaded = max(downloaded, d)
            }

            // Total, re-derived on every report so the UI tracks yt-dlp's
            // moving estimate. For a fragmented (HLS/DASH) download there is
            // no real `total_bytes`; the reliable signal is the fragment
            // ratio, so `downloaded / fraction` is used — this makes the
            // synthesized progress bar exactly track yt-dlp's monotonic
            // fragment progress. `progress.totalBytes` (a real size, or
            // yt-dlp's float estimate before enough fragments exist) is the
            // fallback. Parent spec §6.7.
            if let fraction = progress.fraction, fraction >= 0.005, downloaded > 0 {
                total = Int64(Double(downloaded) / fraction)
            } else if let reported = progress.totalBytes {
                total = reported
            }

            // A byte-less report that still carries a fraction (rare).
            if progress.downloadedBytes == nil,
                let fraction = progress.fraction, let total
            {
                downloaded = max(downloaded, Int64(fraction * Double(total)))
            }
        }
    }

    func reset() {
        lock.withLock {
            downloaded = 0
            total = nil
            phaseValue = .downloading
            active = false
            sawFragmented = false
        }
    }

    var downloadedBytesClamped: Int64 {
        lock.withLock {
            guard let total else { return downloaded }
            return Swift.min(downloaded, total)
        }
    }
    var totalBytes: Int64? { lock.withLock { total } }
    var sawFragmentedProgress: Bool { lock.withLock { sawFragmented } }
    var phase: WholesaleProgress.Phase { lock.withLock { phaseValue } }
    var isActive: Bool { lock.withLock { active } }
}
