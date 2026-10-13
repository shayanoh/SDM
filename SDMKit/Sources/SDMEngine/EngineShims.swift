import Foundation
import SDMCore

extension EngineSettings {
    /// The Swift core's defaults for the tuning fields the app does not expose.
    public init(
        maxConcurrent: Int, segmentsPerItem: Int, globalMaxConnections: Int,
        maxConnectionsPerHost: Int = 8, downloadFolder: URL,
        minSegmentSizeBytes: Int64 = 10 * 1024 * 1024
    ) {
        self.init(
            maxConcurrent: maxConcurrent, segmentsPerItem: segmentsPerItem,
            globalMaxConnections: globalMaxConnections,
            maxConnectionsPerHost: maxConnectionsPerHost, downloadFolder: downloadFolder,
            checkpointIntervalBytes: 8 * 1024 * 1024,
            persistDebounceTicks: AppTiming.ticksPerSecond * 2,
            minSegmentSizeBytes: minSegmentSizeBytes,
            fileCheckIntervalTicks: AppTiming.ticksPerSecond * 5)
    }
}

extension DownloadEngine {
    /// Same call as `speedHistories(itemIds:)`, with the label the app used.
    public func speedHistories(for itemIDs: Set<UUID>) async throws -> [UUID: [Double]] {
        try await speedHistories(itemIds: itemIDs)
    }
}
