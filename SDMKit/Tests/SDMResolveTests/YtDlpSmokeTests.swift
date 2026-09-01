import Foundation
import Testing

@testable import SDMCore
@testable import SDMResolve

private let ytDlpOnDisk =
    FileManager.default.isExecutableFile(atPath: "/usr/local/bin/yt-dlp")
    || FileManager.default.isExecutableFile(atPath: "/opt/homebrew/bin/yt-dlp")

/// Exercises the real `BinaryLocator` + `SystemProcessRunner` wiring against
/// the installed yt-dlp — `--version` only, no network. Skipped when yt-dlp
/// is not installed (parent spec §10.2).
@Test(.enabled(if: ytDlpOnDisk))
func ytDlpVersionRunsThroughSystemRunner() async throws {
    let locator = BinaryLocator()
    let ytdlp = try #require(await locator.locate("yt-dlp"))
    let out = try await SystemProcessRunner().run(
        executable: ytdlp, arguments: ["--version"], timeout: .seconds(10))
    #expect(out.exitCode == 0)
    #expect(!out.stdout.isEmpty)
}
