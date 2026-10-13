import Foundation
import Testing

@testable import SDMResolve

@Test func locatesBinaryInFirstSearchPathThatHasIt() async {
    let a = URL(fileURLWithPath: "/fake/a")
    let b = URL(fileURLWithPath: "/fake/b")
    let present: Set<String> = ["/fake/b/yt-dlp"]
    let locator = BinaryLocator(searchPaths: [a, b], isExecutable: { present.contains($0.path) })
    let found = await locator.locate("yt-dlp")
    #expect(found == URL(fileURLWithPath: "/fake/b/yt-dlp"))
}

@Test func returnsNilWhenNotInAnySearchPath() async {
    let locator = BinaryLocator(
        searchPaths: [URL(fileURLWithPath: "/fake/a")], isExecutable: { _ in false })
    #expect(await locator.locate("ffmpeg") == nil)
}

@Test func overrideWinsOverSearchPathWhenExecutable() async {
    let override = URL(fileURLWithPath: "/custom/yt-dlp")
    let present: Set<String> = ["/fake/a/yt-dlp", "/custom/yt-dlp"]
    let locator = BinaryLocator(
        searchPaths: [URL(fileURLWithPath: "/fake/a")], isExecutable: { present.contains($0.path) })
    await locator.setOverride(override, for: "yt-dlp")
    #expect(await locator.locate("yt-dlp") == override)
}

@Test func nonExecutableOverrideIsIgnored() async {
    let present: Set<String> = ["/fake/a/yt-dlp"]
    let locator = BinaryLocator(
        searchPaths: [URL(fileURLWithPath: "/fake/a")], isExecutable: { present.contains($0.path) })
    await locator.setOverride(URL(fileURLWithPath: "/custom/missing"), for: "yt-dlp")
    #expect(await locator.locate("yt-dlp") == URL(fileURLWithPath: "/fake/a/yt-dlp"))
}

@Test func invalidateForcesReScan() async {
    final class Box: @unchecked Sendable { var present: Set<String> = [] }
    let box = Box()
    let locator = BinaryLocator(
        searchPaths: [URL(fileURLWithPath: "/fake/a")],
        isExecutable: { box.present.contains($0.path) })
    #expect(await locator.locate("yt-dlp") == nil)
    box.present = ["/fake/a/yt-dlp"]
    #expect(await locator.locate("yt-dlp") == nil)  // memoized miss
    await locator.invalidate()
    #expect(await locator.locate("yt-dlp") == URL(fileURLWithPath: "/fake/a/yt-dlp"))
}
