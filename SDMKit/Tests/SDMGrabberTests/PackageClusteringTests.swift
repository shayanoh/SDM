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

@Test func seasonEpisodePatternProperlySplitsTwoDifferentSeasons() {
    let season1 = [
        link("Show.S01E01.1080p.mkv"),
        link("Show.S01E02.1080p.mkv"),
    ]
    let season2 = [
        link("Show.S02E01.1080p.mkv"),
        link("Show.S02E02.1080p.mkv"),
    ]

    let links = season1 + season2
    let candidates = PackageClustering.cluster(links)

    #expect(candidates.count == 2)
    #expect(candidates[0].name == "Show S01")
    #expect(Set(candidates[0].linkIDs) == Set(season1.map(\.id)))
    #expect(candidates[1].name == "Show S02")
    #expect(Set(candidates[1].linkIDs) == Set(season2.map(\.id)))
}

@Test func farsiNamesAreHandledCorrectlyWithRandomNames() {
    let links = [
        link("برنامه هفتگی.xlsx"),
        link("رزومه.docx"),
        link("سریال خوب.S01E01.کیفیت بالا.mkv"),
        link("سریال خوب.S01E02.کیفیت بالا.mkv"),
        link("سریال خوب.S01E03.کیفیت بالا.mkv"),
        link("سریال خوب.S02E01.کیفیت بالا.mkv"),
        link("سریال خوب.txt"),
        link("کتاب اول.pdf"),
        link("کتاب دوم.pdf"),
    ]

    let candidates = PackageClustering.cluster(links)
    #expect(candidates.count == 7)
    let candidateNames = candidates.map(\.name)
    #expect(candidateNames.contains("سریال خوب S01"))
    #expect(candidateNames.contains("سریال خوب S02"))
    #expect(candidateNames.contains("کتاب اول"))
    #expect(candidateNames.contains("کتاب دوم"))
    #expect(candidateNames.contains("برنامه هفتگی"))
    #expect(candidateNames.contains("رزومه"))
    #expect(candidateNames.contains("سریال خوب"))
    #expect(Set(candidates.flatMap(\.linkIDs)) == Set(links.map(\.id)))
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

@Test func singletonTemplatesGroupByName() {
    let a = link("readme.txt")
    let b = link("changelog.md")
    let candidates = PackageClustering.cluster([a, b])

    #expect(candidates.count == 2)
    #expect(candidates[0].name == "Readme")
    #expect(Set(candidates[0].linkIDs) == Set([a.id]))
    #expect(candidates[1].name == "Changelog")
    #expect(Set(candidates[1].linkIDs) == Set([b.id]))
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
