# Phase 5 Part 4 — Grabber: Media Rows, Playlists, Multi-Component Handoff: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status: IMPLEMENTED — merged to `main` 2026-09-02.** This plan is a historical record. It has no open work: anything its Self-Review deferred "to Part 5" was either completed in Part 5 or reviewed and moved to `todo.md` at the repo root. Do not treat the "Deferred to Part 5" notes below as a backlog.

**Goal:** Teach `SDMGrabber` to resolve YouTube URLs (single videos and playlists/channels) into `MediaRow`s alongside the existing HTTP `ProbedLink`s, auto-pick a format via `FormatSelector`, cluster a playlist into one annotated package, and hand a resolved row off to the engine as a 1–2-component `DownloadItem` — holding back rows that have no format yet.

**Architecture:** `GrabberSession` gains an injected `(any LinkResolver)?` and a `QualityPreferences` provider. `ingest` routes each URL: `resolver.canHandle` → resolve path producing a `MediaRow`; else → the untouched HTTP probe path producing a `ProbedLink`. The session keeps a **second map** `mediaRows: [UUID: MediaRow]` next to `links: [UUID: ProbedLink]`, sharing one `order: [UUID]` and one clustering pass (each id contributes a `ClusterableLink` from whichever map holds it). A playlist resolves to N `MediaRow`s sharing a `playlistGroup`, in a `PackageCandidate` whose new `note` field carries `"50 of 320 videos"` when truncated; each entry's format table is then fetched lazily under the existing probe budget. `GrabberController` exposes `downloadItems(inPackage:)` building `DownloadItem`s (multi-component for a mux `FormatChoice`), and `EngineController` gains `addItems(name:note:items:startImmediately:)`. All grabber logic is fixture-tested with a fake resolver; **no SwiftUI in this part** — the Linkgrabber row rendering, the format-picker, the new Settings tab, and promoting `isAssembling` to `ItemState.assembling` are Part 5.

**Tech Stack:** Swift 6 language mode, Swift Testing, local SPM package `SDMKit`.

**Spec:** `docs/superpowers/specs/2026-09-02-phase-5-youtube-resolver-design.md` §6 (grabber routing, `MediaRow`, playlists, handoff), §5.4 (`PackageCandidate.note` → `DownloadPackage.note`). Part 5 = §9 (UI), §12 additions (Settings), and the `ItemState.assembling` promotion.

## Global Constraints

- **Swift 6 language mode, strict concurrency.** Parent spec §2.
- **The full pre-existing suite (380 tests) MUST stay green after every task.**
- **No test may touch the network or spawn yt-dlp.** A fake `LinkResolver` is injected. Parent spec §11.
- **Format with `./format.sh`, lint clean with `./lint.sh` before every commit.**
- **Tests:** `cd SDMKit && swift test`; single `--filter '<Type>/<name>'`, always confirm a non-zero count.
- Playlist cap default **50**, range 10–200; keep the newest N (already enforced inside `YtDlpResolver` from Part 1 — the grabber just displays what it gets).
- Auto-pick uses `FormatSelector.pick`; a `nil` result → `MediaRow.state == .unselected`. Parent spec §4.3 / §6.2.
- Changing quality settings after grab **does not** re-pick any row (parent spec §12a).

---

## File Structure

**`SDMKit/Sources/SDMCore/`:** `PackageCandidate` is in `SDMGrabber`, not Core — no Core change.

**`SDMKit/Sources/SDMGrabber/` (changes):**

| File | Change |
|---|---|
| `MediaRow.swift` *(new)* | `MediaRow`, `MediaRowState` |
| `PackageClustering.swift` | `PackageCandidate.note: String?` |
| `GrabberSession.swift` | injected resolver + prefs; `mediaRows` map; routing in `ingest`; `resolveMedia`/`resolvePlaylist`; lazy per-entry extraction; clustering includes media rows; overrides/remove/clear cover media rows; `setFormatChoice(_:for:)` |
| `GrabberSnapshot.swift` | `mediaRows: [MediaRow]`; `note` carried on packages |

**`SDM/` (changes):**

| File | Change |
|---|---|
| `GrabberController.swift` | forward the new session methods; `downloadItems(inPackage:)`; construct the session with a real `YtDlpResolver` + a `QualityPreferences` provider (literals until Part 5 Settings) |
| `EngineController.swift` | `addItems(name:note:items:startImmediately:)`; `PackageUrlItem` path kept for pure-HTTP callers or migrated |
| `LinkGrabberView.swift` | **minimal** — `addToDownloads` calls the new handoff; media rows render with a placeholder badge (full treatment is Part 5). Keep it compiling and functional, not polished. |
| `SDMApp.swift` | the auto-add-on-grab path uses the new handoff |

**`SDMKit/Tests/SDMGrabberTests/` (new):** `MediaRowRoutingTests.swift`, `PlaylistExpansionTests.swift`, `MediaHandoffTests.swift`, plus a `FakeLinkResolver` in test support.

---

## Task 1: `MediaRow` + `MediaRowState` + `PackageCandidate.note`

**Files:**
- Create: `SDMKit/Sources/SDMGrabber/MediaRow.swift`
- Modify: `SDMKit/Sources/SDMGrabber/PackageClustering.swift`
- Test: `SDMKit/Tests/SDMGrabberTests/MediaRowRoutingTests.swift` *(new — value-type asserts only for now)*

**Interfaces:**
- Consumes: `ResolvedMedia`, `FormatChoice` (`SDMCore`, Part 1).
- Produces:
  - `enum MediaRowState: Sendable, Equatable`: `case resolving`, `case resolved`, `case unselected`, `case unsupported`, `case needsYtDlp`, `case needsFfmpeg`, `case failed(String)`
  - `struct MediaRow: Identifiable, Sendable, Equatable`:
    - `let id: UUID`
    - `let sourceURL: URL`
    - `var title: String` — from flat-playlist / resolve; `sourceURL.lastPathComponent` before it lands
    - `var state: MediaRowState`
    - `var media: ResolvedMedia?`
    - `var choice: FormatChoice?`
    - `var playlistGroup: UUID?`
    - `var isDuplicate: Bool` (mirrors `ProbedLink.isDuplicate`)
    - `init(id: UUID = UUID(), sourceURL: URL, title: String? = nil, state: MediaRowState = .resolving, media: ResolvedMedia? = nil, choice: FormatChoice? = nil, playlistGroup: UUID? = nil, isDuplicate: Bool = false)` — `title` defaults to `sourceURL.lastPathComponent` (or `"video"` if empty)
    - `var displayFilename: String` — `choice`-derived `"<sanitized title> [<videoID>].<container>"` when `media` + `choice` present, else `title`; used for clustering + as the eventual `outputFilename`
    - `var combinedBytes: Int64?` — `choice?.estimatedBytes`
  - `PackageCandidate.note: String?` — trailing defaulted init param; `nil` for ordinary packages; carried through `GrabberSession.recluster` identity reconciliation.

- [ ] **Step 1: Write the failing test**

```swift
// SDMKit/Tests/SDMGrabberTests/MediaRowRoutingTests.swift
import Foundation
import Testing

@testable import SDMCore
@testable import SDMGrabber

@Test func mediaRowFallsBackToUrlTitleBeforeResolving() {
    let row = MediaRow(sourceURL: URL(string: "https://youtu.be/dQw4w9WgXcQ")!)
    #expect(row.state == .resolving)
    #expect(row.title == "dQw4w9WgXcQ")
    #expect(row.displayFilename == "dQw4w9WgXcQ")
    #expect(row.combinedBytes == nil)
}

@Test func mediaRowDisplayFilenameUsesTitleVideoIdAndContainer() {
    let v = MediaFormat(
        id: "137", kind: .videoOnly, height: 1080, width: 1920, vcodec: .h264, acodec: nil,
        container: .mp4, filesize: 100, filesizeApprox: nil, tbr: nil,
        url: URL(string: "https://gv/v")!)
    let a = MediaFormat(
        id: "140", kind: .audioOnly, height: nil, width: nil, vcodec: nil, acodec: .aac,
        container: .m4a, filesize: 10, filesizeApprox: nil, tbr: nil,
        url: URL(string: "https://gv/a")!)
    var row = MediaRow(sourceURL: URL(string: "https://youtu.be/abc")!)
    row.media = ResolvedMedia(
        extractor: "youtube", videoID: "abc", title: "Rick Astley / Never Gonna",
        durationSeconds: 213, formats: [v, a])
    row.choice = FormatChoice(video: v, audio: a, outputContainer: .mp4, estimatedBytes: 110)
    row.state = .resolved
    #expect(row.displayFilename == "Rick Astley  Never Gonna [abc].mp4")
    #expect(row.combinedBytes == 110)
}

@Test func packageCandidateNoteDefaultsNil() {
    #expect(PackageCandidate(name: "P", linkIDs: []).note == nil)
    #expect(PackageCandidate(name: "P", linkIDs: [], note: "50 of 320 videos").note == "50 of 320 videos")
}
```

> Sanitization: strip `/` and control chars, collapse whitespace runs to a
> single space, trim, cap length preserving the `[<videoID>].<ext>` suffix.
> The test above reflects `/` → removed (leaving a double space) — match that
> or tighten the rule and the expectation together.

- [ ] **Step 2: Run** → FAIL (`MediaRow` not found).

- [ ] **Step 3: Implement** `MediaRow.swift` and add `note` to `PackageCandidate` (stored `var note: String?`, `note: String? = nil` as the last init param, `self.note = note`).

```swift
// SDMKit/Sources/SDMGrabber/MediaRow.swift
import Foundation
import SDMCore

public enum MediaRowState: Sendable, Equatable {
    case resolving
    case resolved
    case unselected
    case unsupported
    case needsYtDlp
    case needsFfmpeg
    case failed(String)
}

public struct MediaRow: Identifiable, Sendable, Equatable {
    public let id: UUID
    public let sourceURL: URL
    public var title: String
    public var state: MediaRowState
    public var media: ResolvedMedia?
    public var choice: FormatChoice?
    public var playlistGroup: UUID?
    public var isDuplicate: Bool

    public init(
        id: UUID = UUID(), sourceURL: URL, title: String? = nil,
        state: MediaRowState = .resolving, media: ResolvedMedia? = nil,
        choice: FormatChoice? = nil, playlistGroup: UUID? = nil, isDuplicate: Bool = false
    ) {
        self.id = id
        self.sourceURL = sourceURL
        let last = sourceURL.lastPathComponent
        self.title = title ?? (last.isEmpty ? "video" : last)
        self.state = state
        self.media = media
        self.choice = choice
        self.playlistGroup = playlistGroup
        self.isDuplicate = isDuplicate
    }

    public var displayFilename: String {
        guard let media, choice != nil else { return title }
        let container = choice?.outputContainer ?? .mp4
        return "\(Self.sanitize(media.title)) [\(media.videoID)].\(container.fileExtension)"
    }

    public var combinedBytes: Int64? { choice?.estimatedBytes }

    static func sanitize(_ raw: String) -> String {
        let stripped = raw.unicodeScalars.filter {
            $0 != "/" && !CharacterSet.controlCharacters.contains($0)
        }
        let collapsed = String(String.UnicodeScalarView(stripped))
            .replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
            .trimmingCharacters(in: .whitespaces)
        return collapsed.isEmpty ? "video" : String(collapsed.prefix(180))
    }
}
```

- [ ] **Step 4: Run** → PASS (3 tests). Full suite green.

- [ ] **Step 5: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDMKit/Sources/SDMGrabber/MediaRow.swift SDMKit/Sources/SDMGrabber/PackageClustering.swift SDMKit/Tests/SDMGrabberTests/MediaRowRoutingTests.swift
git commit -m "feat(grabber): MediaRow value type and PackageCandidate.note"
```

---

## Task 2: `GrabberSession` routes YouTube URLs to a resolver

**Files:**
- Modify: `SDMKit/Sources/SDMGrabber/GrabberSession.swift`
- Modify: `SDMKit/Sources/SDMGrabber/GrabberSnapshot.swift`
- Create: `SDMKit/Tests/SDMGrabberTests/FakeLinkResolver.swift` (support)
- Modify: `SDMKit/Tests/SDMGrabberTests/MediaRowRoutingTests.swift` (append)

**Interfaces:**
- Consumes: `LinkResolver`, `ResolvedTarget`, `QualityPreferences`, `FormatChoice`, `FormatSelector` — wait, `FormatSelector` is in `SDMResolve` which `SDMGrabber` does **not** depend on. **Add `SDMResolve` to `SDMGrabber`'s deps in `Package.swift`** (`SDMResolve` depends only on `SDMCore`, no cycle), OR move `FormatSelector` down to `SDMCore`. **Choose: add the dependency** — `FormatSelector` belongs with the resolver.
- Produces (`GrabberSession`):
  - `init(prober:budget:resolver:qualityPreferences:)` — `resolver: (any LinkResolver)? = nil`, `qualityPreferences: @Sendable () -> QualityPreferences = { .default }`
  - `mediaRows: [UUID: MediaRow]` (private), threaded into `order`, `snapshot`, `recluster`, `removeLink`, `removePackage`, `clear`, `setKnownDownloadURLs`, `moveLink`, override handling
  - routing in `ingest(urls:)`: for each fresh http(s) URL, `resolver?.canHandle(url) == true` → `mediaRows[id] = MediaRow(sourceURL: url)`, kick off `resolveMedia(id)`; else the existing `links[id] = ProbedLink(...)` path
  - `resolveMedia(_ id:)` — `await resolver.resolve(url)`:
    - `.binaryMissing` thrown as `ResolveError` → `state = .needsYtDlp`
    - `.unsupported` → `.unsupported`
    - `.authRequired` / `.privateVideo` / `.unavailable` / other → `.failed(<message>)`
    - `.single(media)` → `row.media = media`; `let choice = FormatSelector.pick(media, qualityPreferences())`; `choice == nil` → `.unselected`; else `row.choice = choice`, `.resolved`
    - `.playlist(...)` → Task 3
  - `snapshot()` returns `GrabberSnapshot` with `mediaRows` populated in `order`
  - `setFormatChoice(_ choice: FormatChoice, for id: UUID)` — sets `mediaRows[id]?.choice`, `state = .resolved`; recluster (filename may change)

`GrabberSnapshot` gains `mediaRows: [MediaRow]` (defaulted `[]` in `init`), and `PackageCandidate` values in `packages` now carry `note`.

- [ ] **Step 1: Write the support fake + failing tests**

```swift
// SDMKit/Tests/SDMGrabberTests/FakeLinkResolver.swift
import Foundation

@testable import SDMCore

final class FakeLinkResolver: LinkResolver, @unchecked Sendable {
    var handledHosts: Set<String> = ["youtube.com", "www.youtube.com", "youtu.be"]
    var resolveResult: @Sendable (URL) throws -> ResolvedTarget
    var refreshResult: @Sendable (String) throws -> RefreshedFormat = { id in
        RefreshedFormat(url: URL(string: "https://gv/\(id)")!, filesize: nil, formatID: id)
    }
    init(_ f: @escaping @Sendable (URL) throws -> ResolvedTarget) { resolveResult = f }
    func canHandle(_ url: URL) -> Bool {
        url.host.map { handledHosts.contains($0) } ?? false
    }
    func resolve(_ url: URL) async throws -> ResolvedTarget { try resolveResult(url) }
    func refresh(extractor: String, videoID: String, formatID: String) async throws -> RefreshedFormat {
        try refreshResult(formatID)
    }
}

func singleMedia(videoID: String, title: String, formats: [MediaFormat]) -> ResolvedTarget {
    .single(
        ResolvedMedia(
            extractor: "youtube", videoID: videoID, title: title, durationSeconds: 100,
            formats: formats))
}

func vf(_ id: String, _ h: Int, _ v: VideoCodec, _ c: MediaContainer, size: Int64 = 1000) -> MediaFormat {
    MediaFormat(
        id: id, kind: .videoOnly, height: h, width: h * 16 / 9, vcodec: v, acodec: nil,
        container: c, filesize: size, filesizeApprox: nil, tbr: 1000, url: URL(string: "https://gv/\(id)")!)
}
func af(_ id: String, _ a: AudioCodec, _ c: MediaContainer, size: Int64 = 100) -> MediaFormat {
    MediaFormat(
        id: id, kind: .audioOnly, height: nil, width: nil, vcodec: nil, acodec: a, container: c,
        filesize: size, filesizeApprox: nil, tbr: 128, url: URL(string: "https://gv/\(id)")!)
}
```

```swift
// append to MediaRowRoutingTests.swift
import SDMResolve  // for FakeProbeOrigin / LinkProber already imported via @testable SDMGrabber

private func makeSession(_ resolver: FakeLinkResolver) -> GrabberSession {
    GrabberSession(
        prober: LinkProber(transport: FakeProbeOrigin(), deepSniffEnabled: false),
        resolver: resolver)
}

@Test func aYouTubeUrlBecomesAResolvedMediaRow() async {
    let resolver = FakeLinkResolver { _ in
        singleMedia(videoID: "abc", title: "Song", formats: [vf("137", 1080, .h264, .mp4), af("140", .aac, .m4a)])
    }
    let session = makeSession(resolver)
    await session.ingest(urls: [URL(string: "https://youtu.be/abc")!])
    let snap = await session.snapshot()
    #expect(snap.links.isEmpty)
    #expect(snap.mediaRows.count == 1)
    let row = snap.mediaRows[0]
    #expect(row.state == .resolved)
    #expect(row.choice?.video?.id == "137")
    #expect(row.choice?.audio?.id == "140")
}

@Test func aNonYouTubeUrlStaysOnTheHttpProbePath() async {
    let resolver = FakeLinkResolver { _ in .single(ResolvedMedia(extractor: "x", videoID: "x", title: "x", durationSeconds: nil, formats: [])) }
    let session = makeSession(resolver)
    await session.ingest(urls: [URL(string: "https://example.com/a.mp4")!])
    let snap = await session.snapshot()
    #expect(snap.mediaRows.isEmpty)
    #expect(snap.links.count == 1)
}

@Test func autoPickReturningNilLeavesTheRowUnselected() async {
    let resolver = FakeLinkResolver { _ in
        singleMedia(videoID: "abc", title: "Song", formats: [vf("137", 1080, .h264, .mp4)])  // no audio
    }
    let session = makeSession(resolver)
    await session.ingest(urls: [URL(string: "https://youtu.be/abc")!])
    #expect(await session.snapshot().mediaRows[0].state == .unselected)
}

@Test func anUnsupportedResolveMarksTheRowUnsupported() async {
    let resolver = FakeLinkResolver { _ in throw ResolveError.unsupported }
    let session = makeSession(resolver)
    await session.ingest(urls: [URL(string: "https://youtu.be/live")!])
    #expect(await session.snapshot().mediaRows[0].state == .unsupported)
}

@Test func aMissingBinaryMarksTheRowNeedsYtDlp() async {
    let resolver = FakeLinkResolver { _ in throw ResolveError.binaryMissing }
    let session = makeSession(resolver)
    await session.ingest(urls: [URL(string: "https://youtu.be/x")!])
    #expect(await session.snapshot().mediaRows[0].state == .needsYtDlp)
}

@Test func manuallyChoosingAFormatResolvesAnUnselectedRow() async {
    let resolver = FakeLinkResolver { _ in
        singleMedia(videoID: "abc", title: "Song", formats: [vf("137", 1080, .h264, .mp4)])
    }
    let session = makeSession(resolver)
    await session.ingest(urls: [URL(string: "https://youtu.be/abc")!])
    let id = await session.snapshot().mediaRows[0].id
    let v = vf("137", 1080, .h264, .mp4)
    await session.setFormatChoice(
        FormatChoice(video: v, audio: nil, outputContainer: .mp4, estimatedBytes: 1000), for: id)
    #expect(await session.snapshot().mediaRows[0].state == .resolved)
}
```

> Confirm `FakeProbeOrigin`'s initializer and `LinkProber`'s signature against
> the current `SDMGrabber` sources — the existing `GrabberSessionTests` build a
> session; copy that construction.

- [ ] **Step 2: Run** → FAIL.

- [ ] **Step 3: Implement.** `Package.swift`: `SDMGrabber` deps → `["SDMCore", "SDMResolve"]`; `SDMGrabberTests` deps → add `"SDMResolve"`. Then the `GrabberSession` changes. Keep the HTTP path byte-identical; media rows are a parallel structure sharing `order`.

- [ ] **Step 4: Run** the 6 new tests + full suite (380 + Task 1's 3 + these 6).

- [ ] **Step 5: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDMKit/Package.swift SDMKit/Sources/SDMGrabber/ SDMKit/Tests/SDMGrabberTests/
git commit -m "feat(grabber): route YouTube URLs to the resolver as MediaRows"
```

---

## Task 3: Playlist expansion

**Files:**
- Modify: `SDMKit/Sources/SDMGrabber/GrabberSession.swift`
- Create: `SDMKit/Tests/SDMGrabberTests/PlaylistExpansionTests.swift`

**Interfaces:**
- `resolveMedia` handling `.playlist(title, entries, totalAvailable)`:
  1. Replace the originally-ingested row with **one `MediaRow` per entry**, each `state = .resolving`, `playlistGroup = <one shared UUID>`, `title = entry.title`, `sourceURL = https://www.youtube.com/watch?v=<entry.videoID>`.
  2. Force these rows into one `PackageCandidate` (via the manual-override mechanism) named `title`, with `note = "\(entries.count) of \(totalAvailable) videos"` when `entries.count < totalAvailable`, else `nil`.
  3. Kick off lazy per-entry extraction: `await resolver.resolve(entryURL)` for each, **bounded by `budget.globalMaxConcurrentProbes`** (reuse the `probeBounded` wave pattern, or a simple `TaskGroup` with a semaphore). Each result updates its row exactly as a single-video resolve does (`.resolved` / `.unselected` / `.unsupported` / `.failed`).
- `PackageCandidate.note` must survive `recluster()` — carry it on the manual-override entry (store `[UUID: (name: String, note: String?)]` or a parallel `playlistNotes: [UUID (group) : String]` keyed and reattached in `recluster`). Simplest: `packageNotes: [String: String]` keyed by package name, applied after clustering.

- [ ] **Step 1: Write the failing test**

```swift
// SDMKit/Tests/SDMGrabberTests/PlaylistExpansionTests.swift
import Foundation
import Testing

@testable import SDMCore
@testable import SDMGrabber
import SDMResolve

@Test func aPlaylistUrlExpandsIntoOneRowPerEntryInOnePackage() async {
    let entries = (1...5).map {
        ResolvedMedia(
            extractor: "youtube", videoID: "vid0\($0)", title: "Episode \($0)",
            durationSeconds: nil, formats: [])
    }
    let resolver = FakeLinkResolver { url in
        if url.absoluteString.contains("list=") {
            return .playlist(title: "My Series", entries: entries, totalAvailable: 12)
        }
        // per-entry re-resolve
        let id = URLComponents(url: url, resolvingAgainstBaseURL: false)?
            .queryItems?.first(where: { $0.name == "v" })?.value ?? "?"
        return singleMedia(
            videoID: id, title: "Episode", formats: [vf("137", 720, .h264, .mp4), af("140", .aac, .m4a)])
    }
    let session = GrabberSession(
        prober: LinkProber(transport: FakeProbeOrigin(), deepSniffEnabled: false),
        resolver: resolver)
    await session.ingest(urls: [URL(string: "https://www.youtube.com/playlist?list=PL123")!])
    let snap = await session.snapshot()
    #expect(snap.mediaRows.count == 5)
    #expect(Set(snap.mediaRows.compactMap(\.playlistGroup)).count == 1)
    #expect(snap.packages.count == 1)
    #expect(snap.packages[0].name == "My Series")
    #expect(snap.packages[0].note == "5 of 12 videos")
    #expect(snap.packages[0].linkIDs.count == 5)
    // every entry eventually resolved to a format
    #expect(snap.mediaRows.allSatisfy { $0.state == .resolved })
}

@Test func aFullyListedPlaylistHasNoTruncationNote() async {
    let entries = (1...3).map {
        ResolvedMedia(extractor: "youtube", videoID: "v\($0)", title: "E\($0)", durationSeconds: nil, formats: [])
    }
    let resolver = FakeLinkResolver { url in
        url.absoluteString.contains("list=")
            ? .playlist(title: "Short", entries: entries, totalAvailable: 3)
            : singleMedia(videoID: "v", title: "E", formats: [vf("18", 360, .h264, .mp4)])  // progressive-ish
    }
    let session = GrabberSession(
        prober: LinkProber(transport: FakeProbeOrigin(), deepSniffEnabled: false), resolver: resolver)
    await session.ingest(urls: [URL(string: "https://www.youtube.com/playlist?list=X")!])
    #expect(await session.snapshot().packages[0].note == nil)
}
```

- [ ] **Step 2: Run** → FAIL.

- [ ] **Step 3: Implement** the playlist branch + `packageNotes`.

- [ ] **Step 4: Run** the 2 new tests + full suite.

- [ ] **Step 5: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDMKit/Sources/SDMGrabber/GrabberSession.swift SDMKit/Tests/SDMGrabberTests/PlaylistExpansionTests.swift
git commit -m "feat(grabber): expand a playlist into one clustered, annotated package"
```

---

## Task 4: Handoff to multi-component `DownloadItem`s

**Files:**
- Modify: `SDM/GrabberController.swift`
- Modify: `SDM/EngineController.swift`
- Modify: `SDM/LinkGrabberView.swift` (minimal — new handoff call)
- Modify: `SDM/SDMApp.swift` (auto-add path)
- Test: `SDMKit/Tests/SDMGrabberTests/MediaHandoffTests.swift` *(new — tests the pure builder, extracted into `SDMGrabber`)*

**Design:** the `DownloadItem`-building logic is **pure and testable**, so put it in `SDMGrabber` as a free function rather than in the app's `GrabberController`:

```swift
// SDMKit/Sources/SDMGrabber/MediaHandoff.swift
public enum MediaHandoff {
    /// Builds the engine items for one package's rows. HTTP links become
    /// one-component items; a `.resolved` media row becomes a 1–2-component
    /// item. Rows without a usable format are returned in `heldBack`.
    public static func build(
        httpLinks: [ProbedLink], mediaRows: [MediaRow]
    ) -> (items: [DownloadItem], heldBackCount: Int)
}
```

- HTTP link → `DownloadItem(url: link.originalURL, filename: link.effectiveFilename, totalBytes: link.contentLength)` (the existing convenience init — one `.http` component).
- `.resolved` media row with `media` + `choice`:
  - `choice.video` and `choice.audio` both present → two `FileComponent`s (video index 0, audio index 1), `origin: .resolved(extractor: media.extractor, videoID: media.videoID, formatID: fmt.id)`, `partFilename` = `"<sanitized title> [<videoID>].f<formatID>.<ext>"`, `assembly: .mux`, `outputFilename = row.displayFilename`.
  - only `choice.video` (progressive) → one component, `assembly: .none`, `outputFilename = row.displayFilename` (container = video's).
  - only `choice.audio` (audio-only site) → one component, `assembly: .none`.
- media row **not** `.resolved` → counted in `heldBackCount`, no item.

`GrabberController.downloadItems(inPackage:) -> (items: [DownloadItem], heldBackCount: Int)` calls `MediaHandoff.build` over that package's rows.

`EngineController.addItems(name: String, note: String?, items: [DownloadItem], startImmediately: Bool)` — builds `DownloadPackage(name:, items:, note:)` (each item `.queued` or `.stopped` per `startImmediately`, always enabled), `await engine.add(...)`. Keep `addPackage(name:urlItems:startImmediately:)` as a thin wrapper that maps `PackageUrlItem` → convenience-init `DownloadItem` and calls `addItems` (so `SDMApp`'s auto-add path barely changes).

`LinkGrabberView.addToDownloads` → `let (items, held) = controller.downloadItems(inPackage: package.id)`; `engineController.addItems(name:, note: package.note, items:, startImmediately:)`; then `removeLink` each **resolved/http** link id (not the held-back ones — they stay in the grabber). A `held > 0` toast/log is Part 5.

- [ ] **Step 1: Write the failing test** (`MediaHandoffTests.swift`)

```swift
import Foundation
import Testing

@testable import SDMCore
@testable import SDMGrabber

private func resolvedRow(mux: Bool) -> MediaRow {
    let v = vf("137", 1080, .h264, .mp4)
    let a = af("140", .aac, .m4a)
    var row = MediaRow(sourceURL: URL(string: "https://youtu.be/abc")!)
    row.media = ResolvedMedia(
        extractor: "youtube", videoID: "abc", title: "Clip", durationSeconds: 10,
        formats: [v, a])
    row.choice =
        mux
        ? FormatChoice(video: v, audio: a, outputContainer: .mp4, estimatedBytes: 1100)
        : FormatChoice(video: v, audio: nil, outputContainer: .mp4, estimatedBytes: 1000)
    row.state = .resolved
    return row
}

@Test func aMuxRowBecomesATwoComponentItem() {
    let (items, held) = MediaHandoff.build(httpLinks: [], mediaRows: [resolvedRow(mux: true)])
    #expect(held == 0)
    #expect(items.count == 1)
    #expect(items[0].components.count == 2)
    #expect(items[0].assembly == .mux)
    #expect(items[0].outputFilename == "Clip [abc].mp4")
    if case .resolved(_, let videoID, let formatID) = items[0].components[0].origin {
        #expect(videoID == "abc")
        #expect(formatID == "137")
    } else {
        Issue.record("component 0 should be .resolved")
    }
    #expect(items[0].components[1].partFilename.contains(".f140."))
}

@Test func aProgressiveRowBecomesAOneComponentItem() {
    let (items, _) = MediaHandoff.build(httpLinks: [], mediaRows: [resolvedRow(mux: false)])
    #expect(items[0].components.count == 1)
    #expect(items[0].assembly == .none)
}

@Test func unselectedRowsAreHeldBackButHttpSiblingsGoThrough() {
    var unselected = MediaRow(sourceURL: URL(string: "https://youtu.be/x")!)
    unselected.state = .unselected
    let http = ProbedLink(
        originalURL: URL(string: "https://example.com/f.zip")!, stage: .done, statusCode: 200,
        contentLength: 500, verdict: .online)
    let (items, held) = MediaHandoff.build(httpLinks: [http], mediaRows: [unselected])
    #expect(held == 1)
    #expect(items.count == 1)
    #expect(items[0].components[0].origin == .http)
}
```

- [ ] **Step 2: Run** → FAIL.

- [ ] **Step 3: Implement** `MediaHandoff.swift`, the two controller methods, and the two call-site edits. Build the app: `xcodebuild -scheme SDM -destination 'platform=macOS' build`.

- [ ] **Step 4: Run** the 3 new tests + full suite + `xcodebuild build` succeeds.

- [ ] **Step 5: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDMKit/Sources/SDMGrabber/MediaHandoff.swift SDM/ SDMKit/Tests/SDMGrabberTests/MediaHandoffTests.swift
git commit -m "feat(grabber): hand resolved media rows off as multi-component download items"
```

---

## Task 5: Wire the real resolver + preferences into `GrabberController`

**Files:**
- Modify: `SDM/GrabberController.swift`

**Interfaces:**
- `GrabberController.init` constructs the session with a real `YtDlpResolver` sharing the same `SystemProcessRunner` + `BinaryLocator` pattern as `EngineController` (Part 3, Task 7), and `qualityPreferences: { .default }` (a `YouTubeSettingsStore`-backed provider is Part 5).
- The `needsFfmpeg` row state: a `MediaRow` whose `choice.requiresMux` is true but `BinaryLocator.locate("ffmpeg")` is nil → `state = .needsFfmpeg` instead of `.resolved`. Do this check in `GrabberController` (it has the locator) after the session resolves, OR pass an `ffmpegAvailable: @Sendable () -> Bool` into the session. **Pass it into the session** (keeps the state machine in one place); `GrabberController` supplies `{ binaryLocator.locate("ffmpeg") != nil }` — but `locate` is async on an actor, so cache a `Bool` refreshed on app-active. Simplest: `ffmpegAvailable: @Sendable () -> Bool = { true }` provider, controller wires a cached flag.

- [ ] **Step 1** — add the wiring; if `needsFfmpeg` needs a session signature change, add `ffmpegAvailable` provider to `GrabberSession.init` and a test (`a mux choice with ffmpeg unavailable → .needsFfmpeg`).
- [ ] **Step 2** — `xcodebuild -scheme SDM -destination 'platform=macOS' build` → SUCCEEDED.
- [ ] **Step 3** — `cd SDMKit && swift test` full green.
- [ ] **Step 4: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDM/GrabberController.swift SDMKit/
git commit -m "feat(app): wire YtDlpResolver into the linkgrabber session"
```

---

## Self-Review

**1. Spec coverage (Part 4 = parent spec §6 + §5.4's grabber half):**

| Spec item | Task |
|---|---|
| `GrabberSession` routes YouTube URLs to `resolver` (§6.1) | Task 2 |
| `GrabberRow` two-case model | Tasks 1–2 (parallel `mediaRows` map rather than an enum — same observable result, smaller blast radius) |
| `MediaRow` + `MediaRowState` (§6.2) | Task 1 |
| Auto-pick via `FormatSelector`; nil → `unselected` (§6.2) | Task 2 |
| `unsupported` / `needsYtDlp` / `needsFfmpeg` / `failed` states (§6.2) | Tasks 2, 5 |
| Changing settings after grab doesn't re-pick (§12a) | Task 2 (resolve runs once; no re-pick path exists) |
| Playlist → N rows, one package, `note` when truncated (§6.2) | Task 3 |
| Lazy per-entry format extraction under the probe budget (§6.2) | Task 3 |
| `PackageCandidate.note` → persists as `DownloadPackage.note` (§5.4) | Tasks 1, 3, 4 |
| Handoff builds 1–2-component items; holds back non-resolved rows; HTTP siblings pass (§6.3) | Task 4 |
| Output/part filenames `<title> [<id>].fNNN.<ext>` (§5.2) | Tasks 1, 4 |
| Fixture-tested with a fake resolver, no network (§10.4) | all tasks |

**Deferred to Part 5:** Linkgrabber media-row rendering + spinner + badges; the flat format-picker menu (match-first ordering, ⚠ on non-matching); "N items held back" toast; the YouTube Settings tab (`YouTubeSettingsStore`: max resolution, three allowlists, max playlist videos, cookies-from-browser, yt-dlp/ffmpeg path overrides) and wiring the Part-3/Part-4 `{ .none }` / `{ .default }` / `{ 50 }` literal providers to it; promoting `ItemSnapshot.isAssembling` to `ItemState.assembling` + the details-panel per-component breakout + "Retry mux" button.

**2. Placeholder scan:** No "TBD"/"handle errors". Every task has concrete code or a concrete diff spec. Notes on `FakeProbeOrigin`/`LinkProber` construction and the `ffmpegAvailable` provider carry a stated fallback.

**3. Type consistency:** `MediaRow`/`MediaRowState` field + case names fixed in Task 1, used verbatim in 2–4. `MediaHandoff.build(httpLinks:mediaRows:) -> (items:heldBackCount:)` signature fixed in Task 4. `PackageCandidate.note` — one field, Task 1. `GrabberSession.init(prober:budget:resolver:qualityPreferences:)` — Task 2 fixes it; Task 5 may append `ffmpegAvailable:` (last, defaulted). `GrabberSnapshot.mediaRows` — one name.

---

## Execution Handoff

Executing inline in this session via superpowers:executing-plans, on a fresh branch (no worktree), immediately after writing. Merge to `main` and delete the branch on completion.
