import Foundation
import SDMCore

/// Resume state written alongside the `.incomplete` file, so the file and its
/// recovery information travel together. See spec §4.3.
public struct ResumeSidecar: Codable, Equatable, Sendable {
    public static let currentFormatVersion = 1

    public var formatVersion: Int
    public var sourceURL: URL
    public var totalBytes: Int64
    /// `ETag` or `Last-Modified` captured when the download started.
    public var validator: String?
    public var completed: RangeSet

    public init(
        formatVersion: Int = ResumeSidecar.currentFormatVersion,
        sourceURL: URL,
        totalBytes: Int64,
        validator: String?,
        completed: RangeSet
    ) {
        self.formatVersion = formatVersion
        self.sourceURL = sourceURL
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
