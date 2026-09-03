import Foundation
import Observation
import SDMCore
import SDMGrabber
import SDMResolve

/// Bridges `GrabberSession` to SwiftUI, mirroring `EngineController`'s role
/// for `DownloadEngine`.
@MainActor
@Observable
final class GrabberController {
    private(set) var snapshot = GrabberSnapshot(
        links: [], packages: [], checkedCount: 0, totalCount: 0)

    private let session: GrabberSession
    private var autoClearTask: Task<Void, Never>?
    private let managedBinaries: ManagedBinariesController?

    init(
        binaryLocator: BinaryLocator = BinaryLocator(
            searchPaths: [ManagedBinariesController.binDirectory]),
        managedBinaries: ManagedBinariesController? = nil
    ) {
        self.managedBinaries = managedBinaries
        let processRunner = SystemProcessRunner()
        session = GrabberSession(
            prober: LinkProber(
                transport: URLSessionProbeTransport(),
                deepSniffEnabled: GrabberSettings.deepSniffEnabled
            ),
            budget: GrabberSession.Budget(
                globalMaxConcurrentProbes: EngineSettingsStore.globalMaxConnections,
                maxConcurrentPerHost: EngineSettingsStore.maxConnectionsPerHost
            ),
            resolver: YtDlpResolver(
                runner: processRunner, locator: binaryLocator,
                cookieSource: { MediaSitesSettingsStore.cookieSource },
                maxPlaylistVideos: { MediaSitesSettingsStore.maxPlaylistVideos },
                extraArguments: { ManagedBinariesController.ytDlpExtraArguments }
            ),
            qualityPreferences: { MediaSitesSettingsStore.qualityPreferences },
            ffmpegAvailable: { GrabberController.ffmpegOnDisk }
        )
        startAutoClearLoop()
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
                await self.session.pruneOlderThan(maxAge)
                self.snapshot = await self.session.snapshot()
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
        let newSnapshot = await session.snapshot()
        if newSnapshot != snapshot {
            snapshot = newSnapshot
        }
    }

    func ingest(text: String) async {
        managedBinaries?.kick()
        await session.ingest(text: text)
        snapshot = await session.snapshot()
    }

    func ingest(urls: [URL]) async {
        managedBinaries?.kick()
        await session.ingest(urls: urls)
        snapshot = await session.snapshot()
    }

    func setKnownDownloadURLs(_ urls: Set<URL>) async {
        await session.setKnownDownloadURLs(urls)
        snapshot = await session.snapshot()
    }

    func removeLink(_ id: UUID) async {
        await session.removeLink(id)
        snapshot = await session.snapshot()
    }

    func removePackage(_ id: UUID) async {
        await session.removePackage(id)
        snapshot = await session.snapshot()
    }

    func clear() async {
        await session.clear()
        snapshot = await session.snapshot()
    }

    func moveLink(_ id: UUID, toPackage packageID: UUID) async {
        await session.moveLink(id, toPackage: packageID)
        snapshot = await session.snapshot()
    }

    func renamePackage(_ id: UUID, to newName: String) async {
        await session.renamePackage(id, to: newName)
        snapshot = await session.snapshot()
    }

    func mergePackages(_ sourceID: UUID, into destinationID: UUID) async {
        await session.mergePackages(sourceID, into: destinationID)
        snapshot = await session.snapshot()
    }

    func splitPackage(_ id: UUID) async {
        await session.splitPackage(id)
        snapshot = await session.snapshot()
    }

    func recheckFailed() async {
        await session.recheckFailed()
        snapshot = await session.snapshot()
    }

    /// Re-probes / re-resolves exactly these rows — the per-item "Recheck"
    /// context-menu action, which retries regardless of current verdict.
    func recheck(_ ids: Set<UUID>) async {
        await session.recheck(ids: ids)
        snapshot = await session.snapshot()
    }

    /// Stops in-flight probes/resolves — the header's "Cancel Checks" button.
    func cancelChecks() async {
        await session.cancelChecks()
        snapshot = await session.snapshot()
    }

    func removeLinks(_ ids: Set<UUID>) async {
        for id in ids { await session.removeLink(id) }
        snapshot = await session.snapshot()
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
        await session.setFormatChoice(choice, for: rowID)
        snapshot = await session.snapshot()
    }
}
