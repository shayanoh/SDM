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

/// Compiles only for an `Identifiable` type, and returns the id it reports.
private func identity<T: Identifiable>(_ value: T) -> T.ID { value.id }

@Test func recordsAreIdentifiableByTheirId() throws {
    let item = itemSnapshot()
    #expect(identity(item) == item.id)
    let link = probedLink()
    #expect(identity(link) == link.id)
    let row = mediaRow()
    #expect(identity(row) == row.id)

    let downloadItem = try downloadItemForUrl(
        url: exampleURL, filename: "a.bin", totalBytes: nil, state: .stopped)
    #expect(identity(downloadItem) == downloadItem.id)
    #expect(identity(downloadItem.components[0]) == downloadItem.components[0].id)
    let package = try downloadPackageNew(name: "P", items: [downloadItem], note: nil)
    #expect(identity(package) == package.id)

    let packageSnapshot = PackageSnapshot(
        id: UUID(), name: "p", priority: .normal, items: [item],
        aggregate: try packageAggregateForItems(items: [item], globalRecentSpeed: 0), revision: 0)
    #expect(identity(packageSnapshot) == packageSnapshot.id)
    let candidate = PackageCandidate(
        id: UUID(), name: "p", linkIds: [], isArchive: false, note: nil)
    #expect(identity(candidate) == candidate.id)

    let format = MediaFormat(
        id: "137", kind: .videoOnly, height: 1080, width: 1920, vcodec: .h264, acodec: nil,
        container: .mp4, filesize: 10, filesizeApprox: nil, tbr: nil, url: exampleURL,
        delivery: .direct)
    #expect(identity(format) == "137")
    let option = MediaFormatOption(
        id: "opt", label: "1080p",
        choice: FormatChoice(
            video: format, audio: nil, outputContainer: .mp4, estimatedBytes: nil,
            wholesaleSelector: nil),
        matchesPreferences: true, isWholesale: false)
    #expect(identity(option) == "opt")
    let theme = try #require(ThemeCatalog.builtInThemes().first)
    #expect(identity(theme) == theme.id)
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

/// UserDefaults already holds these strings (the Swift core's `String` raw
/// values); they must read and write unchanged.
@Test func cookieSourceRawValuesAreTheStringsAlreadyPersisted() {
    let expected: [(CookieSource, String)] = [
        (.none, "none"), (.safari, "safari"), (.chrome, "chrome"), (.firefox, "firefox"),
        (.edge, "edge"), (.brave, "brave"),
    ]
    #expect(CookieSource.allCases == expected.map(\.0))
    for (source, raw) in expected {
        #expect(source.rawValue == raw)
        #expect(CookieSource(rawValue: raw) == source)
    }
    #expect(CookieSource(rawValue: "bogus") == nil)
    #expect(CookieSource(rawValue: "") == nil)
}

@Test func channelRawValuesAreTheStringsAlreadyPersisted() {
    #expect(YtDlpChannel.allCases == [.stable, .nightly])
    #expect(YtDlpChannel.stable.rawValue == "stable")
    #expect(YtDlpChannel.nightly.rawValue == "nightly")
    #expect(YtDlpChannel(rawValue: "stable") == .stable)
    #expect(YtDlpChannel(rawValue: "nightly") == .nightly)
    #expect(YtDlpChannel(rawValue: "") == nil)
}

@Test func vendorAssetOldLabelForwards() {
    let url = URL(fileURLWithPath: "/tmp/ffmpeg.lzfse")
    let asset = VendorAsset(name: "ffmpeg", compressedURL: url, version: "1")
    #expect(asset.compressedPath == url)
}

// MARK: SDMEngine

@Test func engineSettingsConvenienceInitUsesTheCoreDefaults() throws {
    let settings = try EngineSettings(
        maxConcurrent: 2, segmentsPerItem: 4, globalMaxConnections: 8,
        downloadFolder: URL(fileURLWithPath: "/tmp"))
    #expect(settings.maxConnectionsPerHost == 8)
    #expect(settings.checkpointIntervalBytes == 8 * 1024 * 1024)
    #expect(settings.persistDebounceTicks == AppTiming.ticksPerSecond * 2)
    #expect(settings.fileCheckIntervalTicks == AppTiming.ticksPerSecond * 5)
    #expect(settings.minSegmentSizeBytes == 10 * 1024 * 1024)
    // The two the app does expose replace the defaults when given.
    let tuned = try EngineSettings(
        maxConcurrent: 2, segmentsPerItem: 4, globalMaxConnections: 8, maxConnectionsPerHost: 3,
        downloadFolder: URL(fileURLWithPath: "/tmp"), minSegmentSizeBytes: 5)
    #expect(tuned.maxConnectionsPerHost == 3 && tuned.minSegmentSizeBytes == 5)
    #expect(throws: CoreError.self) {
        _ = try EngineSettings(
            maxConcurrent: 0, segmentsPerItem: 1, globalMaxConnections: 1,
            downloadFolder: URL(fileURLWithPath: "/tmp"))
    }
}

@Test func speedHistoriesOldLabelForwards() async throws {
    let directory = FileManager.default.temporaryDirectory
        .appendingPathComponent("sdm-shim-tests-\(UUID().uuidString)", isDirectory: true)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: directory) }
    let engine = try DownloadEngine(
        config: EngineConfig(
            stateFile: directory.appendingPathComponent("state.json"),
            settings: try EngineSettings(
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
    var row = mediaRow()
    row.choice = FormatChoice(
        video: nil, audio: nil, outputContainer: .mp4, estimatedBytes: 110, wholesaleSelector: nil)
    #expect(row.combinedBytes == 110)
    row.choice = nil
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
    #expect(GrabberSnapshot.empty.summary == .empty)
    let snapshot = GrabberSnapshot(
        links: [probedLink()], mediaRows: [mediaRow()], packages: [], checkedCount: 1,
        totalCount: 2)
    let summary = snapshot.summary
    #expect(summary.onlineCount == 1)
    #expect(summary.offlineCount == 0 && summary.faultyCount == 0 && summary.failedCount == 0)
    #expect(summary.isChecking)
    #expect(summary.recheckableCount == 0)
    // A failed probe is something to recheck, and counts as failed.
    var failed = probedLink()
    failed.stage = .done
    failed.verdict = .checkFailed
    failed.transportFailed = true
    let withFailure = GrabberSnapshot(
        links: [failed], mediaRows: [], packages: [], checkedCount: 1, totalCount: 1)
    #expect(withFailure.summary.failedCount == 1)
    #expect(withFailure.summary.recheckableCount == 1)
    #expect(withFailure.summary.onlineCount == 0)
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

@Test func formatMenuListsAResolvedMediaFormat() {
    let video = MediaFormat(
        id: "137", kind: .videoOnly, height: 1080, width: 1920, vcodec: .h264, acodec: nil,
        container: .mp4, filesize: 1000, filesizeApprox: nil, tbr: nil, url: exampleURL,
        delivery: .direct)
    let audio = MediaFormat(
        id: "140", kind: .audioOnly, height: nil, width: nil, vcodec: nil, acodec: .aac,
        container: .m4a, filesize: 100, filesizeApprox: nil, tbr: nil, url: exampleURL,
        delivery: .direct)
    let media = ResolvedMedia(
        extractor: "youtube", videoId: "abc", title: "t", durationSeconds: nil,
        formats: [video, audio], sourceUrl: nil)
    let options = MediaFormatMenu.options(for: media, preferences: .default)
    #expect(!options.isEmpty)
    #expect(options.contains { $0.choice.video?.id == "137" })
}

@Test func rangeSetUnionShiftsAndMergesInOneCall() throws {
    let merged = try rangeSetUnion(parts: [
        OffsetRangeSet(set: RangeSet(ranges: [ByteRange(start: 0, end: 100)]), offset: 0),
        OffsetRangeSet(
            set: RangeSet(ranges: [ByteRange(start: 0, end: 50), ByteRange(start: 80, end: 100)]),
            offset: 100),
    ])
    #expect(merged.ranges == [ByteRange(start: 0, end: 150), ByteRange(start: 180, end: 200)])
}

@Test func priorityRawValuesArePinned() {
    #expect(
        [Priority.lowest, .low, .normal, .high, .highest].map(\.rawValue) == [0, 1, 2, 3, 4])
}

@Test func formatMenuForAnUnformattedMediaIsEmpty() {
    let media = ResolvedMedia(
        extractor: "youtube", videoId: "abc", title: "t", durationSeconds: nil, formats: [],
        sourceUrl: nil)
    #expect(MediaFormatMenu.options(for: media, preferences: .default).isEmpty)
}
