import Foundation
import Observation
import SDMCore
import SDMGrabber
import SDMResolve
import os.log

/// Bridges `GrabberSession` to SwiftUI, mirroring `EngineController`'s role
/// for `DownloadEngine`.
@MainActor
@Observable
final class GrabberController {
    private(set) var snapshot = GrabberSnapshot.empty

    private let session: GrabberSession
    private var autoClearTask: Task<Void, Never>?
    private let managedBinaries: ManagedBinariesController?
    private let notifications: NotificationManager?
    private nonisolated static let log = Logger(
        subsystem: "com.shayanoh.SDM", category: "grabber")

    init(
        managedBinaries: ManagedBinariesController? = nil,
        notifications: NotificationManager? = nil
    ) {
        self.managedBinaries = managedBinaries
        self.notifications = notifications
        // Same defaults the Swift `GrabberSession.Budget` had for the resolve
        // limit; the two probe limits follow the engine's connection settings.
        let config = GrabberConfig(
            budget: GrabberBudget(
                globalMaxConcurrentProbes: EngineSettingsStore.globalMaxConnections,
                maxConcurrentPerHost: EngineSettingsStore.maxConnectionsPerHost,
                maxConcurrentResolves: 4),
            deepSniffEnabled: GrabberSettings.deepSniffEnabled,
            binarySearchPaths: [ManagedBinariesController.binDirectory],
            qualityPreferences: MediaSitesSettingsStore.qualityPreferences,
            ffmpegAvailable: GrabberController.ffmpegOnDisk,
            cookieSource: MediaSitesSettingsStore.cookieSource,
            maxPlaylistVideos: UInt32(MediaSitesSettingsStore.maxPlaylistVideos),
            ytDlpExtraArguments: ManagedBinariesController.ytDlpExtraArguments)
        do {
            session = try GrabberSession(config: config)
        } catch {
            // Built from clamped settings and fixed paths: a failure is a bug.
            fatalError("SDM could not start the link grabber: \(error.localizedDescription)")
        }
        startAutoClearLoop()
    }

    /// Re-sends the settings the Swift session read through closures on every
    /// use: the quality preferences, the yt-dlp cookie source and playlist
    /// cap, and whether `ffmpeg` is on disk. Called when Settings saves and
    /// when the managed binaries change.
    func applyStoredSettings() {
        do {
            try session.setQualityPreferences(
                preferences: MediaSitesSettingsStore.qualityPreferences)
            try session.setCookieSource(source: MediaSitesSettingsStore.cookieSource)
            try session.setMaxPlaylistVideos(
                count: UInt32(MediaSitesSettingsStore.maxPlaylistVideos))
            try session.setFfmpegAvailable(available: Self.ffmpegOnDisk)
        } catch {
            report("apply the link grabber settings", error)
        }
    }

    /// Tells the session whether `ffmpeg` is now on disk (it has no way to
    /// find out itself): after the bundled blobs are inflated or an update
    /// lands, muxed formats become offerable.
    func refreshFfmpegAvailability() {
        do {
            try session.setFfmpegAvailable(available: Self.ffmpegOnDisk)
        } catch {
            report("update the ffmpeg status", error)
        }
    }

    /// Points a binary (`"yt-dlp"`, `"ffmpeg"`) at a user-chosen path, or back
    /// at the default lookup with `nil`.
    func setBinaryOverride(name: String, path: URL?) {
        do {
            try session.setBinaryOverride(name: name, path: path)
        } catch {
            report("change the \(name) location", error)
        }
    }

    private func report(_ action: String, _ error: Error) {
        Self.log.error("Could not \(action, privacy: .public): \(error.localizedDescription)")
        notifications?.postInfo("Could not \(action): \(error.localizedDescription)")
    }

    /// Re-reads the session's snapshot. A failure keeps the previous one: the
    /// callers are refreshes, and the next one tries again.
    private func refresh() {
        do {
            let newSnapshot = try session.snapshot()
            if newSnapshot != snapshot { snapshot = newSnapshot }
        } catch {
            Self.log.error("Snapshot failed: \(error.localizedDescription, privacy: .public)")
        }
    }

    /// Runs a synchronous session edit, reports a failure, and refreshes.
    private func edit(_ action: String, _ body: () throws -> Void) {
        do {
            try body()
        } catch {
            report(action, error)
        }
        refresh()
    }

    /// Runs an asynchronous session call, reports a failure, and refreshes.
    private func editAsync(_ action: String, _ body: () async throws -> Void) async {
        do {
            try await body()
        } catch {
            report(action, error)
        }
        refresh()
    }

    /// Once a minute, drops rows older than the "auto-clear grabbed links"
    /// setting. Runs for the app's lifetime (independent of whether the
    /// Linkgrabber tab is visible).
    private func startAutoClearLoop() {
        autoClearTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(60))
                guard let self else { return }
                guard let maxAge = GrabberSettings.autoClearGrabbedLinksAfter.seconds else {
                    continue
                }
                self.edit("clear old links") {
                    try self.session.pruneOlderThan(maxAge: maxAge, now: Date())
                }
            }
        }
    }

    /// A cheap synchronous check of the common Homebrew install paths — the
    /// Settings path override is Part 5.
    nonisolated static var ffmpegOnDisk: Bool {
        FileManager.default.isExecutableFile(
            atPath: ManagedBinariesController.binDirectory.appendingPathComponent("ffmpeg").path)
    }

    func updateSnapshot() async {
        refresh()
    }

    func ingest(text: String) async {
        managedBinaries?.kick()
        await editAsync("add the links") { try await session.ingestText(text: text) }
    }

    func ingest(urls: [URL]) async {
        managedBinaries?.kick()
        await editAsync("add the links") { try await session.ingestUrls(urls: urls) }
    }

    func setKnownDownloadURLs(_ urls: Set<URL>) async {
        edit("update the known downloads") { try session.setKnownDownloadUrls(urls: urls) }
    }

    func removeLink(_ id: UUID) async {
        edit("remove the link") { try session.removeLink(id: id) }
    }

    func removePackage(_ id: UUID) async {
        edit("remove the package") { try session.removePackage(id: id) }
    }

    func clear() async {
        edit("clear the list") { try session.clear() }
    }

    func moveLink(_ id: UUID, toPackage packageID: UUID) async {
        edit("move the link") { try session.moveLink(id: id, packageId: packageID) }
    }

    func renamePackage(_ id: UUID, to newName: String) async {
        edit("rename the package") { try session.renamePackage(id: id, newName: newName) }
    }

    func mergePackages(_ sourceID: UUID, into destinationID: UUID) async {
        edit("merge the packages") {
            try session.mergePackages(sourceId: sourceID, destinationId: destinationID)
        }
    }

    func splitPackage(_ id: UUID) async {
        edit("split the package") { try session.splitPackage(id: id) }
    }

    func recheckFailed() async {
        await editAsync("recheck the links") { try await session.recheckFailed() }
    }

    /// Re-probes / re-resolves exactly these rows — the per-item "Recheck"
    /// context-menu action, which retries regardless of current verdict.
    func recheck(_ ids: Set<UUID>) async {
        await editAsync("recheck the links") { try await session.recheck(ids: ids) }
    }

    /// Stops in-flight probes/resolves — the header's "Cancel Checks" button.
    func cancelChecks() async {
        edit("cancel the checks") { try session.cancelChecks() }
    }

    func removeLinks(_ ids: Set<UUID>) async {
        edit("remove the links") {
            for id in ids { try session.removeLink(id: id) }
        }
    }

    /// The URL shown by "Copy URL": a link's resolved/original address, or a
    /// media row's source page.
    func url(for id: UUID) -> URL? {
        if let link = snapshot.links.first(where: { $0.id == id }) { return link.finalURL }
        return snapshot.mediaRows.first(where: { $0.id == id })?.sourceURL
    }

    /// Whether a row can be handed to the engine right now: an online or
    /// faulty HTTP link, or a fully resolved media row. Offline / check-failed
    /// links and media rows still waiting on a format or a binary can't.
    func isAddable(_ id: UUID) -> Bool {
        if let link = snapshot.links.first(where: { $0.id == id }) {
            switch link.verdict {
            case .online, .faulty: return true
            default: return false
            }
        }
        if let row = snapshot.mediaRows.first(where: { $0.id == id }) {
            return row.state == .resolved
        }
        return false
    }

    /// One engine handoff per package the addable rows among `ids` belong to
    /// (standalone rows each go under their own filename). Non-addable rows
    /// are dropped. Empty when nothing in the selection can be added.
    struct SelectionHandoff: Identifiable {
        let name: String
        let note: String?
        let items: [DownloadItem]
        let handoffIDs: [UUID]
        var id: String { name }
    }

    func handoffGroups(for ids: Set<UUID>) -> [SelectionHandoff] {
        let addable = ids.filter { isAddable($0) }
        guard !addable.isEmpty else { return [] }

        var groupOrder: [String] = []
        var byName: [String: (note: String?, http: [ProbedLink], media: [MediaRow])] = [:]
        for id in addable {
            let package = snapshot.packages.first { $0.linkIDs.contains(id) }
            let name = package?.name ?? standaloneName(for: id)
            if byName[name] == nil {
                byName[name] = (package?.note ?? nil, [], [])
                groupOrder.append(name)
            }
            if let link = snapshot.links.first(where: { $0.id == id }) {
                byName[name]?.http.append(link)
            } else if let row = snapshot.mediaRows.first(where: { $0.id == id }) {
                byName[name]?.media.append(row)
            }
        }

        return groupOrder.compactMap { name in
            guard let group = byName[name] else { return nil }
            let (items, _) = MediaHandoff.build(httpLinks: group.http, mediaRows: group.media)
            guard !items.isEmpty else { return nil }
            return SelectionHandoff(
                name: name, note: group.note, items: items,
                handoffIDs: group.http.map(\.id) + group.media.map(\.id))
        }
    }

    private func standaloneName(for id: UUID) -> String {
        if let link = snapshot.links.first(where: { $0.id == id }) { return link.effectiveFilename }
        if let row = snapshot.mediaRows.first(where: { $0.id == id }) { return row.displayFilename }
        return "Download"
    }

    /// The original URLs of a package's confirmed links, for handoff to the
    /// download engine.
    func urls(inPackage id: UUID) -> [URL] {
        guard let package = snapshot.packages.first(where: { $0.id == id }) else { return [] }
        let ids = Set(package.linkIDs)
        return snapshot.links.filter { ids.contains($0.id) }.map(\.originalURL)
    }

    func probedLinks(inPackage id: UUID) -> [ProbedLink] {
        guard let package = snapshot.packages.first(where: { $0.id == id }) else { return [] }
        let ids = Set(package.linkIDs)
        return snapshot.links.filter { ids.contains($0.id) }
    }

    func mediaRows(inPackage id: UUID) -> [MediaRow] {
        guard let package = snapshot.packages.first(where: { $0.id == id }) else { return [] }
        let ids = Set(package.linkIDs)
        return snapshot.mediaRows.filter { ids.contains($0.id) }
    }

    /// The engine items for a package's rows, plus how many rows were held
    /// back (no format resolved yet). Parent spec §6.3.
    func downloadItems(inPackage id: UUID) -> (items: [DownloadItem], heldBackCount: Int) {
        MediaHandoff.build(
            httpLinks: probedLinks(inPackage: id), mediaRows: mediaRows(inPackage: id))
    }

    /// The link ids in a package that will actually hand off — HTTP links
    /// and `.resolved` media rows. Held-back rows stay in the grabber.
    func handoffLinkIDs(inPackage id: UUID) -> [UUID] {
        let http = probedLinks(inPackage: id).map(\.id)
        let media = mediaRows(inPackage: id).filter { $0.state == .resolved }.map(\.id)
        return http + media
    }

    func setFormatChoice(_ choice: FormatChoice, for rowID: UUID) async {
        edit("change the format") { try session.setFormatChoice(choice: choice, id: rowID) }
    }
}
