# Phase 5 Part 5 — UI Polish: Settings, Format Picker, Assembly Surface: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status: IMPLEMENTED — merged to `main` 2026-09-02.** This plan is the last of the Phase 5 parts and it has no open work. The items its Self-Review lists as "Deferred" were reviewed and recorded in `todo.md` at the repo root — `ItemState.assembling` and per-component connection-demand precision as "Decided — no change"; the per-component details breakout and the yt-dlp/ffmpeg path-override fields as todo items. Do not treat the "Deferred" section below as a backlog.

**Goal:** Give the YouTube feature its user-facing surface: a Settings tab that actually drives the resolver's quality preferences, cookies, and playlist cap; a format-picker menu on each media row; the assembly step visible in the download list with a "Retry Mux" action; and a "held back" message on handoff.

**Architecture:** A new plain (non-`@MainActor`) `YouTubeSettingsStore` over `UserDefaults` mirrors `EngineSettingsStore`, exposing a computed `QualityPreferences`, a `CookieSource`, and `maxPlaylistVideos`. `EngineController` and `GrabberController` replace their Part-3/4 literal provider closures (`{ .none }`, `{ .default }`, `{ 50 }`) with reads from that store — the closures are evaluated per-resolve, so changes take effect without rebuilding. A new pure `MediaFormatMenu.options(for:preferences:)` builds the flat, match-first picker list; `MediaLinkRow` renders it as a `Menu`. `ItemSnapshot` gains `assembly: Assembly` so the download row can show "Assembling…" while `isAssembling` and offer "Retry Mux" on a failed mux item (`DownloadEngine.retryMux` already exists). `ItemState.assembling` as a distinct case, and a per-component details breakout, are **deferred** — the `.running` + `isAssembling` representation is functionally complete.

**Tech Stack:** Swift 6, Swift Testing, SwiftUI (macOS 15+), local SPM package `SDMKit`.

**Spec:** `docs/superpowers/specs/2026-09-02-phase-5-youtube-resolver-design.md` §9 (UI) and §12 (Settings additions), plus §9.2 (format picker) / §9.3 (Retry mux) / §6.3 ("N items held back").

## Global Constraints

- **Swift 6, strict concurrency.** `YouTubeSettingsStore` must be usable from the `@Sendable` provider closures — make it a plain enum over `UserDefaults.standard` (thread-safe), not `@MainActor`.
- **The full pre-existing suite (396 tests) MUST stay green.**
- **`xcodebuild -scheme SDM -destination 'platform=macOS' build` must succeed after every task.**
- **Format with `./format.sh`, lint clean with `./lint.sh` before every commit.**
- Fixed priority orders stay in code (`av1>vp9>h264`, `mp4>webm`, `opus>aac`); Settings only toggles membership. Parent spec §4.2.
- At least one video codec, one container, and one audio codec must always stay enabled — the UI blocks unchecking the last. Parent spec §9.4.
- Max playlist videos: 10–200, default 50. Max resolution: 144–4320, default 1080.

---

## File Structure

**`SDMKit/Sources/SDMEngine/EngineSnapshot.swift`:** `ItemSnapshot.assembly: Assembly` (defaulted `.none`).
**`SDMKit/Sources/SDMEngine/DownloadEngine.swift`:** populate `assembly:` in `snapshot()`.
**`SDMKit/Sources/SDMGrabber/MediaFormatMenu.swift` *(new)*:** the pure picker-list builder.

**`SDM/` (changes):**

| File | Change |
|---|---|
| `YouTubeSettingsStore.swift` *(new)* | `UserDefaults` store + computed `QualityPreferences` / `CookieSource` / `maxPlaylistVideos` |
| `EngineController.swift` | provider closures read `YouTubeSettingsStore`; `retryMux(_:)` forwarder |
| `GrabberController.swift` | provider closures read `YouTubeSettingsStore`; `refreshQualityAffectedRows` (re-sort picker only, never re-pick — matches §12a) is not needed since resolve runs once; just wire the closures |
| `SettingsView.swift` | new "YouTube" tab: max resolution stepper, three codec/container/audio checkbox rows, max-playlist stepper, cookies picker; buffered like the others, `commit()` writes the store |
| `LinkGrabberView.swift` | `MediaLinkRow` gets a format-picker `Menu`; a transient "N items held back" line after `addToDownloads` |
| `PackagesListView.swift` | row status shows "Assembling…" when `item.isAssembling`; context menu adds "Retry Mux" for a failed `.mux` item |

**`SDMKit/Tests/` (new):** `SDMGrabberTests/MediaFormatMenuTests.swift`; `SDMEngineTests` assertion for `ItemSnapshot.assembly`.

---

## Task 1: `ItemSnapshot.assembly`

**Files:**
- Modify: `SDMKit/Sources/SDMEngine/EngineSnapshot.swift`
- Modify: `SDMKit/Sources/SDMEngine/DownloadEngine.swift`
- Test: `SDMKit/Tests/SDMEngineTests/MuxAssemblyTests.swift` (append)

**Interfaces:**
- `ItemSnapshot.assembly: Assembly` — new stored `let`, trailing defaulted param `assembly: Assembly = .none` in `init`, set from `item.assembly` in `DownloadEngine.snapshot()`.

- [ ] **Step 1: Write the failing test** (append to `MuxAssemblyTests.swift`)

```swift
@Test func snapshotCarriesTheItemsAssemblyMode() async throws {
    let dir = try makeScratchDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let router = TwoHostRouter(
        videoHost: "v.example", videoPayload: testPayload(2000),
        audioHost: "a.example", audioPayload: testPayload(500))
    let engine = DownloadEngine(
        transport: router, stateStore: InMemoryStateStore(),
        settings: EngineSettings(
            maxConcurrent: 1, segmentsPerItem: 2, globalMaxConnections: 8, downloadFolder: dir),
        muxer: FakeMuxer())
    await engine.add(
        DownloadPackage(
            name: "P",
            items: [muxItem(URL(string: "https://v.example/v")!, URL(string: "https://a.example/a")!)]))
    let snap = try #require(await engine.snapshot().packages.first?.items.first)
    #expect(snap.assembly == .mux)
}
```

> `muxItem` / `FakeMuxer` / `TwoHostRouter` already exist in `SDMEngineTests`.

- [ ] **Step 2: Run** → FAIL (`assembly` not a member).
- [ ] **Step 3: Implement** — add the field + init param to `ItemSnapshot`; in `DownloadEngine.snapshot()`'s `ItemSnapshot(...)` call add `assembly: item.assembly`.
- [ ] **Step 4: Run** the new test + full suite (396 + 1).
- [ ] **Step 5: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDMKit/Sources/SDMEngine/ SDMKit/Tests/SDMEngineTests/MuxAssemblyTests.swift
git commit -m "feat(engine): expose DownloadItem.assembly on the snapshot"
```

---

## Task 2: `MediaFormatMenu` — the picker-list builder

**Files:**
- Create: `SDMKit/Sources/SDMGrabber/MediaFormatMenu.swift`
- Test: `SDMKit/Tests/SDMGrabberTests/MediaFormatMenuTests.swift`

**Interfaces:**
- Consumes: `ResolvedMedia`, `MediaFormat`, `QualityPreferences`, `FormatChoice` (`SDMCore`); `FormatSelector` (`SDMResolve`).
- Produces:
  - `struct MediaFormatOption: Identifiable, Sendable, Equatable`:
    - `let id: String` — stable (`"\(video.id)+\(audio?.id ?? "-")"`)
    - `var label: String` — `"1080p · av1 · webm · ~82 MB"` (`~` when any size is approximate; `"—"` when unknown)
    - `var choice: FormatChoice`
    - `var matchesPreferences: Bool`
  - `enum MediaFormatMenu` with `static func options(for media: ResolvedMedia, preferences: QualityPreferences) -> [MediaFormatOption]`:
    1. **Progressive** formats → one option each (`video` = the progressive format, `audio` = nil).
    2. **Video-only** formats → one option each, paired with the **best eligible audio** for that option's own eligibility check; when no eligible audio exists, pair with the best audio **ignoring** the allowlist (so the option still shows, marked non-matching).
    3. `matchesPreferences` = the video passes max-res + codec + container filters **and** the paired audio passes the audio-codec filter (progressive: just the video filters).
    4. Sort: matching options first, then non-matching; **within each group** by `FormatSelector.videoRankLess` (height → codec → container → tbr). This makes the auto-pick the first matching row (parent spec §9.2).
  - `estimatedBytes` on each `choice` = sum of `filesizeEffective`, nil if either unknown.

- [ ] **Step 1: Write the failing test**

```swift
// SDMKit/Tests/SDMGrabberTests/MediaFormatMenuTests.swift
import Foundation
import Testing

@testable import SDMCore
@testable import SDMGrabber

private func media(_ formats: [MediaFormat]) -> ResolvedMedia {
    ResolvedMedia(
        extractor: "youtube", videoID: "abc", title: "T", durationSeconds: 60, formats: formats)
}

@Test func matchingOptionsComeFirstRankedByResolutionThenCodec() {
    let m = media([
        vf("v720av1", 720, .av1, .webm),
        vf("v1080vp9", 1080, .vp9, .webm),
        vf("v2160av1", 2160, .av1, .mp4),
        af("a", .opus, .webm),
    ])
    let opts = MediaFormatMenu.options(for: m, preferences: .default)  // maxHeight 1080
    // 2160 fails max-res -> non-matching, sorts after both matchers
    #expect(opts.first?.choice.video?.id == "v1080vp9")
    #expect(opts.map(\.matchesPreferences) == [true, true, false])
    #expect(opts.last?.choice.video?.id == "v2160av1")
}

@Test func progressiveFormatsAppearWithNoAudioPairing() {
    let prog = MediaFormat(
        id: "18", kind: .progressive, height: 360, width: 640, vcodec: .h264, acodec: .aac,
        container: .mp4, filesize: 5_000_000, filesizeApprox: nil, tbr: nil,
        url: URL(string: "https://gv/18")!)
    let opts = MediaFormatMenu.options(for: media([prog]), preferences: .default)
    #expect(opts.count == 1)
    #expect(opts[0].choice.audio == nil)
    #expect(opts[0].label.contains("360p"))
}

@Test func aVideoOnlyOptionWithNoEligibleAudioStillShowsMarkedNonMatching() {
    var prefs = QualityPreferences.default
    prefs.audioCodecs = [.opus]
    let m = media([vf("v", 1080, .h264, .mp4), af("aac140", .aac, .m4a)])
    let opts = MediaFormatMenu.options(for: m, preferences: prefs)
    #expect(opts.count == 1)
    #expect(opts[0].matchesPreferences == false)
    #expect(opts[0].choice.audio?.id == "aac140")
}

@Test func labelMarksApproximateSizes() {
    let v = MediaFormat(
        id: "v", kind: .videoOnly, height: 1080, width: 1920, vcodec: .av1, acodec: nil,
        container: .webm, filesize: nil, filesizeApprox: 80_000_000, tbr: 3000,
        url: URL(string: "https://gv/v")!)
    let opts = MediaFormatMenu.options(for: media([v, af("a", .opus, .webm)]), preferences: .default)
    #expect(opts[0].label.contains("~"))
}
```

- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** `MediaFormatMenu.swift`.
- [ ] **Step 4: Run** 4 new tests + full suite.
- [ ] **Step 5: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDMKit/Sources/SDMGrabber/MediaFormatMenu.swift SDMKit/Tests/SDMGrabberTests/MediaFormatMenuTests.swift
git commit -m "feat(grabber): MediaFormatMenu builds the flat match-first picker list"
```

---

## Task 3: `YouTubeSettingsStore` + wire the providers

**Files:**
- Create: `SDM/YouTubeSettingsStore.swift`
- Modify: `SDM/EngineController.swift`, `SDM/GrabberController.swift`
- Test: `SDMKit` has no visibility of `SDM/` — no unit test; verify via `xcodebuild` + a `#Preview`-free manual check.

**Interfaces (`YouTubeSettingsStore` — plain `enum`, not `@MainActor`):**

```swift
enum YouTubeSettingsStore {
    private static let d = UserDefaults.standard
    private enum Key {
        static let maxHeight = "sdm.yt.maxHeight"
        static let allowAV1 = "sdm.yt.allowAV1"; static let allowVP9 = "sdm.yt.allowVP9"
        static let allowH264 = "sdm.yt.allowH264"
        static let allowMP4 = "sdm.yt.allowMP4"; static let allowWebM = "sdm.yt.allowWebM"
        static let allowOpus = "sdm.yt.allowOpus"; static let allowAAC = "sdm.yt.allowAAC"
        static let maxPlaylistVideos = "sdm.yt.maxPlaylistVideos"
        static let cookieSourceRaw = "sdm.yt.cookieSource"
    }
    // Each `allow*` defaults true; maxHeight default 1080; maxPlaylistVideos default 50 (clamped 10...200).
    // cookieSource default .none.

    static var maxHeight: Int { get / set }
    static var allowAV1/VP9/H264/MP4/WebM/Opus/AAC: Bool { get / set }
    static var maxPlaylistVideos: Int { get { clamp(10...200) } set }
    static var cookieSource: CookieSource { get { CookieSource(rawValue: raw) ?? .none } set }

    /// Live-computed from the toggles. Fixed priority orders live in
    /// `QualityPreferences`/`FormatSelector`, not here.
    static var qualityPreferences: QualityPreferences {
        var v: Set<VideoCodec> = []
        if allowAV1 { v.insert(.av1) }; if allowVP9 { v.insert(.vp9) }; if allowH264 { v.insert(.h264) }
        var c: Set<MediaContainer> = []
        if allowMP4 { c.insert(.mp4) }; if allowWebM { c.insert(.webm) }
        var a: Set<AudioCodec> = []
        if allowOpus { a.insert(.opus) }; if allowAAC { a.insert(.aac) }
        // A guard: never hand FormatSelector an empty set (the Settings UI
        // also blocks this, but a stale defaults file could be empty).
        return QualityPreferences(
            maxHeight: maxHeight,
            videoCodecs: v.isEmpty ? [.av1, .vp9, .h264] : v,
            containers: c.isEmpty ? [.mp4, .webm] : c,
            audioCodecs: a.isEmpty ? [.opus, .aac] : a)
    }
}
```

`import SDMCore` (for the codec/container types + `QualityPreferences`) and `import SDMResolve` (for `CookieSource`).

**Wiring:**
- `EngineController.init`: `YtDlpResolver(cookieSource: { YouTubeSettingsStore.cookieSource }, maxPlaylistVideos: { YouTubeSettingsStore.maxPlaylistVideos })`.
- `GrabberController.init`: same, plus `qualityPreferences: { YouTubeSettingsStore.qualityPreferences }`.
- `EngineController.retryMux(_ itemID: UUID) async { await engine.retryMux(itemID); publish(await engine.snapshot()) }`.

- [ ] **Step 1** — write `YouTubeSettingsStore.swift`.
- [ ] **Step 2** — replace the four literal closures + add `retryMux`.
- [ ] **Step 3** — `xcodebuild -scheme SDM -destination 'platform=macOS' build` → SUCCEEDED; `cd SDMKit && swift test` full green.
- [ ] **Step 4: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDM/YouTubeSettingsStore.swift SDM/EngineController.swift SDM/GrabberController.swift
git commit -m "feat(app): YouTubeSettingsStore drives resolver preferences, cookies, playlist cap"
```

---

## Task 4: The "YouTube" Settings tab

**Files:**
- Modify: `SDM/SettingsView.swift`

**Design:** a fifth tab, buffered exactly like Downloads/Linkgrabber (local `@State`, written in `commit()`). Contents:
- `SteppedNumberField("Maximum resolution (p)", value: $ytMaxHeight, range: 144...4320)` *(or a `Picker` of common heights — a stepper is simpler and matches the file's idiom)*.
- Three `SettingsSection`s: "Video codecs" (Toggles av1/vp9/h264), "Containers" (mp4/webm), "Audio codecs" (opus/aac). Each Toggle's `isOn` binding has a **setter guard**: unchecking is refused when it is the last `true` in its group (leave it on; optionally flash). Simplest: `.disabled(isOnlyOneLeft && thisIsOn)`.
- `SteppedNumberField("Maximum playlist videos", value: $ytMaxPlaylist, range: 10...200)`.
- `Picker("Cookies from browser", selection: $ytCookieSource)` over `CookieSource.allCases` (label: `.none` → "None", else `.rawValue.capitalized`).

`commit()` writes all of the above to `YouTubeSettingsStore` and calls `await controller.applyStoredSettings()` (harmless — the resolver closures are live anyway; keeps the pattern uniform). The grabber picks up new prefs on the next resolve automatically.

- [ ] **Step 1** — add the `@State` vars (init from `YouTubeSettingsStore`), the `youtubeTab` computed view, the `.tabItem` (`Label("YouTube", systemImage: "play.rectangle.on.rectangle")`), and the `commit()` writes.
- [ ] **Step 2** — `xcodebuild` build SUCCEEDED. Launch the app (`./build.sh` or Xcode run), open Settings → YouTube, toggle values, OK, reopen — values persist. Grab a YouTube URL and confirm the picked format respects a lowered max resolution.
- [ ] **Step 3: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDM/SettingsView.swift
git commit -m "feat(app): YouTube settings tab (resolution, codec/container allowlists, cookies, playlist cap)"
```

---

## Task 5: Format picker on media rows + "held back" message

**Files:**
- Modify: `SDM/LinkGrabberView.swift`

**Design:**
- `MediaLinkRow` gains a `Menu` (shown for `.resolved` and `.unselected` states) built from `MediaFormatMenu.options(for: row.media!, preferences: YouTubeSettingsStore.qualityPreferences)`:
  - Each option is a `Button(option.label) { Task { await controller.setFormatChoice(option.choice, for: row.id) } }`.
  - Non-matching options: prefix the label with "⚠ " and/or `.foregroundStyle(.secondary)` — a `Menu` can't easily dim items, so **prefix with "⚠ "** and add a `Divider()` between the matching and non-matching groups.
  - The menu's own label is the current `formatSummary` (or "Choose a format" when `.unselected`).
  - `row.media == nil` (still resolving / failed) → no menu, keep the existing badge.
- After `addToDownloads`, if `held > 0`, set a `@State private var lastHandoffMessage: String?` to `"\(held) item(s) held back — choose a format first"` and show it in the header for a few seconds (a simple `.task`-driven clear, or just leave it until the next handoff). Keep it minimal.

- [ ] **Step 1** — implement the `Menu` + the message state. `MediaLinkRow` needs `controller` passed in (currently only takes `row`, `theme`) — thread it through like `LinkRow` does.
- [ ] **Step 2** — `xcodebuild` build SUCCEEDED. Manual: grab a 1080p+ video, open the picker, pick a lower-res format → the row's size updates instantly (no spinner); pick nothing on an `unselected` row, hit "Add and start" → the header shows "1 item held back".
- [ ] **Step 3: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDM/LinkGrabberView.swift
git commit -m "feat(app): format-picker menu on media rows; held-back count on handoff"
```

---

## Task 6: Assembly surfaced in the download list

**Files:**
- Modify: `SDM/PackagesListView.swift`

**Design:**
- In `ItemRow.describe(_:)` (or `statusLine`), when `item.isAssembling` is true, show `"Assembling…"` instead of `"Running"`; give `stateIconName` a `film`/`wand.and.stars` variant for that case (all guarded by `item.isAssembling`, since `state` is still `.running` — no `ItemState` change).
- Context menu: a `Button("Retry Mux", systemImage: "arrow.trianglehead.clockwise")` shown when `items` contains an item with `if case .failed = item.state, item.assembly == .mux` — calls `await controller.retryMux(item.id)` for each such item. Place it right after "Retry Failed Item".
- The details panel (`singleItemData`): under "Filename", when `item.assembly == .mux`, add a line `"Assembling video + audio"` (a full per-component breakout with individual URLs/paths is deferred — `ItemSnapshot` carries only the primary component).

- [ ] **Step 1** — implement the three touch points.
- [ ] **Step 2** — `xcodebuild` build SUCCEEDED; `cd SDMKit && swift test` full green (no engine change). Manual: force a mux failure (rename `ffmpeg`), download a muxed video → item shows "Failed — mux failed: …", context menu offers "Retry Mux"; restore `ffmpeg`, Retry Mux → completes.
- [ ] **Step 3: Format, lint, commit**

```bash
./format.sh && ./lint.sh
git add SDM/PackagesListView.swift
git commit -m "feat(app): show assembly state and a Retry Mux action in the download list"
```

---

## Self-Review

**1. Spec coverage (Part 5 = parent spec §9 + §12 additions):**

| Spec item | Task |
|---|---|
| Max resolution + video/container/audio allowlists in Settings, fixed priority (§12, §9.4) | Tasks 3, 4 |
| "keep at least one" guard per group (§9.4) | Task 4 (UI) + Task 3 (store fallback) |
| Max playlist videos 10–200 default 50 (§12) | Tasks 3, 4 |
| Cookies-from-browser picker (§8, §12) | Tasks 3, 4 |
| Wire the literal providers to Settings (§7 Task 7 / Part 4 Task 5 follow-up) | Task 3 |
| Format-picker menu: flat, match-first, ⚠ on non-matching, instant size (§9.2) | Tasks 2, 5 |
| "N items held back" surfaced (§6.3) | Task 5 |
| Assembly visible; "Retry Mux" (§7.2, §9.3) | Tasks 1, 6 |

**Deferred (documented, not gaps):**
- `ItemState.assembling` as a distinct enum case — the `.running` + `isAssembling` flag is functionally complete and avoids touching ~8 exhaustive switches. Revisit only if a real need appears.
- Per-component details breakout in the bottom panel (`ItemSnapshot.components: [ComponentSnapshot]`) — needs an engine snapshot addition; the primary-component view is adequate for now.
- yt-dlp / ffmpeg **path override** fields in Settings — need a `BinaryLocator` reference plumbed through both controllers and an `applyStoredSettings` on `GrabberController`. Auto-discovery of the Homebrew paths covers the common case.
- Rich per-row spinner/animation polish beyond the state badge.

**2. Placeholder scan:** No "TBD". Task 3's store is given as a near-complete sketch (getters/setters are one-liners over `UserDefaults`); Tasks 4–6 are concrete SwiftUI diffs against named views. UI tasks verify via `xcodebuild` + a stated manual check rather than unit tests (SwiftUI here is deliberately logic-free — the testable logic is in Tasks 1–2).

**3. Type consistency:** `MediaFormatOption` / `MediaFormatMenu.options(for:preferences:)` fixed in Task 2, consumed in Task 5. `YouTubeSettingsStore.qualityPreferences` / `.cookieSource` / `.maxPlaylistVideos` fixed in Task 3, read in Tasks 3–5. `ItemSnapshot.assembly` fixed in Task 1, read in Task 6. `EngineController.retryMux(_:)` fixed in Task 3, called in Task 6. `CookieSource` is from `SDMResolve` (Part 1) — `SDM/` already links it transitively.

---

## Execution Handoff

Executing inline in this session via superpowers:executing-plans, on a fresh branch (no worktree), immediately after writing. Merge to `main` and delete the branch on completion.
