import Foundation
import SDMCore

/// Resume state written alongside the `.incomplete` file, so the file and its
/// recovery information travel together. See spec §4.3.
public struct ResumeSidecar: Codable, Equatable, Sendable {
    public static let currentFormatVersion = 1

    public var formatVersion: Int
    public var sourceURL: URL
    /// The URL the last successful probe/fetch actually resolved to, after
    /// redirects. A redirecting origin (e.g. a mirror-network frontend, like
    /// Fedora's `download.fedoraproject.org`) can send each connection to a
    /// different backend with its own `ETag`/`Last-Modified`; probing
    /// `sourceURL` fresh on every resume would then compare against whichever
    /// mirror happens to answer *this time*, not the one the bytes on disk
    /// actually came from, and `matches` would spuriously fail and discard a
    /// perfectly good partial download. Reusing the pinned mirror keeps the
    /// validator comparison meaningful across resumes. `nil` for sidecars
    /// written before this field existed; those fall back to re-resolving
    /// `sourceURL`.
    public var resolvedURL: URL?
    public var totalBytes: Int64
    /// `ETag` or `Last-Modified` captured when the download started.
    public var validator: String?
    public var completed: RangeSet
    /// The subset of `completed` that had been `fsync`ed when this sidecar
    /// was written. The file is only flushed to the device about once a
    /// minute (`DownloadTask.Configuration.syncIntervalTicks`), while the
    /// sidecar is rewritten far more often, so `completed` can be ahead of
    /// what would survive a power cut. `nil` for sidecars written before
    /// this field existed — those were always flushed first, so their
    /// `completed` is itself durable.
    public var durableCompleted: RangeSet?
    /// `kern.bootsessionuuid` when this sidecar was written. Same value now
    /// means the machine has not restarted since, so everything written is
    /// still in the page cache (or on disk) and `completed` can be trusted
    /// whole. See `resumableRanges(bootSessionID:)`.
    public var bootSessionID: String?

    public init(
        formatVersion: Int = ResumeSidecar.currentFormatVersion,
        sourceURL: URL,
        resolvedURL: URL?,
        totalBytes: Int64,
        validator: String?,
        completed: RangeSet,
        durableCompleted: RangeSet? = nil,
        bootSessionID: String? = nil
    ) {
        self.durableCompleted = durableCompleted
        self.bootSessionID = bootSessionID
        self.formatVersion = formatVersion
        self.sourceURL = sourceURL
        self.resolvedURL = resolvedURL
        self.totalBytes = totalBytes
        self.validator = validator
        self.completed = completed
    }

    public static func url(for finalURL: URL) -> URL {
        finalURL.appendingPathExtension("sdmpart")
    }

    /// Whether the remote resource still matches what was captured at start.
    public func matches(totalBytes: Int64, validator: String?) -> Bool {
        self.totalBytes == totalBytes && self.validator == validator
    }

    /// The ranges a resume may trust.
    ///
    /// Written bytes that were never `fsync`ed survive a crash or a force
    /// quit — the kernel still holds them — but not a power cut or kernel
    /// panic. A changed boot session is how the difference is told apart:
    /// same session, trust everything recorded; different session (or one
    /// that cannot be determined), fall back to what was flushed. Trusting
    /// `completed` after a power cut would stitch new bytes onto blocks
    /// that never reached the disk and finalize a silently corrupt file.
    public func resumableRanges(bootSessionID current: String?) -> RangeSet {
        guard let durableCompleted else { return completed }
        if let current, let bootSessionID, current == bootSessionID { return completed }
        return durableCompleted
    }

    /// This boot's `kern.bootsessionuuid`, or `nil` if it cannot be read.
    public static let currentBootSessionID: String? = {
        var size = 0
        guard sysctlbyname("kern.bootsessionuuid", nil, &size, nil, 0) == 0, size > 0 else {
            return nil
        }
        var buffer = [CChar](repeating: 0, count: size)
        guard sysctlbyname("kern.bootsessionuuid", &buffer, &size, nil, 0) == 0 else {
            return nil
        }
        return String(cString: buffer)
    }()

    public func save(to url: URL) throws {
        let data = try JSONEncoder().encode(self)
        try data.write(to: url, options: .atomic)
    }

    /// Loads a sidecar, returning `nil` when it is missing, unreadable,
    /// corrupt, or written by a newer format version.
    ///
    /// Deliberately non-throwing: an unusable sidecar always means "restart
    /// from zero", never "retry the load".
    public static func load(from url: URL) -> ResumeSidecar? {
        guard let data = try? Data(contentsOf: url),
            let sidecar = try? JSONDecoder().decode(ResumeSidecar.self, from: data),
            sidecar.formatVersion == currentFormatVersion
        else { return nil }
        return sidecar
    }

    public static func remove(at url: URL) {
        try? FileManager.default.removeItem(at: url)
    }
}
