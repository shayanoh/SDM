import Foundation
import Observation
import SDMCore
import SDMGrabber

/// Bridges `GrabberSession` to SwiftUI, mirroring `EngineController`'s role
/// for `DownloadEngine`.
@MainActor
@Observable
final class GrabberController {
    private(set) var snapshot = GrabberSnapshot(
        links: [], packages: [], checkedCount: 0, totalCount: 0)

    private let session: GrabberSession

    init() {
        session = GrabberSession(
            prober: LinkProber(
                transport: URLSessionProbeTransport(),
                deepSniffEnabled: GrabberSettings.deepSniffEnabled
            ),
            budget: GrabberSession.Budget(
                globalMaxConcurrentProbes: EngineSettingsStore.globalMaxConnections,
                maxConcurrentPerHost: EngineSettingsStore.maxConnectionsPerHost
            )
        )
    }

    func updateSnapshot() async {
        let newSnapshot = await session.snapshot()
        if newSnapshot != snapshot {
            snapshot = newSnapshot
        }
    }

    func ingest(text: String) async {
        await session.ingest(text: text)
        snapshot = await session.snapshot()
    }

    func ingest(urls: [URL]) async {
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
