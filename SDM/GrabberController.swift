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

    func moveLink(_ id: UUID, toPackageNamed name: String) async {
        await session.moveLink(id, toPackageNamed: name)
        snapshot = await session.snapshot()
    }

    /// The original URLs of a package's confirmed links, for handoff to the
    /// download engine.
    func urls(inPackageNamed name: String) -> [URL] {
        guard let package = snapshot.packages.first(where: { $0.name == name }) else { return [] }
        let ids = Set(package.linkIDs)
        return snapshot.links.filter { ids.contains($0.id) }.map(\.originalURL)
    }
}
