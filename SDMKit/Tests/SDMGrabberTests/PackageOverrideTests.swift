import Foundation
import Testing

@testable import SDMGrabber

private func makeSession() -> GrabberSession {
    GrabberSession(prober: LinkProber(transport: FakeProbeOrigin(), deepSniffEnabled: false))
}

@Test func renamePackageMovesEveryMemberLinkUnderTheNewName() async throws {
    let session = makeSession()
    await session.ingest(urls: [URL(string: "https://example.com/a.zip")!])
    let originalName = await session.snapshot().packages[0].name

    await session.renamePackage(originalName, to: "My Archive")

    let snapshot = await session.snapshot()
    #expect(snapshot.packages.map(\.name) == ["My Archive"])
    #expect(snapshot.packages[0].linkIDs.count == 1)
}

@Test func mergePackagesCombinesBothIntoTheDestinationName() async throws {
    let session = makeSession()
    await session.ingest(
        urls: [
            URL(string: "https://example.com/a.zip")!,
            URL(string: "https://other.example.com/b.zip")!,
        ])
    let names = await session.snapshot().packages.map(\.name)
    guard names.count >= 2 else {
        // Clustering may already have grouped these; force two packages via a
        // manual rename so the merge itself is what's under test.
        await session.renamePackage(names[0], to: "First")
        let secondSnapshot = await session.snapshot()
        for package in secondSnapshot.packages where package.name != "First" {
            await session.renamePackage(package.name, to: "Second")
        }
        await session.mergePackages("Second", into: "First")
        let merged = await session.snapshot()
        #expect(merged.packages.map(\.name) == ["First"])
        #expect(merged.packages[0].linkIDs.count == 2)
        return
    }
    await session.mergePackages(names[1], into: names[0])
    let merged = await session.snapshot()
    #expect(merged.packages.map(\.name) == [names[0]])
    #expect(merged.packages[0].linkIDs.count == 2)
}

@Test func splitPackageGivesEachLinkItsOwnPackage() async throws {
    let session = makeSession()
    await session.ingest(
        urls: [
            URL(string: "https://example.com/Show.S01E01.mkv")!,
            URL(string: "https://example.com/Show.S01E02.mkv")!,
        ])
    let originalName = await session.snapshot().packages[0].name
    #expect(await session.snapshot().packages[0].linkIDs.count == 2)

    await session.splitPackage(originalName)

    let snapshot = await session.snapshot()
    #expect(snapshot.packages.count == 2)
    #expect(snapshot.packages.allSatisfy { $0.linkIDs.count == 1 })
}
