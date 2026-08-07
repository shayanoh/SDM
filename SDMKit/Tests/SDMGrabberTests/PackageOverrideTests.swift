import Foundation
import Testing

@testable import SDMGrabber

private func makeSession() -> GrabberSession {
    GrabberSession(prober: LinkProber(transport: FakeProbeOrigin(), deepSniffEnabled: false))
}

@Test func renamePackageMovesEveryMemberLinkUnderTheNewName() async throws {
    let session = makeSession()
    await session.ingest(urls: [URL(string: "https://example.com/a.zip")!])
    let originalID = await session.snapshot().packages[0].id

    await session.renamePackage(originalID, to: "My Archive")

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
    let packages = await session.snapshot().packages
    guard packages.count >= 2 else {
        // Clustering may already have grouped these; force two packages via a
        // manual rename so the merge itself is what's under test.
        await session.renamePackage(packages[0].id, to: "First")
        let secondSnapshot = await session.snapshot()
        for package in secondSnapshot.packages where package.name != "First" {
            await session.renamePackage(package.id, to: "Second")
        }
        let beforeMerge = await session.snapshot().packages
        let source = try #require(beforeMerge.first { $0.name == "Second" })
        let destination = try #require(beforeMerge.first { $0.name == "First" })
        await session.mergePackages(source.id, into: destination.id)
        let merged = await session.snapshot()
        #expect(merged.packages.map(\.name) == ["First"])
        #expect(merged.packages[0].linkIDs.count == 2)
        return
    }
    await session.mergePackages(packages[1].id, into: packages[0].id)
    let merged = await session.snapshot()
    #expect(merged.packages.map(\.name) == [packages[0].name])
    #expect(merged.packages[0].linkIDs.count == 2)
}

@Test func splitPackageGivesEachLinkItsOwnPackage() async throws {
    let session = makeSession()
    await session.ingest(
        urls: [
            URL(string: "https://example.com/Show.S01E01.mkv")!,
            URL(string: "https://example.com/Show.S01E02.mkv")!,
        ])
    let originalID = await session.snapshot().packages[0].id
    #expect(await session.snapshot().packages[0].linkIDs.count == 2)

    await session.splitPackage(originalID)

    let snapshot = await session.snapshot()
    #expect(snapshot.packages.count == 2)
    #expect(snapshot.packages.allSatisfy { $0.linkIDs.count == 1 })
}

@Test func renamingAPackageToAnExistingNameMergesThemInstead() async throws {
    let session = makeSession()
    await session.ingest(
        urls: [
            URL(string: "https://example.com/a.zip")!,
            URL(string: "https://other.example.com/b.zip")!,
        ])
    var packages = await session.snapshot().packages
    if packages.count < 2 {
        await session.renamePackage(packages[0].id, to: "First")
        let after = await session.snapshot().packages
        for package in after where package.name != "First" {
            await session.renamePackage(package.id, to: "Second")
        }
        packages = await session.snapshot().packages
    }
    #expect(packages.count == 2)

    // Renaming the second package to the first's exact current name must
    // merge them into one — a duplicate name is not a valid end state.
    let targetName = packages[0].name
    await session.renamePackage(packages[1].id, to: targetName)

    let merged = await session.snapshot()
    #expect(merged.packages.map(\.name) == [targetName])
    #expect(merged.packages[0].linkIDs.count == 2)
}

@Test func aPackagesIdentitySurvivesReclusteringUnderAnUnchangedName() async throws {
    let session = makeSession()
    await session.ingest(urls: [URL(string: "https://example.com/a.zip")!])
    let original = try #require(await session.snapshot().packages.first)

    // Ingesting more links triggers another `recluster()` pass; the
    // existing package's own id must not change just because clustering ran
    // again — otherwise any id-keyed UI state (a collapsed/expanded
    // disclosure, an in-flight context menu) would reset every time a new,
    // unrelated link came in.
    await session.ingest(urls: [URL(string: "https://other.example.com/c.zip")!])

    let after = await session.snapshot().packages.first { $0.name == original.name }
    #expect(after?.id == original.id)
}
