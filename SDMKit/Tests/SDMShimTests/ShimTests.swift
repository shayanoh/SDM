import Foundation
import SDMCore
import SDMEngine
import SDMGrabber
import SDMResolve
import Testing

private let exampleURL = URL(string: "https://example.com/dir/movie.mp4")!

private func probedLink(suggestedFilename: String? = nil, url: URL = exampleURL) -> ProbedLink {
    ProbedLink(
        id: UUID(), originalUrl: url, finalUrl: url, stage: .done, statusCode: 200,
        contentLength: 10, contentType: nil, suggestedFilename: suggestedFilename,
        validator: nil, acceptsRanges: true, sniffedSignature: nil, transportFailed: false,
        verdict: .online, isDuplicate: false)
}

private func mediaRow() -> MediaRow {
    MediaRow(
        id: UUID(), sourceUrl: URL(string: "https://www.youtube.com/watch?v=abc")!,
        title: "abc", state: .resolving, media: nil, choice: nil, playlistGroup: nil,
        isDuplicate: false)
}

private func itemSnapshot(
    state: ItemState = .running, failedAttempts: Int? = nil, total: Int64? = 100,
    completed: [ByteRange] = []
) -> ItemSnapshot {
    ItemSnapshot(
        id: UUID(), url: exampleURL, filename: "a.bin", totalBytes: total,
        completed: RangeSet(ranges: completed), state: state, isEnabled: true,
        isResumable: nil, activeSegments: 0, configuredSegments: 1, bytesPerSecond: 0,
        speedHistory: [], checkpointFailure: nil, remainingAttempts: nil,
        failedAttemptCount: failedAttempts, lastFailureReason: nil, retryHoldSeconds: nil,
        fileMissing: false, isAssembling: false, assembly: .none, partFilenames: ["a.bin"],
        metadata: nil)
}

// MARK: SDMCore

@Test func ticksPerSecondComesFromTheCore() {
    #expect(AppTiming.ticksPerSecond == 5)
}

@Test func averageRecentSpeedForwardsAndFallsBack() {
    #expect(averageRecentSpeed(of: [], fallback: 7) == 7)
    #expect(averageRecentSpeed(of: [10, 10], fallback: 0) == 10)
}

@Test func byteRangeLengthAndEmptiness() {
    #expect(ByteRange(start: 5, end: 12).length == 7)
    #expect(ByteRange(start: 5, end: 5).isEmpty)
}

@Test func rangeSetInsertMergesAndCounts() {
    var set = RangeSet()
    set.insert(ByteRange(start: 0, end: 10))
    set.insert(ByteRange(start: 10, end: 20))
    set.insert(ByteRange(start: 30, end: 40))
    #expect(set.ranges == [ByteRange(start: 0, end: 20), ByteRange(start: 30, end: 40)])
    #expect(set.totalBytes == 30)
    #expect(set.gaps(within: 50) == [ByteRange(start: 20, end: 30), ByteRange(start: 40, end: 50)])
}

@Test func rejectedRangeLeavesTheSetUnchanged() {
    var set = RangeSet()
    set.insert(ByteRange(start: 0, end: 10))
    set.insert(ByteRange(start: -5, end: 3))
    #expect(set.totalBytes == 10)
}

@Test func defaultQualityPreferences() {
    #expect(QualityPreferences.default.maxHeight == 1080)
    #expect(QualityPreferences.default.containers == [.mp4, .webm])
}

@Test func emptyEngineSnapshotHasNothing() {
    #expect(EngineSnapshot.empty.packages.isEmpty)
    #expect(EngineSnapshot.empty.figures.runningCount == 0)
}

@Test func formatChoiceKnowsWholesale() {
    let plain = FormatChoice(
        video: nil, audio: nil, outputContainer: .mp4, estimatedBytes: nil, wholesaleSelector: nil)
    var wholesale = plain
    wholesale.wholesaleSelector = "bv*+ba"
    #expect(!plain.isWholesale)
    #expect(wholesale.isWholesale)
}

@Test func containerFileExtensions() {
    #expect(MediaContainer.mp4.fileExtension == "mp4")
    #expect(MediaContainer.m4a.fileExtension == "m4a")
    #expect(MediaContainer.other("ogg").fileExtension == "ogg")
}

@Test func retryingMeansQueuedAfterAFailure() {
    #expect(itemSnapshot(state: .queued, failedAttempts: 1).isRetrying)
    #expect(!itemSnapshot(state: .queued).isRetrying)
    #expect(!itemSnapshot(state: .running, failedAttempts: 1).isRetrying)
}

@Test func itemFractionCompleted() {
    let item = itemSnapshot(total: 200, completed: [ByteRange(start: 0, end: 50)])
    #expect(item.fractionCompleted == 0.25)
    #expect(itemSnapshot(total: nil).fractionCompleted == 0)
}

@Test func builtInThemesLoad() {
    #expect(ThemeCatalog.builtInThemes().contains { $0.id == "light" })
}

@Test func recordsAreIdentifiableByTheirId() {
    let item = itemSnapshot()
    #expect(item.id == item.id)
    let ids: [UUID] = [probedLink().id, mediaRow().id]
    #expect(ids.count == 2)
}

@Test func constructorsFillIdsAndDefaults() throws {
    let a = try downloadItemForUrl(
        url: exampleURL, filename: "a.bin", totalBytes: nil, state: .stopped)
    let b = try downloadItemForUrl(
        url: exampleURL, filename: "a.bin", totalBytes: 5, state: .queued)
    #expect(a.id != b.id)
    #expect(a.isEnabled && a.state == .stopped)
    let package = try downloadPackageNew(name: "P", items: [a, b], note: nil)
    #expect(package.items.count == 2)
}

// MARK: SDMResolve

@Test func cookieSourceRawValuesRoundTrip() {
    #expect(CookieSource.allCases.count == 6)
    for source in CookieSource.allCases {
        #expect(CookieSource(rawValue: source.rawValue) == source)
    }
    #expect(CookieSource(rawValue: "bogus") == nil)
}

@Test func channelRawValuesRoundTrip() {
    #expect(YtDlpChannel(rawValue: "nightly") == .nightly)
    #expect(YtDlpChannel.stable.rawValue == "stable")
    #expect(YtDlpChannel(rawValue: "") == nil)
}

@Test func vendorAssetOldLabelForwards() {
    let url = URL(fileURLWithPath: "/tmp/ffmpeg.lzfse")
    let asset = VendorAsset(name: "ffmpeg", compressedURL: url, version: "1")
    #expect(asset.compressedPath == url)
}

// MARK: SDMEngine

@Test func engineSettingsConvenienceInitUsesTheSwiftDefaults() {
    let settings = EngineSettings(
        maxConcurrent: 2, segmentsPerItem: 4, globalMaxConnections: 8,
        downloadFolder: URL(fileURLWithPath: "/tmp"))
    #expect(settings.maxConnectionsPerHost == 8)
    #expect(settings.checkpointIntervalBytes == 8 * 1024 * 1024)
    #expect(settings.persistDebounceTicks == AppTiming.ticksPerSecond * 2)
    #expect(settings.fileCheckIntervalTicks == AppTiming.ticksPerSecond * 5)
    #expect(settings.minSegmentSizeBytes == 10 * 1024 * 1024)
}

@Test func speedHistoriesOldLabelForwards() async throws {
    let directory = FileManager.default.temporaryDirectory
        .appendingPathComponent("sdm-shim-tests-\(UUID().uuidString)", isDirectory: true)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }
    let engine = try DownloadEngine(
        config: EngineConfig(
            stateFile: directory.appendingPathComponent("state.json"),
            settings: EngineSettings(
                maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 2,
                downloadFolder: directory),
            binarySearchPaths: [], cookieSource: .none, maxPlaylistVideos: 50,
            ytDlpExtraArguments: []))
    let histories = try await engine.speedHistories(for: [UUID()])
    #expect(histories.isEmpty)
    try await engine.shutdown()
}

// MARK: SDMGrabber

@Test func probedLinkAccessors() {
    let link = probedLink()
    #expect(link.originalURL == exampleURL)
    #expect(link.finalURL == exampleURL)
    #expect(link.effectiveFilename == "movie.mp4")
    #expect(probedLink(suggestedFilename: "named.zip").effectiveFilename == "named.zip")
}

@Test func mediaRowAccessors() {
    let row = mediaRow()
    #expect(row.sourceURL == row.sourceUrl)
    #expect(row.combinedBytes == nil)
    #expect(row.displayFilename == "abc")
}

@Test func packageCandidateLinkIDs() {
    let id = UUID()
    let package = PackageCandidate(
        id: UUID(), name: "p", linkIds: [id], isArchive: false, note: nil)
    #expect(package.linkIDs == [id])
}

@Test func grabberSnapshotCountsComeFromTheCore() {
    #expect(GrabberSnapshot.empty.onlineCount == 0 && !GrabberSnapshot.empty.isChecking)
    let snapshot = GrabberSnapshot(
        links: [probedLink()], mediaRows: [mediaRow()], packages: [], checkedCount: 1,
        totalCount: 2)
    #expect(snapshot.onlineCount == 1)
    #expect(snapshot.offlineCount == 0 && snapshot.faultyCount == 0 && snapshot.failedCount == 0)
    #expect(snapshot.isChecking)
    #expect(snapshot.recheckableCount == 0)
}

@Test func handoffBuildHoldsBackUnresolvedRows() {
    let result = MediaHandoff.build(httpLinks: [probedLink()], mediaRows: [mediaRow()])
    #expect(result.items.count == 1)
    #expect(result.heldBackCount == 1)
}

@Test func urlExtractorFindsLinks() {
    #expect(URLExtractor.extractLinks(from: "see https://example.com/a.zip now").count == 1)
    #expect(URLExtractor.extractLinks(from: "nothing here").isEmpty)
}

@Test func formatMenuForAnUnformattedMediaIsEmpty() {
    let media = ResolvedMedia(
        extractor: "youtube", videoId: "abc", title: "t", durationSeconds: nil, formats: [],
        sourceUrl: nil)
    #expect(MediaFormatMenu.options(for: media, preferences: .default).isEmpty)
}
