# Phase 5 Part 1 — SDMResolve & the Resolver Seam: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the `LinkResolver` seam to `SDMCore` and a new `SDMResolve` SPM target containing a yt-dlp-backed resolver, a pure format-selection function, a subprocess runner, and a binary locator — with no changes to `SDMEngine` or `SDMGrabber`.

**Architecture:** `SDMCore` gains the `LinkResolver` protocol and the value types it trades in (`ResolvedTarget`, `ResolvedMedia`, `MediaFormat`, `QualityPreferences`, `FormatChoice`, `RefreshedFormat`). A new `SDMResolve` target (depends only on `SDMCore`, mirroring `SDMEngine`/`SDMGrabber`) holds `SystemProcessRunner` (a `Foundation.Process` wrapper), `BinaryLocator`, `FormatSelector` (a pure function with a fixture table), and `YtDlpResolver` (parses `yt-dlp -J` output into `ResolvedMedia`, selects formats, and refreshes expired URLs). All subprocess and filesystem access is injected as a protocol/closure so every test runs with no yt-dlp binary and no network.

**Tech Stack:** Swift 6 language mode (strict concurrency), Swift Testing (`@Test`/`#expect`), local SPM package `SDMKit`, `Foundation.Process`.

**Spec:** `docs/superpowers/specs/2026-09-02-phase-5-youtube-resolver-design.md` (Part 1 implements §3 `SDMResolve` row, §4 in full, and the §10.1–§10.2 test sections). Parts 2 (multi-component engine) and 3 (grabber + UI) get their own plans.

## Global Constraints

- **Swift 6 language mode, strict concurrency.** Every type crossing an
  `async` boundary is `Sendable`. Copied verbatim from parent spec §2.
- **macOS 15.0 baseline**; macOS 26 APIs (none in Part 1) behind `if #available`.
- **Unsandboxed.** `Foundation.Process` is available and used directly.
- **No test may touch the network, spawn the real `yt-dlp`/`ffmpeg`, or sleep on
  a real clock for timing assertions.** Parent spec §11.1 / §10. One
  `SystemProcessRunner` test may exec `/bin/echo` and `/bin/sleep` (local,
  deterministic, fast) — that is the sole exception and is not network.
- **Format Swift with `swift-format` before every commit:** run
  `./format.sh` (or `swift-format format -ri SDMKit`) as the last action before
  each `git commit`.
- **Tests live in `SDMKit/Tests/<TargetName>Tests/`.** Run the suite with
  `cd SDMKit && swift test`; run one test with
  `cd SDMKit && swift test --filter '<TypeName>/<testName>'` and **always check
  the reported test count is non-zero** (a mistyped filter silently matches 0 —
  see the project's `swift-testing-filter-gotcha` note).
- Fixed priority orders, **not user-configurable**: video `av1 > vp9 > h264`;
  container `mp4 > webm > (others alphabetical)`; audio `opus > aac > (others
  alphabetical)`. Parent spec §4.2.
- Playlist cap default **50**, range **10–200**. Parent spec §9.4.
- `canHandle` host set: `youtube.com`, `www.youtube.com`, `m.youtube.com`,
  `music.youtube.com`, `youtu.be`. Parent spec §4.4.

---

## File Structure

**`SDMKit/Sources/SDMCore/` (new files):**

| File | Responsibility |
|---|---|
| `MediaFormat.swift` | `MediaFormat`, `MediaKind`, `VideoCodec`, `AudioCodec`, `MediaContainer` value types + fixed priority orders + `filesizeEffective` |
| `ResolvedMedia.swift` | `ResolvedMedia`, `ResolvedTarget` |
| `QualityPreferences.swift` | `QualityPreferences`, `FormatChoice` |
| `LinkResolver.swift` | `LinkResolver` protocol, `RefreshedFormat`, `ResolveError` |

**`SDMKit/Sources/SDMResolve/` (new target):**

| File | Responsibility |
|---|---|
| `ProcessRunner.swift` | `ProcessRunner` protocol, `ProcessOutput`, `ProcessRunError` |
| `SystemProcessRunner.swift` | `Foundation.Process` implementation with timeout + cancellation |
| `BinaryLocator.swift` | `BinaryLocator` actor: override → search-path scan → `nil`, with re-scan |
| `FormatSelector.swift` | pure `FormatSelector.pick(_:_:)` + the shared rank comparator |
| `CookieSource.swift` | `CookieSource` enum + its `--cookies-from-browser` argument |
| `YtDlpJSON.swift` | `Codable` structs mirroring `yt-dlp -J` output + `toResolvedMedia()` mapping |
| `YtDlpResolver.swift` | `YtDlpResolver: LinkResolver` — `canHandle`, `resolve`, `refresh` |

**`SDMKit/Tests/` (new):**

| File | Covers |
|---|---|
| `SDMCoreTests/MediaFormatTests.swift` | priority orders, `filesizeEffective` |
| `SDMResolveTests/FormatSelectorTests.swift` | the §10.1 fixture table |
| `SDMResolveTests/SystemProcessRunnerTests.swift` | exit code, stdout/stderr capture, timeout, cancellation |
| `SDMResolveTests/BinaryLocatorTests.swift` | override precedence, search-path scan, miss, re-scan |
| `SDMResolveTests/YtDlpJSONTests.swift` | `-J` → `ResolvedMedia` mapping |
| `SDMResolveTests/YtDlpResolverTests.swift` | `canHandle`, `resolve` (video + playlist + error paths), `refresh` |
| `SDMResolveTests/Fixtures/*.json` | recorded/trimmed `yt-dlp -J` output |
| `SDMResolveTests/Support.swift` | `FakeProcessRunner`, fixture loader |

**`SDMKit/Package.swift`:** add the `SDMResolve` library product, target, and test target.

---

## Task 1: Value types in `SDMCore` — media formats

**Files:**
- Create: `SDMKit/Sources/SDMCore/MediaFormat.swift`
- Create: `SDMKit/Sources/SDMCore/ResolvedMedia.swift`
- Test: `SDMKit/Tests/SDMCoreTests/MediaFormatTests.swift`

**Interfaces:**
- Produces:
  - `enum MediaKind: String, Sendable, Codable, Equatable { case progressive, videoOnly, audioOnly }`
  - `enum VideoCodec: Sendable, Codable, Equatable, Hashable { case av1, vp9, h264, other(String) }` with `static let priority: [VideoCodec] = [.av1, .vp9, .h264]` and `var rank: Int` (index in `priority`, or `Int.max` for `.other`)
  - `enum AudioCodec: Sendable, Codable, Equatable, Hashable { case opus, aac, other(String) }` with `static let priority: [AudioCodec] = [.opus, .aac]` and `var rank: Int`
  - `enum MediaContainer: Sendable, Codable, Equatable, Hashable { case mp4, webm, m4a, other(String) }` with `static let priority: [MediaContainer] = [.mp4, .webm]` and `var rank: Int`; `var fileExtension: String`
  - `struct MediaFormat: Sendable, Codable, Equatable, Identifiable` — fields per parent spec §4.2 (`id: String`, `kind: MediaKind`, `height: Int?`, `width: Int?`, `vcodec: VideoCodec?`, `acodec: AudioCodec?`, `container: MediaContainer`, `filesize: Int64?`, `filesizeApprox: Int64?`, `tbr: Double?`, `url: URL`); computed `var filesizeEffective: Int64?` returning `filesize ?? filesizeApprox` (nil when both nil); `var isApproximateSize: Bool` (`filesize == nil`)
  - `struct ResolvedMedia: Sendable, Codable, Equatable` — `extractor: String`, `videoID: String`, `title: String`, `durationSeconds: Double?`, `formats: [MediaFormat]`
  - `enum ResolvedTarget: Sendable { case single(ResolvedMedia); case playlist(title: String, entries: [ResolvedMedia], totalAvailable: Int) }`

- [ ] **Step 1: Write the failing test**

```swift
// SDMKit/Tests/SDMCoreTests/MediaFormatTests.swift
import Foundation
import Testing

@testable import SDMCore

@Test func videoCodecPriorityOrdersAv1FirstH264Last() {
    #expect(VideoCodec.av1.rank < VideoCodec.vp9.rank)
    #expect(VideoCodec.vp9.rank < VideoCodec.h264.rank)
    #expect(VideoCodec.h264.rank < VideoCodec.other("theora").rank)
}

@Test func containerPriorityOrdersMp4BeforeWebm() {
    #expect(MediaContainer.mp4.rank < MediaContainer.webm.rank)
    #expect(MediaContainer.webm.rank < MediaContainer.other("mkv").rank)
}

@Test func audioCodecPriorityOrdersOpusBeforeAac() {
    #expect(AudioCodec.opus.rank < AudioCodec.aac.rank)
}

@Test func filesizeEffectivePrefersExactThenApproxThenNil() {
    let exact = MediaFormat(
        id: "137", kind: .videoOnly, height: 1080, width: 1920, vcodec: .h264,
        acodec: nil, container: .mp4, filesize: 100, filesizeApprox: 90, tbr: 4000,
        url: URL(string: "https://x/1")!)
    #expect(exact.filesizeEffective == 100)
    #expect(exact.isApproximateSize == false)

    let approx = MediaFormat(
        id: "248", kind: .videoOnly, height: 1080, width: 1920, vcodec: .vp9,
        acodec: nil, container: .webm, filesize: nil, filesizeApprox: 90, tbr: 3800,
        url: URL(string: "https://x/2")!)
    #expect(approx.filesizeEffective == 90)
    #expect(approx.isApproximateSize == true)

    let unknown = MediaFormat(
        id: "600", kind: .videoOnly, height: 1080, width: 1920, vcodec: .av1,
        acodec: nil, container: .mp4, filesize: nil, filesizeApprox: nil, tbr: nil,
        url: URL(string: "https://x/3")!)
    #expect(unknown.filesizeEffective == nil)
}

@Test func mediaContainerFileExtension() {
    #expect(MediaContainer.mp4.fileExtension == "mp4")
    #expect(MediaContainer.other("mkv").fileExtension == "mkv")
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd SDMKit && swift test --filter 'MediaFormatTests'`
Expected: FAIL — `cannot find 'VideoCodec' in scope` (build error).

- [ ] **Step 3: Write the implementation**

```swift
// SDMKit/Sources/SDMCore/MediaFormat.swift
import Foundation

public enum MediaKind: String, Sendable, Codable, Equatable {
    case progressive, videoOnly, audioOnly
}

public enum VideoCodec: Sendable, Codable, Equatable, Hashable {
    case av1, vp9, h264, other(String)

    public static let priority: [VideoCodec] = [.av1, .vp9, .h264]

    public var rank: Int {
        VideoCodec.priority.firstIndex(of: self) ?? Int.max
    }
}

public enum AudioCodec: Sendable, Codable, Equatable, Hashable {
    case opus, aac, other(String)

    public static let priority: [AudioCodec] = [.opus, .aac]

    public var rank: Int {
        AudioCodec.priority.firstIndex(of: self) ?? Int.max
    }
}

public enum MediaContainer: Sendable, Codable, Equatable, Hashable {
    case mp4, webm, m4a, other(String)

    public static let priority: [MediaContainer] = [.mp4, .webm]

    public var rank: Int {
        MediaContainer.priority.firstIndex(of: self) ?? Int.max
    }

    public var fileExtension: String {
        switch self {
        case .mp4: return "mp4"
        case .webm: return "webm"
        case .m4a: return "m4a"
        case .other(let ext): return ext
        }
    }
}

public struct MediaFormat: Sendable, Codable, Equatable, Identifiable {
    public let id: String
    public var kind: MediaKind
    public var height: Int?
    public var width: Int?
    public var vcodec: VideoCodec?
    public var acodec: AudioCodec?
    public var container: MediaContainer
    public var filesize: Int64?
    public var filesizeApprox: Int64?
    public var tbr: Double?
    public var url: URL

    public init(
        id: String, kind: MediaKind, height: Int?, width: Int?,
        vcodec: VideoCodec?, acodec: AudioCodec?, container: MediaContainer,
        filesize: Int64?, filesizeApprox: Int64?, tbr: Double?, url: URL
    ) {
        self.id = id
        self.kind = kind
        self.height = height
        self.width = width
        self.vcodec = vcodec
        self.acodec = acodec
        self.container = container
        self.filesize = filesize
        self.filesizeApprox = filesizeApprox
        self.tbr = tbr
        self.url = url
    }

    public var filesizeEffective: Int64? { filesize ?? filesizeApprox }
    public var isApproximateSize: Bool { filesize == nil }
}
```

```swift
// SDMKit/Sources/SDMCore/ResolvedMedia.swift
import Foundation

public struct ResolvedMedia: Sendable, Codable, Equatable {
    public var extractor: String
    public var videoID: String
    public var title: String
    public var durationSeconds: Double?
    public var formats: [MediaFormat]

    public init(
        extractor: String, videoID: String, title: String,
        durationSeconds: Double?, formats: [MediaFormat]
    ) {
        self.extractor = extractor
        self.videoID = videoID
        self.title = title
        self.durationSeconds = durationSeconds
        self.formats = formats
    }
}

public enum ResolvedTarget: Sendable {
    case single(ResolvedMedia)
    case playlist(title: String, entries: [ResolvedMedia], totalAvailable: Int)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd SDMKit && swift test --filter 'MediaFormatTests'`
Expected: PASS, 5 tests.

- [ ] **Step 5: Format and commit**

```bash
./format.sh
git add SDMKit/Sources/SDMCore/MediaFormat.swift SDMKit/Sources/SDMCore/ResolvedMedia.swift SDMKit/Tests/SDMCoreTests/MediaFormatTests.swift
git commit -m "feat(core): media format value types for the resolver seam"
```

---

## Task 2: `QualityPreferences`, `FormatChoice`, `LinkResolver` protocol

**Files:**
- Create: `SDMKit/Sources/SDMCore/QualityPreferences.swift`
- Create: `SDMKit/Sources/SDMCore/LinkResolver.swift`
- Test: `SDMKit/Tests/SDMCoreTests/MediaFormatTests.swift` (append)

**Interfaces:**
- Consumes: `MediaFormat`, `VideoCodec`, `AudioCodec`, `MediaContainer`, `ResolvedTarget` (Task 1).
- Produces:
  - `struct QualityPreferences: Sendable, Codable, Equatable` — `maxHeight: Int`, `videoCodecs: Set<VideoCodec>`, `containers: Set<MediaContainer>`, `audioCodecs: Set<AudioCodec>`; `static let `default` = QualityPreferences(maxHeight: 1080, videoCodecs: [.av1, .vp9, .h264], containers: [.mp4, .webm], audioCodecs: [.opus, .aac])`
  - `struct FormatChoice: Sendable, Codable, Equatable` — `video: MediaFormat?`, `audio: MediaFormat?`, `outputContainer: MediaContainer`, `estimatedBytes: Int64?`; `var requiresMux: Bool { video != nil && audio != nil }`; `var formatIDs: [String]` (non-nil components' ids, video first)
  - `protocol LinkResolver: Sendable` with `func canHandle(_ url: URL) -> Bool`, `func resolve(_ url: URL) async throws -> ResolvedTarget`, `func refresh(extractor: String, videoID: String, formatID: String) async throws -> RefreshedFormat`
  - `struct RefreshedFormat: Sendable, Equatable` — `url: URL`, `filesize: Int64?`, `formatID: String`
  - `enum ResolveError: Error, Equatable, Sendable { case binaryMissing; case unsupported; case formatGone; case authRequired; case privateVideo; case unavailable; case timeout; case ytDlpFailed(stderrTail: String) }`

- [ ] **Step 1: Write the failing test** (append to `MediaFormatTests.swift`)

```swift
@Test func defaultQualityPreferencesAcceptEverythingUpTo1080() {
    let prefs = QualityPreferences.default
    #expect(prefs.maxHeight == 1080)
    #expect(prefs.videoCodecs == [.av1, .vp9, .h264])
    #expect(prefs.containers == [.mp4, .webm])
    #expect(prefs.audioCodecs == [.opus, .aac])
}

@Test func formatChoiceRequiresMuxOnlyWhenBothStreamsPresent() {
    let v = MediaFormat(
        id: "137", kind: .videoOnly, height: 1080, width: 1920, vcodec: .h264,
        acodec: nil, container: .mp4, filesize: 100, filesizeApprox: nil, tbr: nil,
        url: URL(string: "https://x/v")!)
    let a = MediaFormat(
        id: "140", kind: .audioOnly, height: nil, width: nil, vcodec: nil,
        acodec: .aac, container: .m4a, filesize: 10, filesizeApprox: nil, tbr: nil,
        url: URL(string: "https://x/a")!)
    let muxed = FormatChoice(video: v, audio: a, outputContainer: .mp4, estimatedBytes: 110)
    #expect(muxed.requiresMux)
    #expect(muxed.formatIDs == ["137", "140"])

    let progressive = FormatChoice(video: v, audio: nil, outputContainer: .mp4, estimatedBytes: 100)
    #expect(progressive.requiresMux == false)
    #expect(progressive.formatIDs == ["137"])
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd SDMKit && swift test --filter 'MediaFormatTests'`
Expected: FAIL — `cannot find 'QualityPreferences' in scope`.

- [ ] **Step 3: Write the implementation**

```swift
// SDMKit/Sources/SDMCore/QualityPreferences.swift
import Foundation

public struct QualityPreferences: Sendable, Codable, Equatable {
    public var maxHeight: Int
    public var videoCodecs: Set<VideoCodec>
    public var containers: Set<MediaContainer>
    public var audioCodecs: Set<AudioCodec>

    public init(
        maxHeight: Int, videoCodecs: Set<VideoCodec>,
        containers: Set<MediaContainer>, audioCodecs: Set<AudioCodec>
    ) {
        self.maxHeight = maxHeight
        self.videoCodecs = videoCodecs
        self.containers = containers
        self.audioCodecs = audioCodecs
    }

    public static let `default` = QualityPreferences(
        maxHeight: 1080,
        videoCodecs: [.av1, .vp9, .h264],
        containers: [.mp4, .webm],
        audioCodecs: [.opus, .aac])
}

public struct FormatChoice: Sendable, Codable, Equatable {
    public var video: MediaFormat?
    public var audio: MediaFormat?
    public var outputContainer: MediaContainer
    public var estimatedBytes: Int64?

    public init(
        video: MediaFormat?, audio: MediaFormat?,
        outputContainer: MediaContainer, estimatedBytes: Int64?
    ) {
        self.video = video
        self.audio = audio
        self.outputContainer = outputContainer
        self.estimatedBytes = estimatedBytes
    }

    public var requiresMux: Bool { video != nil && audio != nil }

    public var formatIDs: [String] {
        [video?.id, audio?.id].compactMap { $0 }
    }
}
```

```swift
// SDMKit/Sources/SDMCore/LinkResolver.swift
import Foundation

public struct RefreshedFormat: Sendable, Equatable {
    public var url: URL
    public var filesize: Int64?
    public var formatID: String

    public init(url: URL, filesize: Int64?, formatID: String) {
        self.url = url
        self.filesize = filesize
        self.formatID = formatID
    }
}

public enum ResolveError: Error, Equatable, Sendable {
    case binaryMissing
    case unsupported
    case formatGone
    case authRequired
    case privateVideo
    case unavailable
    case timeout
    case ytDlpFailed(stderrTail: String)
}

/// The extension seam for extractor-backed sites. Generic HTTP has no
/// resolver — a `nil` resolver, or `canHandle == false`, means "use the
/// existing probe/download path." Parent spec §4.1.
public protocol LinkResolver: Sendable {
    /// Cheap, synchronous, no I/O — a host/path check only.
    func canHandle(_ url: URL) -> Bool

    /// Grab-time resolution. May shell out; may take seconds.
    func resolve(_ url: URL) async throws -> ResolvedTarget

    /// Mid-download URL refresh for one expired component.
    func refresh(
        extractor: String, videoID: String, formatID: String
    ) async throws -> RefreshedFormat
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd SDMKit && swift test --filter 'MediaFormatTests'`
Expected: PASS, 7 tests.

- [ ] **Step 5: Format and commit**

```bash
./format.sh
git add SDMKit/Sources/SDMCore/QualityPreferences.swift SDMKit/Sources/SDMCore/LinkResolver.swift SDMKit/Tests/SDMCoreTests/MediaFormatTests.swift
git commit -m "feat(core): QualityPreferences, FormatChoice, LinkResolver protocol"
```

---

## Task 3: Add the `SDMResolve` target

**Files:**
- Modify: `SDMKit/Package.swift`
- Create: `SDMKit/Sources/SDMResolve/CookieSource.swift`
- Create: `SDMKit/Tests/SDMResolveTests/CookieSourceTests.swift`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces:
  - `enum CookieSource: String, Sendable, Codable, Equatable, CaseIterable { case none, safari, chrome, firefox, edge, brave }`
  - `var CookieSource.ytDlpArguments: [String]` — `[]` for `.none`, else `["--cookies-from-browser", rawValue]`

- [ ] **Step 1: Add target to `Package.swift`**

In `products`, after the `SDMGrabber` library line, add:
```swift
        .library(name: "SDMResolve", targets: ["SDMResolve"]),
```
In `targets`, after the `SDMGrabber` target line, add:
```swift
        .target(name: "SDMResolve", dependencies: ["SDMCore"]),
```
After the `SDMGrabberTests` test target line, add:
```swift
        .testTarget(name: "SDMResolveTests", dependencies: ["SDMResolve"]),
```

- [ ] **Step 2: Write the failing test**

```swift
// SDMKit/Tests/SDMResolveTests/CookieSourceTests.swift
import Testing

@testable import SDMResolve

@Test func noneProducesNoArguments() {
    #expect(CookieSource.none.ytDlpArguments == [])
}

@Test func chromeProducesCookiesFromBrowserArgument() {
    #expect(CookieSource.chrome.ytDlpArguments == ["--cookies-from-browser", "chrome"])
}

@Test func everyBrowserCaseHasARawValue() {
    for source in CookieSource.allCases where source != .none {
        #expect(source.ytDlpArguments.count == 2)
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cd SDMKit && swift test --filter 'CookieSourceTests'`
Expected: FAIL — no such module `SDMResolve` / `CookieSource` not found.

- [ ] **Step 4: Write the implementation**

```swift
// SDMKit/Sources/SDMResolve/CookieSource.swift
import Foundation

/// Which browser's cookie jar yt-dlp should borrow for age-restricted,
/// private, or members-only videos. Parent spec §8.
public enum CookieSource: String, Sendable, Codable, Equatable, CaseIterable {
    case none, safari, chrome, firefox, edge, brave

    public var ytDlpArguments: [String] {
        switch self {
        case .none: return []
        default: return ["--cookies-from-browser", rawValue]
        }
    }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd SDMKit && swift test --filter 'CookieSourceTests'`
Expected: PASS, 3 tests.

- [ ] **Step 6: Run the whole suite to confirm nothing broke**

Run: `cd SDMKit && swift test`
Expected: PASS — existing count + the new tests from Tasks 1–3.

- [ ] **Step 7: Format and commit**

```bash
./format.sh
git add SDMKit/Package.swift SDMKit/Sources/SDMResolve/CookieSource.swift SDMKit/Tests/SDMResolveTests/CookieSourceTests.swift
git commit -m "feat(resolve): add SDMResolve target with CookieSource"
```

---

## Task 4: `FormatSelector` — the pure selection function

**Files:**
- Create: `SDMKit/Sources/SDMResolve/FormatSelector.swift`
- Create: `SDMKit/Tests/SDMResolveTests/FormatSelectorTests.swift`

**Interfaces:**
- Consumes: `MediaFormat`, `MediaKind`, `VideoCodec`, `AudioCodec`, `MediaContainer`, `ResolvedMedia`, `QualityPreferences`, `FormatChoice` (Tasks 1–2).
- Produces:
  - `enum FormatSelector` with `static func pick(_ media: ResolvedMedia, _ prefs: QualityPreferences) -> FormatChoice?`
  - `static func rankedVideoFormats(_ media: ResolvedMedia, _ prefs: QualityPreferences) -> [MediaFormat]` (eligible video/progressive formats, best first) — reused by the picker UI in Part 3
  - `static func rankedAudioFormats(_ media: ResolvedMedia, _ prefs: QualityPreferences) -> [MediaFormat]` (eligible audio formats, best first)
  - `static func videoRankLess(_ a: MediaFormat, _ b: MediaFormat) -> Bool` — the shared comparator: higher `height` first, then `vcodec.rank`, then `container.rank`, then higher `tbr`

**Selection algorithm (parent spec §4.3):**
1. Eligible video = `kind ∈ {progressive, videoOnly}`, `height ?? 0 <= prefs.maxHeight`, `vcodec` present and `∈ prefs.videoCodecs`, `container ∈ prefs.containers`.
2. Rank by `videoRankLess`; take the top `V`.
3. `V.kind == .progressive` → `FormatChoice(video: V, audio: nil, outputContainer: V.container, estimatedBytes: V.filesizeEffective)`.
4. Else pick best eligible audio `A` (`kind == .audioOnly`, `acodec` present and `∈ prefs.audioCodecs`; rank by `acodec.rank` then higher `tbr`). No `A` → `nil`.
5. `FormatChoice(video: V, audio: A, outputContainer: V.container, estimatedBytes: sum of filesizeEffective, nil if either unknown)`.
6. No eligible video at all: if the media has **no** video-bearing formats (`kind == .videoOnly || .progressive`) then treat as audio-only → best eligible audio as `FormatChoice(video: nil, audio: A, outputContainer: A.container, ...)`, `nil` if no eligible audio. Otherwise (videos exist but all filtered out) → `nil`.

- [ ] **Step 1: Write the failing test**

```swift
// SDMKit/Tests/SDMResolveTests/FormatSelectorTests.swift
import Foundation
import Testing

@testable import SDMResolve

@testable import SDMCore

private func vf(
    _ id: String, _ height: Int, _ vcodec: VideoCodec, _ container: MediaContainer,
    size: Int64 = 1000, tbr: Double = 1000, progressive: Bool = false, acodec: AudioCodec? = nil
) -> MediaFormat {
    MediaFormat(
        id: id, kind: progressive ? .progressive : .videoOnly, height: height, width: height * 16 / 9,
        vcodec: vcodec, acodec: acodec, container: container, filesize: size,
        filesizeApprox: nil, tbr: tbr, url: URL(string: "https://gv/\(id)")!)
}

private func af(
    _ id: String, _ acodec: AudioCodec, _ container: MediaContainer,
    size: Int64 = 100, tbr: Double = 128
) -> MediaFormat {
    MediaFormat(
        id: id, kind: .audioOnly, height: nil, width: nil, vcodec: nil, acodec: acodec,
        container: container, filesize: size, filesizeApprox: nil, tbr: tbr,
        url: URL(string: "https://gv/\(id)")!)
}

private func media(_ formats: [MediaFormat]) -> ResolvedMedia {
    ResolvedMedia(
        extractor: "youtube", videoID: "abc", title: "T", durationSeconds: 10, formats: formats)
}

@Test func picksHighestResolutionThenCodecThenContainer() {
    let m = media([
        vf("v720av1", 720, .av1, .webm),
        vf("v1080vp9", 1080, .vp9, .webm),
        vf("v1080h264", 1080, .h264, .mp4),
        af("a", .opus, .webm),
    ])
    let choice = FormatSelector.pick(m, .default)
    #expect(choice?.video?.id == "v1080vp9")  // 1080 beats 720; vp9 beats h264
    #expect(choice?.audio?.id == "a")
    #expect(choice?.outputContainer == .webm)
    #expect(choice?.requiresMux == true)
}

@Test func progressiveFormatNeedsNoAudioAndNoMux() {
    let m = media([vf("prog", 720, .h264, .mp4, progressive: true, acodec: .aac)])
    let choice = FormatSelector.pick(m, .default)
    #expect(choice?.video?.id == "prog")
    #expect(choice?.audio == nil)
    #expect(choice?.requiresMux == false)
    #expect(choice?.outputContainer == .mp4)
}

@Test func maxHeightCapDropsHigherFormats() {
    var prefs = QualityPreferences.default
    prefs.maxHeight = 720
    let m = media([vf("v2160", 2160, .av1, .mp4), vf("v720", 720, .h264, .mp4), af("a", .aac, .m4a)])
    #expect(FormatSelector.pick(m, prefs)?.video?.id == "v720")
}

@Test func codecAllowlistExcludingTopFormat() {
    var prefs = QualityPreferences.default
    prefs.videoCodecs = [.h264]
    let m = media([vf("av1", 1080, .av1, .mp4), vf("h264", 1080, .h264, .mp4), af("a", .aac, .m4a)])
    #expect(FormatSelector.pick(m, prefs)?.video?.id == "h264")
}

@Test func videoOnlyWithNoEligibleAudioReturnsNil() {
    var prefs = QualityPreferences.default
    prefs.audioCodecs = [.opus]
    let m = media([vf("v", 1080, .av1, .webm), af("a", .aac, .m4a)])
    #expect(FormatSelector.pick(m, prefs) == nil)
}

@Test func noEligibleVideoButVideosExistReturnsNil() {
    var prefs = QualityPreferences.default
    prefs.maxHeight = 144
    let m = media([vf("v1080", 1080, .av1, .webm), af("a", .opus, .webm)])
    #expect(FormatSelector.pick(m, prefs) == nil)
}

@Test func audioOnlyMediaPicksBestAudio() {
    let m = media([af("aac", .aac, .m4a), af("opus", .opus, .webm)])
    let choice = FormatSelector.pick(m, .default)
    #expect(choice?.video == nil)
    #expect(choice?.audio?.id == "opus")
    #expect(choice?.outputContainer == .webm)
}

@Test func estimatedBytesNilWhenAComponentSizeUnknown() {
    let vNoSize = MediaFormat(
        id: "v", kind: .videoOnly, height: 1080, width: 1920, vcodec: .av1, acodec: nil,
        container: .webm, filesize: nil, filesizeApprox: nil, tbr: nil,
        url: URL(string: "https://gv/v")!)
    let m = ResolvedMedia(
        extractor: "youtube", videoID: "abc", title: "T", durationSeconds: 10,
        formats: [vNoSize, af("a", .opus, .webm)])
    #expect(FormatSelector.pick(m, .default)?.estimatedBytes == nil)
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd SDMKit && swift test --filter 'FormatSelectorTests'`
Expected: FAIL — `cannot find 'FormatSelector' in scope`.

- [ ] **Step 3: Write the implementation**

```swift
// SDMKit/Sources/SDMResolve/FormatSelector.swift
import Foundation
import SDMCore

/// Pure selection of the best format(s) for a resolved video given the
/// user's quality preferences. Fixture-tested, no I/O. Parent spec §4.3.
public enum FormatSelector {
    public static func pick(
        _ media: ResolvedMedia, _ prefs: QualityPreferences
    ) -> FormatChoice? {
        let video = rankedVideoFormats(media, prefs).first
        let hasAnyVideo = media.formats.contains {
            $0.kind == .videoOnly || $0.kind == .progressive
        }

        if let video {
            if video.kind == .progressive {
                return FormatChoice(
                    video: video, audio: nil, outputContainer: video.container,
                    estimatedBytes: video.filesizeEffective)
            }
            guard let audio = rankedAudioFormats(media, prefs).first else { return nil }
            return FormatChoice(
                video: video, audio: audio, outputContainer: video.container,
                estimatedBytes: sumSize(video, audio))
        }

        // No eligible video. Only fall back to audio-only when the media
        // genuinely has no video-bearing formats at all.
        guard !hasAnyVideo, let audio = rankedAudioFormats(media, prefs).first else {
            return nil
        }
        return FormatChoice(
            video: nil, audio: audio, outputContainer: audio.container,
            estimatedBytes: audio.filesizeEffective)
    }

    public static func rankedVideoFormats(
        _ media: ResolvedMedia, _ prefs: QualityPreferences
    ) -> [MediaFormat] {
        media.formats
            .filter { format in
                (format.kind == .videoOnly || format.kind == .progressive)
                    && (format.height ?? 0) <= prefs.maxHeight
                    && format.vcodec.map { prefs.videoCodecs.contains($0) } == true
                    && prefs.containers.contains(format.container)
            }
            .sorted(by: videoRankLess)
    }

    public static func rankedAudioFormats(
        _ media: ResolvedMedia, _ prefs: QualityPreferences
    ) -> [MediaFormat] {
        media.formats
            .filter { format in
                format.kind == .audioOnly
                    && format.acodec.map { prefs.audioCodecs.contains($0) } == true
            }
            .sorted { a, b in
                let ra = a.acodec?.rank ?? Int.max
                let rb = b.acodec?.rank ?? Int.max
                if ra != rb { return ra < rb }
                return (a.tbr ?? 0) > (b.tbr ?? 0)
            }
    }

    /// Higher resolution first, then codec priority, then container
    /// priority, then higher bitrate. Shared with the picker UI so the
    /// auto-pick is always the first matching row in the list.
    public static func videoRankLess(_ a: MediaFormat, _ b: MediaFormat) -> Bool {
        if (a.height ?? 0) != (b.height ?? 0) { return (a.height ?? 0) > (b.height ?? 0) }
        let ca = a.vcodec?.rank ?? Int.max
        let cb = b.vcodec?.rank ?? Int.max
        if ca != cb { return ca < cb }
        if a.container.rank != b.container.rank { return a.container.rank < b.container.rank }
        return (a.tbr ?? 0) > (b.tbr ?? 0)
    }

    private static func sumSize(_ a: MediaFormat, _ b: MediaFormat) -> Int64? {
        guard let sa = a.filesizeEffective, let sb = b.filesizeEffective else { return nil }
        return sa + sb
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd SDMKit && swift test --filter 'FormatSelectorTests'`
Expected: PASS, 8 tests.

- [ ] **Step 5: Format and commit**

```bash
./format.sh
git add SDMKit/Sources/SDMResolve/FormatSelector.swift SDMKit/Tests/SDMResolveTests/FormatSelectorTests.swift
git commit -m "feat(resolve): pure FormatSelector with fixture table"
```

---

## Task 5: `ProcessRunner` protocol + `SystemProcessRunner`

**Files:**
- Create: `SDMKit/Sources/SDMResolve/ProcessRunner.swift`
- Create: `SDMKit/Sources/SDMResolve/SystemProcessRunner.swift`
- Create: `SDMKit/Tests/SDMResolveTests/SystemProcessRunnerTests.swift`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces:
  - `struct ProcessOutput: Sendable, Equatable { var stdout: Data; var stderr: Data; var exitCode: Int32 }`
  - `enum ProcessRunError: Error, Equatable, Sendable { case launchFailed(String); case timedOut }`
  - `protocol ProcessRunner: Sendable { func run(executable: URL, arguments: [String], timeout: Duration) async throws -> ProcessOutput }`
  - `struct SystemProcessRunner: ProcessRunner` — `public init()`

- [ ] **Step 1: Write the failing test**

```swift
// SDMKit/Tests/SDMResolveTests/SystemProcessRunnerTests.swift
import Foundation
import Testing

@testable import SDMResolve

@Test func capturesStdoutAndZeroExit() async throws {
    let runner = SystemProcessRunner()
    let out = try await runner.run(
        executable: URL(fileURLWithPath: "/bin/echo"),
        arguments: ["hello"], timeout: .seconds(5))
    #expect(out.exitCode == 0)
    #expect(String(decoding: out.stdout, as: UTF8.self) == "hello\n")
}

@Test func capturesNonZeroExit() async throws {
    let runner = SystemProcessRunner()
    let out = try await runner.run(
        executable: URL(fileURLWithPath: "/bin/sh"),
        arguments: ["-c", "exit 3"], timeout: .seconds(5))
    #expect(out.exitCode == 3)
}

@Test func timesOutALongProcess() async {
    let runner = SystemProcessRunner()
    await #expect(throws: ProcessRunError.timedOut) {
        try await runner.run(
            executable: URL(fileURLWithPath: "/bin/sleep"),
            arguments: ["10"], timeout: .milliseconds(200))
    }
}

@Test func throwsLaunchFailedForMissingExecutable() async {
    let runner = SystemProcessRunner()
    await #expect(throws: (any Error).self) {
        try await runner.run(
            executable: URL(fileURLWithPath: "/nonexistent/xyz"),
            arguments: [], timeout: .seconds(1))
    }
}

@Test func cancellationTerminatesTheProcess() async throws {
    let runner = SystemProcessRunner()
    let task = Task {
        try await runner.run(
            executable: URL(fileURLWithPath: "/bin/sleep"),
            arguments: ["10"], timeout: .seconds(30))
    }
    try await Task.sleep(for: .milliseconds(100))
    task.cancel()
    await #expect(throws: (any Error).self) { try await task.value }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd SDMKit && swift test --filter 'SystemProcessRunnerTests'`
Expected: FAIL — `cannot find 'SystemProcessRunner' in scope`.

- [ ] **Step 3: Write the implementation**

```swift
// SDMKit/Sources/SDMResolve/ProcessRunner.swift
import Foundation

public struct ProcessOutput: Sendable, Equatable {
    public var stdout: Data
    public var stderr: Data
    public var exitCode: Int32

    public init(stdout: Data, stderr: Data, exitCode: Int32) {
        self.stdout = stdout
        self.stderr = stderr
        self.exitCode = exitCode
    }
}

public enum ProcessRunError: Error, Equatable, Sendable {
    case launchFailed(String)
    case timedOut
}

/// Injected everywhere a subprocess is run, so tests never spawn the real
/// `yt-dlp`/`ffmpeg`. Parent spec §4.5 / §10.2.
public protocol ProcessRunner: Sendable {
    func run(
        executable: URL, arguments: [String], timeout: Duration
    ) async throws -> ProcessOutput
}
```

```swift
// SDMKit/Sources/SDMResolve/SystemProcessRunner.swift
import Foundation

/// `Foundation.Process` implementation. Unsandboxed target — this is
/// allowed. Kills the child on timeout or task cancellation.
public struct SystemProcessRunner: ProcessRunner {
    public init() {}

    public func run(
        executable: URL, arguments: [String], timeout: Duration
    ) async throws -> ProcessOutput {
        let process = Process()
        process.executableURL = executable
        process.arguments = arguments
        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe

        // Read the pipes on background queues so a large output can never
        // deadlock against a full pipe buffer while the process waits.
        let stdoutData = DataAccumulator()
        let stderrData = DataAccumulator()
        stdoutPipe.fileHandleForReading.readabilityHandler = { handle in
            let chunk = handle.availableData
            if chunk.isEmpty { handle.readabilityHandler = nil } else { stdoutData.append(chunk) }
        }
        stderrPipe.fileHandleForReading.readabilityHandler = { handle in
            let chunk = handle.availableData
            if chunk.isEmpty { handle.readabilityHandler = nil } else { stderrData.append(chunk) }
        }

        do {
            try process.run()
        } catch {
            throw ProcessRunError.launchFailed(error.localizedDescription)
        }

        return try await withTaskCancellationHandler {
            try await withThrowingTaskGroup(of: TerminationCause.self) { group in
                group.addTask {
                    await withCheckedContinuation { continuation in
                        process.terminationHandler = { _ in continuation.resume() }
                    }
                    return .exited
                }
                group.addTask {
                    try? await Task.sleep(for: timeout)
                    return .timedOut
                }

                let cause = try await group.next()!
                group.cancelAll()

                if cause == .timedOut {
                    process.terminate()
                    throw ProcessRunError.timedOut
                }

                // Drain any bytes still buffered in the pipes.
                try? stdoutPipe.fileHandleForReading.close()
                try? stderrPipe.fileHandleForReading.close()
                return ProcessOutput(
                    stdout: stdoutData.snapshot(),
                    stderr: stderrData.snapshot(),
                    exitCode: process.terminationStatus)
            }
        } onCancel: {
            process.terminate()
        }
    }

    private enum TerminationCause: Sendable, Equatable { case exited, timedOut }
}

/// Thread-safe byte accumulator for the pipe readability handlers.
private final class DataAccumulator: @unchecked Sendable {
    private let lock = NSLock()
    private var data = Data()

    func append(_ chunk: Data) {
        lock.lock()
        data.append(chunk)
        lock.unlock()
    }

    func snapshot() -> Data {
        lock.lock()
        defer { lock.unlock() }
        return data
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd SDMKit && swift test --filter 'SystemProcessRunnerTests'`
Expected: PASS, 5 tests. (If `cancellationTerminatesTheProcess` is flaky under load, it still must pass locally; do not weaken it without noting why.)

- [ ] **Step 5: Format and commit**

```bash
./format.sh
git add SDMKit/Sources/SDMResolve/ProcessRunner.swift SDMKit/Sources/SDMResolve/SystemProcessRunner.swift SDMKit/Tests/SDMResolveTests/SystemProcessRunnerTests.swift
git commit -m "feat(resolve): ProcessRunner protocol and Foundation.Process implementation"
```

---

## Task 6: `BinaryLocator`

**Files:**
- Create: `SDMKit/Sources/SDMResolve/BinaryLocator.swift`
- Create: `SDMKit/Tests/SDMResolveTests/BinaryLocatorTests.swift`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces:
  - `actor BinaryLocator` with:
    - `init(searchPaths: [URL] = BinaryLocator.defaultSearchPaths, isExecutable: @escaping @Sendable (URL) -> Bool = { FileManager.default.isExecutableFile(atPath: $0.path) })`
    - `static var defaultSearchPaths: [URL]` — `/opt/homebrew/bin`, `/usr/local/bin`, `/usr/bin`, `~/.local/bin` (expanded)
    - `func setOverride(_ url: URL?, for name: String)` — a nil clears it
    - `func locate(_ name: String) -> URL?` — override (if executable) → first executable hit in `searchPaths` → `nil`; result memoized
    - `func invalidate()` — drops the memo so the next `locate` re-scans

- [ ] **Step 1: Write the failing test**

```swift
// SDMKit/Tests/SDMResolveTests/BinaryLocatorTests.swift
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
        searchPaths: [URL(fileURLWithPath: "/fake/a")], isExecutable: { box.present.contains($0.path) })
    #expect(await locator.locate("yt-dlp") == nil)
    box.present = ["/fake/a/yt-dlp"]
    #expect(await locator.locate("yt-dlp") == nil)  // memoized miss
    await locator.invalidate()
    #expect(await locator.locate("yt-dlp") == URL(fileURLWithPath: "/fake/a/yt-dlp"))
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd SDMKit && swift test --filter 'BinaryLocatorTests'`
Expected: FAIL — `cannot find 'BinaryLocator' in scope`.

- [ ] **Step 3: Write the implementation**

```swift
// SDMKit/Sources/SDMResolve/BinaryLocator.swift
import Foundation

/// Finds `yt-dlp` / `ffmpeg`: a Settings override first, then a fixed list
/// of common install locations. Parent spec §4.5. A `.app` launched from
/// Finder has no shell `PATH`, so callers always pass the absolute path
/// this returns.
public actor BinaryLocator {
    public static var defaultSearchPaths: [URL] {
        [
            "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin",
            NSString(string: "~/.local/bin").expandingTildeInPath,
        ].map { URL(fileURLWithPath: $0) }
    }

    private let searchPaths: [URL]
    private let isExecutable: @Sendable (URL) -> Bool
    private var overrides: [String: URL] = [:]
    private var memo: [String: URL?] = [:]

    public init(
        searchPaths: [URL] = BinaryLocator.defaultSearchPaths,
        isExecutable: @escaping @Sendable (URL) -> Bool = {
            FileManager.default.isExecutableFile(atPath: $0.path)
        }
    ) {
        self.searchPaths = searchPaths
        self.isExecutable = isExecutable
    }

    public func setOverride(_ url: URL?, for name: String) {
        overrides[name] = url
        memo[name] = nil
        memo.removeValue(forKey: name)
    }

    public func invalidate() {
        memo.removeAll()
    }

    public func locate(_ name: String) -> URL? {
        if let cached = memo[name] { return cached }
        let resolved = resolve(name)
        memo[name] = .some(resolved)
        return resolved
    }

    private func resolve(_ name: String) -> URL? {
        if let override = overrides[name], isExecutable(override) { return override }
        for directory in searchPaths {
            let candidate = directory.appendingPathComponent(name)
            if isExecutable(candidate) { return candidate }
        }
        return nil
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd SDMKit && swift test --filter 'BinaryLocatorTests'`
Expected: PASS, 5 tests.

- [ ] **Step 5: Format and commit**

```bash
./format.sh
git add SDMKit/Sources/SDMResolve/BinaryLocator.swift SDMKit/Tests/SDMResolveTests/BinaryLocatorTests.swift
git commit -m "feat(resolve): BinaryLocator for yt-dlp/ffmpeg discovery"
```

---

## Task 7: `yt-dlp -J` JSON models + mapping to `ResolvedMedia`

**Files:**
- Create: `SDMKit/Sources/SDMResolve/YtDlpJSON.swift`
- Create: `SDMKit/Tests/SDMResolveTests/YtDlpJSONTests.swift`
- Create: `SDMKit/Tests/SDMResolveTests/Support.swift`
- Create: `SDMKit/Tests/SDMResolveTests/Fixtures/video_muxed.json`
- Create: `SDMKit/Tests/SDMResolveTests/Fixtures/video_progressive_only.json`
- Create: `SDMKit/Tests/SDMResolveTests/Fixtures/video_hls_only.json`

**Interfaces:**
- Consumes: `MediaFormat`, `MediaKind`, `VideoCodec`, `AudioCodec`, `MediaContainer`, `ResolvedMedia` (Task 1).
- Produces:
  - `struct YtDlpDump: Decodable` — top-level `-J` object: `id: String?`, `title: String?`, `duration: Double?`, `extractor: String?`, `_type: String?`, `formats: [YtDlpFormat]?`, `entries: [YtDlpEntry]?`
  - `struct YtDlpFormat: Decodable` — `format_id`, `ext`, `vcodec`, `acodec`, `height: Int?`, `width: Int?`, `filesize: Int64?`, `filesize_approx: Int64?`, `tbr: Double?`, `url: String?`, `protocol: String?` (decoded via CodingKeys, `protocol` is a keyword → `proto`)
  - `struct YtDlpEntry: Decodable` — `id: String?`, `title: String?`, `url: String?`, `ie_key: String?`
  - `enum YtDlpParser` with:
    - `static func codecFor(vcodec raw: String?) -> VideoCodec?` — nil/`"none"` → nil; prefix `av01`/`av1` → `.av1`; `vp9`/`vp09` → `.vp9`; `avc1`/`h264` → `.h264`; else `.other(raw)`
    - `static func codecFor(acodec raw: String?) -> AudioCodec?` — nil/`"none"` → nil; prefix `opus` → `.opus`; `mp4a`/`aac` → `.aac`; else `.other(raw)`
    - `static func container(ext raw: String?) -> MediaContainer` — `mp4`/`m4v` → `.mp4`; `webm` → `.webm`; `m4a` → `.m4a`; nil/`""` → `.other("bin")`; else `.other(raw)`
    - `static func hasDirectURL(_ f: YtDlpFormat) -> Bool` — `url != nil && !url.isEmpty` and `proto` in `{nil, "https", "http", "https_native"}` (NOT `m3u8`, `m3u8_native`, `http_dash_segments`, `dash`)
    - `static func mediaFormat(from f: YtDlpFormat) -> MediaFormat?` — nil when `format_id` missing, `url` unusable, `hasDirectURL == false`, or `ext` is `mhtml`/`nil` storyboard; kind = `.audioOnly` if vcodec nil & acodec present, `.videoOnly` if vcodec present & acodec nil, `.progressive` if both present (skip if both nil)
    - `static func resolvedMedia(from dump: YtDlpDump) throws -> ResolvedMedia` — throws `ResolveError.unsupported` when the id/title are present but **no** format survives `mediaFormat(from:)` while the raw `formats` array was non-empty (HLS/DASH-only); throws `ResolveError.unavailable` when `id`/`formats` missing entirely

- [ ] **Step 1: Add the fixtures**

`SDMKit/Tests/SDMResolveTests/Fixtures/video_muxed.json` — a trimmed real `yt-dlp -J` shape (only the fields the models read):

```json
{
  "id": "dQw4w9WgXcQ",
  "title": "Rick Astley - Never Gonna Give You Up",
  "extractor": "youtube",
  "duration": 213.0,
  "formats": [
    { "format_id": "sb0", "ext": "mhtml", "vcodec": "none", "acodec": "none", "protocol": "mhtml", "url": "https://i.ytimg.com/sb/x.jpg" },
    { "format_id": "233", "ext": "mp4", "vcodec": "none", "acodec": "none", "protocol": "m3u8_native", "url": "https://manifest.googlevideo.com/x.m3u8" },
    { "format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "tbr": 129.5, "filesize": 3456789, "protocol": "https", "url": "https://rr1---sn.googlevideo.com/audio140" },
    { "format_id": "251", "ext": "webm", "vcodec": "none", "acodec": "opus", "tbr": 122.1, "filesize": 3311999, "protocol": "https", "url": "https://rr1---sn.googlevideo.com/audio251" },
    { "format_id": "137", "ext": "mp4", "vcodec": "avc1.640028", "acodec": "none", "height": 1080, "width": 1920, "tbr": 4412.6, "filesize": 118000000, "protocol": "https", "url": "https://rr1---sn.googlevideo.com/video137" },
    { "format_id": "248", "ext": "webm", "vcodec": "vp9", "acodec": "none", "height": 1080, "width": 1920, "tbr": 2800.4, "filesize": 74000000, "protocol": "https", "url": "https://rr1---sn.googlevideo.com/video248" },
    { "format_id": "399", "ext": "mp4", "vcodec": "av01.0.08M.08", "acodec": "none", "height": 1080, "width": 1920, "tbr": 2400.0, "filesize": 64000000, "protocol": "https", "url": "https://rr1---sn.googlevideo.com/video399" },
    { "format_id": "18", "ext": "mp4", "vcodec": "avc1.42001E", "acodec": "mp4a.40.2", "height": 360, "width": 640, "tbr": 696.7, "filesize": 18500000, "protocol": "https", "url": "https://rr1---sn.googlevideo.com/prog18" }
  ]
}
```

`Fixtures/video_progressive_only.json`:

```json
{
  "id": "progOnly123",
  "title": "Old Low-Res Clip",
  "extractor": "youtube",
  "duration": 45.0,
  "formats": [
    { "format_id": "18", "ext": "mp4", "vcodec": "avc1.42001E", "acodec": "mp4a.40.2", "height": 360, "width": 640, "tbr": 696.7, "filesize": 4200000, "protocol": "https", "url": "https://rr1---sn.googlevideo.com/prog18" }
  ]
}
```

`Fixtures/video_hls_only.json`:

```json
{
  "id": "hlsOnly456",
  "title": "Live Stream VOD",
  "extractor": "youtube",
  "duration": 3600.0,
  "formats": [
    { "format_id": "233", "ext": "mp4", "vcodec": "none", "acodec": "none", "protocol": "m3u8_native", "url": "https://manifest.googlevideo.com/a.m3u8" },
    { "format_id": "270", "ext": "mp4", "vcodec": "avc1.640028", "acodec": "none", "height": 1080, "protocol": "m3u8_native", "url": "https://manifest.googlevideo.com/v.m3u8" }
  ]
}
```

> **Optional (preferred where the machine has network + yt-dlp):** replace these
> with real captures — `yt-dlp -J '<url>' | python3 -m json.tool > fixture.json`
> then trim to the fields above. The hand-authored versions are valid fixtures
> on their own; keep them if capture is unavailable.

- [ ] **Step 2: Write the support file + failing test**

```swift
// SDMKit/Tests/SDMResolveTests/Support.swift
import Foundation
import Testing

@testable import SDMResolve

@testable import SDMCore

func fixtureData(_ name: String) throws -> Data {
    let url = Bundle.module.url(forResource: name, withExtension: "json", subdirectory: "Fixtures")
    let resolved = try #require(url, "missing fixture \(name).json")
    return try Data(contentsOf: resolved)
}

func fixtureDump(_ name: String) throws -> YtDlpDump {
    try JSONDecoder().decode(YtDlpDump.self, from: fixtureData(name))
}

/// Records the arguments it was called with and replays a scripted output.
final class FakeProcessRunner: ProcessRunner, @unchecked Sendable {
    struct Call: Sendable { var executable: URL; var arguments: [String] }
    private let lock = NSLock()
    private(set) var calls: [Call] = []
    /// Matched against the joined argument string; first match wins.
    var responses: [(match: String, output: Result<ProcessOutput, any Error>)] = []
    var defaultOutput: Result<ProcessOutput, any Error> =
        .success(ProcessOutput(stdout: Data(), stderr: Data(), exitCode: 0))

    func run(
        executable: URL, arguments: [String], timeout: Duration
    ) async throws -> ProcessOutput {
        lock.lock()
        calls.append(Call(executable: executable, arguments: arguments))
        lock.unlock()
        let joined = arguments.joined(separator: " ")
        let picked = responses.first { joined.contains($0.match) }?.output ?? defaultOutput
        return try picked.get()
    }
}

func ok(_ stdout: Data, stderr: Data = Data()) -> Result<ProcessOutput, any Error> {
    .success(ProcessOutput(stdout: stdout, stderr: stderr, exitCode: 0))
}

func fail(_ stderr: String, exitCode: Int32 = 1) -> Result<ProcessOutput, any Error> {
    .success(ProcessOutput(stdout: Data(), stderr: Data(stderr.utf8), exitCode: exitCode))
}
```

```swift
// SDMKit/Tests/SDMResolveTests/YtDlpJSONTests.swift
import Foundation
import Testing

@testable import SDMResolve

@testable import SDMCore

@Test func mapsMuxedVideoDumpToResolvedMediaWithDirectFormatsOnly() throws {
    let media = try YtDlpParser.resolvedMedia(from: fixtureDump("video_muxed"))
    #expect(media.videoID == "dQw4w9WgXcQ")
    #expect(media.extractor == "youtube")
    #expect(media.durationSeconds == 213.0)
    // storyboard (mhtml) and the two m3u8 entries are dropped:
    let ids = media.formats.map(\.id).sorted()
    #expect(ids == ["137", "140", "18", "248", "251", "399"])
}

@Test func classifiesStreamKinds() throws {
    let media = try YtDlpParser.resolvedMedia(from: fixtureDump("video_muxed"))
    #expect(media.formats.first { $0.id == "137" }?.kind == .videoOnly)
    #expect(media.formats.first { $0.id == "251" }?.kind == .audioOnly)
    #expect(media.formats.first { $0.id == "18" }?.kind == .progressive)
}

@Test func mapsCodecsAndContainers() throws {
    let media = try YtDlpParser.resolvedMedia(from: fixtureDump("video_muxed"))
    #expect(media.formats.first { $0.id == "399" }?.vcodec == .av1)
    #expect(media.formats.first { $0.id == "248" }?.vcodec == .vp9)
    #expect(media.formats.first { $0.id == "137" }?.vcodec == .h264)
    #expect(media.formats.first { $0.id == "251" }?.acodec == .opus)
    #expect(media.formats.first { $0.id == "140" }?.acodec == .aac)
    #expect(media.formats.first { $0.id == "248" }?.container == .webm)
}

@Test func progressiveOnlyDumpStillResolves() throws {
    let media = try YtDlpParser.resolvedMedia(from: fixtureDump("video_progressive_only"))
    #expect(media.formats.count == 1)
    #expect(media.formats[0].kind == .progressive)
}

@Test func hlsOnlyDumpThrowsUnsupported() throws {
    #expect(throws: ResolveError.unsupported) {
        try YtDlpParser.resolvedMedia(from: fixtureDump("video_hls_only"))
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cd SDMKit && swift test --filter 'YtDlpJSONTests'`
Expected: FAIL — `cannot find 'YtDlpParser' in scope`.

- [ ] **Step 4: Write the implementation**

```swift
// SDMKit/Sources/SDMResolve/YtDlpJSON.swift
import Foundation
import SDMCore

struct YtDlpDump: Decodable {
    var id: String?
    var title: String?
    var duration: Double?
    var extractor: String?
    var _type: String?
    var formats: [YtDlpFormat]?
    var entries: [YtDlpEntry]?
}

struct YtDlpFormat: Decodable {
    var format_id: String?
    var ext: String?
    var vcodec: String?
    var acodec: String?
    var height: Int?
    var width: Int?
    var filesize: Int64?
    var filesize_approx: Int64?
    var tbr: Double?
    var url: String?
    var proto: String?

    enum CodingKeys: String, CodingKey {
        case format_id, ext, vcodec, acodec, height, width, filesize
        case filesize_approx
        case tbr, url
        case proto = "protocol"
    }
}

struct YtDlpEntry: Decodable {
    var id: String?
    var title: String?
    var url: String?
    var ie_key: String?
}

enum YtDlpParser {
    static func codecFor(vcodec raw: String?) -> VideoCodec? {
        guard let raw, !raw.isEmpty, raw != "none" else { return nil }
        let lower = raw.lowercased()
        if lower.hasPrefix("av01") || lower.hasPrefix("av1") { return .av1 }
        if lower.hasPrefix("vp9") || lower.hasPrefix("vp09") { return .vp9 }
        if lower.hasPrefix("avc1") || lower.hasPrefix("h264") { return .h264 }
        return .other(raw)
    }

    static func codecFor(acodec raw: String?) -> AudioCodec? {
        guard let raw, !raw.isEmpty, raw != "none" else { return nil }
        let lower = raw.lowercased()
        if lower.hasPrefix("opus") { return .opus }
        if lower.hasPrefix("mp4a") || lower.hasPrefix("aac") { return .aac }
        return .other(raw)
    }

    static func container(ext raw: String?) -> MediaContainer {
        switch (raw ?? "").lowercased() {
        case "mp4", "m4v": return .mp4
        case "webm": return .webm
        case "m4a": return .m4a
        case "": return .other("bin")
        default: return .other(raw!)
        }
    }

    static func hasDirectURL(_ f: YtDlpFormat) -> Bool {
        guard let url = f.url, !url.isEmpty else { return false }
        switch f.proto {
        case nil, "https", "http", "https_native": return true
        default: return false
        }
    }

    static func mediaFormat(from f: YtDlpFormat) -> MediaFormat? {
        guard let id = f.format_id, hasDirectURL(f),
            let urlString = f.url, let url = URL(string: urlString),
            (f.ext ?? "") != "mhtml"
        else { return nil }

        let vcodec = codecFor(vcodec: f.vcodec)
        let acodec = codecFor(acodec: f.acodec)
        let kind: MediaKind
        switch (vcodec, acodec) {
        case (.some, .some): kind = .progressive
        case (.some, .none): kind = .videoOnly
        case (.none, .some): kind = .audioOnly
        case (.none, .none): return nil
        }

        return MediaFormat(
            id: id, kind: kind, height: f.height, width: f.width,
            vcodec: vcodec, acodec: acodec, container: container(ext: f.ext),
            filesize: f.filesize, filesizeApprox: f.filesize_approx, tbr: f.tbr, url: url)
    }

    static func resolvedMedia(from dump: YtDlpDump) throws -> ResolvedMedia {
        guard let id = dump.id, let rawFormats = dump.formats, !rawFormats.isEmpty else {
            throw ResolveError.unavailable
        }
        let formats = rawFormats.compactMap(mediaFormat(from:))
        guard !formats.isEmpty else { throw ResolveError.unsupported }
        return ResolvedMedia(
            extractor: dump.extractor ?? "unknown",
            videoID: id,
            title: dump.title ?? id,
            durationSeconds: dump.duration,
            formats: formats)
    }
}
```

- [ ] **Step 5: Wire the fixtures as test resources in `Package.swift`**

Change the `SDMResolveTests` target line to:
```swift
        .testTarget(
            name: "SDMResolveTests", dependencies: ["SDMResolve"],
            resources: [.copy("Fixtures")]),
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cd SDMKit && swift test --filter 'YtDlpJSONTests'`
Expected: PASS, 5 tests.

- [ ] **Step 7: Format and commit**

```bash
./format.sh
git add SDMKit/Package.swift SDMKit/Sources/SDMResolve/YtDlpJSON.swift SDMKit/Tests/SDMResolveTests/
git commit -m "feat(resolve): parse yt-dlp -J output into ResolvedMedia"
```

---

## Task 8: `YtDlpResolver.canHandle` + `resolve` for a single video

**Files:**
- Create: `SDMKit/Sources/SDMResolve/YtDlpResolver.swift`
- Create: `SDMKit/Tests/SDMResolveTests/YtDlpResolverTests.swift`
- Create: `SDMKit/Tests/SDMResolveTests/Fixtures/video_age_restricted_stderr.txt` *(see step 1)*
- Create: `SDMKit/Tests/SDMResolveTests/Fixtures/video_private_stderr.txt`

**Interfaces:**
- Consumes: `LinkResolver`, `ResolvedTarget`, `ResolvedMedia`, `ResolveError` (Task 2); `ProcessRunner`, `ProcessOutput` (Task 5); `BinaryLocator` (Task 6); `YtDlpDump`, `YtDlpParser` (Task 7); `CookieSource` (Task 3).
- Produces:
  - `struct YtDlpResolver: LinkResolver`:
    - `init(runner: any ProcessRunner, locator: BinaryLocator, cookieSource: @escaping @Sendable () -> CookieSource = { .none }, maxPlaylistVideos: @escaping @Sendable () -> Int = { 50 }, resolveTimeout: Duration = .seconds(60), playlistTimeout: Duration = .seconds(120))`
    - `func canHandle(_ url: URL) -> Bool`
    - `func resolve(_ url: URL) async throws -> ResolvedTarget`
    - `func refresh(extractor:videoID:formatID:) async throws -> RefreshedFormat` *(Task 10 fills this in; stub it to `throw ResolveError.unsupported` for now with a `// Task 10` marker)*
  - `static let handledHosts: Set<String>` = the §canHandle host set
  - `enum YtDlpResolver.Classifier` with `static func error(fromStderr stderr: String, exitCode: Int32) -> ResolveError` — maps `"Sign in to confirm your age"` / `"confirm you're not a bot"` / `"Sign in to confirm you're not a bot"` → `.authRequired`; `"Private video"` → `.privateVideo`; `"Video unavailable"` / `"This video is not available"` / `"has been removed"` → `.unavailable`; anything else → `.ytDlpFailed(stderrTail: <last 500 chars>)`

**`canHandle` rules:** scheme is `http`/`https`; host (lowercased, `www.` kept as-is but the set includes `www.` variants) ∈ `handledHosts`. `youtu.be` short links included.

**`resolve` (single video) flow:**
1. `guard let ytdlp = await locator.locate("yt-dlp") else { throw ResolveError.binaryMissing }`
2. Build args: `["-J", "--no-warnings", "--no-playlist"] + cookieSource().ytDlpArguments + [url.absoluteString]`. *(Playlist detection & the `--flat-playlist` path are Task 9; for now always `--no-playlist`.)*
3. `let out = try await runner.run(executable: ytdlp, arguments: args, timeout: resolveTimeout)`
4. `guard out.exitCode == 0 else { throw Classifier.error(fromStderr: String(decoding: out.stderr, as: UTF8.self), exitCode: out.exitCode) }`
5. `let dump = try JSONDecoder().decode(YtDlpDump.self, from: out.stdout)` — a decode failure → `throw ResolveError.ytDlpFailed(stderrTail: "unparseable -J output")`
6. `return .single(try YtDlpParser.resolvedMedia(from: dump))`
7. A `ProcessRunError.timedOut` thrown by the runner is caught and rethrown as `ResolveError.timeout`.

- [ ] **Step 1: Add the stderr fixtures**

`Fixtures/video_age_restricted_stderr.txt`:
```
ERROR: [youtube] abc123XYZ: Sign in to confirm your age. This video may be inappropriate for some users.
```

`Fixtures/video_private_stderr.txt`:
```
ERROR: [youtube] def456UVW: Private video. Sign in if you've been granted access to this video
```

- [ ] **Step 2: Write the failing test**

```swift
// SDMKit/Tests/SDMResolveTests/YtDlpResolverTests.swift
import Foundation
import Testing

@testable import SDMResolve

@testable import SDMCore

private func makeLocator(hasYtDlp: Bool = true, hasFfmpeg: Bool = true) -> BinaryLocator {
    var present: Set<String> = []
    if hasYtDlp { present.insert("/bin/yt-dlp") }
    if hasFfmpeg { present.insert("/bin/ffmpeg") }
    return BinaryLocator(
        searchPaths: [URL(fileURLWithPath: "/bin")], isExecutable: { present.contains($0.path) })
}

private func url(_ s: String) -> URL { URL(string: s)! }

@Test func canHandleYouTubeHostsOnly() {
    let r = YtDlpResolver(runner: FakeProcessRunner(), locator: makeLocator())
    #expect(r.canHandle(url("https://www.youtube.com/watch?v=abc")))
    #expect(r.canHandle(url("https://youtu.be/abc")))
    #expect(r.canHandle(url("https://music.youtube.com/watch?v=abc")))
    #expect(r.canHandle(url("https://m.youtube.com/watch?v=abc")))
    #expect(!r.canHandle(url("https://vimeo.com/12345")))
    #expect(!r.canHandle(url("https://example.com/video.mp4")))
    #expect(!r.canHandle(url("ftp://youtube.com/x")))
}

@Test func resolveSingleVideoReturnsResolvedMedia() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", ok(try fixtureData("video_muxed")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    let target = try await r.resolve(url("https://www.youtube.com/watch?v=dQw4w9WgXcQ"))
    guard case .single(let media) = target else { Issue.record("expected .single"); return }
    #expect(media.videoID == "dQw4w9WgXcQ")
    #expect(media.formats.contains { $0.id == "399" })
    // args carried the URL and --no-playlist
    let call = try #require(runner.calls.first)
    #expect(call.arguments.contains("--no-playlist"))
    #expect(call.arguments.contains("https://www.youtube.com/watch?v=dQw4w9WgXcQ"))
}

@Test func resolveWithoutYtDlpThrowsBinaryMissing() async {
    let r = YtDlpResolver(runner: FakeProcessRunner(), locator: makeLocator(hasYtDlp: false))
    await #expect(throws: ResolveError.binaryMissing) {
        try await r.resolve(url("https://youtu.be/abc"))
    }
}

@Test func ageRestrictedVideoThrowsAuthRequired() async throws {
    let runner = FakeProcessRunner()
    let stderr = try fixtureData("video_age_restricted_stderr").map { $0 }  // ensure file loads
    _ = stderr
    runner.responses = [("-J", fail(String(decoding: try fixtureData("video_age_restricted_stderr"), as: UTF8.self)))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    await #expect(throws: ResolveError.authRequired) {
        try await r.resolve(url("https://youtu.be/abc"))
    }
}

@Test func privateVideoThrowsPrivateVideo() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", fail(String(decoding: try fixtureData("video_private_stderr"), as: UTF8.self)))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    await #expect(throws: ResolveError.privateVideo) {
        try await r.resolve(url("https://youtu.be/def"))
    }
}

@Test func hlsOnlyVideoThrowsUnsupported() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", ok(try fixtureData("video_hls_only")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    await #expect(throws: ResolveError.unsupported) {
        try await r.resolve(url("https://youtu.be/hls"))
    }
}

@Test func cookieSourceAddsArguments() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", ok(try fixtureData("video_muxed")))]
    let r = YtDlpResolver(
        runner: runner, locator: makeLocator(), cookieSource: { .firefox })
    _ = try await r.resolve(url("https://youtu.be/abc"))
    let call = try #require(runner.calls.first)
    #expect(call.arguments.contains("--cookies-from-browser"))
    #expect(call.arguments.contains("firefox"))
}

@Test func runnerTimeoutBecomesResolveTimeout() async {
    let runner = FakeProcessRunner()
    runner.defaultOutput = .failure(ProcessRunError.timedOut)
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    await #expect(throws: ResolveError.timeout) {
        try await r.resolve(url("https://youtu.be/abc"))
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cd SDMKit && swift test --filter 'YtDlpResolverTests'`
Expected: FAIL — `cannot find 'YtDlpResolver' in scope`.

- [ ] **Step 4: Write the implementation**

```swift
// SDMKit/Sources/SDMResolve/YtDlpResolver.swift
import Foundation
import SDMCore

/// yt-dlp as a metadata extractor only — never as a downloader. Parent
/// spec §4.4. All subprocess access goes through the injected `ProcessRunner`.
public struct YtDlpResolver: LinkResolver {
    public static let handledHosts: Set<String> = [
        "youtube.com", "www.youtube.com", "m.youtube.com",
        "music.youtube.com", "youtu.be",
    ]

    private let runner: any ProcessRunner
    private let locator: BinaryLocator
    private let cookieSource: @Sendable () -> CookieSource
    private let maxPlaylistVideos: @Sendable () -> Int
    private let resolveTimeout: Duration
    private let playlistTimeout: Duration

    public init(
        runner: any ProcessRunner,
        locator: BinaryLocator,
        cookieSource: @escaping @Sendable () -> CookieSource = { .none },
        maxPlaylistVideos: @escaping @Sendable () -> Int = { 50 },
        resolveTimeout: Duration = .seconds(60),
        playlistTimeout: Duration = .seconds(120)
    ) {
        self.runner = runner
        self.locator = locator
        self.cookieSource = cookieSource
        self.maxPlaylistVideos = maxPlaylistVideos
        self.resolveTimeout = resolveTimeout
        self.playlistTimeout = playlistTimeout
    }

    public func canHandle(_ url: URL) -> Bool {
        guard let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https",
            let host = url.host?.lowercased()
        else { return false }
        return YtDlpResolver.handledHosts.contains(host)
    }

    public func resolve(_ url: URL) async throws -> ResolvedTarget {
        let ytdlp = try await requireYtDlp()
        let args =
            ["-J", "--no-warnings", "--no-playlist"]
            + cookieSource().ytDlpArguments + [url.absoluteString]
        let out = try await runYtDlp(ytdlp, args, timeout: resolveTimeout)
        let dump: YtDlpDump
        do {
            dump = try JSONDecoder().decode(YtDlpDump.self, from: out.stdout)
        } catch {
            throw ResolveError.ytDlpFailed(stderrTail: "unparseable -J output")
        }
        return .single(try YtDlpParser.resolvedMedia(from: dump))
    }

    public func refresh(
        extractor: String, videoID: String, formatID: String
    ) async throws -> RefreshedFormat {
        throw ResolveError.unsupported  // Task 10
    }

    // MARK: - Helpers

    private func requireYtDlp() async throws -> URL {
        guard let ytdlp = await locator.locate("yt-dlp") else { throw ResolveError.binaryMissing }
        return ytdlp
    }

    private func runYtDlp(
        _ executable: URL, _ arguments: [String], timeout: Duration
    ) async throws -> ProcessOutput {
        let out: ProcessOutput
        do {
            out = try await runner.run(executable: executable, arguments: arguments, timeout: timeout)
        } catch ProcessRunError.timedOut {
            throw ResolveError.timeout
        }
        guard out.exitCode == 0 else {
            throw Classifier.error(
                fromStderr: String(decoding: out.stderr, as: UTF8.self), exitCode: out.exitCode)
        }
        return out
    }

    enum Classifier {
        static func error(fromStderr stderr: String, exitCode: Int32) -> ResolveError {
            let lower = stderr.lowercased()
            if lower.contains("sign in to confirm your age")
                || lower.contains("confirm you're not a bot")
                || lower.contains("sign in to confirm you're not a bot")
            {
                return .authRequired
            }
            if lower.contains("private video") { return .privateVideo }
            if lower.contains("video unavailable")
                || lower.contains("this video is not available")
                || lower.contains("has been removed")
            {
                return .unavailable
            }
            return .ytDlpFailed(stderrTail: String(stderr.suffix(500)))
        }
    }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd SDMKit && swift test --filter 'YtDlpResolverTests'`
Expected: PASS, 9 tests.

- [ ] **Step 6: Format and commit**

```bash
./format.sh
git add SDMKit/Sources/SDMResolve/YtDlpResolver.swift SDMKit/Tests/SDMResolveTests/
git commit -m "feat(resolve): YtDlpResolver canHandle + single-video resolve"
```

---

## Task 9: Playlist & channel resolution

**Files:**
- Modify: `SDMKit/Sources/SDMResolve/YtDlpResolver.swift`
- Modify: `SDMKit/Sources/SDMResolve/YtDlpJSON.swift` (add flat-playlist parsing)
- Modify: `SDMKit/Tests/SDMResolveTests/YtDlpResolverTests.swift` (append)
- Create: `SDMKit/Tests/SDMResolveTests/Fixtures/playlist_flat.json`

**Interfaces:**
- Consumes: everything from Task 8.
- Produces:
  - `YtDlpParser.flatEntries(from dump: YtDlpDump) -> [(videoID: String, title: String)]` — reads `dump.entries`, keeps those with a non-nil `id`, `title` falls back to `id`
  - `YtDlpResolver.isPlaylistURL(_ url: URL) -> Bool` — true when the URL has a `list=` query item, or the path starts with `/playlist`, `/@`, `/channel/`, `/c/`, `/user/`, or ends with `/videos`
  - `resolve(_:)` now branches: `isPlaylistURL` → flat-playlist path returning `.playlist(...)`; else the Task 8 single path
  - Each `.playlist` entry is a `ResolvedMedia` with `formats: []` (lazily filled by the grabber in Part 3) — `extractor: "youtube"`, `videoID`, `title`, `durationSeconds: nil`

**Flat-playlist flow:**
1. `requireYtDlp()`
2. args: `["-J", "--flat-playlist", "--no-warnings"] + cookieSource().ytDlpArguments + [url.absoluteString]`, `timeout: playlistTimeout`
3. decode `YtDlpDump`; `let all = YtDlpParser.flatEntries(from: dump)`
4. `guard !all.isEmpty else { throw ResolveError.unavailable }`
5. `let cap = max(10, min(200, maxPlaylistVideos()))`
6. **newest N:** channel listings (`/@`, `/channel/`, `/c/`, `/user/`, `/videos`) are newest-first → `Array(all.prefix(cap))`. Playlists (`list=`, `/playlist`) put new additions at the tail → `Array(all.suffix(cap))`. *(Verify against current yt-dlp ordering behavior at implementation — see parent spec §12; if uncertain, the tail-for-playlist / head-for-channel split above is the documented default.)*
7. `let entries = kept.map { ResolvedMedia(extractor: "youtube", videoID: $0.videoID, title: $0.title, durationSeconds: nil, formats: []) }`
8. `return .playlist(title: dump.title ?? "Playlist", entries: entries, totalAvailable: all.count)`

- [ ] **Step 1: Add the fixture**

`Fixtures/playlist_flat.json` (12 entries so a `cap` of 10 visibly truncates):

```json
{
  "_type": "playlist",
  "id": "PLtest",
  "title": "My Test Playlist",
  "entries": [
    { "_type": "url", "id": "vid01", "title": "Episode 1", "ie_key": "Youtube" },
    { "_type": "url", "id": "vid02", "title": "Episode 2", "ie_key": "Youtube" },
    { "_type": "url", "id": "vid03", "title": "Episode 3", "ie_key": "Youtube" },
    { "_type": "url", "id": "vid04", "title": "Episode 4", "ie_key": "Youtube" },
    { "_type": "url", "id": "vid05", "title": "Episode 5", "ie_key": "Youtube" },
    { "_type": "url", "id": "vid06", "title": "Episode 6", "ie_key": "Youtube" },
    { "_type": "url", "id": "vid07", "title": "Episode 7", "ie_key": "Youtube" },
    { "_type": "url", "id": "vid08", "title": "Episode 8", "ie_key": "Youtube" },
    { "_type": "url", "id": "vid09", "title": "Episode 9", "ie_key": "Youtube" },
    { "_type": "url", "id": "vid10", "title": "Episode 10", "ie_key": "Youtube" },
    { "_type": "url", "id": "vid11", "title": "Episode 11", "ie_key": "Youtube" },
    { "_type": "url", "id": "vid12", "title": "Episode 12", "ie_key": "Youtube" }
  ]
}
```

- [ ] **Step 2: Write the failing test** (append to `YtDlpResolverTests.swift`)

```swift
@Test func detectsPlaylistAndChannelURLs() {
    let r = YtDlpResolver(runner: FakeProcessRunner(), locator: makeLocator())
    #expect(r.isPlaylistURL(url("https://www.youtube.com/playlist?list=PLabc")))
    #expect(r.isPlaylistURL(url("https://www.youtube.com/watch?v=abc&list=PLabc")))
    #expect(r.isPlaylistURL(url("https://www.youtube.com/@SomeCreator")))
    #expect(r.isPlaylistURL(url("https://www.youtube.com/channel/UCxyz/videos")))
    #expect(!r.isPlaylistURL(url("https://www.youtube.com/watch?v=abc")))
    #expect(!r.isPlaylistURL(url("https://youtu.be/abc")))
}

@Test func resolvePlaylistReturnsCappedNewestEntries() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("--flat-playlist", ok(try fixtureData("playlist_flat")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator(), maxPlaylistVideos: { 10 })
    let target = try await r.resolve(url("https://www.youtube.com/playlist?list=PLtest"))
    guard case .playlist(let title, let entries, let total) = target else {
        Issue.record("expected .playlist"); return
    }
    #expect(title == "My Test Playlist")
    #expect(total == 12)
    #expect(entries.count == 10)
    // playlist → tail kept (newest additions at the end)
    #expect(entries.first?.videoID == "vid03")
    #expect(entries.last?.videoID == "vid12")
    #expect(entries.allSatisfy { $0.formats.isEmpty })
}

@Test func resolveChannelKeepsHeadEntries() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("--flat-playlist", ok(try fixtureData("playlist_flat")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator(), maxPlaylistVideos: { 10 })
    let target = try await r.resolve(url("https://www.youtube.com/@Creator/videos"))
    guard case .playlist(_, let entries, _) = target else { Issue.record("expected .playlist"); return }
    #expect(entries.first?.videoID == "vid01")
    #expect(entries.last?.videoID == "vid10")
}

@Test func playlistCapClampsToTenTwoHundred() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("--flat-playlist", ok(try fixtureData("playlist_flat")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator(), maxPlaylistVideos: { 3 })
    let target = try await r.resolve(url("https://www.youtube.com/playlist?list=PLtest"))
    guard case .playlist(_, let entries, _) = target else { Issue.record("expected .playlist"); return }
    #expect(entries.count == 10)  // clamped up to the floor of 10
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cd SDMKit && swift test --filter 'YtDlpResolverTests/detectsPlaylistAndChannelURLs'`
Expected: FAIL — `value of type 'YtDlpResolver' has no member 'isPlaylistURL'`.

- [ ] **Step 4: Write the implementation**

Add to `YtDlpJSON.swift` inside `enum YtDlpParser`:
```swift
    static func flatEntries(from dump: YtDlpDump) -> [(videoID: String, title: String)] {
        (dump.entries ?? []).compactMap { entry in
            guard let id = entry.id else { return nil }
            return (id, entry.title ?? id)
        }
    }
```

In `YtDlpResolver.swift`, replace `resolve(_:)` body's single branch with:
```swift
    public func resolve(_ url: URL) async throws -> ResolvedTarget {
        isPlaylistURL(url) ? try await resolvePlaylist(url) : try await resolveSingle(url)
    }

    func isPlaylistURL(_ url: URL) -> Bool {
        if let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems,
            items.contains(where: { $0.name == "list" })
        {
            return true
        }
        let path = url.path.lowercased()
        return path.hasPrefix("/playlist") || path.hasPrefix("/@")
            || path.hasPrefix("/channel/") || path.hasPrefix("/c/")
            || path.hasPrefix("/user/") || path.hasSuffix("/videos")
    }

    private func isChannelURL(_ url: URL) -> Bool {
        let path = url.path.lowercased()
        return path.hasPrefix("/@") || path.hasPrefix("/channel/")
            || path.hasPrefix("/c/") || path.hasPrefix("/user/") || path.hasSuffix("/videos")
    }

    private func resolveSingle(_ url: URL) async throws -> ResolvedTarget {
        let ytdlp = try await requireYtDlp()
        let args =
            ["-J", "--no-warnings", "--no-playlist"]
            + cookieSource().ytDlpArguments + [url.absoluteString]
        let out = try await runYtDlp(ytdlp, args, timeout: resolveTimeout)
        let dump: YtDlpDump
        do {
            dump = try JSONDecoder().decode(YtDlpDump.self, from: out.stdout)
        } catch {
            throw ResolveError.ytDlpFailed(stderrTail: "unparseable -J output")
        }
        return .single(try YtDlpParser.resolvedMedia(from: dump))
    }

    private func resolvePlaylist(_ url: URL) async throws -> ResolvedTarget {
        let ytdlp = try await requireYtDlp()
        let args =
            ["-J", "--flat-playlist", "--no-warnings"]
            + cookieSource().ytDlpArguments + [url.absoluteString]
        let out = try await runYtDlp(ytdlp, args, timeout: playlistTimeout)
        let dump: YtDlpDump
        do {
            dump = try JSONDecoder().decode(YtDlpDump.self, from: out.stdout)
        } catch {
            throw ResolveError.ytDlpFailed(stderrTail: "unparseable --flat-playlist output")
        }
        let all = YtDlpParser.flatEntries(from: dump)
        guard !all.isEmpty else { throw ResolveError.unavailable }
        let cap = max(10, min(200, maxPlaylistVideos()))
        let kept = isChannelURL(url) ? Array(all.prefix(cap)) : Array(all.suffix(cap))
        let entries = kept.map {
            ResolvedMedia(
                extractor: "youtube", videoID: $0.videoID, title: $0.title,
                durationSeconds: nil, formats: [])
        }
        return .playlist(
            title: dump.title ?? "Playlist", entries: entries, totalAvailable: all.count)
    }
```
Delete the now-duplicated single-branch code path from the old `resolve`.

- [ ] **Step 5: Run test to verify it passes**

Run: `cd SDMKit && swift test --filter 'YtDlpResolverTests'`
Expected: PASS, 13 tests.

- [ ] **Step 6: Format and commit**

```bash
./format.sh
git add SDMKit/Sources/SDMResolve/ SDMKit/Tests/SDMResolveTests/
git commit -m "feat(resolve): playlist and channel resolution with newest-N cap"
```

---

## Task 10: `YtDlpResolver.refresh` — expired URL refresh

**Files:**
- Modify: `SDMKit/Sources/SDMResolve/YtDlpResolver.swift`
- Modify: `SDMKit/Tests/SDMResolveTests/YtDlpResolverTests.swift` (append)
- Create: `SDMKit/Tests/SDMResolveTests/Fixtures/video_refresh_ok.json`
- Create: `SDMKit/Tests/SDMResolveTests/Fixtures/video_refresh_format_gone.json`

**Interfaces:**
- Consumes: everything from Task 8; `RefreshedFormat` (Task 2).
- Produces: a working `refresh(extractor:videoID:formatID:) async throws -> RefreshedFormat` replacing the Task 8 stub.

**`refresh` flow:**
1. `requireYtDlp()`
2. canonical URL: `https://www.youtube.com/watch?v=<videoID>` (the `extractor` argument is currently always `"youtube"`; keep the parameter for the seam but only YouTube is handled — a non-`youtube` extractor throws `ResolveError.unsupported`)
3. args: `["-J", "--no-warnings", "--no-playlist"] + cookieSource().ytDlpArguments + [canonicalURL]`, `timeout: resolveTimeout`
4. decode `YtDlpDump`; find the raw `YtDlpFormat` whose `format_id == formatID`
5. not found, or `YtDlpParser.mediaFormat(from:)` returns nil for it → `throw ResolveError.formatGone`
6. `return RefreshedFormat(url: mapped.url, filesize: mapped.filesizeEffective, formatID: formatID)`

- [ ] **Step 1: Add the fixtures**

`Fixtures/video_refresh_ok.json`:
```json
{
  "id": "dQw4w9WgXcQ",
  "title": "Rick Astley - Never Gonna Give You Up",
  "extractor": "youtube",
  "duration": 213.0,
  "formats": [
    { "format_id": "137", "ext": "mp4", "vcodec": "avc1.640028", "acodec": "none", "height": 1080, "width": 1920, "tbr": 4412.6, "filesize": 118000000, "protocol": "https", "url": "https://rr5---sn-FRESH.googlevideo.com/video137?expire=9999999999" }
  ]
}
```

`Fixtures/video_refresh_format_gone.json`:
```json
{
  "id": "dQw4w9WgXcQ",
  "title": "Rick Astley - Never Gonna Give You Up",
  "extractor": "youtube",
  "duration": 213.0,
  "formats": [
    { "format_id": "248", "ext": "webm", "vcodec": "vp9", "acodec": "none", "height": 1080, "tbr": 2800.4, "filesize": 74000000, "protocol": "https", "url": "https://rr5---sn-FRESH.googlevideo.com/video248" }
  ]
}
```

- [ ] **Step 2: Write the failing test** (append)

```swift
@Test func refreshReturnsFreshURLForSameFormat() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", ok(try fixtureData("video_refresh_ok")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    let refreshed = try await r.refresh(
        extractor: "youtube", videoID: "dQw4w9WgXcQ", formatID: "137")
    #expect(refreshed.formatID == "137")
    #expect(refreshed.filesize == 118_000_000)
    #expect(refreshed.url.absoluteString.contains("sn-FRESH"))
    let call = try #require(runner.calls.first)
    #expect(call.arguments.contains("https://www.youtube.com/watch?v=dQw4w9WgXcQ"))
}

@Test func refreshThrowsFormatGoneWhenFormatMissing() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", ok(try fixtureData("video_refresh_format_gone")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    await #expect(throws: ResolveError.formatGone) {
        try await r.refresh(extractor: "youtube", videoID: "dQw4w9WgXcQ", formatID: "137")
    }
}

@Test func refreshWithoutYtDlpThrowsBinaryMissing() async {
    let r = YtDlpResolver(runner: FakeProcessRunner(), locator: makeLocator(hasYtDlp: false))
    await #expect(throws: ResolveError.binaryMissing) {
        try await r.refresh(extractor: "youtube", videoID: "x", formatID: "137")
    }
}

@Test func refreshRejectsNonYouTubeExtractor() async {
    let r = YtDlpResolver(runner: FakeProcessRunner(), locator: makeLocator())
    await #expect(throws: ResolveError.unsupported) {
        try await r.refresh(extractor: "vimeo", videoID: "x", formatID: "1")
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cd SDMKit && swift test --filter 'YtDlpResolverTests/refreshReturnsFreshURLForSameFormat'`
Expected: FAIL — currently throws `ResolveError.unsupported` (the stub).

- [ ] **Step 4: Write the implementation** — replace the `refresh` stub:

```swift
    public func refresh(
        extractor: String, videoID: String, formatID: String
    ) async throws -> RefreshedFormat {
        guard extractor == "youtube" else { throw ResolveError.unsupported }
        let ytdlp = try await requireYtDlp()
        let canonical = "https://www.youtube.com/watch?v=\(videoID)"
        let args =
            ["-J", "--no-warnings", "--no-playlist"]
            + cookieSource().ytDlpArguments + [canonical]
        let out = try await runYtDlp(ytdlp, args, timeout: resolveTimeout)
        let dump: YtDlpDump
        do {
            dump = try JSONDecoder().decode(YtDlpDump.self, from: out.stdout)
        } catch {
            throw ResolveError.ytDlpFailed(stderrTail: "unparseable -J output")
        }
        guard let raw = (dump.formats ?? []).first(where: { $0.format_id == formatID }),
            let mapped = YtDlpParser.mediaFormat(from: raw)
        else { throw ResolveError.formatGone }
        return RefreshedFormat(
            url: mapped.url, filesize: mapped.filesizeEffective, formatID: formatID)
    }
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd SDMKit && swift test --filter 'YtDlpResolverTests'`
Expected: PASS, 17 tests.

- [ ] **Step 6: Run the full suite**

Run: `cd SDMKit && swift test`
Expected: PASS — the pre-existing count (unchanged; no existing file was touched) plus all new `SDMCoreTests` and `SDMResolveTests`.

- [ ] **Step 7: Format and commit**

```bash
./format.sh
git add SDMKit/Sources/SDMResolve/ SDMKit/Tests/SDMResolveTests/
git commit -m "feat(resolve): refresh expired format URLs via yt-dlp -J"
```

---

## Task 11: Real-binary smoke test (skipped when yt-dlp absent)

**Files:**
- Create: `SDMKit/Tests/SDMResolveTests/YtDlpSmokeTests.swift`

**Interfaces:**
- Consumes: `SystemProcessRunner` (Task 5), `BinaryLocator` (Task 6), `YtDlpResolver` (Tasks 8–10).

This test exercises the real `BinaryLocator` + `SystemProcessRunner` wiring
(no network) and is `.disabled` when yt-dlp is not installed, so CI without the
binary stays green. Parent spec §10.2's "skipped when `BinaryLocator` reports
`.notFound`".

- [ ] **Step 1: Write the test**

```swift
// SDMKit/Tests/SDMResolveTests/YtDlpSmokeTests.swift
import Foundation
import Testing

@testable import SDMResolve

@testable import SDMCore

private func ytDlpInstalled() async -> Bool {
    await BinaryLocator().locate("yt-dlp") != nil
}

@Test(.enabled(if: FileManager.default.isExecutableFile(atPath: "/usr/local/bin/yt-dlp")
    || FileManager.default.isExecutableFile(atPath: "/opt/homebrew/bin/yt-dlp")))
func ytDlpVersionRunsThroughSystemRunner() async throws {
    let locator = BinaryLocator()
    let ytdlp = try #require(await locator.locate("yt-dlp"))
    let out = try await SystemProcessRunner().run(
        executable: ytdlp, arguments: ["--version"], timeout: .seconds(10))
    #expect(out.exitCode == 0)
    #expect(!out.stdout.isEmpty)
}
```

> This test does **not** hit YouTube — it only runs `yt-dlp --version`, proving
> the locator + runner + real binary compose. A networked end-to-end resolve is
> deliberately out of the automated suite (parent spec §11.1).

- [ ] **Step 2: Run it**

Run: `cd SDMKit && swift test --filter 'YtDlpSmokeTests'`
Expected: PASS (1 test) on a machine with yt-dlp; SKIPPED on one without.

- [ ] **Step 3: Format and commit**

```bash
./format.sh
git add SDMKit/Tests/SDMResolveTests/YtDlpSmokeTests.swift
git commit -m "test(resolve): real-binary smoke test for the locator+runner path"
```

---

## Self-Review

**1. Spec coverage (Part 1 scope = parent §3 `SDMResolve` row, §4, §10.1–§10.2):**

| Spec item | Task |
|---|---|
| `LinkResolver` protocol in `SDMCore` (§4.1) | Task 2 |
| `ResolvedTarget`, `ResolvedMedia`, `MediaFormat`, `QualityPreferences`, `FormatChoice`, `RefreshedFormat` (§4.2) | Tasks 1–2 |
| Fixed priority orders av1>vp9>h264, mp4>webm, opus>aac (§4.2) | Task 1 |
| `FormatSelector` pure function + algorithm (§4.3) | Task 4 |
| `FormatSelector` fixture table (§10.1) | Task 4 |
| `YtDlpResolver.canHandle` narrow host set (§4.4) | Task 8 |
| `resolve` single video, `-J` parsing, drop storyboard/HLS (§4.4) | Tasks 7–8 |
| `resolve` playlist/channel, flat-playlist, newest-N cap (§4.4) | Task 9 |
| `refresh` for expired URLs (§4.4) | Task 10 |
| `ResolveError` cases incl. `.authRequired`/`.privateVideo`/`.unsupported` (§4.4) | Tasks 2, 8 |
| `ProcessRunner` + `SystemProcessRunner` timeout/cancel (§4.5) | Task 5 |
| `BinaryLocator` override→scan→nil, re-scan (§4.5) | Task 6 |
| Cookies-from-browser arguments (§8, partial — Settings UI is Part 3) | Task 3 |
| Recorded `-J` JSON fixtures, no binary in CI (§10.2, §11.6) | Tasks 7–10 |
| Real-ffmpeg/yt-dlp smoke skipped when absent (§10.2) | Task 11 |

**Deferred to Part 2:** `FileComponent`, `DownloadItem` rework, `assembling`/`Muxer`, sidecar v2, 403 wiring into `DownloadTask`.
**Deferred to Part 3:** `GrabberRow`/`MediaRow`, playlist row expansion, handoff, `DownloadPackage.note`, format-picker UI, Settings screens, `EngineController`/`GrabberController` wiring of the concrete `YtDlpResolver`.

**2. Placeholder scan:** No "TBD"/"handle edge cases"/"similar to Task N". Every code step has real code. Fixtures are concrete JSON, with a note that real captures may replace them.

**3. Type consistency:** `MediaFormat`/`MediaKind`/`VideoCodec`/`AudioCodec`/`MediaContainer` defined Task 1, used Tasks 4/7/8/10 with matching cases. `FormatChoice.formatIDs`/`.requiresMux` defined Task 2, used consistently. `ProcessOutput`/`ProcessRunError` defined Task 5, consumed Tasks 6/8. `BinaryLocator.locate` returns `URL?` throughout. `ResolveError` case names (`.privateVideo`, not `.private`) consistent Tasks 2/8/10. `YtDlpResolver.init` signature is fixed at Task 8 and only `refresh`'s body changes in Tasks 9–10 (the stub marker is called out).

One known follow-up left for the executor, not a gap: Task 6's `setOverride` has a redundant `memo[name] = nil` then `removeValue` — collapse to just `removeValue(forKey:)` during implementation.

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-09-02-phase-5-part-1-resolver-seam.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** — tasks run in this session via executing-plans, batched with checkpoints for review.

**Which approach?**
