# Phase 5: yt-dlp Resolver Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status: SUPERSEDED — never executed. DO NOT IMPLEMENT.** Phase 5 was
re-brainstormed from scratch on 2026-09-02 against a dedicated spec
(`docs/superpowers/specs/2026-09-02-phase-5-youtube-resolver-design.md`)
and delivered as five separate part-plans
(`docs/superpowers/plans/2026-09-02-phase-5-part-1..5-*.md`), all merged to
`main` on 2026-09-02. The final design differs from this document — the
target is `SDMResolve` (not `SDMResolver`), the resolver is injected
directly into `DownloadEngine` and `GrabberSession` (no `URLRefresher`
seam), and HLS/DASH wholesale download is deferred (tracked in `todo.md`),
not built as a `WholesaleDownloader`. This file is kept only as a record of
the earlier approach.

**Goal:** Add a `LinkResolver` protocol backed by yt-dlp, so YouTube links grabbed in the Linkgrabber resolve to a real format table, download through the existing segmented engine as first-class items, refresh their signed URL on 403 instead of dying, and mux separately-downloaded video/audio into one file via ffmpeg — with a `--cookies-from-browser` setting (Safari default, Chrome optional with a red warning) and a graceful "requires yt-dlp" state when the tool is missing.

**Architecture:** A new `SDMResolver` SPM target (depends only on `SDMCore`, following the existing layering where `SDMEngine` and `SDMGrabber` both sit one level above `SDMCore` and never depend on each other) owns everything that talks to yt-dlp/ffmpeg: process invocation, format-table parsing, quality selection, muxing, and the wholesale-download fallback. `SDMCore` gains two small shared pieces — a `Muxer` protocol and two value types (`ResolverBinding`, `MuxCompanion`) — so `DownloadItem` can carry resolver provenance without `SDMEngine` needing to depend on `SDMResolver` at all: `DownloadEngine` is instead handed a plain `URLRefresher` protocol (defined in `SDMEngine` itself) and a `Muxer` (from `SDMCore`) at construction time, and the concrete yt-dlp-backed implementations are wired up only in the `SDM` app target, exactly where `URLSessionTransport`/`JSONStateStore` are wired up today. `SDMGrabber` depends on `SDMResolver` directly, since extracting a format table is conceptually the same kind of thing as probing a link.

The HLS/DASH-manifest-only fallback (spec §8: "handed to yt-dlp to download wholesale... the degraded path and should be rare") is deliberately **not** wired into `DownloadEngine`'s segmented-worker machinery — `RangeSet`/`SparseFile`/`HTTPTransport` all assume a single Range-able resource, which an `.m3u8` manifest is not. It gets its own small, separately-tested `WholesaleDownloader` component and its own tiny UI surface, matching the spec's own framing of it as the exceptional path.

**Tech Stack:** Swift 6, Swift Testing, Foundation's `Process` (new to this codebase — no existing helper to follow), yt-dlp and ffmpeg as user-installed Homebrew binaries (confirmed present on this machine: yt-dlp 2026.07.04 at `/usr/local/bin/yt-dlp`, ffmpeg 8.1.2 at `/usr/local/bin/ffmpeg`).

## Global Constraints

- **Deployment target stays macOS 15.0.** Nothing in this plan needs a newer API — `Process`, `FileHandle`, `JSONDecoder` are all long available.
- **Swift tools version 6.2**, Swift 6 language mode, strict concurrency enabled. Confirmed on this machine: `swift --version` reports Apple Swift version 6.2 (swiftlang-6.2.0.19.9), target `x86_64-apple-macosx15.0`; `xcodebuild -version` reports Xcode 26.0.1 (Build 17A400); `swift-format --version` reports 603.0.0. Re-check if the implementer's toolchain differs.
- **Zero third-party Swift package dependencies.** Foundation, SwiftUI, AppKit, and the Swift standard library only. yt-dlp and ffmpeg are external *binaries*, invoked via `Process`, not linked libraries — this is unchanged from spec §14's "bundling yt-dlp and ffmpeg" staying explicitly out of scope; they remain user-installed.
- **Swift Testing only** (`@Test` / `#expect`). Every UI-wiring task (the new Settings tab, the format picker, the red Chrome warning) gets a "build and run, verify by hand" step per spec §11.7 — no snapshot-testing harness.
- **No test may touch the real network or spawn a real process.** `ProcessRunner` is an injected protocol (Task 3) exactly like `HTTPTransport` — every resolver/muxer/wholesale-downloader test drives a `FakeProcessRunner`, never `/usr/local/bin/yt-dlp` itself. The one exception is this plan's own authoring step, already done: the real `yt-dlp -J --cookies-from-browser safari` output was captured once, by hand, outside the test suite, and trimmed into the two fixtures Task 5 creates.
- **Format before every commit:** `swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests`.
- **Run tests with:** `swift test --package-path SDMKit`.
- **New files under `SDM/` need no Xcode project edit** — `SDM.xcodeproj` uses `PBXFileSystemSynchronizedRootGroup` for the `SDM` folder (confirmed in Phase 3's plan), so any `.swift` file created under `SDM/` is picked up automatically on the next build.
- **`SDMResolver` is added to `SDMKit/Package.swift` as a fourth library product**, depended on by `SDMGrabber` and by the `SDM` app target (added via Xcode, same as any other framework dependency), never by `SDMEngine` — see Architecture above for why.
- **yt-dlp is a metadata extractor, never the primary downloader** (spec §8) — the one exception is the wholesale-fallback path (Task 14), which is explicitly the "should be rare" degraded case.
- **This plan does not run or screenshot the app.** Per the user's instruction, implementation proceeds without visually testing the UI; the plan ends with an explicit list of what to verify by hand afterward, including the real yt-dlp round trip against `https://www.youtube.com/watch?v=IlIJa_FDK-0` with `--cookies-from-browser safari`.
- **Deferred items named by earlier phases' own "Deferred to later phases" sections and owed to this one:** yt-dlp/YouTube resolver, format tables, muxing, URL refresh on 403 (named by Phases 1–4, all picked up here). Bundling yt-dlp/ffmpeg themselves stays out of scope per spec §14 — unchanged by this plan.

---

### Task 1: `SDMResolver` module scaffold

**Files:**
- Modify: `SDMKit/Package.swift`
- Create: `SDMKit/Sources/SDMResolver/SDMResolver.swift`
- Create: `SDMKit/Tests/SDMResolverTests/SmokeTests.swift`

**Interfaces:**
- Consumes: nothing
- Produces: the `SDMResolver` module itself, importable as `import SDMResolver`

- [ ] **Step 1: Write the failing test**

`SDMKit/Tests/SDMResolverTests/SmokeTests.swift`:

```swift
import Testing

@testable import SDMResolver

@Test func moduleLoads() {
    #expect(SDMResolverModuleMarker.name == "SDMResolver")
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter SDMResolverTests`
Expected: FAIL — the package build itself fails (`no such module 'SDMResolver'`), since neither the target nor the file exist yet.

- [ ] **Step 3: Add the target to `Package.swift` and create the marker file**

Replace `SDMKit/Package.swift` in full:

```swift
// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "SDMKit",
    platforms: [.macOS(.v15)],
    products: [
        .library(name: "SDMCore", targets: ["SDMCore"]),
        .library(name: "SDMEngine", targets: ["SDMEngine"]),
        .library(name: "SDMGrabber", targets: ["SDMGrabber"]),
        .library(name: "SDMResolver", targets: ["SDMResolver"]),
    ],
    targets: [
        .target(name: "SDMCore", resources: [.process("Resources")]),
        .target(name: "SDMEngine", dependencies: ["SDMCore"]),
        .target(name: "SDMGrabber", dependencies: ["SDMCore"]),
        .target(name: "SDMResolver", dependencies: ["SDMCore"]),
        .testTarget(name: "SDMCoreTests", dependencies: ["SDMCore"]),
        .testTarget(name: "SDMEngineTests", dependencies: ["SDMEngine"]),
        .testTarget(name: "SDMGrabberTests", dependencies: ["SDMGrabber"]),
        .testTarget(name: "SDMResolverTests", dependencies: ["SDMResolver"]),
    ]
)
```

`SDMKit/Sources/SDMResolver/SDMResolver.swift`:

```swift
/// Marker so `SmokeTests` can assert the module actually built and loaded,
/// the same pattern `SDMCoreTests/SmokeTests.swift` established in Phase 1.
public enum SDMResolverModuleMarker {
    public static let name = "SDMResolver"
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `swift test --package-path SDMKit --filter SDMResolverTests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add SDMKit/Package.swift SDMKit/Sources/SDMResolver/SDMResolver.swift SDMKit/Tests/SDMResolverTests/SmokeTests.swift
git commit -m "feat: scaffold the SDMResolver module"
```

---

### Task 2: `CookiesSource`

**Files:**
- Create: `SDMKit/Sources/SDMResolver/CookiesSource.swift`
- Create: `SDMKit/Tests/SDMResolverTests/CookiesSourceTests.swift`

**Interfaces:**
- Consumes: nothing
- Produces: `public enum CookiesSource: String, Codable, Sendable, CaseIterable { case none, safari, chrome }`, `public var ytDlpArguments: [String]`

- [ ] **Step 1: Write the failing test**

`SDMKit/Tests/SDMResolverTests/CookiesSourceTests.swift`:

```swift
import Testing

@testable import SDMResolver

@Test func noneAddsNoArguments() {
    #expect(CookiesSource.none.ytDlpArguments == [])
}

@Test func safariAddsCookiesFlag() {
    #expect(CookiesSource.safari.ytDlpArguments == ["--cookies-from-browser", "safari"])
}

@Test func chromeAddsCookiesFlag() {
    #expect(CookiesSource.chrome.ytDlpArguments == ["--cookies-from-browser", "chrome"])
}

@Test func allCasesCoversExactlyThree() {
    #expect(CookiesSource.allCases == [.none, .safari, .chrome])
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter CookiesSourceTests`
Expected: FAIL — `cannot find 'CookiesSource' in scope`.

- [ ] **Step 3: Implement `CookiesSource`**

`SDMKit/Sources/SDMResolver/CookiesSource.swift`:

```swift
/// Spec-extension setting (not in the original design doc): which browser's
/// cookie jar yt-dlp should read to authenticate YouTube requests. Default is
/// `.safari` — Chrome's cookie store is keychain-protected per-launch, which
/// can prompt the user for their login password on every extraction unless
/// they choose "Always Allow" (surfaced as a warning in Settings, see
/// `SettingsView`'s YouTube tab). `.none` skips cookies entirely, which is
/// fine for most public videos but can fail on age-restricted or
/// sign-in-required ones.
public enum CookiesSource: String, Codable, Sendable, CaseIterable {
    case none
    case safari
    case chrome

    /// The `-J`/download-invocation arguments this source contributes.
    public var ytDlpArguments: [String] {
        switch self {
        case .none: return []
        case .safari: return ["--cookies-from-browser", "safari"]
        case .chrome: return ["--cookies-from-browser", "chrome"]
        }
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `swift test --package-path SDMKit --filter CookiesSourceTests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add SDMKit/Sources/SDMResolver/CookiesSource.swift SDMKit/Tests/SDMResolverTests/CookiesSourceTests.swift
git commit -m "feat: add CookiesSource for yt-dlp browser-cookie selection"
```

---

### Task 3: `ProcessRunner` — injectable process execution

**Files:**
- Create: `SDMKit/Sources/SDMResolver/ProcessRunner.swift`
- Create: `SDMKit/Sources/SDMResolver/FakeProcessRunner.swift`
- Create: `SDMKit/Tests/SDMResolverTests/FakeProcessRunnerTests.swift`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `public struct ProcessInvocation: Sendable { public var executablePath: String; public var arguments: [String] }`
  - `public struct ProcessResult: Sendable { public var exitCode: Int32; public var standardOutput: Data; public var standardError: Data }`
  - `public protocol ProcessRunner: Sendable { func run(_ invocation: ProcessInvocation) async throws -> ProcessResult }`
  - `public actor FakeProcessRunner: ProcessRunner` — programmable, records every invocation

This is `SDMResolver`'s equivalent of `HTTPTransport`/`FakeOrigin`: the seam every later task (`YouTubeResolver`, `FFmpegMuxer`, `YtDlpWholesaleDownloader`, `ExternalToolLocator`) is tested through, so no test in this plan ever spawns a real process.

- [ ] **Step 1: Write the failing test**

`SDMKit/Tests/SDMResolverTests/FakeProcessRunnerTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMResolver

@Test func fakeReturnsTheProgrammedResultForAMatchingInvocation() async throws {
    let runner = FakeProcessRunner()
    await runner.program(
        executablePath: "/usr/local/bin/yt-dlp",
        result: ProcessResult(exitCode: 0, standardOutput: Data("{}".utf8), standardError: Data())
    )

    let result = try await runner.run(
        ProcessInvocation(executablePath: "/usr/local/bin/yt-dlp", arguments: ["-J"]))

    #expect(result.exitCode == 0)
    #expect(result.standardOutput == Data("{}".utf8))
}

@Test func fakeRecordsEveryInvocationInOrder() async throws {
    let runner = FakeProcessRunner()
    await runner.program(
        executablePath: "/usr/local/bin/yt-dlp",
        result: ProcessResult(exitCode: 0, standardOutput: Data(), standardError: Data()))

    _ = try? await runner.run(
        ProcessInvocation(executablePath: "/usr/local/bin/yt-dlp", arguments: ["-J", "url-one"]))
    _ = try? await runner.run(
        ProcessInvocation(executablePath: "/usr/local/bin/yt-dlp", arguments: ["-J", "url-two"]))

    let invocations = await runner.recordedInvocations
    #expect(invocations.count == 2)
    #expect(invocations[0].arguments == ["-J", "url-one"])
    #expect(invocations[1].arguments == ["-J", "url-two"])
}

@Test func fakeThrowsWhenNothingIsProgrammedForThatExecutable() async {
    let runner = FakeProcessRunner()
    await #expect(throws: FakeProcessRunner.NotProgrammed.self) {
        try await runner.run(ProcessInvocation(executablePath: "/usr/local/bin/ffmpeg", arguments: []))
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter FakeProcessRunnerTests`
Expected: FAIL — `cannot find type 'ProcessResult' in scope` (nothing in this file exists yet).

- [ ] **Step 3: Implement `ProcessRunner` and `FakeProcessRunner`**

`SDMKit/Sources/SDMResolver/ProcessRunner.swift`:

```swift
import Foundation

/// One external-binary invocation: the executable's absolute path and its
/// arguments. No shell involved — `Process` execs directly, so no argument
/// ever needs quoting against injection.
public struct ProcessInvocation: Sendable, Equatable {
    public var executablePath: String
    public var arguments: [String]

    public init(executablePath: String, arguments: [String]) {
        self.executablePath = executablePath
        self.arguments = arguments
    }
}

public struct ProcessResult: Sendable, Equatable {
    public var exitCode: Int32
    public var standardOutput: Data
    public var standardError: Data

    public init(exitCode: Int32, standardOutput: Data, standardError: Data) {
        self.exitCode = exitCode
        self.standardOutput = standardOutput
        self.standardError = standardError
    }
}

/// SDMResolver's only route to an external binary — yt-dlp or ffmpeg.
/// Injected exactly like `HTTPTransport`, so no test in this module ever
/// spawns a real process.
public protocol ProcessRunner: Sendable {
    func run(_ invocation: ProcessInvocation) async throws -> ProcessResult
}
```

`SDMKit/Sources/SDMResolver/FakeProcessRunner.swift`:

```swift
import Foundation

/// An in-process stand-in for `ProcessRunner`, programmable per executable
/// path, that records every invocation it receives — this module's
/// equivalent of `SDMEngine.FakeOrigin`/`SDMGrabber.FakeProbeOrigin`. Public
/// and living in library sources (not `Tests/`) so it is reusable the same
/// way those two are.
public actor FakeProcessRunner: ProcessRunner {
    public struct NotProgrammed: Error, Equatable {
        public let executablePath: String
    }

    private var programmedResults: [String: Result<ProcessResult, any Error>] = [:]
    public private(set) var recordedInvocations: [ProcessInvocation] = []

    public init() {}

    public func program(executablePath: String, result: ProcessResult) {
        programmedResults[executablePath] = .success(result)
    }

    public func program(executablePath: String, throwing error: any Error) {
        programmedResults[executablePath] = .failure(error)
    }

    public func run(_ invocation: ProcessInvocation) async throws -> ProcessResult {
        recordedInvocations.append(invocation)
        guard let programmed = programmedResults[invocation.executablePath] else {
            throw NotProgrammed(executablePath: invocation.executablePath)
        }
        return try programmed.get()
    }
}
```

Note: `Result<ProcessResult, any Error>` cannot conform to `Equatable` when the error is existential, and `NotProgrammed` above declares `Equatable` — that is fine, `NotProgrammed` itself only holds a `String`. Nothing here requires `Result` to be `Equatable`.

- [ ] **Step 4: Run the test to verify it passes**

Run: `swift test --package-path SDMKit --filter FakeProcessRunnerTests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add SDMKit/Sources/SDMResolver/ProcessRunner.swift SDMKit/Sources/SDMResolver/FakeProcessRunner.swift SDMKit/Tests/SDMResolverTests/FakeProcessRunnerTests.swift
git commit -m "feat: add ProcessRunner protocol and FakeProcessRunner test double"
```

---

### Task 4: `RealProcessRunner` and `ExternalToolLocator`

**Files:**
- Create: `SDMKit/Sources/SDMResolver/RealProcessRunner.swift`
- Create: `SDMKit/Sources/SDMResolver/ExternalTool.swift`
- Create: `SDMKit/Tests/SDMResolverTests/ExternalToolLocatorTests.swift`

**Interfaces:**
- Consumes: `ProcessRunner`, `ProcessInvocation`, `ProcessResult` (Task 3)
- Produces:
  - `public struct RealProcessRunner: ProcessRunner` — the production `Process`-based implementation
  - `public enum ExternalTool: String, Sendable { case ytDlp = "yt-dlp"; case ffmpeg }`
  - `public struct ExternalToolLocator: Sendable { public init(searchPaths: [String] = ExternalToolLocator.defaultSearchPaths, fileManager: FileManager = .default); public func locate(_ tool: ExternalTool) -> String? }`

`ExternalToolLocator` is deliberately synchronous and `FileManager`-only (no process spawn, no `which`): it just checks whether an executable file exists at each of a few well-known Homebrew/system paths. That makes it trivially testable with a real temp directory holding a dummy executable — no `ProcessRunner` involved at all.

- [ ] **Step 1: Write the failing test**

`SDMKit/Tests/SDMResolverTests/ExternalToolLocatorTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMResolver

private func makeExecutable(named name: String, in directory: URL) throws -> URL {
    let url = directory.appendingPathComponent(name)
    try Data("#!/bin/sh\necho fake\n".utf8).write(to: url)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
    return url
}

@Test func locatesAToolThatExistsAtASearchPath() throws {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: dir) }
    let toolURL = try makeExecutable(named: "yt-dlp", in: dir)

    let locator = ExternalToolLocator(searchPaths: [dir.path])
    #expect(locator.locate(.ytDlp) == toolURL.path)
}

@Test func returnsNilWhenNoSearchPathHasTheTool() throws {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: dir) }

    let locator = ExternalToolLocator(searchPaths: [dir.path])
    #expect(locator.locate(.ytDlp) == nil)
}

@Test func checksSearchPathsInOrderAndReturnsTheFirstMatch() throws {
    let dirA = FileManager.default.temporaryDirectory
        .appendingPathComponent(UUID().uuidString, isDirectory: true)
    let dirB = FileManager.default.temporaryDirectory
        .appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: dirA, withIntermediateDirectories: true)
    try FileManager.default.createDirectory(at: dirB, withIntermediateDirectories: true)
    defer {
        try? FileManager.default.removeItem(at: dirA)
        try? FileManager.default.removeItem(at: dirB)
    }
    _ = try makeExecutable(named: "ffmpeg", in: dirA)
    let secondURL = try makeExecutable(named: "ffmpeg", in: dirB)

    // Only dirB is searched, so the match must come from there even though
    // dirA also has one — proves search order is respected rather than a
    // global filesystem scan.
    let locator = ExternalToolLocator(searchPaths: [dirB.path])
    #expect(locator.locate(.ffmpeg) == secondURL.path)
}

@Test func defaultSearchPathsCoverHomebrewOnBothArchitectures() {
    #expect(ExternalToolLocator.defaultSearchPaths.contains("/usr/local/bin"))
    #expect(ExternalToolLocator.defaultSearchPaths.contains("/opt/homebrew/bin"))
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter ExternalToolLocatorTests`
Expected: FAIL — `cannot find 'ExternalToolLocator' in scope`.

- [ ] **Step 3: Implement `RealProcessRunner` and `ExternalToolLocator`**

`SDMKit/Sources/SDMResolver/RealProcessRunner.swift`:

```swift
import Foundation

/// Production `ProcessRunner`: spawns a real `Foundation.Process`, captures
/// stdout/stderr fully (yt-dlp's `-J` output can be tens of KB — fine to
/// buffer whole), and waits for exit. The first thing in this codebase to
/// invoke an external binary — see this plan's Global Constraints for why no
/// prior helper exists to follow.
public struct RealProcessRunner: ProcessRunner {
    public init() {}

    public func run(_ invocation: ProcessInvocation) async throws -> ProcessResult {
        try await withCheckedThrowingContinuation { continuation in
            let process = Process()
            process.executableURL = URL(fileURLWithPath: invocation.executablePath)
            process.arguments = invocation.arguments

            let stdoutPipe = Pipe()
            let stderrPipe = Pipe()
            process.standardOutput = stdoutPipe
            process.standardError = stderrPipe

            do {
                try process.run()
            } catch {
                continuation.resume(throwing: error)
                return
            }

            // Read both pipes off the calling queue before waiting: a
            // process whose output exceeds the pipe's kernel buffer (64 KB)
            // would otherwise deadlock against `waitUntilExit()`, since nothing
            // would be draining the pipe it is blocked writing to.
            let stdoutData = stdoutPipe.fileHandleForReading.readDataToEndOfFile()
            let stderrData = stderrPipe.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()

            continuation.resume(
                returning: ProcessResult(
                    exitCode: process.terminationStatus,
                    standardOutput: stdoutData,
                    standardError: stderrData
                )
            )
        }
    }
}
```

`SDMKit/Sources/SDMResolver/ExternalTool.swift`:

```swift
import Foundation

/// An external binary SDM shells out to. Never bundled — see spec §14 and
/// this plan's Global Constraints.
public enum ExternalTool: String, Sendable {
    case ytDlp = "yt-dlp"
    case ffmpeg
}

/// Finds an external tool by checking well-known install locations directly
/// via `FileManager`, rather than spawning `which` (which would itself need
/// a `ProcessRunner` round trip and depends on `PATH`, which a GUI app
/// launched from Finder does not inherit from the user's shell profile).
public struct ExternalToolLocator: Sendable {
    private let searchPaths: [String]
    private let fileManager: FileManager

    /// Homebrew's two install prefixes (Apple Silicon and Intel) plus the
    /// two standard system paths, in the order a Homebrew install is most
    /// likely to be found.
    public static let defaultSearchPaths = [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
    ]

    public init(searchPaths: [String] = ExternalToolLocator.defaultSearchPaths, fileManager: FileManager = .default) {
        self.searchPaths = searchPaths
        self.fileManager = fileManager
    }

    /// Returns the tool's full path, or `nil` if it is not executable at any
    /// configured search path.
    public func locate(_ tool: ExternalTool) -> String? {
        for path in searchPaths {
            let candidate = (path as NSString).appendingPathComponent(tool.rawValue)
            if fileManager.isExecutableFile(atPath: candidate) {
                return candidate
            }
        }
        return nil
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `swift test --package-path SDMKit --filter ExternalToolLocatorTests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add SDMKit/Sources/SDMResolver/RealProcessRunner.swift SDMKit/Sources/SDMResolver/ExternalTool.swift SDMKit/Tests/SDMResolverTests/ExternalToolLocatorTests.swift
git commit -m "feat: add RealProcessRunner and ExternalToolLocator"
```

---

### Task 5: `YtDlpFormat` / `YtDlpExtractionResult` — decoding the real `-J` JSON shape

**Files:**
- Create: `SDMKit/Sources/SDMResolver/YtDlpFormat.swift`
- Create: `SDMKit/Sources/SDMResolver/YtDlpExtractionResult.swift`
- Create: `SDMKit/Tests/SDMResolverTests/Fixtures/youtube_progressive_and_hls.json`
- Create: `SDMKit/Tests/SDMResolverTests/Fixtures/youtube_dash_adaptive.json`
- Create: `SDMKit/Tests/SDMResolverTests/YtDlpExtractionResultTests.swift`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `public struct YtDlpFormat: Codable, Equatable, Sendable` with `formatID, ext, vcodec, acodec, protocolName, width, height, filesize, filesizeApprox, formatNote, tbr, url`, plus computed `isVideoOnly`, `isAudioOnly`, `isProgressive`, `isDirectlyDownloadable`, `effectiveFilesize`
  - `public struct YtDlpExtractionResult: Codable, Equatable, Sendable` with `id, title, ext, webpageURL, formats: [YtDlpFormat]`

The two fixtures were captured for real, once, by hand (`yt-dlp -J --cookies-from-browser safari "https://www.youtube.com/watch?v=IlIJa_FDK-0"`, run on 2026-08-07, yt-dlp 2026.07.04) — see this plan's Global Constraints. `youtube_progressive_and_hls.json` is that real response, trimmed to 4 representative formats (a storyboard, an HLS-only 144p, the one `https`-protocol progressive 360p, and an HLS-only 1080p) with every signed-URL query string replaced by a short placeholder — the real `sig=`/`expire=` tokens are live googlevideo credentials and do not belong in a committed fixture even though nothing in this codebase ever dereferences them. `youtube_dash_adaptive.json` is hand-authored in the exact same schema (confirmed field names against the real capture) to cover the muxing case the real capture didn't happen to produce: a genuinely separate video-only (`137`) and audio-only (`140`) DASH pair, both `https` and both carrying a real `filesize`.

- [ ] **Step 1: Create the fixtures**

`SDMKit/Tests/SDMResolverTests/Fixtures/youtube_progressive_and_hls.json`:

```json
{
  "id": "IlIJa_FDK-0",
  "title": "macOS 27 Golden Gate - Top 10 Features!",
  "ext": "mp4",
  "webpage_url": "https://www.youtube.com/watch?v=IlIJa_FDK-0",
  "formats": [
    {
      "format_id": "sb0",
      "ext": "mhtml",
      "vcodec": "none",
      "acodec": "none",
      "protocol": "mhtml",
      "width": 320,
      "height": 180,
      "filesize_approx": null,
      "format_note": "storyboard",
      "tbr": null,
      "url": "https://i.ytimg.com/sb/IlIJa_FDK-0/storyboard3_L3/M$M.jpg?sqp=fake"
    },
    {
      "format_id": "91",
      "ext": "mp4",
      "vcodec": "avc1.4D400C",
      "acodec": "mp4a.40.5",
      "protocol": "m3u8_native",
      "width": 256,
      "height": 144,
      "tbr": 175.254,
      "url": "https://manifest.googlevideo.com/api/manifest/hls_playlist/expire/1786099780/id/2252096bf1432bed/itag/91/playlist/index.m3u8"
    },
    {
      "format_id": "18",
      "ext": "mp4",
      "vcodec": "avc1.42001E",
      "acodec": "mp4a.40.2",
      "protocol": "https",
      "width": 640,
      "height": 360,
      "filesize": 15283803,
      "filesize_approx": 15283802,
      "format_note": "360p",
      "tbr": 275.377,
      "url": "https://rr1---sn-5hne6nzy.googlevideo.com/videoplayback?expire=1786099780&itag=18&id=o-AAgL9OYYtneYuJjFvmBIDa&mime=video%2Fmp4&clen=15283803&sig=fake_signature_18"
    },
    {
      "format_id": "96",
      "ext": "mp4",
      "vcodec": "avc1.640028",
      "acodec": "mp4a.40.2",
      "protocol": "m3u8_native",
      "width": 1920,
      "height": 1080,
      "tbr": 3955.267,
      "url": "https://manifest.googlevideo.com/api/manifest/hls_playlist/expire/1786099780/id/2252096bf1432bed/itag/96/playlist/index.m3u8"
    }
  ]
}
```

`SDMKit/Tests/SDMResolverTests/Fixtures/youtube_dash_adaptive.json`:

```json
{
  "id": "dQw4w9WgXcQ",
  "title": "Example Adaptive Video",
  "ext": "mp4",
  "webpage_url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
  "formats": [
    {
      "format_id": "18",
      "ext": "mp4",
      "vcodec": "avc1.42001E",
      "acodec": "mp4a.40.2",
      "protocol": "https",
      "width": 640,
      "height": 360,
      "filesize": 12000000,
      "filesize_approx": 12000000,
      "format_note": "360p",
      "tbr": 300.0,
      "url": "https://rr1---sn-fake.googlevideo.com/videoplayback?itag=18&sig=fake_signature_18"
    },
    {
      "format_id": "140",
      "ext": "m4a",
      "vcodec": "none",
      "acodec": "mp4a.40.2",
      "protocol": "https",
      "width": null,
      "height": null,
      "filesize": 7185672,
      "filesize_approx": 7185672,
      "format_note": "audio only, medium",
      "tbr": 129.6,
      "url": "https://rr1---sn-fake.googlevideo.com/videoplayback?itag=140&sig=fake_signature_140"
    },
    {
      "format_id": "137",
      "ext": "mp4",
      "vcodec": "avc1.640028",
      "acodec": "none",
      "protocol": "https",
      "width": 1920,
      "height": 1080,
      "filesize": 70948726,
      "filesize_approx": 70948726,
      "format_note": "1080p, video only",
      "tbr": 1279.5,
      "url": "https://rr1---sn-fake.googlevideo.com/videoplayback?itag=137&sig=fake_signature_137"
    },
    {
      "format_id": "247",
      "ext": "webm",
      "vcodec": "vp9",
      "acodec": "none",
      "protocol": "https",
      "width": 1280,
      "height": 720,
      "filesize": 35000000,
      "filesize_approx": 35000000,
      "format_note": "720p, video only",
      "tbr": 630.2,
      "url": "https://rr1---sn-fake.googlevideo.com/videoplayback?itag=247&sig=fake_signature_247"
    }
  ]
}
```

- [ ] **Step 2: Write the failing test**

`SDMKit/Tests/SDMResolverTests/YtDlpExtractionResultTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMResolver

private func loadFixture(_ name: String) throws -> Data {
    let url = Bundle.module.url(forResource: name, withExtension: "json", subdirectory: "Fixtures")!
    return try Data(contentsOf: url)
}

@Test func decodesTheRealCapturedProgressiveAndHLSResponse() throws {
    let data = try loadFixture("youtube_progressive_and_hls")
    let result = try JSONDecoder().decode(YtDlpExtractionResult.self, from: data)

    #expect(result.id == "IlIJa_FDK-0")
    #expect(result.title == "macOS 27 Golden Gate - Top 10 Features!")
    #expect(result.formats.count == 4)

    let progressive = try #require(result.formats.first { $0.formatID == "18" })
    #expect(progressive.protocolName == "https")
    #expect(progressive.isProgressive)
    #expect(progressive.isDirectlyDownloadable)
    #expect(progressive.effectiveFilesize == 15_283_803)
    #expect(progressive.height == 360)

    let hlsOnly = try #require(result.formats.first { $0.formatID == "96" })
    #expect(hlsOnly.protocolName == "m3u8_native")
    #expect(!hlsOnly.isDirectlyDownloadable)
    #expect(hlsOnly.height == 1080)

    let storyboard = try #require(result.formats.first { $0.formatID == "sb0" })
    #expect(storyboard.vcodec == "none")
    #expect(storyboard.acodec == "none")
    #expect(storyboard.effectiveFilesize == nil)
}

@Test func decodesTheHandAuthoredDashAdaptiveFixture() throws {
    let data = try loadFixture("youtube_dash_adaptive")
    let result = try JSONDecoder().decode(YtDlpExtractionResult.self, from: data)

    let videoOnly = try #require(result.formats.first { $0.formatID == "137" })
    #expect(videoOnly.isVideoOnly)
    #expect(!videoOnly.isAudioOnly)
    #expect(videoOnly.isDirectlyDownloadable)
    #expect(videoOnly.effectiveFilesize == 70_948_726)

    let audioOnly = try #require(result.formats.first { $0.formatID == "140" })
    #expect(audioOnly.isAudioOnly)
    #expect(!audioOnly.isVideoOnly)
    #expect(audioOnly.width == nil)

    let progressive = try #require(result.formats.first { $0.formatID == "18" })
    #expect(progressive.isProgressive)
    #expect(!progressive.isVideoOnly)
    #expect(!progressive.isAudioOnly)
}

@Test func filesizeFallsBackToApproxWhenExactIsMissing() {
    let format = YtDlpFormat(
        formatID: "91", ext: "mp4", vcodec: "avc1", acodec: "mp4a", protocolName: "m3u8_native",
        width: 256, height: 144, filesize: nil, filesizeApprox: 999, formatNote: nil, tbr: nil,
        url: URL(string: "https://example.com/x.m3u8")!)
    #expect(format.effectiveFilesize == 999)
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter YtDlpExtractionResultTests`
Expected: FAIL — `cannot find 'YtDlpExtractionResult' in scope`, and the fixtures are not yet resource-registered.

- [ ] **Step 4: Register the fixtures as a resource and implement the two types**

Modify `SDMKit/Package.swift` — add a `resources:` entry to the `SDMResolverTests` test target:

```swift
        .testTarget(
            name: "SDMResolverTests", dependencies: ["SDMResolver"],
            resources: [.copy("Fixtures")]),
```

`SDMKit/Sources/SDMResolver/YtDlpFormat.swift`:

```swift
import Foundation

/// One entry from yt-dlp's `-J` `formats` array. Field names and shape are
/// taken directly from a real capture — see `YtDlpExtractionResultTests` and
/// this plan's Global Constraints for provenance.
public struct YtDlpFormat: Codable, Equatable, Sendable {
    public var formatID: String
    public var ext: String
    public var vcodec: String
    public var acodec: String
    public var protocolName: String
    public var width: Int?
    public var height: Int?
    public var filesize: Int64?
    public var filesizeApprox: Int64?
    public var formatNote: String?
    public var tbr: Double?
    public var url: URL

    private enum CodingKeys: String, CodingKey {
        case formatID = "format_id"
        case ext
        case vcodec
        case acodec
        case protocolName = "protocol"
        case width
        case height
        case filesize
        case filesizeApprox = "filesize_approx"
        case formatNote = "format_note"
        case tbr
        case url
    }

    public init(
        formatID: String, ext: String, vcodec: String, acodec: String, protocolName: String,
        width: Int?, height: Int?, filesize: Int64?, filesizeApprox: Int64?, formatNote: String?,
        tbr: Double?, url: URL
    ) {
        self.formatID = formatID
        self.ext = ext
        self.vcodec = vcodec
        self.acodec = acodec
        self.protocolName = protocolName
        self.width = width
        self.height = height
        self.filesize = filesize
        self.filesizeApprox = filesizeApprox
        self.formatNote = formatNote
        self.tbr = tbr
        self.url = url
    }

    /// `"none"` is yt-dlp's own sentinel for "this track is absent," not a
    /// missing/null field — a storyboard and a DASH audio-only stream both
    /// use it for `vcodec`/`acodec` respectively.
    public var isVideoOnly: Bool { vcodec != "none" && acodec == "none" }
    public var isAudioOnly: Bool { vcodec == "none" && acodec != "none" }
    public var isProgressive: Bool { vcodec != "none" && acodec != "none" }

    /// Whether `url` points at a single Range-able resource the existing
    /// segmented engine can download, as opposed to a manifest (`m3u8_native`,
    /// `m3u8`, `http_dash_segments`) with no single byte-addressable file.
    public var isDirectlyDownloadable: Bool { protocolName == "https" || protocolName == "http" }

    public var effectiveFilesize: Int64? { filesize ?? filesizeApprox }
}
```

`SDMKit/Sources/SDMResolver/YtDlpExtractionResult.swift`:

```swift
import Foundation

/// The top-level shape of `yt-dlp -J <url>`, trimmed to the fields SDM
/// actually reads. yt-dlp's real output carries dozens more (thumbnails,
/// chapters, subtitles, uploader info, ...) which are simply ignored by
/// `Codable`'s default "unknown keys are dropped" behavior.
public struct YtDlpExtractionResult: Codable, Equatable, Sendable {
    public var id: String
    public var title: String
    public var ext: String
    public var webpageURL: URL
    public var formats: [YtDlpFormat]

    private enum CodingKeys: String, CodingKey {
        case id
        case title
        case ext
        case webpageURL = "webpage_url"
        case formats
    }

    public init(id: String, title: String, ext: String, webpageURL: URL, formats: [YtDlpFormat]) {
        self.id = id
        self.title = title
        self.ext = ext
        self.webpageURL = webpageURL
        self.formats = formats
    }
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `swift test --package-path SDMKit --filter YtDlpExtractionResultTests`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add SDMKit/Package.swift SDMKit/Sources/SDMResolver/YtDlpFormat.swift SDMKit/Sources/SDMResolver/YtDlpExtractionResult.swift SDMKit/Tests/SDMResolverTests/Fixtures SDMKit/Tests/SDMResolverTests/YtDlpExtractionResultTests.swift
git commit -m "feat: decode yt-dlp -J format tables from real captured fixtures"
```

---

### Task 6: `LinkResolver`, `ResolvedMedia`, `YouTubeResolver`

**Files:**
- Create: `SDMKit/Sources/SDMResolver/ResolvedMedia.swift`
- Create: `SDMKit/Sources/SDMResolver/YouTubeResolver.swift`
- Create: `SDMKit/Tests/SDMResolverTests/YouTubeResolverTests.swift`

**Interfaces:**
- Consumes: `ProcessRunner`, `ExternalToolLocator`, `ExternalTool`, `CookiesSource`, `YtDlpFormat`, `YtDlpExtractionResult` (Tasks 2–5)
- Produces:
  - `public protocol LinkResolver: Sendable { func canHandle(_ url: URL) -> Bool; func resolve(_ url: URL) async throws -> [ResolvedMedia] }` — spec §8's exact seam
  - `public struct ResolvedMedia: Equatable, Sendable { public var extractor: String; public var mediaID: String; public var title: String; public var formats: [YtDlpFormat]; public var sourceURL: URL }`
  - `public struct YouTubeResolver: LinkResolver`
  - `public enum YtDlpResolverError: Error, Equatable { case toolMissing; case processFailed(exitCode: Int32, stderr: String); case malformedJSON }`

- [ ] **Step 1: Write the failing test**

`SDMKit/Tests/SDMResolverTests/YouTubeResolverTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMResolver

private func fixtureData(_ name: String) throws -> Data {
    let url = Bundle.module.url(forResource: name, withExtension: "json", subdirectory: "Fixtures")!
    return try Data(contentsOf: url)
}

private let watchURL = URL(string: "https://www.youtube.com/watch?v=IlIJa_FDK-0")!

@Test func canHandleAcceptsYouTubeWatchAndShortURLsOnly() {
    let resolver = YouTubeResolver(
        processRunner: FakeProcessRunner(), toolLocator: ExternalToolLocator(searchPaths: []),
        cookiesSource: .safari)
    #expect(resolver.canHandle(watchURL))
    #expect(resolver.canHandle(URL(string: "https://youtu.be/IlIJa_FDK-0")!))
    #expect(!resolver.canHandle(URL(string: "https://example.com/video.mp4")!))
}

@Test func resolveDecodesAHappyPathExtraction() async throws {
    let runner = FakeProcessRunner()
    await runner.program(
        executablePath: "/fake/yt-dlp",
        result: ProcessResult(
            exitCode: 0, standardOutput: try fixtureData("youtube_progressive_and_hls"),
            standardError: Data()))
    let locator = ExternalToolLocator(searchPaths: ["/fake"])
    // `ExternalToolLocator` checks the filesystem, so point it at a real
    // directory holding a dummy executable named "yt-dlp" rather than the
    // nonexistent "/fake" used for the FakeProcessRunner's programming key.
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: dir) }
    let toolURL = dir.appendingPathComponent("yt-dlp")
    try Data().write(to: toolURL)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: toolURL.path)
    await runner.program(
        executablePath: toolURL.path,
        result: ProcessResult(
            exitCode: 0, standardOutput: try fixtureData("youtube_progressive_and_hls"),
            standardError: Data()))

    let resolver = YouTubeResolver(
        processRunner: runner, toolLocator: ExternalToolLocator(searchPaths: [dir.path]),
        cookiesSource: .safari)
    let media = try await resolver.resolve(watchURL)

    #expect(media.count == 1)
    #expect(media[0].extractor == "youtube")
    #expect(media[0].mediaID == "IlIJa_FDK-0")
    #expect(media[0].formats.count == 4)
    #expect(media[0].sourceURL == URL(string: "https://www.youtube.com/watch?v=IlIJa_FDK-0")!)

    let invocations = await runner.recordedInvocations
    #expect(invocations.count == 1)
    #expect(invocations[0].arguments.contains("--cookies-from-browser"))
    #expect(invocations[0].arguments.contains("safari"))
    #expect(invocations[0].arguments.contains(watchURL.absoluteString))
}

@Test func resolveThrowsToolMissingWhenYtDlpIsNotFound() async {
    let resolver = YouTubeResolver(
        processRunner: FakeProcessRunner(), toolLocator: ExternalToolLocator(searchPaths: []),
        cookiesSource: .safari)
    await #expect(throws: YtDlpResolverError.toolMissing) {
        try await resolver.resolve(watchURL)
    }
}

@Test func resolveThrowsProcessFailedOnNonZeroExit() async throws {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: dir) }
    let toolURL = dir.appendingPathComponent("yt-dlp")
    try Data().write(to: toolURL)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: toolURL.path)

    let runner = FakeProcessRunner()
    await runner.program(
        executablePath: toolURL.path,
        result: ProcessResult(
            exitCode: 1, standardOutput: Data(),
            standardError: Data("ERROR: Video unavailable".utf8)))

    let resolver = YouTubeResolver(
        processRunner: runner, toolLocator: ExternalToolLocator(searchPaths: [dir.path]),
        cookiesSource: .none)

    do {
        _ = try await resolver.resolve(watchURL)
        Issue.record("expected processFailed")
    } catch let YtDlpResolverError.processFailed(exitCode, stderr) {
        #expect(exitCode == 1)
        #expect(stderr.contains("Video unavailable"))
    }
}

@Test func resolveThrowsMalformedJSONOnUnparseableOutput() async throws {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: dir) }
    let toolURL = dir.appendingPathComponent("yt-dlp")
    try Data().write(to: toolURL)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: toolURL.path)

    let runner = FakeProcessRunner()
    await runner.program(
        executablePath: toolURL.path,
        result: ProcessResult(exitCode: 0, standardOutput: Data("not json".utf8), standardError: Data()))

    let resolver = YouTubeResolver(
        processRunner: runner, toolLocator: ExternalToolLocator(searchPaths: [dir.path]),
        cookiesSource: .none)
    await #expect(throws: YtDlpResolverError.malformedJSON) {
        try await resolver.resolve(watchURL)
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter YouTubeResolverTests`
Expected: FAIL — `cannot find 'YouTubeResolver' in scope`.

- [ ] **Step 3: Implement `ResolvedMedia` and `YouTubeResolver`**

`SDMKit/Sources/SDMResolver/ResolvedMedia.swift`:

```swift
import Foundation

/// Spec §8's `LinkResolver` protocol: `canHandle(URL) -> Bool`,
/// `resolve(URL) -> [ResolvedMedia]`. Generic HTTP downloads never go through
/// this — it exists purely as the extension seam for sites like YouTube that
/// need metadata extraction before a direct, Range-able URL is known.
public protocol LinkResolver: Sendable {
    func canHandle(_ url: URL) -> Bool
    func resolve(_ url: URL) async throws -> [ResolvedMedia]
}

/// One resolved video: its full format table plus enough provenance
/// (`extractor`, `mediaID`, `sourceURL`) to ask the resolver to refresh a
/// single format's URL later, once it expires. Plural return from
/// `resolve(_:)` accommodates a future playlist URL resolving to many; a
/// single watch URL resolves to exactly one.
public struct ResolvedMedia: Equatable, Sendable {
    public var extractor: String
    public var mediaID: String
    public var title: String
    public var formats: [YtDlpFormat]
    public var sourceURL: URL

    public init(extractor: String, mediaID: String, title: String, formats: [YtDlpFormat], sourceURL: URL) {
        self.extractor = extractor
        self.mediaID = mediaID
        self.title = title
        self.formats = formats
        self.sourceURL = sourceURL
    }
}
```

`SDMKit/Sources/SDMResolver/YouTubeResolver.swift`:

```swift
import Foundation

public enum YtDlpResolverError: Error, Equatable {
    case toolMissing
    case processFailed(exitCode: Int32, stderr: String)
    case malformedJSON
}

/// `LinkResolver` backed by yt-dlp as a metadata extractor — spec §8: "yt-dlp
/// is used as an extractor, not a downloader." `resolve(_:)` never downloads
/// any bytes; it only runs `yt-dlp -J` and parses the format table.
public struct YouTubeResolver: LinkResolver, Sendable {
    private let processRunner: any ProcessRunner
    private let toolLocator: ExternalToolLocator
    private let cookiesSource: CookiesSource

    public init(processRunner: any ProcessRunner, toolLocator: ExternalToolLocator, cookiesSource: CookiesSource) {
        self.processRunner = processRunner
        self.toolLocator = toolLocator
        self.cookiesSource = cookiesSource
    }

    public func canHandle(_ url: URL) -> Bool {
        guard let host = url.host?.lowercased() else { return false }
        return host == "youtube.com" || host == "www.youtube.com" || host == "m.youtube.com"
            || host == "youtu.be"
    }

    public func resolve(_ url: URL) async throws -> [ResolvedMedia] {
        guard let toolPath = toolLocator.locate(.ytDlp) else {
            throw YtDlpResolverError.toolMissing
        }

        let arguments = ["-J", "--no-warnings"] + cookiesSource.ytDlpArguments + [url.absoluteString]
        let result = try await processRunner.run(
            ProcessInvocation(executablePath: toolPath, arguments: arguments))

        guard result.exitCode == 0 else {
            throw YtDlpResolverError.processFailed(
                exitCode: result.exitCode,
                stderr: String(decoding: result.standardError, as: UTF8.self))
        }

        guard
            let extraction = try? JSONDecoder().decode(
                YtDlpExtractionResult.self, from: result.standardOutput)
        else {
            throw YtDlpResolverError.malformedJSON
        }

        return [
            ResolvedMedia(
                extractor: "youtube", mediaID: extraction.id, title: extraction.title,
                formats: extraction.formats, sourceURL: extraction.webpageURL)
        ]
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `swift test --package-path SDMKit --filter YouTubeResolverTests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add SDMKit/Sources/SDMResolver/ResolvedMedia.swift SDMKit/Sources/SDMResolver/YouTubeResolver.swift SDMKit/Tests/SDMResolverTests/YouTubeResolverTests.swift
git commit -m "feat: add LinkResolver protocol and YouTubeResolver"
```

---

### Task 7: `QualityPreference` and `QualitySelector` — pure format selection

**Files:**
- Create: `SDMKit/Sources/SDMResolver/QualityPreference.swift`
- Create: `SDMKit/Sources/SDMResolver/QualitySelector.swift`
- Create: `SDMKit/Tests/SDMResolverTests/QualitySelectorTests.swift`

**Interfaces:**
- Consumes: `YtDlpFormat` (Task 5)
- Produces:
  - `public struct QualityPreference: Equatable, Sendable, Codable { public var resolutionLadder: [Int]; public var preferredCodecPrefix: String?; public var preferredContainer: String?; public var maxFilesizeBytes: Int64? }`, `public static func standardLadder(maxHeight: Int) -> [Int]`
  - `public enum QualitySelector { public static func selectFormat(from formats: [YtDlpFormat], preference: QualityPreference) -> YtDlpFormat? }`

Table-driven per spec §11.6 ("format selection... tested with no binary installed"), matching `VerdictRules`/`PackageClustering`'s existing pure-function-plus-fixtures pattern.

- [ ] **Step 1: Write the failing test**

`SDMKit/Tests/SDMResolverTests/QualitySelectorTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMResolver

private func format(
    id: String, height: Int?, vcodec: String = "avc1.640028", acodec: String = "mp4a.40.2",
    protocolName: String = "https", filesize: Int64? = nil, tbr: Double = 100
) -> YtDlpFormat {
    YtDlpFormat(
        formatID: id, ext: "mp4", vcodec: vcodec, acodec: acodec, protocolName: protocolName,
        width: nil, height: height, filesize: filesize, filesizeApprox: nil, formatNote: nil,
        tbr: tbr, url: URL(string: "https://example.com/\(id)")!)
}

private let defaultPreference = QualityPreference(
    resolutionLadder: [1080, 720, 480, 360], preferredCodecPrefix: "avc1",
    preferredContainer: "mp4", maxFilesizeBytes: nil)

@Test func picksTheFirstLadderRungThatHasAMatch() {
    let formats = [format(id: "360p", height: 360), format(id: "720p", height: 720)]
    let picked = QualitySelector.selectFormat(from: formats, preference: defaultPreference)
    #expect(picked?.formatID == "720p")
}

@Test func fallsThroughTheLadderWhenTheTopRungIsUnavailable() {
    // Nothing at 1080 or 720; 480 is the first rung with a match.
    let formats = [format(id: "360p", height: 360), format(id: "480p", height: 480)]
    let picked = QualitySelector.selectFormat(from: formats, preference: defaultPreference)
    #expect(picked?.formatID == "480p")
}

@Test func excludesFormatsOverTheFilesizeCap() {
    let preference = QualityPreference(
        resolutionLadder: [1080, 720], preferredCodecPrefix: nil, preferredContainer: nil,
        maxFilesizeBytes: 10_000_000)
    let formats = [
        format(id: "1080p-huge", height: 1080, filesize: 50_000_000),
        format(id: "720p-ok", height: 720, filesize: 5_000_000),
    ]
    let picked = QualitySelector.selectFormat(from: formats, preference: preference)
    #expect(picked?.formatID == "720p-ok")
}

@Test func keepsFormatsWithUnknownFilesizeRatherThanExcludingThem() {
    // HLS entries frequently omit `filesize` entirely — an unknown size must
    // not be treated as "definitely over the cap."
    let preference = QualityPreference(
        resolutionLadder: [1080], preferredCodecPrefix: nil, preferredContainer: nil,
        maxFilesizeBytes: 1_000)
    let formats = [format(id: "1080p-unknown-size", height: 1080, filesize: nil)]
    let picked = QualitySelector.selectFormat(from: formats, preference: preference)
    #expect(picked?.formatID == "1080p-unknown-size")
}

@Test func prefersTheConfiguredCodecAmongTiedRungCandidates() {
    let formats = [
        format(id: "720p-vp9", height: 720, vcodec: "vp9"),
        format(id: "720p-avc1", height: 720, vcodec: "avc1.640020"),
    ]
    let picked = QualitySelector.selectFormat(from: formats, preference: defaultPreference)
    #expect(picked?.formatID == "720p-avc1")
}

@Test func prefersADirectlyDownloadableFormatOverAnHLSManifestAtTheSameRung() {
    let formats = [
        format(id: "720p-hls", height: 720, protocolName: "m3u8_native"),
        format(id: "720p-https", height: 720, protocolName: "https"),
    ]
    let picked = QualitySelector.selectFormat(from: formats, preference: defaultPreference)
    #expect(picked?.formatID == "720p-https")
}

@Test func fallsBackToTheHighestAvailableHeightWhenNoRungMatchesAtAll() {
    let formats = [format(id: "144p", height: 144), format(id: "240p", height: 240)]
    let picked = QualitySelector.selectFormat(from: formats, preference: defaultPreference)
    #expect(picked?.formatID == "240p")
}

@Test func excludesAudioOnlyAndStoryboardFormatsFromVideoSelection() {
    let formats = [
        format(id: "audio", height: nil, vcodec: "none", acodec: "mp4a.40.2"),
        format(id: "storyboard", height: 90, vcodec: "none", acodec: "none"),
        format(id: "720p", height: 720),
    ]
    let picked = QualitySelector.selectFormat(from: formats, preference: defaultPreference)
    #expect(picked?.formatID == "720p")
}

@Test func returnsNilWhenNoVideoFormatExistsAtAll() {
    let formats = [format(id: "audio", height: nil, vcodec: "none")]
    #expect(QualitySelector.selectFormat(from: formats, preference: defaultPreference) == nil)
}

@Test func standardLadderIsCappedAtAndBelowTheRequestedMaxHeight() {
    #expect(QualityPreference.standardLadder(maxHeight: 720) == [720, 480, 360, 240, 144])
    #expect(QualityPreference.standardLadder(maxHeight: 2160).first == 2160)
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter QualitySelectorTests`
Expected: FAIL — `cannot find 'QualityPreference' in scope`.

- [ ] **Step 3: Implement `QualityPreference` and `QualitySelector`**

`SDMKit/Sources/SDMResolver/QualityPreference.swift`:

```swift
/// Spec §8 and §12: "an ordered rule list in Settings (resolution ladder,
/// codec preference, container preference, max filesize cap)." Default per
/// spec §12: "1080p → 720p, prefer H.264."
public struct QualityPreference: Equatable, Sendable, Codable {
    public var resolutionLadder: [Int]
    public var preferredCodecPrefix: String?
    public var preferredContainer: String?
    public var maxFilesizeBytes: Int64?

    public init(
        resolutionLadder: [Int], preferredCodecPrefix: String?, preferredContainer: String?,
        maxFilesizeBytes: Int64?
    ) {
        self.resolutionLadder = resolutionLadder
        self.preferredCodecPrefix = preferredCodecPrefix
        self.preferredContainer = preferredContainer
        self.maxFilesizeBytes = maxFilesizeBytes
    }

    /// The full standard YouTube resolution ladder, truncated to start at
    /// `maxHeight` — what Settings' single "maximum resolution" picker
    /// expands into. Spec §12's default ("1080p → 720p") is
    /// `standardLadder(maxHeight: 1080)`.
    public static func standardLadder(maxHeight: Int) -> [Int] {
        let full = [2160, 1440, 1080, 720, 480, 360, 240, 144]
        return full.filter { $0 <= maxHeight }
    }
}
```

`SDMKit/Sources/SDMResolver/QualitySelector.swift`:

```swift
/// Pure function evaluating `QualityPreference` against a resolved video's
/// actual available formats. Spec §8: "The Linkgrabber evaluates it against
/// the actual available formats." Never considers audio-only or storyboard
/// entries — picking the paired audio track for a video-only selection is
/// `MuxPlanner`'s job, not this one's.
public enum QualitySelector {
    public static func selectFormat(from formats: [YtDlpFormat], preference: QualityPreference)
        -> YtDlpFormat?
    {
        var candidates = formats.filter { $0.vcodec != "none" }
        if let cap = preference.maxFilesizeBytes {
            candidates = candidates.filter { format in
                guard let size = format.effectiveFilesize else { return true }
                return size <= cap
            }
        }
        guard !candidates.isEmpty else { return nil }

        var rungMatched = false
        for rung in preference.resolutionLadder {
            let atThisRung = candidates.filter { $0.height == rung }
            if !atThisRung.isEmpty {
                candidates = atThisRung
                rungMatched = true
                break
            }
        }
        if !rungMatched {
            // No configured rung had a match at all: best-effort fall back to
            // whatever the highest available resolution is, rather than
            // returning nothing.
            let maxHeight = candidates.compactMap(\.height).max()
            candidates = candidates.filter { $0.height == maxHeight }
        }

        if let codecPrefix = preference.preferredCodecPrefix {
            let matching = candidates.filter { $0.vcodec.hasPrefix(codecPrefix) }
            if !matching.isEmpty { candidates = matching }
        }
        if let container = preference.preferredContainer {
            let matching = candidates.filter { $0.ext == container }
            if !matching.isEmpty { candidates = matching }
        }
        let downloadable = candidates.filter(\.isDirectlyDownloadable)
        if !downloadable.isEmpty { candidates = downloadable }

        return candidates.max { lhs, rhs in
            let lhsBitrate = lhs.tbr ?? 0
            let rhsBitrate = rhs.tbr ?? 0
            if lhsBitrate != rhsBitrate { return lhsBitrate < rhsBitrate }
            return lhs.formatID > rhs.formatID
        }
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `swift test --package-path SDMKit --filter QualitySelectorTests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add SDMKit/Sources/SDMResolver/QualityPreference.swift SDMKit/Sources/SDMResolver/QualitySelector.swift SDMKit/Tests/SDMResolverTests/QualitySelectorTests.swift
git commit -m "feat: add QualityPreference and the pure QualitySelector"
```

---

### Task 8: `MuxPlanner` — pure video/audio pairing for muxing

**Files:**
- Create: `SDMKit/Sources/SDMResolver/MuxPlan.swift`
- Create: `SDMKit/Tests/SDMResolverTests/MuxPlannerTests.swift`

**Interfaces:**
- Consumes: `YtDlpFormat`, `QualityPreference` (Tasks 5, 7)
- Produces: `public struct MuxPlan: Equatable, Sendable { public var videoFormat: YtDlpFormat; public var audioFormat: YtDlpFormat?; public var requiresMuxing: Bool }`, `public enum MuxPlanner { public static func plan(selectedVideo: YtDlpFormat, availableFormats: [YtDlpFormat]) -> MuxPlan }`

- [ ] **Step 1: Write the failing test**

`SDMKit/Tests/SDMResolverTests/MuxPlannerTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMResolver

private func format(
    id: String, vcodec: String, acodec: String, tbr: Double = 100, protocolName: String = "https"
) -> YtDlpFormat {
    YtDlpFormat(
        formatID: id, ext: "mp4", vcodec: vcodec, acodec: acodec, protocolName: protocolName,
        width: nil, height: nil, filesize: nil, filesizeApprox: nil, formatNote: nil, tbr: tbr,
        url: URL(string: "https://example.com/\(id)")!)
}

@Test func aProgressiveFormatNeedsNoMuxing() {
    let progressive = format(id: "18", vcodec: "avc1.42001E", acodec: "mp4a.40.2")
    let plan = MuxPlanner.plan(selectedVideo: progressive, availableFormats: [progressive])
    #expect(!plan.requiresMuxing)
    #expect(plan.audioFormat == nil)
}

@Test func aVideoOnlyFormatIsPairedWithTheHighestBitrateAudioTrack() {
    let video = format(id: "137", vcodec: "avc1.640028", acodec: "none")
    let lowAudio = format(id: "139", vcodec: "none", acodec: "mp4a.40.5", tbr: 48)
    let highAudio = format(id: "140", vcodec: "none", acodec: "mp4a.40.2", tbr: 128)
    let plan = MuxPlanner.plan(
        selectedVideo: video, availableFormats: [video, lowAudio, highAudio])
    #expect(plan.requiresMuxing)
    #expect(plan.audioFormat?.formatID == "140")
}

@Test func aVideoOnlyFormatWithNoAudioTrackAvailableDoesNotRequireMuxing() {
    let video = format(id: "137", vcodec: "avc1.640028", acodec: "none")
    let plan = MuxPlanner.plan(selectedVideo: video, availableFormats: [video])
    #expect(!plan.requiresMuxing)
    #expect(plan.audioFormat == nil)
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter MuxPlannerTests`
Expected: FAIL — `cannot find 'MuxPlanner' in scope`.

- [ ] **Step 3: Implement `MuxPlan` and `MuxPlanner`**

`SDMKit/Sources/SDMResolver/MuxPlan.swift`:

```swift
/// Spec §8: "high-resolution YouTube formats are video-only. Video and audio
/// download as two items bound to one output, then mux via ffmpeg."
public struct MuxPlan: Equatable, Sendable {
    public var videoFormat: YtDlpFormat
    public var audioFormat: YtDlpFormat?

    public init(videoFormat: YtDlpFormat, audioFormat: YtDlpFormat?) {
        self.videoFormat = videoFormat
        self.audioFormat = audioFormat
    }

    public var requiresMuxing: Bool { audioFormat != nil }
}

public enum MuxPlanner {
    /// Pure: given the video format already chosen by `QualitySelector`,
    /// decides whether it needs an audio companion and, if so, which one —
    /// the highest-bitrate audio-only format in `availableFormats`. Returns
    /// `audioFormat: nil` (no muxing) both when `selectedVideo` already
    /// carries its own audio track and when it is video-only but no audio
    /// track exists in the table at all — a video-only, silent download is
    /// still strictly better than refusing outright.
    public static func plan(selectedVideo: YtDlpFormat, availableFormats: [YtDlpFormat]) -> MuxPlan {
        guard selectedVideo.isVideoOnly else {
            return MuxPlan(videoFormat: selectedVideo, audioFormat: nil)
        }
        let audioCandidates = availableFormats.filter(\.isAudioOnly)
        let bestAudio = audioCandidates.max { ($0.tbr ?? 0) < ($1.tbr ?? 0) }
        return MuxPlan(videoFormat: selectedVideo, audioFormat: bestAudio)
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `swift test --package-path SDMKit --filter MuxPlannerTests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add SDMKit/Sources/SDMResolver/MuxPlan.swift SDMKit/Tests/SDMResolverTests/MuxPlannerTests.swift
git commit -m "feat: add MuxPlanner for video/audio pairing"
```

---

### Task 9: `Muxer` protocol (SDMCore) and `FFmpegMuxer` (SDMResolver)

**Files:**
- Create: `SDMKit/Sources/SDMCore/Muxer.swift`
- Create: `SDMKit/Sources/SDMResolver/FFmpegMuxer.swift`
- Create: `SDMKit/Tests/SDMResolverTests/FFmpegMuxerTests.swift`

**Interfaces:**
- Consumes: `ProcessRunner`, `ExternalToolLocator` (Tasks 3–4)
- Produces:
  - `public protocol Muxer: Sendable { func mux(videoPath: URL, audioPath: URL, outputPath: URL) async throws }` (in `SDMCore`, so `SDMEngine` can accept one without depending on `SDMResolver` — see Task 12)
  - `public struct FFmpegMuxer: Muxer` (in `SDMResolver`)
  - `public enum FFmpegMuxerError: Error, Equatable { case toolMissing; case processFailed(exitCode: Int32, stderr: String) }`

`Muxer` lives in `SDMCore` rather than `SDMResolver` specifically so `DownloadEngine` (which depends only on `SDMCore`, per this plan's Architecture) can accept `any Muxer` at construction without a new cross-module dependency — the same reasoning as `URLRefresher` in Task 11.

- [ ] **Step 1: Write the failing test**

`SDMKit/Tests/SDMResolverTests/FFmpegMuxerTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMResolver

private func makeToolDirectory() throws -> URL {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let toolURL = dir.appendingPathComponent("ffmpeg")
    try Data().write(to: toolURL)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: toolURL.path)
    return dir
}

@Test func muxInvokesFfmpegWithCopyCodecAndBothInputs() async throws {
    let dir = try makeToolDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let toolPath = dir.appendingPathComponent("ffmpeg").path

    let runner = FakeProcessRunner()
    await runner.program(
        executablePath: toolPath,
        result: ProcessResult(exitCode: 0, standardOutput: Data(), standardError: Data()))

    let muxer = FFmpegMuxer(processRunner: runner, toolLocator: ExternalToolLocator(searchPaths: [dir.path]))
    let video = URL(fileURLWithPath: "/tmp/video.mp4")
    let audio = URL(fileURLWithPath: "/tmp/audio.m4a")
    let output = URL(fileURLWithPath: "/tmp/output.mp4")
    try await muxer.mux(videoPath: video, audioPath: audio, outputPath: output)

    let invocations = await runner.recordedInvocations
    #expect(invocations.count == 1)
    #expect(invocations[0].arguments.contains(video.path))
    #expect(invocations[0].arguments.contains(audio.path))
    #expect(invocations[0].arguments.contains(output.path))
    #expect(invocations[0].arguments.contains("copy"))
}

@Test func muxThrowsToolMissingWhenFfmpegIsNotFound() async {
    let muxer = FFmpegMuxer(
        processRunner: FakeProcessRunner(), toolLocator: ExternalToolLocator(searchPaths: []))
    await #expect(throws: FFmpegMuxerError.toolMissing) {
        try await muxer.mux(
            videoPath: URL(fileURLWithPath: "/tmp/v"), audioPath: URL(fileURLWithPath: "/tmp/a"),
            outputPath: URL(fileURLWithPath: "/tmp/o"))
    }
}

@Test func muxThrowsProcessFailedOnNonZeroExit() async throws {
    let dir = try makeToolDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let toolPath = dir.appendingPathComponent("ffmpeg").path

    let runner = FakeProcessRunner()
    await runner.program(
        executablePath: toolPath,
        result: ProcessResult(
            exitCode: 1, standardOutput: Data(), standardError: Data("Invalid data".utf8)))

    let muxer = FFmpegMuxer(processRunner: runner, toolLocator: ExternalToolLocator(searchPaths: [dir.path]))
    do {
        try await muxer.mux(
            videoPath: URL(fileURLWithPath: "/tmp/v"), audioPath: URL(fileURLWithPath: "/tmp/a"),
            outputPath: URL(fileURLWithPath: "/tmp/o"))
        Issue.record("expected processFailed")
    } catch let FFmpegMuxerError.processFailed(exitCode, stderr) {
        #expect(exitCode == 1)
        #expect(stderr.contains("Invalid data"))
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter FFmpegMuxerTests`
Expected: FAIL — `cannot find 'FFmpegMuxer' in scope`.

- [ ] **Step 3: Implement `Muxer` and `FFmpegMuxer`**

`SDMKit/Sources/SDMCore/Muxer.swift`:

```swift
import Foundation

/// Combines a video-only and audio-only file into one output — spec §8's
/// muxing step. Lives in `SDMCore` (not `SDMResolver`) purely so `SDMEngine`
/// can accept `any Muxer` without depending on `SDMResolver` at all; the only
/// real implementation, `FFmpegMuxer`, lives in `SDMResolver`.
public protocol Muxer: Sendable {
    func mux(videoPath: URL, audioPath: URL, outputPath: URL) async throws
}
```

`SDMKit/Sources/SDMResolver/FFmpegMuxer.swift`:

```swift
import Foundation
import SDMCore

public enum FFmpegMuxerError: Error, Equatable {
    case toolMissing
    case processFailed(exitCode: Int32, stderr: String)
}

/// `ffmpeg -c copy` mux: no re-encoding, since the video and audio streams
/// are already in their final codecs — this only re-containers them together.
public struct FFmpegMuxer: Muxer {
    private let processRunner: any ProcessRunner
    private let toolLocator: ExternalToolLocator

    public init(processRunner: any ProcessRunner, toolLocator: ExternalToolLocator = ExternalToolLocator()) {
        self.processRunner = processRunner
        self.toolLocator = toolLocator
    }

    public func mux(videoPath: URL, audioPath: URL, outputPath: URL) async throws {
        guard let toolPath = toolLocator.locate(.ffmpeg) else {
            throw FFmpegMuxerError.toolMissing
        }
        let arguments = [
            "-y",
            "-i", videoPath.path,
            "-i", audioPath.path,
            "-c", "copy",
            outputPath.path,
        ]
        let result = try await processRunner.run(
            ProcessInvocation(executablePath: toolPath, arguments: arguments))
        guard result.exitCode == 0 else {
            throw FFmpegMuxerError.processFailed(
                exitCode: result.exitCode,
                stderr: String(decoding: result.standardError, as: UTF8.self))
        }
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `swift test --package-path SDMKit --filter FFmpegMuxerTests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add SDMKit/Sources/SDMCore/Muxer.swift SDMKit/Sources/SDMResolver/FFmpegMuxer.swift SDMKit/Tests/SDMResolverTests/FFmpegMuxerTests.swift
git commit -m "feat: add Muxer protocol and the FFmpegMuxer implementation"
```

---

### Task 10: `ResolverBinding`, `MuxCompanion`, and `DownloadItem`/`ResumeSidecar` provenance fields

**Files:**
- Create: `SDMKit/Sources/SDMCore/ResolverBinding.swift`
- Modify: `SDMKit/Sources/SDMCore/DownloadItem.swift`
- Modify: `SDMKit/Sources/SDMEngine/ResumeSidecar.swift`
- Create: `SDMKit/Tests/SDMCoreTests/ResolverBindingTests.swift`
- Modify: `SDMKit/Tests/SDMEngineTests/ResumeSidecarTests.swift` (or create if no such file exists — check `ls SDMKit/Tests/SDMEngineTests/` first; add to whichever sidecar-persistence test file already exists, following that file's existing style)

**Interfaces:**
- Consumes: nothing
- Produces:
  - `public struct ResolverBinding: Equatable, Codable, Sendable { public var extractor: String; public var sourceURL: URL; public var formatID: String }`
  - `public struct MuxCompanion: Equatable, Codable, Sendable { public enum Role: String, Codable, Sendable { case video, audio }; public var pairedItemID: UUID; public var role: Role; public var outputFilename: String }`
  - `DownloadItem.resolverBinding: ResolverBinding?`, `DownloadItem.muxCompanion: MuxCompanion?` (both default `nil`)
  - `ResumeSidecar.resolverBinding: ResolverBinding?` (default `nil`)

Spec §4.1: "the source URL and, for resolver-backed items, the video ID + format ID needed to refresh an expired URL" — `ResolverBinding` is exactly that triple (`sourceURL` doubling as "video ID" via the webpage URL, since yt-dlp re-resolves from the page URL, not a bare ID). Both new `DownloadItem` fields are `Optional` with a `nil` default, so every existing call site across the codebase (dozens, per the Phase 1–4 plans) keeps compiling unchanged.

- [ ] **Step 1: Write the failing test**

`SDMKit/Tests/SDMCoreTests/ResolverBindingTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMCore

@Test func downloadItemRoundTripsAResolverBindingThroughJSON() throws {
    let binding = ResolverBinding(
        extractor: "youtube", sourceURL: URL(string: "https://www.youtube.com/watch?v=abc123")!,
        formatID: "137")
    let item = DownloadItem(
        url: URL(string: "https://rr1.googlevideo.com/videoplayback?itag=137")!,
        filename: "video.mp4", resolverBinding: binding)

    let data = try JSONEncoder().encode(item)
    let decoded = try JSONDecoder().decode(DownloadItem.self, from: data)

    #expect(decoded.resolverBinding == binding)
}

@Test func downloadItemDefaultsResolverBindingAndMuxCompanionToNil() {
    let item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    #expect(item.resolverBinding == nil)
    #expect(item.muxCompanion == nil)
}

@Test func muxCompanionRoundTripsThroughJSON() throws {
    let pairedID = UUID()
    let companion = MuxCompanion(pairedItemID: pairedID, role: .video, outputFilename: "final.mp4")
    let item = DownloadItem(
        url: URL(string: "https://example.com/video-only.mp4")!, filename: "video-only.mp4",
        muxCompanion: companion)

    let data = try JSONEncoder().encode(item)
    let decoded = try JSONDecoder().decode(DownloadItem.self, from: data)

    #expect(decoded.muxCompanion?.pairedItemID == pairedID)
    #expect(decoded.muxCompanion?.role == .video)
    #expect(decoded.muxCompanion?.outputFilename == "final.mp4")
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter ResolverBindingTests`
Expected: FAIL — `cannot find 'ResolverBinding' in scope`, and `DownloadItem.init` does not accept `resolverBinding:`/`muxCompanion:`.

- [ ] **Step 3: Implement `ResolverBinding`/`MuxCompanion` and extend `DownloadItem`**

`SDMKit/Sources/SDMCore/ResolverBinding.swift`:

```swift
import Foundation

/// Spec §4.1: "for resolver-backed items, the video ID + format ID needed to
/// refresh an expired URL." `sourceURL` is the original page URL (e.g. a
/// YouTube watch URL) a `LinkResolver` re-resolves from — yt-dlp itself takes
/// a page URL, not a bare video ID, so storing the page URL is what actually
/// lets `URLRefresher` (see `SDMEngine`) ask for a fresh format table.
public struct ResolverBinding: Equatable, Codable, Sendable {
    public var extractor: String
    public var sourceURL: URL
    public var formatID: String

    public init(extractor: String, sourceURL: URL, formatID: String) {
        self.extractor = extractor
        self.sourceURL = sourceURL
        self.formatID = formatID
    }
}

/// Spec §8: "Video and audio download as two items bound to one output, then
/// mux via ffmpeg." Each of the pair carries a `MuxCompanion` pointing at the
/// other; once both items reach `.completed`, `DownloadEngine` invokes the
/// injected `Muxer` to combine them into `outputFilename`.
public struct MuxCompanion: Equatable, Codable, Sendable {
    public enum Role: String, Codable, Sendable {
        case video
        case audio
    }

    public var pairedItemID: UUID
    public var role: Role
    public var outputFilename: String

    public init(pairedItemID: UUID, role: Role, outputFilename: String) {
        self.pairedItemID = pairedItemID
        self.role = role
        self.outputFilename = outputFilename
    }
}
```

Modify `SDMKit/Sources/SDMCore/DownloadItem.swift` — add the two new stored properties after `validator`, and thread them through `init`:

```swift
    /// Server validator captured at download start, used to detect a changed remote file.
    public var validator: String?
    /// Present only for items whose URL came from a `LinkResolver` (Phase 5) —
    /// what to ask the resolver to re-resolve if this URL 403s from expiry.
    public var resolverBinding: ResolverBinding?
    /// Present only for one half of a video/audio pair awaiting muxing
    /// (Phase 5) — see `MuxCompanion`'s doc comment.
    public var muxCompanion: MuxCompanion?

    public init(
        id: UUID = UUID(),
        url: URL,
        filename: String,
        totalBytes: Int64? = nil,
        completed: RangeSet = RangeSet(),
        state: ItemState = .queued,
        isEnabled: Bool = true,
        isResumable: Bool? = nil,
        priority: Priority? = nil,
        position: Int = 0,
        validator: String? = nil,
        resolverBinding: ResolverBinding? = nil,
        muxCompanion: MuxCompanion? = nil
    ) {
        precondition(!filename.isEmpty, "filename must not be empty")
        self.id = id
        self.url = url
        self.filename = filename
        self.totalBytes = totalBytes
        self.completed = completed
        self.state = state
        self.isEnabled = isEnabled
        self.isResumable = isResumable
        self.priority = priority
        self.position = position
        self.validator = validator
        self.resolverBinding = resolverBinding
        self.muxCompanion = muxCompanion
    }
```

Modify `SDMKit/Sources/SDMEngine/ResumeSidecar.swift` — add the field after `completed`, defaulted so a sidecar written by an earlier build still decodes (the field is simply absent → `nil`, `Codable`'s standard behavior for a missing key against an `Optional` property):

```swift
    public var completed: RangeSet
    /// Carried so a relaunch mid-download of a resolver-backed item still
    /// knows what to ask `URLRefresher` for, without re-reading it off the
    /// (by-then-detached) in-memory `DownloadItem`.
    public var resolverBinding: ResolverBinding?

    public init(
        formatVersion: Int = ResumeSidecar.currentFormatVersion,
        sourceURL: URL,
        totalBytes: Int64,
        validator: String?,
        completed: RangeSet,
        resolverBinding: ResolverBinding? = nil
    ) {
        self.formatVersion = formatVersion
        self.sourceURL = sourceURL
        self.totalBytes = totalBytes
        self.validator = validator
        self.completed = completed
        self.resolverBinding = resolverBinding
    }
```

Add a decode-compatibility test to whichever file already tests `ResumeSidecar` persistence — run `ls SDMKit/Tests/SDMEngineTests/ | grep -i sidecar` to find it, then append:

```swift
@Test func decodesAnOlderSidecarThatPredatesTheResolverBindingField() throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let url = dir.appendingPathComponent("old.sdmpart")

    // Hand-written JSON matching the pre-Phase-5 shape: no "resolverBinding" key at all.
    let json = """
        {
            "formatVersion": 1,
            "sourceURL": "https://example.com/a.bin",
            "totalBytes": 1000,
            "validator": "etag-1",
            "completed": []
        }
        """
    try Data(json.utf8).write(to: url)

    let sidecar = try #require(ResumeSidecar.load(from: url))
    #expect(sidecar.resolverBinding == nil)
    #expect(sidecar.totalBytes == 1000)
}
```

If `RangeSet`'s JSON shape is not a bare `[]` (check `SDMKit/Sources/SDMCore/RangeSet.swift`'s `Codable` conformance before pasting this literally), adjust the `"completed"` value in the JSON literal to match whatever `RangeSet`'s empty-set encoding actually is — verify with a quick throwaway `print(String(decoding: try! JSONEncoder().encode(RangeSet()), as: UTF8.self))` before writing the fixture literal, since guessing wrong here would make the test fail for the wrong reason (a `RangeSet` decode error) rather than testing what it's meant to.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `swift test --package-path SDMKit --filter ResolverBindingTests`
Run: `swift test --package-path SDMKit --filter SDMEngineTests` (broad, to catch the sidecar-compatibility test plus confirm nothing else broke from the `DownloadItem`/`ResumeSidecar` signature changes)
Expected: PASS on both.

- [ ] **Step 5: Commit**

```bash
git add SDMKit/Sources/SDMCore/ResolverBinding.swift SDMKit/Sources/SDMCore/DownloadItem.swift SDMKit/Sources/SDMEngine/ResumeSidecar.swift SDMKit/Tests/SDMCoreTests/ResolverBindingTests.swift
git add -u SDMKit/Tests/SDMEngineTests
git commit -m "feat: add ResolverBinding/MuxCompanion and thread them through DownloadItem/ResumeSidecar"
```

---

### Task 11: `URLRefresher` and the 403-refresh retry path

**Files:**
- Create: `SDMKit/Sources/SDMEngine/URLRefresher.swift`
- Modify: `SDMKit/Sources/SDMEngine/DownloadEngine.swift`
- Create: `SDMKit/Tests/SDMEngineTests/URLRefreshOn403Tests.swift`

**Interfaces:**
- Consumes: `ResolverBinding` (Task 10), `TransportError` (existing)
- Produces: `public protocol URLRefresher: Sendable { func refreshURL(for binding: ResolverBinding) async throws -> URL }`; `DownloadEngine.init` gains `urlRefresher: (any URLRefresher)? = nil`

This closes the gap named in Phase 1's own "Deferred to later phases" section: *"Signed-URL refresh on 403 (§5.3) — RetryPolicy already classifies 403 as transient, which is the hook, but there is no resolver to refresh from yet."* `URLRefresher` lives in `SDMEngine`, not `SDMResolver` — see Task 9's rationale for `Muxer`, identical reasoning: `DownloadEngine` accepts the protocol, the `SDM` app target wires the concrete `YouTubeResolver`-backed adapter (Task 19).

- [ ] **Step 1: Write the failing test**

`SDMKit/Tests/SDMEngineTests/URLRefreshOn403Tests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMEngine

/// Mirrors `EngineRetryTests.swift`'s local helpers — kept file-local rather
/// than shared, matching that file's own precedent (its `pump`/`makeEngine`
/// are private to it too).
private func pump(_ engine: DownloadEngine, ticks count: Int) async throws {
    for _ in 0..<count {
        await engine.tick()
        try await engine.runUntilIdle()
    }
}

private actor FakeURLRefresher: URLRefresher {
    private let origin: FakeOrigin
    private let refreshedURL: URL
    private(set) var refreshCount = 0

    init(origin: FakeOrigin, refreshedURL: URL) {
        self.origin = origin
        self.refreshedURL = refreshedURL
    }

    func refreshURL(for binding: ResolverBinding) async throws -> URL {
        refreshCount += 1
        // Simulates "the newly-resolved URL is not expired": the real
        // `URLRefresher` adapter would call `YouTubeResolver.resolve(_:)`
        // again and hand back a fresh signed URL; `FakeOrigin` does not
        // actually key its behavior off the request URL, so clearing its
        // `statusOverride` here is what stands in for that URL now working.
        await origin.setBehavior(FakeOrigin.Behavior())
        return refreshedURL
    }
}

private struct RefreshFailed: Error {}

private actor AlwaysFailingRefresher: URLRefresher {
    private(set) var callCount = 0
    func refreshURL(for binding: ResolverBinding) async throws -> URL {
        callCount += 1
        throw RefreshFailed()
    }
}

@Test func aResolverBoundItemRefreshesItsURLOn403AndCompletes() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let payload = testPayload(2000)

    var behavior = FakeOrigin.Behavior()
    behavior.statusOverride = 403
    let origin = FakeOrigin(payload: payload, behavior: behavior)
    let refresher = FakeURLRefresher(
        origin: origin, refreshedURL: URL(string: "https://example.com/a-refreshed.bin")!)

    let engine = DownloadEngine(
        transport: origin,
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir),
        retryPolicy: RetryPolicy(maxAttempts: 5, baseDelay: .seconds(2)),
        urlRefresher: refresher
    )

    let binding = ResolverBinding(
        extractor: "youtube", sourceURL: URL(string: "https://www.youtube.com/watch?v=abc")!,
        formatID: "18")
    let item = DownloadItem(
        url: URL(string: "https://example.com/a.bin")!, filename: "a.bin", resolverBinding: binding)
    let itemID = item.id
    await engine.add(DownloadPackage(name: "Batch", items: [item]))
    try await engine.runUntilIdle()

    // The refresh fires from within the very attempt that saw the 403, not
    // after further backoff.
    #expect(await refresher.refreshCount == 1)
    let refreshedURL = await engine.snapshot().packages.flatMap(\.items)
        .first { $0.id == itemID }?.url
    #expect(refreshedURL == URL(string: "https://example.com/a-refreshed.bin")!)

    try await pump(engine, ticks: 5 * AppTiming.ticksPerSecond)

    let finalState = await engine.snapshot().packages.flatMap(\.items)
        .first { $0.id == itemID }?.state
    #expect(finalState == .completed)
    let destination = dir.appendingPathComponent("Batch").appendingPathComponent("a.bin")
    #expect(try Data(contentsOf: destination) == payload)
}

@Test func aFailedRefreshFallsBackToOrdinaryBackoffInsteadOfCrashing() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    var behavior = FakeOrigin.Behavior()
    behavior.statusOverride = 403
    let origin = FakeOrigin(payload: testPayload(1000), behavior: behavior)
    let refresher = AlwaysFailingRefresher()

    let engine = DownloadEngine(
        transport: origin,
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir),
        retryPolicy: RetryPolicy(maxAttempts: 5, baseDelay: .seconds(2)),
        urlRefresher: refresher
    )

    let binding = ResolverBinding(
        extractor: "youtube", sourceURL: URL(string: "https://www.youtube.com/watch?v=abc")!,
        formatID: "18")
    let item = DownloadItem(
        url: URL(string: "https://example.com/a.bin")!, filename: "a.bin", resolverBinding: binding)
    let itemID = item.id
    await engine.add(DownloadPackage(name: "Batch", items: [item]))
    try await engine.runUntilIdle()

    #expect(await refresher.callCount == 1)
    let state = await engine.snapshot().packages.flatMap(\.items).first { $0.id == itemID }?.state
    #expect(state == .queued)
}

@Test func anItemWithNoResolverBindingIgnoresTheRefresherEntirely() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }

    var behavior = FakeOrigin.Behavior()
    behavior.statusOverride = 403
    let origin = FakeOrigin(payload: testPayload(1000), behavior: behavior)
    let refresher = AlwaysFailingRefresher()

    let engine = DownloadEngine(
        transport: origin,
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir),
        retryPolicy: RetryPolicy(maxAttempts: 5, baseDelay: .seconds(2)),
        urlRefresher: refresher
    )

    // No `resolverBinding` — a plain generic HTTP download that happens to
    // 403 must not touch the refresher at all.
    let item = DownloadItem(url: URL(string: "https://example.com/a.bin")!, filename: "a.bin")
    await engine.add(DownloadPackage(name: "Batch", items: [item]))
    try await engine.runUntilIdle()

    #expect(await refresher.callCount == 0)
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter URLRefreshOn403Tests`
Expected: FAIL — `cannot find 'URLRefresher' in scope`, and `DownloadEngine.init` does not accept `urlRefresher:`.

- [ ] **Step 3: Add `URLRefresher` and wire it into `DownloadEngine`**

`SDMKit/Sources/SDMEngine/URLRefresher.swift`:

```swift
import SDMCore
import Foundation

/// Refreshes an expired signed URL for a resolver-bound item. Spec §5.3: "a
/// 403 mid-download triggers the resolver to refresh the URL for the stored
/// video ID + format ID, after which workers resume against the existing
/// set." Lives in `SDMEngine`, not `SDMResolver` — see `Muxer`'s doc comment
/// in `SDMCore` for the identical layering reason.
public protocol URLRefresher: Sendable {
    func refreshURL(for binding: ResolverBinding) async throws -> URL
}
```

Modify `SDMKit/Sources/SDMEngine/DownloadEngine.swift`. First, the stored property and `init` (around line 132 and 164):

```swift
    private let retryPolicy: RetryPolicy
    /// Spec §5.3: refreshes an item's URL on a 403 rather than retrying the
    /// same expired one forever. `nil` for an engine with no resolver wired
    /// up (every existing test, and any item with no `resolverBinding`).
    private let urlRefresher: (any URLRefresher)?
```

```swift
    public init(
        transport: any HTTPTransport,
        stateStore: any StateStore,
        settings: EngineSettings,
        retryPolicy: RetryPolicy = RetryPolicy(),
        urlRefresher: (any URLRefresher)? = nil
    ) {
        self.transport = transport
        self.stateStore = stateStore
        self.settings = settings
        self.retryPolicy = retryPolicy
        self.urlRefresher = urlRefresher
    }
```

Then the two call sites of `failureState`, which becomes `async`. In `reconcile()`:

```swift
            } catch {
                // Creating the package folder failed. Left as `try?` this was
                // invisible: `SparseFile` then failed to open, the failure
                // classified transient, and the item re-attempted once a
                // second forever with nothing anywhere saying why.
                let landing = await failureState(for: error, itemID: itemID)
                mutateItem(itemID) { $0.state = landing }
                failedAttempts[itemID] = nil
                changed = true
                continue
            }
```

In `run()`:

```swift
            case .none:
                let progressed =
                    await task.completedRanges.totalBytes > (attemptStartBytes[itemID] ?? 0)
                state = await failureState(for: error, itemID: itemID, madeProgress: progressed)
            }
```

Finally, `failureState` itself gains the refresh branch and becomes `async`:

```swift
    private func failureState(
        for error: any Error,
        itemID: UUID,
        madeProgress: Bool = false
    ) async -> ItemState {
        if case .permanent(let reason) = retryPolicy.classify(error) {
            failedAttempts[itemID] = nil
            retryHoldTicks[itemID] = nil
            return .failed(reason: reason)
        }

        if isExpiredSignedURL(error), let urlRefresher, let binding = resolverBinding(for: itemID) {
            if let refreshed = try? await urlRefresher.refreshURL(for: binding) {
                mutateItem(itemID) { $0.url = refreshed }
                failedAttempts[itemID] = nil
                // A minimal hold rather than zero: `reconcile()` runs
                // synchronously right after this returns and would otherwise
                // immediately re-desire the item inside the very call that
                // is still unwinding its retiring runner for the same slot.
                retryHoldTicks[itemID] = 1
                return .queued
            }
            // Refresh itself failed (network hiccup, tool missing, video
            // pulled) — fall through to the ordinary transient path below,
            // exactly as if no refresher were configured at all.
        }

        if madeProgress { failedAttempts[itemID] = nil }
        let attempt = (failedAttempts[itemID] ?? 0) + 1
        failedAttempts[itemID] = attempt

        guard attempt < retryPolicy.maxAttempts else {
            failedAttempts[itemID] = nil
            retryHoldTicks[itemID] = nil
            return .failed(
                reason:
                    "Gave up after \(attempt) attempts: \(Self.describe(error))"
            )
        }

        // `delay(forAttempt:)` is seconds; scale by the heartbeat rate to
        // get ticks. At least one tick, so a sub-second backoff still costs
        // a beat rather than re-attempting immediately.
        let seconds = retryPolicy.delay(forAttempt: attempt - 1).components.seconds
        retryHoldTicks[itemID] = Swift.max(1, Int(seconds) * AppTiming.ticksPerSecond)
        return .queued
    }

    private func isExpiredSignedURL(_ error: any Error) -> Bool {
        guard let transportError = error as? TransportError, case .http(let status) = transportError
        else { return false }
        return status == 403
    }

    private func resolverBinding(for itemID: UUID) -> ResolverBinding? {
        for package in packages {
            if let item = package.items.first(where: { $0.id == itemID }) {
                return item.resolverBinding
            }
        }
        return nil
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `swift test --package-path SDMKit --filter URLRefreshOn403Tests`
Run: `swift test --package-path SDMKit --filter SDMEngineTests` (confirm the `failureState` signature change did not break any existing retry test)
Expected: PASS on both.

- [ ] **Step 5: Commit**

```bash
git add SDMKit/Sources/SDMEngine/URLRefresher.swift SDMKit/Sources/SDMEngine/DownloadEngine.swift SDMKit/Tests/SDMEngineTests/URLRefreshOn403Tests.swift
git commit -m "feat: refresh a resolver-bound item's URL on 403 instead of retrying it forever"
```

---

### Task 12: Mux-on-completion in `DownloadEngine`

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/DownloadEngine.swift`
- Create: `SDMKit/Tests/SDMEngineTests/MuxOnCompletionTests.swift`

**Interfaces:**
- Consumes: `Muxer` (Task 9), `MuxCompanion` (Task 10)
- Produces: `DownloadEngine.init` gains `muxer: (any Muxer)? = nil`

When both halves of a `MuxCompanion` pair reach `.completed`, the engine invokes the injected `Muxer`, replaces the video item's file with the muxed output, and removes the now-redundant audio item from its package — spec §8: "Video and audio download as two items bound to one output."

- [ ] **Step 1: Write the failing test**

`SDMKit/Tests/SDMEngineTests/MuxOnCompletionTests.swift`:

```swift
import Foundation
import SDMCore
import Testing

@testable import SDMEngine

private func pump(_ engine: DownloadEngine, ticks count: Int) async throws {
    for _ in 0..<count {
        await engine.tick()
        try await engine.runUntilIdle()
    }
}

private actor FakeMuxer: Muxer {
    struct Invocation: Equatable {
        let videoPath: URL
        let audioPath: URL
        let outputPath: URL
    }
    private(set) var invocations: [Invocation] = []

    func mux(videoPath: URL, audioPath: URL, outputPath: URL) async throws {
        invocations.append(Invocation(videoPath: videoPath, audioPath: audioPath, outputPath: outputPath))
        try Data("muxed-output".utf8).write(to: outputPath)
    }
}

@Test func bothHalvesCompletingTriggersAMuxAndCollapsesToOneItem() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let origin = FakeOrigin(payload: testPayload(500))
    let muxer = FakeMuxer()

    let engine = DownloadEngine(
        transport: origin,
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 2, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir),
        muxer: muxer
    )

    let videoID = UUID()
    let audioID = UUID()
    let videoItem = DownloadItem(
        id: videoID, url: URL(string: "https://example.com/video-only.mp4")!,
        filename: "video-only.mp4",
        muxCompanion: MuxCompanion(pairedItemID: audioID, role: .video, outputFilename: "Final.mp4"))
    let audioItem = DownloadItem(
        id: audioID, url: URL(string: "https://example.com/audio-only.m4a")!,
        filename: "audio-only.m4a",
        muxCompanion: MuxCompanion(pairedItemID: videoID, role: .audio, outputFilename: "Final.mp4"))

    await engine.add(DownloadPackage(name: "Batch", items: [videoItem, audioItem]))
    try await pump(engine, ticks: 3)

    let items = await engine.snapshot().packages.first { $0.name == "Batch" }?.items ?? []
    #expect(items.count == 1)
    #expect(items.first?.filename == "Final.mp4")
    #expect(items.first?.muxCompanion == nil)

    let invocations = await muxer.invocations
    #expect(invocations.count == 1)
    #expect(invocations[0].videoPath.lastPathComponent == "video-only.mp4")
    #expect(invocations[0].audioPath.lastPathComponent == "audio-only.m4a")
    #expect(invocations[0].outputPath.lastPathComponent == "Final.mp4")

    let finalContents = try Data(
        contentsOf: dir.appendingPathComponent("Batch").appendingPathComponent("Final.mp4"))
    #expect(finalContents == Data("muxed-output".utf8))
}

@Test func aVideoOnlyItemWithNoMuxCompanionIsUnaffected() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let origin = FakeOrigin(payload: testPayload(200))
    let muxer = FakeMuxer()

    let engine = DownloadEngine(
        transport: origin,
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 1, globalMaxConnections: 8, downloadFolder: dir),
        muxer: muxer
    )
    let item = DownloadItem(url: URL(string: "https://example.com/plain.bin")!, filename: "plain.bin")
    await engine.add(DownloadPackage(name: "Batch", items: [item]))
    try await pump(engine, ticks: 3)

    #expect(await muxer.invocations.isEmpty)
    let items = await engine.snapshot().packages.first { $0.name == "Batch" }?.items ?? []
    #expect(items.count == 1)
    #expect(items.first?.filename == "plain.bin")
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter MuxOnCompletionTests`
Expected: FAIL — `DownloadEngine.init` does not accept `muxer:`.

- [ ] **Step 3: Wire the `muxer` and the completion hook**

Modify `SDMKit/Sources/SDMEngine/DownloadEngine.swift`. Stored property, next to `urlRefresher`:

```swift
    private let urlRefresher: (any URLRefresher)?
    /// Spec §8: invoked once both halves of a `MuxCompanion` pair reach
    /// `.completed`. `nil` for an engine with no resolver support wired up.
    private let muxer: (any Muxer)?
```

`init`:

```swift
    public init(
        transport: any HTTPTransport,
        stateStore: any StateStore,
        settings: EngineSettings,
        retryPolicy: RetryPolicy = RetryPolicy(),
        urlRefresher: (any URLRefresher)? = nil,
        muxer: (any Muxer)? = nil
    ) {
        self.transport = transport
        self.stateStore = stateStore
        self.settings = settings
        self.retryPolicy = retryPolicy
        self.urlRefresher = urlRefresher
        self.muxer = muxer
    }
```

In `run(itemID:task:)`, right after the existing `finish(...)` call and before `await persist()`:

```swift
        finish(
            itemID: itemID,
            task: task,
            completed: completed,
            totalBytes: totalBytes,
            isResumable: isResumable,
            state: state
        )
        await handleMuxIfNeeded(itemID: itemID)
        await persist()
    }
```

New private methods, placed after `finish(...)` in the "Failure handling"-adjacent section:

```swift
    /// Invoked after every runner retires. A no-op unless this item just
    /// landed `.completed` with a `MuxCompanion` whose paired item has
    /// *also* already reached `.completed` — the second of the pair to
    /// finish is therefore always the one that actually performs the mux;
    /// the first one's own call here finds the pair still incomplete and
    /// returns immediately. Only the `.video` half proceeds even once both
    /// are ready, so a pair that lands in the same tick is not muxed twice.
    private func handleMuxIfNeeded(itemID: UUID) async {
        guard let muxer,
            let item = currentItem(itemID),
            item.state == .completed,
            let companion = item.muxCompanion,
            companion.role == .video,
            let pairedItem = currentItem(companion.pairedItemID),
            pairedItem.state == .completed,
            let package = packages.first(where: { $0.items.contains { $0.id == itemID } })
        else { return }

        let folder = settings.downloadFolder.appendingPathComponent(package.name)
        let videoPath = folder.appendingPathComponent(item.filename)
        let audioPath = folder.appendingPathComponent(pairedItem.filename)
        let outputPath = folder.appendingPathComponent(companion.outputFilename)

        do {
            try await muxer.mux(videoPath: videoPath, audioPath: audioPath, outputPath: outputPath)
        } catch {
            // Left as two separate, still-`.completed` items with their own
            // raw streams on disk — a mux failure should not masquerade as a
            // download failure, since both downloads genuinely succeeded.
            checkpointFailures[itemID] = "Mux failed: \(error)"
            return
        }

        try? FileManager.default.removeItem(at: videoPath)
        try? FileManager.default.trashItem(at: audioPath, resultingItemURL: nil)
        mutateItem(itemID) {
            $0.filename = companion.outputFilename
            $0.muxCompanion = nil
        }
        removeCompanionItem(pairedItem.id, inPackage: package.id)
    }

    private func currentItem(_ itemID: UUID) -> DownloadItem? {
        for package in packages {
            if let item = package.items.first(where: { $0.id == itemID }) { return item }
        }
        return nil
    }

    /// Drops the now-redundant audio half of a muxed pair from its package,
    /// including its retry/telemetry bookkeeping — mirrors what
    /// `removeItem(_:deleteFile:)` does for a user-initiated removal, minus
    /// the file deletion (already handled by the caller) and the
    /// persist/reconcile calls (the caller's own `handleMuxIfNeeded` runs
    /// inside `run()`, which already persists right after).
    private func removeCompanionItem(_ itemID: UUID, inPackage packageID: UUID) {
        guard let packageIndex = packages.firstIndex(where: { $0.id == packageID }) else { return }
        packages[packageIndex].items.removeAll { $0.id == itemID }
        clearItemBookkeeping(itemID)
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `swift test --package-path SDMKit --filter MuxOnCompletionTests`
Run: `swift test --package-path SDMKit --filter SDMEngineTests`
Expected: PASS on both.

- [ ] **Step 5: Commit**

```bash
git add SDMKit/Sources/SDMEngine/DownloadEngine.swift SDMKit/Tests/SDMEngineTests/MuxOnCompletionTests.swift
git commit -m "feat: mux video/audio companion items once both complete"
```

---

### Task 13: `YouTubeHandoff` — pure `ResolvedMedia` → `[DownloadItem]` conversion

**Files:**
- Create: `SDMKit/Sources/SDMResolver/YouTubeHandoff.swift`
- Create: `SDMKit/Tests/SDMResolverTests/YouTubeHandoffTests.swift`

**Interfaces:**
- Consumes: `ResolvedMedia`, `YtDlpFormat`, `MuxPlanner` (Tasks 6, 8), `DownloadItem`, `ResolverBinding`, `MuxCompanion` (`SDMCore`, Task 10)
- Produces: `public enum YouTubeHandoff { public static func makeDownloadItems(for media: ResolvedMedia, selectedFormatID: String) -> [DownloadItem] }`

The one pure function that turns a resolved video plus a chosen format ID into what `DownloadEngine.add(_:)` actually consumes — one `DownloadItem` for a progressive/audio-inclusive pick, two `MuxCompanion`-bound items for a video-only pick. Kept pure and directly testable (no actor, no `EngineController`) rather than folded into the app-layer wiring in Task 19.

- [ ] **Step 1: Write the failing test**

`SDMKit/Tests/SDMResolverTests/YouTubeHandoffTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMResolver

private func format(
    id: String, ext: String = "mp4", vcodec: String = "avc1.640028", acodec: String = "mp4a.40.2",
    tbr: Double = 100
) -> YtDlpFormat {
    YtDlpFormat(
        formatID: id, ext: ext, vcodec: vcodec, acodec: acodec, protocolName: "https", width: nil,
        height: nil, filesize: nil, filesizeApprox: nil, formatNote: nil, tbr: tbr,
        url: URL(string: "https://example.com/\(id)")!)
}

private let sourceURL = URL(string: "https://www.youtube.com/watch?v=abc123")!

@Test func aProgressiveSelectionProducesOneItemWithAResolverBinding() {
    let progressive = format(id: "18")
    let media = ResolvedMedia(
        extractor: "youtube", mediaID: "abc123", title: "My Video", formats: [progressive],
        sourceURL: sourceURL)

    let items = YouTubeHandoff.makeDownloadItems(for: media, selectedFormatID: "18")

    #expect(items.count == 1)
    #expect(items[0].url == progressive.url)
    #expect(items[0].filename == "My Video.mp4")
    #expect(items[0].resolverBinding?.extractor == "youtube")
    #expect(items[0].resolverBinding?.sourceURL == sourceURL)
    #expect(items[0].resolverBinding?.formatID == "18")
    #expect(items[0].muxCompanion == nil)
}

@Test func aVideoOnlySelectionWithAnAvailableAudioTrackProducesTwoBoundItems() {
    let video = format(id: "137", vcodec: "avc1.640028", acodec: "none")
    let audio = format(id: "140", vcodec: "none", acodec: "mp4a.40.2")
    let media = ResolvedMedia(
        extractor: "youtube", mediaID: "abc123", title: "My Video", formats: [video, audio],
        sourceURL: sourceURL)

    let items = YouTubeHandoff.makeDownloadItems(for: media, selectedFormatID: "137")

    #expect(items.count == 2)
    let videoItem = try! #require(items.first { $0.resolverBinding?.formatID == "137" })
    let audioItem = try! #require(items.first { $0.resolverBinding?.formatID == "140" })

    #expect(videoItem.muxCompanion?.role == .video)
    #expect(audioItem.muxCompanion?.role == .audio)
    #expect(videoItem.muxCompanion?.pairedItemID == audioItem.id)
    #expect(audioItem.muxCompanion?.pairedItemID == videoItem.id)
    #expect(videoItem.muxCompanion?.outputFilename == "My Video.mp4")
    #expect(audioItem.muxCompanion?.outputFilename == "My Video.mp4")
    #expect(videoItem.url == video.url)
    #expect(audioItem.url == audio.url)
}

@Test func aVideoOnlySelectionWithNoAudioTrackProducesOneSilentItem() {
    let video = format(id: "137", vcodec: "avc1.640028", acodec: "none")
    let media = ResolvedMedia(
        extractor: "youtube", mediaID: "abc123", title: "My Video", formats: [video],
        sourceURL: sourceURL)

    let items = YouTubeHandoff.makeDownloadItems(for: media, selectedFormatID: "137")

    #expect(items.count == 1)
    #expect(items[0].muxCompanion == nil)
}

@Test func anUnknownFormatIDProducesNoItems() {
    let media = ResolvedMedia(
        extractor: "youtube", mediaID: "abc123", title: "My Video", formats: [format(id: "18")],
        sourceURL: sourceURL)
    #expect(YouTubeHandoff.makeDownloadItems(for: media, selectedFormatID: "999").isEmpty)
}

@Test func filesystemUnsafeCharactersInTheTitleAreSanitized() {
    let media = ResolvedMedia(
        extractor: "youtube", mediaID: "abc123", title: "Part 1/2: The Sequel",
        formats: [format(id: "18")], sourceURL: sourceURL)
    let items = YouTubeHandoff.makeDownloadItems(for: media, selectedFormatID: "18")
    #expect(items[0].filename == "Part 1-2- The Sequel.mp4")
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter YouTubeHandoffTests`
Expected: FAIL — `cannot find 'YouTubeHandoff' in scope`.

- [ ] **Step 3: Implement `YouTubeHandoff`**

`SDMKit/Sources/SDMResolver/YouTubeHandoff.swift`:

```swift
import Foundation
import SDMCore

/// Converts a resolved video plus a chosen format into what
/// `DownloadEngine.add(_:)` consumes. The one impure decision this makes —
/// generating fresh `UUID`s for a muxed pair — is why this returns
/// `[DownloadItem]` rather than being folded into `MuxPlanner`, which stays
/// free of identity concerns entirely.
public enum YouTubeHandoff {
    public static func makeDownloadItems(for media: ResolvedMedia, selectedFormatID: String)
        -> [DownloadItem]
    {
        guard let selected = media.formats.first(where: { $0.formatID == selectedFormatID }) else {
            return []
        }
        let title = sanitized(media.title)
        let plan = MuxPlanner.plan(selectedVideo: selected, availableFormats: media.formats)

        guard let audio = plan.audioFormat else {
            let filename = "\(title).\(selected.ext)"
            return [
                DownloadItem(
                    url: selected.url, filename: filename,
                    resolverBinding: binding(for: selected, media: media))
            ]
        }

        let outputFilename = "\(title).\(selected.ext)"
        let videoID = UUID()
        let audioID = UUID()
        let videoItem = DownloadItem(
            id: videoID, url: selected.url, filename: "\(title) (video).\(selected.ext)",
            resolverBinding: binding(for: selected, media: media),
            muxCompanion: MuxCompanion(pairedItemID: audioID, role: .video, outputFilename: outputFilename))
        let audioItem = DownloadItem(
            id: audioID, url: audio.url, filename: "\(title) (audio).\(audio.ext)",
            resolverBinding: binding(for: audio, media: media),
            muxCompanion: MuxCompanion(pairedItemID: videoID, role: .audio, outputFilename: outputFilename))
        return [videoItem, audioItem]
    }

    private static func binding(for format: YtDlpFormat, media: ResolvedMedia) -> ResolverBinding {
        ResolverBinding(extractor: media.extractor, sourceURL: media.sourceURL, formatID: format.formatID)
    }

    /// Replaces path-separator characters a title could plausibly contain
    /// (`/` splits into subdirectories, `:` is a legacy HFS+ path separator
    /// still worth avoiding) so the title is always safe as a single path
    /// component.
    private static func sanitized(_ title: String) -> String {
        title.replacingOccurrences(of: "/", with: "-").replacingOccurrences(of: ":", with: "-")
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `swift test --package-path SDMKit --filter YouTubeHandoffTests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add SDMKit/Sources/SDMResolver/YouTubeHandoff.swift SDMKit/Tests/SDMResolverTests/YouTubeHandoffTests.swift
git commit -m "feat: add YouTubeHandoff to convert resolved media into download items"
```

---

### Task 14: `WholesaleDownloader` — the HLS/DASH-manifest fallback path

**Files:**
- Create: `SDMKit/Sources/SDMResolver/StreamingProcessRunner.swift`
- Create: `SDMKit/Sources/SDMResolver/YtDlpProgressParser.swift`
- Create: `SDMKit/Sources/SDMResolver/WholesaleDownloader.swift`
- Create: `SDMKit/Tests/SDMResolverTests/YtDlpProgressParserTests.swift`
- Create: `SDMKit/Tests/SDMResolverTests/WholesaleDownloaderTests.swift`

**Interfaces:**
- Consumes: `CookiesSource`, `ExternalToolLocator` (Tasks 2, 4)
- Produces:
  - `public protocol StreamingProcessRunner: Sendable { func run(_ invocation: ProcessInvocation) -> AsyncThrowingStream<String, any Error> }` and a `FakeStreamingProcessRunner`/`RealStreamingProcessRunner` pair
  - `public enum YtDlpProgressParser { public static func parseFraction(from line: String) -> Double? }`
  - `public protocol WholesaleDownloader: Sendable { func download(sourceURL: URL, cookiesSource: CookiesSource, outputPath: URL, onProgress: @escaping @Sendable (Double) -> Void) async throws }`, `public struct YtDlpWholesaleDownloader: WholesaleDownloader`

Spec §8's degraded path: "formats available only as HLS/DASH manifests with no single direct URL are handed to yt-dlp to download wholesale, with progress parsed from its output." This needs a *streaming* process runner — unlike every other yt-dlp/ffmpeg invocation in this plan, which just runs to completion and reads one final result, a wholesale download can take minutes and must report intermediate progress. `ProcessRunner` (Task 3) stays a clean run-to-completion abstraction for the fast calls (`-J` extraction, muxing); this is a separate, narrower protocol used only here.

- [ ] **Step 1: Write the failing test for the pure progress parser**

`SDMKit/Tests/SDMResolverTests/YtDlpProgressParserTests.swift`:

```swift
import Testing

@testable import SDMResolver

@Test func parsesAMidProgressLine() {
    let line = "[download]  42.5% of   10.00MiB at    1.23MiB/s ETA 00:05"
    #expect(YtDlpProgressParser.parseFraction(from: line) == 0.425)
}

@Test func parsesACompleteLine() {
    let line = "[download] 100.0% of 10.00MiB in 00:08"
    #expect(YtDlpProgressParser.parseFraction(from: line) == 1.0)
}

@Test func ignoresNonProgressLines() {
    #expect(YtDlpProgressParser.parseFraction(from: "[download] Destination: file.mp4") == nil)
    #expect(YtDlpProgressParser.parseFraction(from: "[Merger] Merging formats into \"file.mkv\"") == nil)
    #expect(YtDlpProgressParser.parseFraction(from: "") == nil)
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter YtDlpProgressParserTests`
Expected: FAIL — `cannot find 'YtDlpProgressParser' in scope`.

- [ ] **Step 3: Implement `YtDlpProgressParser`**

`SDMKit/Sources/SDMResolver/YtDlpProgressParser.swift`:

```swift
/// Parses one line of `yt-dlp --newline` output into a 0...1 completion
/// fraction, or `nil` for any line that is not a progress update — yt-dlp
/// interleaves plenty of others (destination, merging, warnings).
public enum YtDlpProgressParser {
    public static func parseFraction(from line: String) -> Double? {
        guard line.hasPrefix("[download]") else { return nil }
        guard let percentRange = line.range(of: "%") else { return nil }
        let beforePercent = line[..<percentRange.lowerBound]
        let numberCharacters = beforePercent.reversed().prefix { $0.isNumber || $0 == "." }
        let numberString = String(numberCharacters.reversed())
        guard let value = Double(numberString) else { return nil }
        return value / 100
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `swift test --package-path SDMKit --filter YtDlpProgressParserTests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add SDMKit/Sources/SDMResolver/YtDlpProgressParser.swift SDMKit/Tests/SDMResolverTests/YtDlpProgressParserTests.swift
git commit -m "feat: add YtDlpProgressParser for wholesale-download progress lines"
```

- [ ] **Step 6: Write the failing test for the wholesale downloader**

`SDMKit/Tests/SDMResolverTests/WholesaleDownloaderTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMResolver

private func makeToolDirectory(named name: String) throws -> URL {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let toolURL = dir.appendingPathComponent(name)
    try Data().write(to: toolURL)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: toolURL.path)
    return dir
}

@Test func downloadReportsEachParsedProgressFraction() async throws {
    let dir = try makeToolDirectory(named: "yt-dlp")
    defer { try? FileManager.default.removeItem(at: dir) }
    let toolPath = dir.appendingPathComponent("yt-dlp").path

    let runner = FakeStreamingProcessRunner()
    await runner.program(
        executablePath: toolPath,
        lines: [
            "[download] Destination: out.mp4",
            "[download]  10.0% of 10.00MiB at 1.00MiB/s ETA 00:09",
            "[download]  55.0% of 10.00MiB at 1.00MiB/s ETA 00:04",
            "[download] 100.0% of 10.00MiB in 00:10",
        ],
        exitCode: 0
    )

    let downloader = YtDlpWholesaleDownloader(
        streamingRunner: runner, toolLocator: ExternalToolLocator(searchPaths: [dir.path]))

    var reported: [Double] = []
    try await downloader.download(
        sourceURL: URL(string: "https://www.youtube.com/watch?v=abc")!, cookiesSource: .safari,
        outputPath: URL(fileURLWithPath: "/tmp/out.mp4"),
        onProgress: { fraction in reported.append(fraction) })

    #expect(reported == [0.10, 0.55, 1.0])

    let invocations = await runner.recordedInvocations
    #expect(invocations.count == 1)
    #expect(invocations[0].arguments.contains("--newline"))
    #expect(invocations[0].arguments.contains("--cookies-from-browser"))
    #expect(invocations[0].arguments.contains("safari"))
}

@Test func downloadThrowsToolMissingWhenYtDlpIsNotFound() async {
    let downloader = YtDlpWholesaleDownloader(
        streamingRunner: FakeStreamingProcessRunner(), toolLocator: ExternalToolLocator(searchPaths: []))
    await #expect(throws: WholesaleDownloadError.toolMissing) {
        try await downloader.download(
            sourceURL: URL(string: "https://www.youtube.com/watch?v=abc")!, cookiesSource: .none,
            outputPath: URL(fileURLWithPath: "/tmp/out.mp4"), onProgress: { _ in })
    }
}

@Test func downloadThrowsProcessFailedOnNonZeroExit() async throws {
    let dir = try makeToolDirectory(named: "yt-dlp")
    defer { try? FileManager.default.removeItem(at: dir) }
    let toolPath = dir.appendingPathComponent("yt-dlp").path

    let runner = FakeStreamingProcessRunner()
    await runner.program(executablePath: toolPath, lines: ["ERROR: Sign in to confirm your age"], exitCode: 1)

    let downloader = YtDlpWholesaleDownloader(
        streamingRunner: runner, toolLocator: ExternalToolLocator(searchPaths: [dir.path]))

    do {
        try await downloader.download(
            sourceURL: URL(string: "https://www.youtube.com/watch?v=abc")!, cookiesSource: .none,
            outputPath: URL(fileURLWithPath: "/tmp/out.mp4"), onProgress: { _ in })
        Issue.record("expected processFailed")
    } catch let WholesaleDownloadError.processFailed(exitCode) {
        #expect(exitCode == 1)
    }
}
```

- [ ] **Step 7: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter WholesaleDownloaderTests`
Expected: FAIL — `cannot find 'FakeStreamingProcessRunner' in scope`.

- [ ] **Step 8: Implement `StreamingProcessRunner` and `WholesaleDownloader`**

`SDMKit/Sources/SDMResolver/StreamingProcessRunner.swift`:

```swift
import Foundation

/// A process whose output must be consumed incrementally while it runs,
/// rather than read whole at the end — the one shape `ProcessRunner` (Task
/// 3) deliberately does not cover. Yields each line of combined
/// stdout+stderr as it arrives; the stream finishes normally on a zero exit
/// and throws `StreamingProcessError.nonZeroExit` on any other.
public protocol StreamingProcessRunner: Sendable {
    func run(_ invocation: ProcessInvocation) -> AsyncThrowingStream<String, any Error>
}

public enum StreamingProcessError: Error, Equatable {
    case nonZeroExit(Int32)
}

/// Production `StreamingProcessRunner`: reads `Process`'s stdout pipe
/// line-by-line via a `readabilityHandler`, forwarding each line into the
/// stream as it is produced.
public struct RealStreamingProcessRunner: StreamingProcessRunner {
    public init() {}

    public func run(_ invocation: ProcessInvocation) -> AsyncThrowingStream<String, any Error> {
        AsyncThrowingStream { continuation in
            let process = Process()
            process.executableURL = URL(fileURLWithPath: invocation.executablePath)
            process.arguments = invocation.arguments
            let pipe = Pipe()
            process.standardOutput = pipe
            process.standardError = pipe

            var buffer = Data()
            pipe.fileHandleForReading.readabilityHandler = { handle in
                let chunk = handle.availableData
                guard !chunk.isEmpty else { return }
                buffer.append(chunk)
                while let newlineIndex = buffer.firstIndex(of: 0x0A) {
                    let lineData = buffer[..<newlineIndex]
                    buffer.removeSubrange(...newlineIndex)
                    if let line = String(data: lineData, encoding: .utf8) {
                        continuation.yield(line)
                    }
                }
            }

            process.terminationHandler = { finished in
                pipe.fileHandleForReading.readabilityHandler = nil
                if finished.terminationStatus == 0 {
                    continuation.finish()
                } else {
                    continuation.finish(throwing: StreamingProcessError.nonZeroExit(finished.terminationStatus))
                }
            }

            do {
                try process.run()
            } catch {
                continuation.finish(throwing: error)
            }
        }
    }
}

/// Test double: yields a fixed, pre-programmed sequence of lines immediately,
/// then finishes (or throws) with a fixed exit code — no real process, no
/// real timing.
public actor FakeStreamingProcessRunner: StreamingProcessRunner {
    private struct Programmed { let lines: [String]; let exitCode: Int32 }
    private var programmed: [String: Programmed] = [:]
    public private(set) var recordedInvocations: [ProcessInvocation] = []

    public init() {}

    public func program(executablePath: String, lines: [String], exitCode: Int32) {
        programmed[executablePath] = Programmed(lines: lines, exitCode: exitCode)
    }

    public nonisolated func run(_ invocation: ProcessInvocation) -> AsyncThrowingStream<String, any Error> {
        AsyncThrowingStream { continuation in
            Task {
                await self.recordInvocation(invocation)
                guard let programmed = await self.programmed(for: invocation.executablePath) else {
                    continuation.finish(throwing: FakeProcessRunner.NotProgrammed(executablePath: invocation.executablePath))
                    return
                }
                for line in programmed.lines { continuation.yield(line) }
                if programmed.exitCode == 0 {
                    continuation.finish()
                } else {
                    continuation.finish(throwing: StreamingProcessError.nonZeroExit(programmed.exitCode))
                }
            }
        }
    }

    private func recordInvocation(_ invocation: ProcessInvocation) {
        recordedInvocations.append(invocation)
    }

    private func programmed(for executablePath: String) -> Programmed? {
        programmed[executablePath]
    }
}
```

`SDMKit/Sources/SDMResolver/WholesaleDownloader.swift`:

```swift
import Foundation

public enum WholesaleDownloadError: Error, Equatable {
    case toolMissing
    case processFailed(exitCode: Int32)
}

/// Spec §8's degraded fallback: yt-dlp downloads the whole file itself
/// (rather than SDM's segmented engine), with progress parsed line-by-line
/// from `--newline` output. Deliberately outside `DownloadEngine` entirely —
/// see this plan's Architecture section for why.
public protocol WholesaleDownloader: Sendable {
    func download(
        sourceURL: URL, cookiesSource: CookiesSource, outputPath: URL,
        onProgress: @escaping @Sendable (Double) -> Void
    ) async throws
}

public struct YtDlpWholesaleDownloader: WholesaleDownloader {
    private let streamingRunner: any StreamingProcessRunner
    private let toolLocator: ExternalToolLocator

    public init(streamingRunner: any StreamingProcessRunner, toolLocator: ExternalToolLocator = ExternalToolLocator()) {
        self.streamingRunner = streamingRunner
        self.toolLocator = toolLocator
    }

    public func download(
        sourceURL: URL, cookiesSource: CookiesSource, outputPath: URL,
        onProgress: @escaping @Sendable (Double) -> Void
    ) async throws {
        guard let toolPath = toolLocator.locate(.ytDlp) else {
            throw WholesaleDownloadError.toolMissing
        }
        let arguments =
            ["--newline", "-o", outputPath.path] + cookiesSource.ytDlpArguments
            + [sourceURL.absoluteString]

        do {
            for try await line in streamingRunner.run(
                ProcessInvocation(executablePath: toolPath, arguments: arguments))
            {
                if let fraction = YtDlpProgressParser.parseFraction(from: line) {
                    onProgress(fraction)
                }
            }
        } catch let StreamingProcessError.nonZeroExit(code) {
            throw WholesaleDownloadError.processFailed(exitCode: code)
        }
    }
}
```

- [ ] **Step 9: Run the tests to verify they pass**

Run: `swift test --package-path SDMKit --filter WholesaleDownloaderTests`
Expected: PASS

- [ ] **Step 10: Commit**

```bash
git add SDMKit/Sources/SDMResolver/StreamingProcessRunner.swift SDMKit/Sources/SDMResolver/WholesaleDownloader.swift SDMKit/Tests/SDMResolverTests/WholesaleDownloaderTests.swift
git commit -m "feat: add the wholesale-download fallback for HLS/DASH-only formats"
```

---

### Task 15: `ResolverState` and `ProbedLink`'s resolver fields

**Files:**
- Modify: `SDMKit/Package.swift` (`SDMGrabber` gains a dependency on `SDMResolver`)
- Create: `SDMKit/Sources/SDMGrabber/ResolverState.swift`
- Modify: `SDMKit/Sources/SDMGrabber/ProbedLink.swift`
- Create: `SDMKit/Tests/SDMGrabberTests/ResolverStateTests.swift`

**Interfaces:**
- Consumes: `ResolvedMedia` (`SDMResolver`, Task 6)
- Produces:
  - `public enum ResolverState: Equatable, Sendable { case resolving; case resolved; case toolMissing(tool: String); case failed(reason: String) }`
  - `ProbedLink.resolverState: ResolverState?`, `ProbedLink.resolvedMedia: ResolvedMedia?`, `ProbedLink.selectedFormatID: String?` (all default `nil`)

`resolverState` is a separate axis from `Verdict` rather than a new `Verdict` case: `VerdictRules.evaluate` stays a pure function over the plain HTTP probe (a bare YouTube watch URL already evaluates `.online` today, unchanged), and `resolverState` only exists on links a `LinkResolver` actually handles — a generic file link's `resolverState` stays `nil` forever. The UI (Task 22) checks `resolverState` first when non-nil, falling back to `verdict` otherwise.

- [ ] **Step 1: Write the failing test**

`SDMKit/Tests/SDMGrabberTests/ResolverStateTests.swift`:

```swift
import Foundation
import Testing

@testable import SDMGrabber

@Test func probedLinkDefaultsResolverFieldsToNil() {
    let link = ProbedLink(originalURL: URL(string: "https://example.com/a.bin")!)
    #expect(link.resolverState == nil)
    #expect(link.resolvedMedia == nil)
    #expect(link.selectedFormatID == nil)
}

@Test func resolverStateCasesAreDistinguishable() {
    #expect(ResolverState.resolving != ResolverState.resolved)
    #expect(ResolverState.toolMissing(tool: "yt-dlp") == ResolverState.toolMissing(tool: "yt-dlp"))
    #expect(ResolverState.failed(reason: "a") != ResolverState.failed(reason: "b"))
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter ResolverStateTests`
Expected: FAIL — `cannot find 'ResolverState' in scope`.

- [ ] **Step 3: Add the `SDMResolver` dependency and implement**

Modify `SDMKit/Package.swift` — change the `SDMGrabber` target line:

```swift
        .target(name: "SDMGrabber", dependencies: ["SDMCore", "SDMResolver"]),
```

`SDMKit/Sources/SDMGrabber/ResolverState.swift`:

```swift
/// A link's progress through resolver-backed extraction (yt-dlp), tracked
/// separately from `Verdict` — see `ProbedLink`'s doc comment on
/// `resolverState` for why the two axes stay independent.
public enum ResolverState: Equatable, Sendable {
    case resolving
    case resolved
    case toolMissing(tool: String)
    case failed(reason: String)
}
```

Modify `SDMKit/Sources/SDMGrabber/ProbedLink.swift` — add `import SDMResolver` at the top, then the three new stored properties after `isDuplicate`, threaded through `init`:

```swift
import Foundation
import SDMResolver
```

```swift
    public var verdict: Verdict?
    public var isDuplicate: Bool
    /// Set only for links a `LinkResolver` handles (currently: YouTube).
    /// `nil` forever for every other link — a plain file URL never touches
    /// this axis at all.
    public var resolverState: ResolverState?
    public var resolvedMedia: ResolvedMedia?
    /// The format ID currently chosen for handoff — defaults to
    /// `QualitySelector`'s pick once `resolvedMedia` lands, user-overridable
    /// via `GrabberSession.selectFormat(_:formatID:)` with no re-resolve.
    public var selectedFormatID: String?

    public init(
        id: UUID = UUID(),
        originalURL: URL,
        finalURL: URL? = nil,
        stage: Stage = .queued,
        statusCode: Int? = nil,
        contentLength: Int64? = nil,
        contentType: String? = nil,
        suggestedFilename: String? = nil,
        validator: String? = nil,
        acceptsRanges: Bool = false,
        sniffedSignature: FileSignature? = nil,
        transportFailed: Bool = false,
        verdict: Verdict? = nil,
        isDuplicate: Bool = false,
        resolverState: ResolverState? = nil,
        resolvedMedia: ResolvedMedia? = nil,
        selectedFormatID: String? = nil
    ) {
        self.id = id
        self.originalURL = originalURL
        self.finalURL = finalURL ?? originalURL
        self.stage = stage
        self.statusCode = statusCode
        self.contentLength = contentLength
        self.contentType = contentType
        self.suggestedFilename = suggestedFilename
        self.validator = validator
        self.acceptsRanges = acceptsRanges
        self.sniffedSignature = sniffedSignature
        self.transportFailed = transportFailed
        self.verdict = verdict
        self.isDuplicate = isDuplicate
        self.resolverState = resolverState
        self.resolvedMedia = resolvedMedia
        self.selectedFormatID = selectedFormatID
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `swift test --package-path SDMKit --filter ResolverStateTests`
Run: `swift test --package-path SDMKit --filter SDMGrabberTests` (confirm the `ProbedLink.init` signature change didn't break existing literal-constructed test fixtures, e.g. `LinkGrabberView.swift`'s preview and any `#require`/direct-init test — this is a Swift *app-target* file, not covered by `swift test`, so also grep for `ProbedLink(` usages outside `SDMKit/` once this step passes, to confirm none broke: `grep -rn "ProbedLink(" SDM/`)
Expected: PASS on the `swift test` run; the `grep` should show only keyword-argument call sites, which remain source-compatible with new trailing defaulted parameters.

- [ ] **Step 5: Commit**

```bash
git add SDMKit/Package.swift SDMKit/Sources/SDMGrabber/ResolverState.swift SDMKit/Sources/SDMGrabber/ProbedLink.swift SDMKit/Tests/SDMGrabberTests/ResolverStateTests.swift
git commit -m "feat: add ResolverState and thread resolver fields through ProbedLink"
```

---

### Task 16: `GrabberSession` — the YouTube extraction stage

**Files:**
- Modify: `SDMKit/Sources/SDMGrabber/GrabberSession.swift`
- Create: `SDMKit/Tests/SDMGrabberTests/GrabberSessionYouTubeTests.swift`

**Interfaces:**
- Consumes: `LinkResolver`, `YtDlpResolverError`, `QualityPreference`, `QualitySelector` (`SDMResolver`, Tasks 6–7)
- Produces: `GrabberSession.init` gains `youtubeResolver: (any LinkResolver)? = nil` and `qualityPreference: QualityPreference = .init(...)`; `GrabberSession.selectFormat(_:formatID:)`

After the existing probe stage finishes for a batch of freshly-ingested links, any link whose host the resolver claims (`canHandle`) gets a second, independent extraction pass — sequential rather than concurrency-budgeted like `probeBounded`, since spawning many concurrent `yt-dlp` processes is expensive and YouTube links are a small minority of any real batch.

- [ ] **Step 1: Write the failing test**

`SDMKit/Tests/SDMGrabberTests/GrabberSessionYouTubeTests.swift`:

```swift
import Foundation
import SDMResolver
import Testing

@testable import SDMGrabber

private let watchURL = URL(string: "https://www.youtube.com/watch?v=abc123")!

private func format(id: String, height: Int = 720) -> YtDlpFormat {
    YtDlpFormat(
        formatID: id, ext: "mp4", vcodec: "avc1.640028", acodec: "mp4a.40.2", protocolName: "https",
        width: nil, height: height, filesize: nil, filesizeApprox: nil, formatNote: nil, tbr: 500,
        url: URL(string: "https://example.com/\(id)")!)
}

private struct FakeYouTubeResolver: LinkResolver {
    var result: Result<[ResolvedMedia], any Error>
    func canHandle(_ url: URL) -> Bool { url.host == "www.youtube.com" }
    func resolve(_ url: URL) async throws -> [ResolvedMedia] { try result.get() }
}

private func makeSession(resolver: (any LinkResolver)?) -> (GrabberSession, FakeProbeOrigin) {
    let origin = FakeProbeOrigin()
    let session = GrabberSession(
        prober: LinkProber(transport: origin, deepSniffEnabled: false), youtubeResolver: resolver)
    return (session, origin)
}

@Test func aYouTubeLinkGetsResolvedAndAssignedADefaultFormat() async throws {
    let media = ResolvedMedia(
        extractor: "youtube", mediaID: "abc123", title: "Video", formats: [format(id: "22")],
        sourceURL: watchURL)
    let (session, origin) = makeSession(resolver: FakeYouTubeResolver(result: .success([media])))
    await origin.setBehavior(FakeProbeOrigin.Behavior(), for: watchURL)

    await session.ingest(urls: [watchURL])
    let link = await session.snapshot().links.first { $0.originalURL == watchURL }

    #expect(link?.resolverState == .resolved)
    #expect(link?.resolvedMedia?.mediaID == "abc123")
    #expect(link?.selectedFormatID == "22")
}

@Test func toolMissingSurfacesAsAResolverStateNotACrash() async throws {
    let (session, origin) = makeSession(
        resolver: FakeYouTubeResolver(result: .failure(YtDlpResolverError.toolMissing)))
    await origin.setBehavior(FakeProbeOrigin.Behavior(), for: watchURL)

    await session.ingest(urls: [watchURL])
    let link = await session.snapshot().links.first { $0.originalURL == watchURL }

    #expect(link?.resolverState == .toolMissing(tool: "yt-dlp"))
}

@Test func aNonYouTubeLinkNeverTouchesTheResolver() async throws {
    let fileURL = URL(string: "https://example.com/movie.mp4")!
    let (session, origin) = makeSession(
        resolver: FakeYouTubeResolver(result: .failure(YtDlpResolverError.toolMissing)))
    await origin.setBehavior(FakeProbeOrigin.Behavior(), for: fileURL)

    await session.ingest(urls: [fileURL])
    let link = await session.snapshot().links.first { $0.originalURL == fileURL }

    #expect(link?.resolverState == nil)
}

@Test func selectFormatUpdatesTheChosenFormatIDWithoutReResolving() async throws {
    let media = ResolvedMedia(
        extractor: "youtube", mediaID: "abc123", title: "Video",
        formats: [format(id: "22", height: 720), format(id: "18", height: 360)], sourceURL: watchURL)
    let (session, origin) = makeSession(resolver: FakeYouTubeResolver(result: .success([media])))
    await origin.setBehavior(FakeProbeOrigin.Behavior(), for: watchURL)
    await session.ingest(urls: [watchURL])
    let linkID = try #require(await session.snapshot().links.first?.id)

    await session.selectFormat(linkID, formatID: "18")

    let link = await session.snapshot().links.first { $0.id == linkID }
    #expect(link?.selectedFormatID == "18")
}

@Test func selectFormatIgnoresAFormatIDNotInTheResolvedTable() async throws {
    let media = ResolvedMedia(
        extractor: "youtube", mediaID: "abc123", title: "Video", formats: [format(id: "22")],
        sourceURL: watchURL)
    let (session, origin) = makeSession(resolver: FakeYouTubeResolver(result: .success([media])))
    await origin.setBehavior(FakeProbeOrigin.Behavior(), for: watchURL)
    await session.ingest(urls: [watchURL])
    let linkID = try #require(await session.snapshot().links.first?.id)

    await session.selectFormat(linkID, formatID: "does-not-exist")

    let link = await session.snapshot().links.first { $0.id == linkID }
    #expect(link?.selectedFormatID == "22")
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `swift test --package-path SDMKit --filter GrabberSessionYouTubeTests`
Expected: FAIL — `GrabberSession.init` does not accept `youtubeResolver:`.

- [ ] **Step 3: Wire the extraction stage into `GrabberSession`**

Modify `SDMKit/Sources/SDMGrabber/GrabberSession.swift`. Add the import and two new stored properties, next to `prober`/`budget`:

```swift
import Foundation
import SDMResolver
```

```swift
    private let prober: LinkProber
    private let budget: Budget
    private let youtubeResolver: (any LinkResolver)?
    private let qualityPreference: QualityPreference
```

`init`:

```swift
    public init(
        prober: LinkProber, budget: Budget = Budget(), youtubeResolver: (any LinkResolver)? = nil,
        qualityPreference: QualityPreference = QualityPreference(
            resolutionLadder: QualityPreference.standardLadder(maxHeight: 1080),
            preferredCodecPrefix: "avc1", preferredContainer: nil, maxFilesizeBytes: nil)
    ) {
        self.prober = prober
        self.budget = budget
        self.youtubeResolver = youtubeResolver
        self.qualityPreference = qualityPreference
    }
```

In `ingest(urls:)`, resolve YouTube links right after the existing probe pass and before `recluster()`:

```swift
    public func ingest(urls: [URL]) async {
        var fresh: [UUID] = []
        for url in urls where seenURLs.insert(url).inserted {
            let id = UUID()
            links[id] = ProbedLink(
                id: id, originalURL: url, isDuplicate: knownDownloadURLs.contains(url))
            order.append(id)
            fresh.append(id)
        }
        guard !fresh.isEmpty else { return }
        await probeBounded(fresh)
        await resolveYouTubeLinks(fresh)
        recluster()
    }
```

New methods, placed after `recheckFailed()`:

```swift
    /// Spec §8: yt-dlp extraction for any link the resolver claims.
    /// Sequential rather than `probeBounded`'s concurrency-budgeted fan-out
    /// — see this task's own description for why.
    private func resolveYouTubeLinks(_ ids: [UUID]) async {
        guard let youtubeResolver else { return }
        for id in ids {
            guard let url = links[id]?.originalURL, youtubeResolver.canHandle(url) else { continue }
            links[id]?.resolverState = .resolving
            do {
                let resolved = try await youtubeResolver.resolve(url)
                guard let media = resolved.first else {
                    links[id]?.resolverState = .failed(reason: "yt-dlp returned no media")
                    continue
                }
                links[id]?.resolvedMedia = media
                links[id]?.selectedFormatID = QualitySelector.selectFormat(
                    from: media.formats, preference: qualityPreference)?.formatID
                links[id]?.resolverState = .resolved
            } catch YtDlpResolverError.toolMissing {
                links[id]?.resolverState = .toolMissing(tool: "yt-dlp")
            } catch let YtDlpResolverError.processFailed(_, stderr) {
                links[id]?.resolverState = .failed(
                    reason: stderr.isEmpty ? "yt-dlp failed" : stderr)
            } catch {
                links[id]?.resolverState = .failed(reason: "\(error)")
            }
        }
    }

    /// Spec §8: "Changing it swaps the selected format ID and the displayed
    /// filesize updates instantly... no re-probe, no network round trip." A
    /// no-op for a format ID absent from the already-fetched table, or a
    /// link with no resolved media at all.
    public func selectFormat(_ linkID: UUID, formatID: String) {
        guard let resolvedMedia = links[linkID]?.resolvedMedia,
            resolvedMedia.formats.contains(where: { $0.formatID == formatID })
        else { return }
        links[linkID]?.selectedFormatID = formatID
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `swift test --package-path SDMKit --filter GrabberSessionYouTubeTests`
Run: `swift test --package-path SDMKit --filter SDMGrabberTests`
Expected: PASS on both.

- [ ] **Step 5: Commit**

```bash
git add SDMKit/Sources/SDMGrabber/GrabberSession.swift SDMKit/Tests/SDMGrabberTests/GrabberSessionYouTubeTests.swift
git commit -m "feat: resolve YouTube links through LinkResolver in GrabberSession"
```

---

### Task 17: `ResolverSettings` — the app-layer settings store

**Files:**
- Create: `SDM/ResolverSettings.swift`

**Interfaces:**
- Consumes: `CookiesSource`, `QualityPreference` (`SDMResolver`, Tasks 2, 7)
- Produces: `ResolverSettings.cookiesSource: CookiesSource` (default `.safari`), `ResolverSettings.maxResolution: Int` (default `1080`), `ResolverSettings.preferH264: Bool` (default `true`), `ResolverSettings.maxFilesizeMB: Int` (default `0`, meaning unlimited), `ResolverSettings.qualityPreference: QualityPreference` (computed)

This is an app-target-only file with no dedicated unit test, mirroring `EngineSettingsStore`/`GrabberSettings` — neither of those has one either; both are thin `UserDefaults` wrappers exercised by building and using Settings by hand (Task 20's manual-verification step covers this one the same way).

- [ ] **Step 1: Implement `ResolverSettings`**

`SDM/ResolverSettings.swift`:

```swift
import Foundation
import SDMResolver

/// Backs the Phase 5 YouTube-quality and cookies settings. Mirrors
/// `EngineSettingsStore`/`GrabberSettings`'s direct-`UserDefaults` pattern —
/// see their doc comments for why there is still no single unified Settings
/// model.
@MainActor
enum ResolverSettings {
    private static let defaults = UserDefaults.standard

    private enum Key {
        static let cookiesSource = "sdm.youtubeCookiesSource"
        static let maxResolution = "sdm.youtubePreferredMaxResolution"
        static let preferH264 = "sdm.youtubePreferH264"
        static let maxFilesizeMB = "sdm.youtubeMaxFilesizeMB"
    }

    /// Default `.safari`: Safari's cookie jar is readable with no keychain
    /// prompt, unlike Chrome's — see the Settings YouTube tab's warning
    /// (Task 20).
    static var cookiesSource: CookiesSource {
        get {
            guard let raw = defaults.string(forKey: Key.cookiesSource),
                let value = CookiesSource(rawValue: raw)
            else { return .safari }
            return value
        }
        set { defaults.set(newValue.rawValue, forKey: Key.cookiesSource) }
    }

    /// Spec §12 default: "1080p → 720p" — expands via
    /// `QualityPreference.standardLadder(maxHeight:)`.
    static var maxResolution: Int {
        get { defaults.object(forKey: Key.maxResolution) as? Int ?? 1080 }
        set { defaults.set(newValue, forKey: Key.maxResolution) }
    }

    /// Spec §12 default: "prefer H.264."
    static var preferH264: Bool {
        get { defaults.object(forKey: Key.preferH264) as? Bool ?? true }
        set { defaults.set(newValue, forKey: Key.preferH264) }
    }

    /// `0` means unlimited — the Settings field's "no cap" state.
    static var maxFilesizeMB: Int {
        get { defaults.object(forKey: Key.maxFilesizeMB) as? Int ?? 0 }
        set { defaults.set(newValue, forKey: Key.maxFilesizeMB) }
    }

    static var qualityPreference: QualityPreference {
        QualityPreference(
            resolutionLadder: QualityPreference.standardLadder(maxHeight: maxResolution),
            preferredCodecPrefix: preferH264 ? "avc1" : nil,
            preferredContainer: nil,
            maxFilesizeBytes: maxFilesizeMB > 0 ? Int64(maxFilesizeMB) * 1024 * 1024 : nil
        )
    }
}
```

- [ ] **Step 2: Build the app target to confirm it compiles**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -configuration Debug build 2>&1 | tail -40`
Expected: `BUILD SUCCEEDED` (this file has no callers yet, so nothing else changes behavior — this step only confirms the new file itself compiles cleanly before later tasks depend on it).

- [ ] **Step 3: Commit**

```bash
git add SDM/ResolverSettings.swift
git commit -m "feat: add ResolverSettings for YouTube quality and cookies preferences"
```

---

### Task 18: Wire the resolver, muxer, and URL-refresh adapter into `EngineController`

**Files:**
- Create: `SDM/URLRefresherAdapter.swift`
- Modify: `SDM/EngineController.swift`

**Interfaces:**
- Consumes: `ExternalToolLocator`, `RealProcessRunner`, `YouTubeResolver`, `FFmpegMuxer` (`SDMResolver`), `URLRefresher` (`SDMEngine`), `ResolverBinding` (`SDMCore`), `ResolverSettings` (Task 17)
- Produces: `struct URLRefresherAdapter: URLRefresher`; `EngineController`'s `DownloadEngine` is now constructed with real `urlRefresher`/`muxer`; `EngineController.addPackage(name:items:startImmediately:)`

- [ ] **Step 1: Implement `URLRefresherAdapter`**

`SDM/URLRefresherAdapter.swift`:

```swift
import Foundation
import SDMCore
import SDMEngine
import SDMResolver

/// Bridges `SDMEngine`'s `URLRefresher` protocol to a concrete
/// `LinkResolver`: re-resolves the binding's original page and picks out the
/// same format ID's now-fresh URL. Lives in the `SDM` app target — the one
/// place allowed to depend on both `SDMEngine` and `SDMResolver` at once, per
/// this plan's Architecture section.
struct URLRefresherAdapter: URLRefresher {
    enum RefreshError: Error {
        case formatNoLongerAvailable
    }

    let resolver: any LinkResolver

    func refreshURL(for binding: ResolverBinding) async throws -> URL {
        let media = try await resolver.resolve(binding.sourceURL)
        guard let format = media.first?.formats.first(where: { $0.formatID == binding.formatID })
        else {
            throw RefreshError.formatNoLongerAvailable
        }
        return format.url
    }
}
```

- [ ] **Step 2: Wire it into `EngineController.init` and add the `items:` handoff overload**

Modify `SDM/EngineController.swift` — add the import:

```swift
import SDMResolver
```

Replace `init()` in full:

```swift
    init() {
        let support = FileManager.default.urls(
            for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("SDM", isDirectory: true)
        let downloads = FileManager.default.urls(for: .downloadsDirectory, in: .userDomainMask)[0]
        downloadFolder = downloads

        let toolLocator = ExternalToolLocator()
        let processRunner = RealProcessRunner()
        let youtubeResolver = YouTubeResolver(
            processRunner: processRunner, toolLocator: toolLocator,
            cookiesSource: ResolverSettings.cookiesSource)
        let muxer = FFmpegMuxer(processRunner: processRunner, toolLocator: toolLocator)

        engine = DownloadEngine(
            transport: URLSessionTransport(),
            stateStore: JSONStateStore(fileURL: support.appendingPathComponent("state.json")),
            settings: EngineSettings(
                maxConcurrent: EngineSettingsStore.maxConcurrent,
                segmentsPerItem: EngineSettingsStore.segmentsPerItem,
                globalMaxConnections: EngineSettingsStore.globalMaxConnections,
                maxConnectionsPerHost: EngineSettingsStore.maxConnectionsPerHost,
                downloadFolder: downloads,
                minSegmentSizeBytes: Int64(EngineSettingsStore.minSegmentSizeMB) * 1024 * 1024
            ),
            urlRefresher: URLRefresherAdapter(resolver: youtubeResolver),
            muxer: muxer
        )
    }
```

Add the new handoff overload right after the existing `addPackage(name:urls:startImmediately:)`:

```swift
    /// Spec §8's YouTube handoff: pre-built items (from `YouTubeHandoff`, or
    /// plain `DownloadItem(url:filename:)` for a non-resolver link) rather
    /// than bare URLs, so a video/audio mux pair keeps its `MuxCompanion`
    /// binding intact end to end. `startImmediately` is applied uniformly
    /// here — same reasoning as the `urls:` overload above — rather than
    /// trusted from whatever `state` the caller's items happened to carry.
    func addPackage(name: String, items: [DownloadItem], startImmediately: Bool) async {
        guard !items.isEmpty else { return }
        let stateApplied = items.map { item -> DownloadItem in
            var item = item
            item.state = startImmediately ? .queued : .stopped
            return item
        }
        await engine.add(DownloadPackage(name: name, items: stateApplied))
        publish(await engine.snapshot())
    }
```

- [ ] **Step 3: Build the app target**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -configuration Debug build 2>&1 | tail -40`
Expected: `BUILD SUCCEEDED`

- [ ] **Step 4: Commit**

```bash
git add SDM/URLRefresherAdapter.swift SDM/EngineController.swift
git commit -m "feat: wire YouTubeResolver/FFmpegMuxer into DownloadEngine via EngineController"
```

---

### Task 19: Wire the resolver into `GrabberController`

**Files:**
- Modify: `SDM/GrabberController.swift`

**Interfaces:**
- Consumes: `ExternalToolLocator`, `RealProcessRunner`, `YouTubeResolver` (`SDMResolver`), `ResolverSettings` (Task 17)
- Produces: `GrabberController`'s `GrabberSession` is now constructed with a real `youtubeResolver`/`qualityPreference`; `GrabberController.selectFormat(_:formatID:)`; `GrabberController.links(inPackage:) -> [ProbedLink]`

- [ ] **Step 1: Wire the resolver into `init` and add the two new methods**

Modify `SDM/GrabberController.swift` — add the import:

```swift
import SDMResolver
```

Replace `init()` in full:

```swift
    init() {
        let toolLocator = ExternalToolLocator()
        let processRunner = RealProcessRunner()
        let youtubeResolver = YouTubeResolver(
            processRunner: processRunner, toolLocator: toolLocator,
            cookiesSource: ResolverSettings.cookiesSource)

        session = GrabberSession(
            prober: LinkProber(
                transport: URLSessionProbeTransport(),
                deepSniffEnabled: GrabberSettings.deepSniffEnabled
            ),
            budget: GrabberSession.Budget(
                globalMaxConcurrentProbes: EngineSettingsStore.globalMaxConnections,
                maxConcurrentPerHost: EngineSettingsStore.maxConnectionsPerHost
            ),
            youtubeResolver: youtubeResolver,
            qualityPreference: ResolverSettings.qualityPreference
        )
    }
```

Add the two new methods, next to `urls(inPackage:)`:

```swift
    /// Spec §8's per-link format picker: swaps the selected format ID with
    /// no re-probe, no network round trip — `GrabberSession.selectFormat`
    /// only ever reads the already-fetched table.
    func selectFormat(_ linkID: UUID, formatID: String) async {
        await session.selectFormat(linkID, formatID: formatID)
        snapshot = await session.snapshot()
    }

    /// Like `urls(inPackage:)`, but the full `ProbedLink` records — needed
    /// for the download handoff (Task 21), which must see a YouTube link's
    /// `resolvedMedia`/`selectedFormatID`, not just its bare URL.
    func links(inPackage id: UUID) -> [ProbedLink] {
        guard let package = snapshot.packages.first(where: { $0.id == id }) else { return [] }
        let ids = Set(package.linkIDs)
        return snapshot.links.filter { ids.contains($0.id) }
    }
```

- [ ] **Step 2: Build the app target**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -configuration Debug build 2>&1 | tail -40`
Expected: `BUILD SUCCEEDED`

- [ ] **Step 3: Commit**

```bash
git add SDM/GrabberController.swift
git commit -m "feat: wire YouTubeResolver into GrabberController, add selectFormat/links(inPackage:)"
```

---

### Task 20: Settings — the YouTube tab, quality controls, and the Chrome cookie warning

**Files:**
- Modify: `SDM/SettingsView.swift`

**Interfaces:**
- Consumes: `ResolverSettings` (Task 17), `CookiesSource` (`SDMResolver`)
- Produces: a fifth Settings tab, "YouTube," between Linkgrabber and Notifications

Per the user's explicit requirement (not in the original spec): a cookies-source picker defaulting to Safari, with a red warning below it — using `theme.failedColor`, the codebase's existing "this is broken/needs attention, in red" role (see Phase 4's `Theme` role table; there is no dedicated `warning`/`danger` role, and adding one would mean touching all fourteen bundled theme JSON files for a single one-off warning label, which this plan does not do) — shown only when Chrome is selected, since Safari's cookie store needs no keychain prompt.

- [ ] **Step 1: Add the import, the four `@State` vars, the tab, and the commit wiring**

Modify `SDM/SettingsView.swift` — add the import:

```swift
import SDMResolver
```

Add four `@State` properties, after `deepSniffEnabled`:

```swift
    @State private var deepSniffEnabled = GrabberSettings.deepSniffEnabled
    @State private var youtubeMaxResolution = ResolverSettings.maxResolution
    @State private var youtubePreferH264 = ResolverSettings.preferH264
    @State private var youtubeMaxFilesizeMB = ResolverSettings.maxFilesizeMB
    @State private var youtubeCookiesSource = ResolverSettings.cookiesSource
```

Add the tab to the `TabView` in `body`, between `linkgrabberTab` and `notificationsTab`:

```swift
            TabView {
                appearanceTab.tabItem { Label("Appearance", systemImage: "paintbrush") }
                downloadsTab.tabItem { Label("Downloads", systemImage: "arrow.down.circle") }
                linkgrabberTab.tabItem { Label("Linkgrabber", systemImage: "link") }
                youtubeTab.tabItem { Label("YouTube", systemImage: "play.rectangle") }
                notificationsTab.tabItem { Label("Notifications", systemImage: "bell") }
            }
```

Add the tab's body, after `linkgrabberTab`:

```swift
    private var youtubeTab: some View {
        VStack(alignment: .leading, spacing: 16) {
            SettingsSection(title: "Quality", theme: theme) {
                Grid(alignment: .leading, verticalSpacing: 10) {
                    GridRow {
                        Text("Maximum resolution")
                        Picker("", selection: $youtubeMaxResolution) {
                            ForEach([2160, 1440, 1080, 720, 480, 360], id: \.self) { height in
                                Text("\(height)p").tag(height)
                            }
                        }
                        .labelsHidden()
                        .gridColumnAlignment(.trailing)
                    }
                    SteppedNumberField(
                        label: "Max filesize (MB, 0 = unlimited)", value: $youtubeMaxFilesizeMB,
                        range: 0...100_000)
                }
                Toggle("Prefer H.264 codec", isOn: $youtubePreferH264)
            }
            SettingsSection(title: "Cookies", theme: theme) {
                VStack(alignment: .leading, spacing: 6) {
                    HStack {
                        Text("Use cookies from browser")
                        Spacer()
                        Picker("", selection: $youtubeCookiesSource) {
                            Text("None").tag(CookiesSource.none)
                            Text("Safari").tag(CookiesSource.safari)
                            Text("Chrome").tag(CookiesSource.chrome)
                        }
                        .labelsHidden()
                        .frame(width: 160)
                    }
                    if youtubeCookiesSource == .chrome {
                        Text(
                            "Chrome may repeatedly prompt for your login password to read its cookie storage. Choose \u{201c}Always Allow\u{201d} when prompted, or SDM will ask again on every YouTube grab."
                        )
                        .font(.caption)
                        .foregroundStyle(theme.failedColor)
                        .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.top, 12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(theme.surfacePrimaryColor)
    }
```

Add the four write-backs to `commit()`, after `GrabberSettings.deepSniffEnabled = deepSniffEnabled`:

```swift
        GrabberSettings.deepSniffEnabled = deepSniffEnabled
        ResolverSettings.maxResolution = youtubeMaxResolution
        ResolverSettings.preferH264 = youtubePreferH264
        ResolverSettings.maxFilesizeMB = youtubeMaxFilesizeMB
        ResolverSettings.cookiesSource = youtubeCookiesSource
```

- [ ] **Step 2: Build the app target**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -configuration Debug build 2>&1 | tail -40`
Expected: `BUILD SUCCEEDED`. Per spec §11.7 and this plan's Global Constraints, this tab gets no automated test — Task 23's manual-verification checklist covers it (open Settings, select Chrome, confirm the red warning appears; select Safari/None, confirm it disappears).

- [ ] **Step 3: Commit**

```bash
git add SDM/SettingsView.swift
git commit -m "feat: add the YouTube settings tab with quality controls and the Chrome cookie warning"
```

---

### Task 21: Linkgrabber UI — the resolver badge, format picker, and YouTube-aware handoff

**Files:**
- Modify: `SDM/LinkGrabberView.swift`

**Interfaces:**
- Consumes: `ResolverState`, `YtDlpFormat` (`SDMGrabber`/`SDMResolver`), `YouTubeHandoff` (`SDMResolver`, Task 13), `GrabberController.selectFormat`/`links(inPackage:)` (Task 19), `EngineController.addPackage(name:items:startImmediately:)` (Task 18)

Spec §8: "a format picker on any YouTube row... the displayed filesize updates instantly." Spec §8's "requires yt-dlp" state gets the same free-text-badge treatment `.faulty(reason:)` already established for generic links (see `VerdictRules`'s doc comment in Task 15).

- [ ] **Step 1: Add the import, extend `verdictBadge`, add the format picker, and rewrite `addToDownloads`**

Modify `SDM/LinkGrabberView.swift` — add the import:

```swift
import SDMResolver
```

Replace `verdictBadge` in `LinkRow` in full, and add the two new private members right after it:

```swift
    @ViewBuilder
    private var verdictBadge: some View {
        if let resolverState = link.resolverState {
            resolverBadge(resolverState)
        } else {
            switch link.verdict {
            case .online:
                Text("online").font(.caption).foregroundStyle(theme.onlineColor)
            case .offline:
                Text("offline").font(.caption).foregroundStyle(theme.offlineColor)
            case .checkFailed:
                Text("check failed").font(.caption).foregroundStyle(theme.failedColor)
            case .faulty(let reason):
                // Spec §7.3: the faulty reason *is* the badge text.
                Text(reason).font(.caption).foregroundStyle(theme.faultyColor)
            case nil:
                // No verdict yet: spec §7.5's queued → probing → sniffing → done
                // per-link state, shown literally rather than a bare spinner.
                HStack(spacing: 4) {
                    ProgressView().controlSize(.small)
                    Text(stageLabel).font(.caption).foregroundStyle(theme.textSecondaryColor)
                }
            }
        }
    }

    @ViewBuilder
    private func resolverBadge(_ state: ResolverState) -> some View {
        switch state {
        case .resolving:
            HStack(spacing: 4) {
                ProgressView().controlSize(.small)
                Text("resolving").font(.caption).foregroundStyle(theme.textSecondaryColor)
            }
        case .resolved:
            if let format = selectedFormat {
                formatPicker(current: format)
            } else {
                Text("resolved").font(.caption).foregroundStyle(theme.onlineColor)
            }
        case .toolMissing(let tool):
            VStack(alignment: .trailing, spacing: 2) {
                Text("requires \(tool)").font(.caption).foregroundStyle(theme.faultyColor)
                Text("brew install \(tool)")
                    .font(.caption2.monospaced())
                    .foregroundStyle(theme.textSecondaryColor)
                    .textSelection(.enabled)
            }
        case .failed(let reason):
            Text(reason).font(.caption).foregroundStyle(theme.failedColor)
        }
    }

    private var selectedFormat: YtDlpFormat? {
        guard let media = link.resolvedMedia, let formatID = link.selectedFormatID else { return nil }
        return media.formats.first { $0.formatID == formatID }
    }

    /// Spec §8: "Changing it swaps the selected format ID and the displayed
    /// filesize updates instantly... no re-probe" — `fileSize` below already
    /// reads `selectedFormat`, so a `Menu` selection alone is enough; no
    /// local `@State` is needed here, `controller.selectFormat` round-trips
    /// through the same `snapshot` this row already observes.
    @ViewBuilder
    private func formatPicker(current: YtDlpFormat) -> some View {
        Menu {
            ForEach(
                (link.resolvedMedia?.formats ?? []).filter { $0.vcodec != "none" },
                id: \.formatID
            ) { format in
                Button(formatLabel(format)) {
                    let linkID = link.id
                    let formatID = format.formatID
                    Task { await controller.selectFormat(linkID, formatID: formatID) }
                }
            }
        } label: {
            Text(formatLabel(current))
                .font(.caption)
                .foregroundStyle(theme.onlineColor)
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
    }

    private func formatLabel(_ format: YtDlpFormat) -> String {
        let resolution = format.height.map { "\($0)p" } ?? format.formatID
        return "\(resolution) · \(format.ext)"
    }
```

Modify `fileSize` to prefer the selected format's size when one exists:

```swift
    private var fileSize: String {
        let bytes = selectedFormat?.effectiveFilesize ?? link.contentLength
        guard let bytes, bytes > 0 else { return "" }
        let formatter = ByteCountFormatter()
        formatter.countStyle = .binary
        return formatter.string(fromByteCount: bytes)
    }
```

Finally, replace `addToDownloads` in `LinkGrabberView` in full, to build resolver-aware items instead of bare URLs:

```swift
    /// Hands the package's links to the engine, then clears them out of the
    /// grabber. A YouTube link (`resolvedMedia` + `selectedFormatID` both
    /// present) goes through `YouTubeHandoff`, which may produce one item or
    /// a muxed pair; every other link becomes the same plain
    /// `DownloadItem(url:filename:)` this always built.
    private func addToDownloads(_ package: PackageCandidate, startImmediately: Bool) {
        let links = controller.links(inPackage: package.id)
        let name = package.name
        let linkIDs = package.linkIDs
        Task {
            let items = links.flatMap { link -> [DownloadItem] in
                if let media = link.resolvedMedia, let formatID = link.selectedFormatID {
                    return YouTubeHandoff.makeDownloadItems(for: media, selectedFormatID: formatID)
                }
                let filename =
                    link.effectiveFilename.isEmpty ? "download" : link.effectiveFilename
                return [DownloadItem(url: link.originalURL, filename: filename)]
            }
            await engineController.addPackage(name: name, items: items, startImmediately: startImmediately)
            for linkID in linkIDs {
                await controller.removeLink(linkID)
            }
        }
    }
```

- [ ] **Step 2: Build the app target**

Run: `xcodebuild -project SDM.xcodeproj -scheme SDM -configuration Debug build 2>&1 | tail -40`
Expected: `BUILD SUCCEEDED`

- [ ] **Step 3: Commit**

```bash
git add SDM/LinkGrabberView.swift
git commit -m "feat: show resolver state/format picker in the Linkgrabber and hand off through YouTubeHandoff"
```

---

### Task 22: Full-suite verification and formatting pass

**Files:** none (verification only)

- [ ] **Step 1: Format everything**

```bash
swift-format --in-place --recursive SDMKit/Sources SDMKit/Tests
```

- [ ] **Step 2: Run the entire `SDMKit` test suite**

```bash
swift test --package-path SDMKit 2>&1 | tail -60
```

Expected: every test passes, including every pre-existing Phase 1–4 test (this task exists specifically to catch anything the `DownloadItem`/`ProbedLink`/`DownloadEngine` signature changes in Tasks 10–12/15 might have silently broken elsewhere).

- [ ] **Step 3: Build the full app target**

```bash
xcodebuild -project SDM.xcodeproj -scheme SDM -configuration Debug build 2>&1 | tail -60
```

Expected: `BUILD SUCCEEDED`.

- [ ] **Step 4: Format-check the diff is clean and commit if `swift-format` changed anything**

```bash
git status --short
```

If `swift-format` (Step 1) touched any file beyond what each task's own commit already captured, stage and commit it now:

```bash
git add -u SDMKit
git commit -m "style: swift-format pass over Phase 5 files"
```

---

## Completion criteria

- [ ] `SDMResolver` target exists, depended on by `SDMGrabber` and the `SDM` app target, never by `SDMEngine`.
- [ ] `LinkResolver`/`ResolvedMedia` (spec §8's exact protocol shape) implemented by `YouTubeResolver`, backed by `yt-dlp -J`, with `--cookies-from-browser` driven by `CookiesSource`.
- [ ] `YtDlpFormat`/`YtDlpExtractionResult` decode real yt-dlp `-J` JSON (verified against a real, hand-captured fixture).
- [ ] `QualitySelector` picks a format from the resolution ladder / codec / container / max-filesize preference, table-tested per spec §11.6.
- [ ] Per-link format override (`GrabberSession.selectFormat`) updates the displayed filesize instantly with no re-probe and no network call — spec §8.
- [ ] Video-only formats are paired with an audio track (`MuxPlanner`) and muxed via `ffmpeg -c copy` (`FFmpegMuxer`) once both download items complete (`DownloadEngine.handleMuxIfNeeded`) — spec §8.
- [ ] A 403 on a resolver-bound item's URL triggers `URLRefresher.refreshURL(for:)` and resumes against the refreshed URL, rather than retrying the same expired one forever — spec §5.3, and the exact gap named in Phase 1's "Deferred to later phases."
- [ ] HLS/DASH-manifest-only formats have a `WholesaleDownloader` fallback with real progress parsing (`YtDlpProgressParser`), kept outside `DownloadEngine`'s segmented machinery — spec §8.
- [ ] A missing yt-dlp installation surfaces as `ResolverState.toolMissing` with a copyable `brew install yt-dlp` instruction, never a crash or a silently stuck link — spec §8.
- [ ] Settings has a YouTube tab: resolution/codec/filesize-cap controls, and a cookies-source picker (None/Safari/Chrome, default Safari) with a red warning under it when Chrome is selected.
- [ ] Every new `SDMKit` file passes `swift test --package-path SDMKit`; the full `SDM` app target builds.
- [ ] Every commit in this plan is a real, working state — no task leaves the build red.

## What to verify visually (not done as part of this plan — build and run SDM yourself)

1. **Settings → YouTube tab.** Confirm the resolution/codec/filesize controls render, and that selecting Chrome in the cookies picker shows the red warning text immediately, disappearing again on Safari/None.
2. **A real YouTube grab.** With Safari selected as the cookies source (the default), paste `https://www.youtube.com/watch?v=IlIJa_FDK-0` into the Linkgrabber's Add Links sheet. Confirm: the row goes through a brief "resolving" state, then shows a format badge (e.g. "1080p · mp4") rather than a generic "online." Click the format badge to confirm the picker menu lists the other available resolutions and that choosing one changes the displayed filesize instantly with no visible re-probe.
3. **A real download.** Click "Add and start" on that package and confirm the download actually runs to completion through the normal Downloads list — segmented progress bar, speed, the works — proving the resolved googlevideo URL really does work with SDM's existing `Range`-based engine.
4. **Missing-tool handling.** Temporarily rename `/usr/local/bin/yt-dlp` (or point `ExternalToolLocator`'s search paths away from it, whichever is easier to undo) and re-grab the same link; confirm it shows "requires yt-dlp" with a copyable `brew install yt-dlp` line instead of hanging or crashing. Restore the binary afterward.
5. **Chrome cookies path.** Switch Settings to Chrome and re-grab a YouTube link once. Confirm SDM either succeeds silently or macOS's own keychain-access prompt appears (expected, per the Settings warning) — this is the one item this plan cannot verify headlessly at all, since it depends on the real macOS keychain UI.
6. **A genuinely video-only pick (muxing).** In the format picker, deliberately choose the highest resolution available (YouTube typically only offers 1080p+ as video-only DASH). Add and start it; confirm two items briefly appear in the download list, then collapse into one finished item once muxing completes, and that the resulting file plays back with both video and audio (open it in QuickTime Player).
7. **A 403-refresh in practice** is hard to trigger on demand (it needs a real signed URL to actually expire, which takes hours) — this plan's engine-level test (Task 11) is the real coverage here; no manual step is prescribed for it.

## Deferred to later phases

Deliberately **not** in Phase 5:

- **Bundling yt-dlp and ffmpeg.** Spec §14, unchanged — both remain user-installed Homebrew binaries, located via `ExternalToolLocator`'s fixed search paths.
- **A resolver for any site other than YouTube.** `LinkResolver` is a real extension seam (spec §8's own framing), but no second implementation is written here.
- **Playlist URLs.** `LinkResolver.resolve(_:)` returns `[ResolvedMedia]` to accommodate a future playlist resolving to many videos, but `YouTubeResolver`'s current implementation always returns exactly one — a playlist URL passed to it today resolves whatever yt-dlp's default single-video behavior for that URL shape produces, untested and unspecified by this plan.
- **Live-progress UI for the wholesale-download fallback.** `WholesaleDownloader.download(...)`'s `onProgress` callback exists and is unit-tested (Task 14), but no view in `SDM/` subscribes to it — the fallback path has no dedicated list row of its own yet. Wiring a small `WholesaleDownloadController` (mentioned as a possibility in this plan's own research, not committed to here) is left for whenever the fallback path is observed to actually matter in practice, matching spec §8's own framing of it as "should be rare."
- **Per-error-class retry policies, beyond treating 403 as the one special case that triggers a refresh.** `RetryPolicy.classify` is otherwise untouched by this plan.
- **A theme-level `warning`/`danger` role.** The Chrome-cookies warning reuses `theme.failedColor` (see Task 20's rationale) rather than adding a new semantic role to `Theme` and all fourteen bundled palettes.
- **Custom theme editor, theme import, activation-policy/Liquid Glass polish beyond what Phase 4 already shipped.** Untouched by this plan.
- **Relocating a running or completed item's already-downloaded bytes when moved cross-package, and live-updating `GrabberSession.Budget` when Settings changes.** Both carried forward unchanged from Phase 3's own deferred list (repeated again in Phase 4's); nothing in this plan touches either.

