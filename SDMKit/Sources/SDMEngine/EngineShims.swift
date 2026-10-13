import Foundation
import SDMCore

extension EngineSettings {
    /// The core's defaults for the tuning fields the app does not expose; the
    /// per-host limit and minimum segment size are replaced when given. Throws
    /// when a count is below 1.
    public init(
        maxConcurrent: Int, segmentsPerItem: Int, globalMaxConnections: Int,
        maxConnectionsPerHost: Int? = nil, downloadFolder: URL,
        minSegmentSizeBytes: Int64? = nil
    ) throws {
        self = try engineSettingsDefaults(
            maxConcurrent: maxConcurrent, segmentsPerItem: segmentsPerItem,
            globalMaxConnections: globalMaxConnections, downloadFolder: downloadFolder)
        if let maxConnectionsPerHost { self.maxConnectionsPerHost = maxConnectionsPerHost }
        if let minSegmentSizeBytes { self.minSegmentSizeBytes = minSegmentSizeBytes }
    }
}

extension DownloadEngine {
    /// Same call as `speedHistories(itemIds:)`, with the label the app used.
    public func speedHistories(for itemIDs: Set<UUID>) async throws -> [UUID: [Double]] {
        try await speedHistories(itemIds: itemIDs)
    }
}
