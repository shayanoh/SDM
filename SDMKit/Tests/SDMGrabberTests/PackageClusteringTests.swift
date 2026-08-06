import Foundation
import Testing

@testable import SDMGrabber

private func link(_ filename: String, host: String = "cdn.example.com", path: String = "/season1")
    -> ClusterableLink
{
    ClusterableLink(id: UUID(), filename: filename, host: host, directoryPath: path)
}

@Test func episodesWithTheSameTemplateClusterTogether() {
    let e1 = link("Show.S01E01.1080p.mkv")
    let e2 = link("Show.S01E02.1080p.mkv")
    let candidates = PackageClustering.cluster([e1, e2])

    #expect(candidates.count == 1)
    #expect(candidates[0].name == "Show S01")
    #expect(Set(candidates[0].linkIDs) == Set([e1.id, e2.id]))
    #expect(candidates[0].isArchive == false)
}

@Test func seasonEpisodePatternKeepsTheFullSeasonNumberNotJustItsFirstDigit() {
    // Regression case: a naive character-by-character common prefix over
    // "S01E01.1080p" / "S01E02.1080p" stops at "S0" — the very first digit
    // where the two diverge — which is meaningless. This must keep "S01"
    // whole and drop only the episode-specific remainder.
    let e1 = link("S01E01.1080p.mkv")
    let e2 = link("S01E02.1080p.mkv")
    let candidates = PackageClustering.cluster([e1, e2])

    #expect(candidates.count == 1)
    #expect(candidates[0].name == "S01")
}

@Test func dotsAndDashesInAPackageNameBecomeSpaces() {
    let a = link("the.matrix.1999.bluray.mkv")
    let b = link("the.matrix.1999.bluray.nfo")
    let candidates = PackageClustering.cluster([a, b])

    #expect(candidates.count == 1)
    #expect(candidates[0].name == "The Matrix 1999 Bluray")
}

@Test func archivePartsLockTogetherRegardlessOfTemplate() {
    let parts = [
        link("Movie.part01.rar"),
        link("Movie.part02.rar"),
        link("Movie.part03.rar"),
    ]
    let candidates = PackageClustering.cluster(parts)

    #expect(candidates.count == 1)
    #expect(candidates[0].name == "Movie")
    #expect(candidates[0].isArchive == true)
    #expect(Set(candidates[0].linkIDs) == Set(parts.map(\.id)))
}

@Test func singletonTemplatesGroupByHostAndPath() {
    let a = link("readme.txt")
    let b = link("changelog.md")
    let candidates = PackageClustering.cluster([a, b])

    #expect(candidates.count == 1)
    #expect(candidates[0].name == "cdn.example.com")
    #expect(Set(candidates[0].linkIDs) == Set([a.id, b.id]))
}

@Test func dissimilarTemplatesOnDifferentHostsStaySeparate() {
    let a = link("readme.txt", host: "one.example.com")
    let b = link("changelog.md", host: "two.example.com")
    let candidates = PackageClustering.cluster([a, b])

    #expect(candidates.count == 2)
    #expect(Set(candidates.flatMap(\.linkIDs)) == Set([a.id, b.id]))
}

@Test func clusteringEmptyInputReturnsNoPackages() {
    #expect(PackageClustering.cluster([]).isEmpty)
}

@Test func outputOrderIsDeterministicAcrossRepeatedCalls() {
    let links = (0..<12).map { link("file\($0).bin", host: "h\($0).example.com") }
    let first = PackageClustering.cluster(links).map(\.name)
    let second = PackageClustering.cluster(links).map(\.name)
    #expect(first == second)
}
