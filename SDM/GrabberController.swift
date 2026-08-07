import Foundation
import Observation
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
}
