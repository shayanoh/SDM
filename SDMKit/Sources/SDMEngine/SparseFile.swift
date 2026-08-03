import Foundation

/// The on-disk destination for a download, written at arbitrary offsets by
/// multiple concurrent workers.
///
/// Marked `@unchecked Sendable` because it wraps a POSIX file descriptor
/// guarded by an internal lock; `pwrite` to disjoint offsets is safe across
/// threads. This is the only type in the engine using that escape hatch.
public final class SparseFile: @unchecked Sendable {
    public enum FileError: Error, Equatable {
        case couldNotOpen(path: String, errno: Int32)
        case writeFailed(errno: Int32)
        case truncateFailed(errno: Int32)
    }

    public let finalURL: URL
    public let incompleteURL: URL
    private let descriptor: Int32
    private let lock = NSLock()
    private var isClosed = false

    public static func incompleteURL(for finalURL: URL) -> URL {
        finalURL.appendingPathExtension("incomplete")
    }

    /// Opens (creating if needed) the `.incomplete` file and preallocates it.
    public init(finalURL: URL, totalBytes: Int64) throws {
        precondition(totalBytes >= 0, "totalBytes must be non-negative")
        self.finalURL = finalURL
        self.incompleteURL = Self.incompleteURL(for: finalURL)

        let path = incompleteURL.path
        let fd = open(path, O_RDWR | O_CREAT, 0o644)
        guard fd >= 0 else { throw FileError.couldNotOpen(path: path, errno: errno) }
        self.descriptor = fd

        // Sparse on APFS: allocates no blocks until bytes are actually written.
        guard ftruncate(fd, off_t(totalBytes)) == 0 else {
            _ = Foundation.close(fd)
            throw FileError.truncateFailed(errno: errno)
        }
    }

    public func write(_ data: Data, at offset: Int64) throws {
        precondition(offset >= 0, "offset must be non-negative")
        guard !data.isEmpty else { return }

        lock.lock()
        defer { lock.unlock() }

        try data.withUnsafeBytes { buffer in
            var written = 0
            while written < buffer.count {
                let result = pwrite(
                    descriptor,
                    buffer.baseAddress!.advanced(by: written),
                    buffer.count - written,
                    off_t(offset) + off_t(written)
                )
                guard result > 0 else { throw FileError.writeFailed(errno: errno) }
                written += result
            }
        }
    }

    public func sync() throws {
        lock.lock()
        defer { lock.unlock() }
        guard fsync(descriptor) == 0 else { throw FileError.writeFailed(errno: errno) }
    }

    /// Flushes, closes, and renames the file to its final name.
    public func finalize() throws -> URL {
        try sync()
        close()
        try FileManager.default.moveItem(at: incompleteURL, to: finalURL)
        return finalURL
    }

    public func close() {
        lock.lock()
        defer { lock.unlock() }
        guard !isClosed else { return }
        _ = Foundation.close(descriptor)
        isClosed = true
    }

    deinit {
        if !isClosed { _ = Foundation.close(descriptor) }
    }
}
