# Multi-Site Resolver & HLS/DASH Wholesale Fallback — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans (inline). Steps use `- [ ]` checkboxes.

**Goal:** Open the yt-dlp resolver gate from 5 YouTube hosts to a curated
~50-site registry, generalize playlist handling, and add the
yt-dlp-as-downloader "wholesale" path for HLS/DASH-only videos.

**Architecture:** A data-driven `SiteRegistry` replaces `handledHosts`.
`YtDlpParser` stops rejecting non-direct protocols and tags each
`MediaFormat` with a `MediaDelivery`. `FormatSelector` falls back to a
wholesale `FormatChoice` when no direct formats fit. A new injected
`WholesaleDownloader` protocol (concrete `YtDlpWholesaleDownloader` in
`SDMResolve`, `Fake` in tests) runs yt-dlp as a downloader; the engine
drives it through a new `WholesaleComponentTask` that mirrors the slice of
`DownloadTask`'s interface the engine touches, with progress kept as a
synthesized contiguous `RangeSet`.

**Tech Stack:** Swift 6 strict concurrency, Swift Testing, local SPM
package `SDMKit` (targets `SDMCore`/`SDMEngine`/`SDMGrabber`/`SDMResolve`),
Xcode app target `SDM`. yt-dlp/ffmpeg/ffprobe/qjs managed binaries.

**Spec:** `docs/superpowers/specs/2026-09-03-multi-site-resolver-design.md`

## Global Constraints

- macOS 15.0 baseline; macOS 26 behind `if #available`. Swift 6 language
  mode, strict concurrency.
- **No test may touch the network or sleep on a real clock.** Time is
  tick-driven (`engine.tick()`). New subprocess work goes through injected
  `ProcessRunner` / `WholesaleDownloader` fakes.
- Format Swift with `swift-format` before every commit. Dart n/a here.
- Never hand-edit `Package.resolved`. Vendored blobs are Git LFS;
  regenerate via `scripts/vendor-binaries.sh`.
- Colors from theme roles, never literals (SettingsView task).
- yt-dlp is a **metadata extractor** everywhere except the wholesale
  component, which is the only place it downloads.
- Wholesale downloads are **always non-resumable** (`isResumable == false`).
- Commit messages end with the two trailers from the session reminder
  (`Co-Authored-By:` + `Claude-Session:`).
- Build check: `cd SDMKit && swift build && swift test` for package work;
  `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
  for the app target.

---

## Phase A — `SDMCore` value types

### Task 1: `MediaDelivery` + `MediaFormat.delivery`

**Files:**
- Modify: `SDMKit/Sources/SDMCore/MediaFormat.swift`
- Test: `SDMKit/Tests/SDMCoreTests/MediaFormatTests.swift` (create)

**Interfaces:**
- Produces: `enum MediaDelivery: String, Codable, Sendable, Equatable { case direct, hls, dash }`;
  `MediaFormat.delivery: MediaDelivery` (new stored prop, `init` param
  defaulted `.direct` and placed **last** so existing call sites compile
  unchanged); `MediaFormat.isDirect: Bool { delivery == .direct }`.

- [ ] **Step 1 — failing test**

```swift
import Testing
@testable import SDMCore

@Test func mediaFormatDefaultsToDirectDelivery() {
    let f = MediaFormat(id: "1", kind: .progressive, height: 720, width: 1280,
        vcodec: .h264, acodec: .aac, container: .mp4,
        filesize: 100, filesizeApprox: nil, tbr: nil, url: URL(string: "https://x/y")!)
    #expect(f.delivery == .direct)
    #expect(f.isDirect)
}

@Test func mediaFormatCarriesExplicitDelivery() {
    let f = MediaFormat(id: "2", kind: .videoOnly, height: 1080, width: 1920,
        vcodec: .h264, acodec: nil, container: .mp4,
        filesize: nil, filesizeApprox: nil, tbr: nil,
        url: URL(string: "https://x/m.m3u8")!, delivery: .hls)
    #expect(f.delivery == .hls)
    #expect(!f.isDirect)
}
```

- [ ] **Step 2** — `swift test --filter MediaFormatTests` → fails to compile (`delivery` unknown).
- [ ] **Step 3 — implement.** Add the enum above `MediaFormat`. Add
  `public var delivery: MediaDelivery` stored prop. Add `delivery:
  MediaDelivery = .direct` as the **last** `init` parameter; assign it.
  Add `public var isDirect: Bool { delivery == .direct }`. `Codable`
  synthesis handles the new key; decoding an old payload without
  `delivery` will fail synthesized `Decodable` — add an explicit
  `init(from:)`? **No:** `MediaFormat` is only persisted inside
  `FileComponent`/sidecars via `ComponentOrigin` (which stores only a
  formatID string) and inside `DownloadItem` snapshots (transient). Grep
  confirms `MediaFormat` itself is never Codable-persisted standalone.
  Keep synthesized `Codable`. If grep finds a persistence path, add
  `decodeIfPresent(..) ?? .direct` in a custom `init(from:)`.
- [ ] **Step 4** — `swift test --filter MediaFormatTests` → pass; `swift build` → ok.
- [ ] **Step 5 — commit**: `feat(core): MediaDelivery + MediaFormat.delivery`

Run first: `rg -n "MediaFormat" SDMKit/Sources --glob '*.swift' | rg -i "encode|decode|Codable|JSONEncoder"` — expect no standalone-persistence hit.

### Task 2: `ComponentOrigin.wholesale`

**Files:**
- Modify: `SDMKit/Sources/SDMCore/FileComponent.swift:10-57`
- Test: `SDMKit/Tests/SDMCoreTests/ComponentOriginTests.swift` (create)

**Interfaces:**
- Produces: `ComponentOrigin.wholesale(formatSelector: String)`, Codable as
  `{"kind":"wholesale","formatSelector":"…"}`; unknown `kind` still decodes
  to `.http`.

- [ ] **Step 1 — failing tests**

```swift
@Test func wholesaleOriginRoundTrips() throws {
    let o = ComponentOrigin.wholesale(formatSelector: "bv*+ba/b")
    let data = try JSONEncoder().encode(o)
    #expect(try JSONDecoder().decode(ComponentOrigin.self, from: data) == o)
}
@Test func unknownKindDecodesToHttp() throws {
    let data = Data(#"{"kind":"martian"}"#.utf8)
    #expect(try JSONDecoder().decode(ComponentOrigin.self, from: data) == .http)
}
@Test func legacyResolvedStillDecodes() throws {
    let data = Data(#"{"kind":"resolved","formatID":"137"}"#.utf8)
    #expect(try JSONDecoder().decode(ComponentOrigin.self, from: data) == .resolved(formatID: "137"))
}
```

- [ ] **Step 2** — run → the wholesale case fails.
- [ ] **Step 3 — implement.** In the enum add `case wholesale(formatSelector: String)`.
  In `Kind` add `case wholesale`. In `init(from:)` current-shape switch add
  `case .wholesale: self = .wholesale(formatSelector: try container.decode(String.self, forKey: .formatID))`
  — reuse the existing `.formatID` `CodingKey` (rename the coding key
  comment to "formatID / formatSelector"). In `encode(to:)` add
  `case .wholesale(let sel): try container.encode(Kind.wholesale, forKey: .kind); try container.encode(sel, forKey: .formatID)`.
  The `default`/unknown path already yields `.http`.
- [ ] **Step 4** — run tests + `swift build`.
- [ ] **Step 5 — commit**: `feat(core): ComponentOrigin.wholesale case`

### Task 3: `WholesaleDownloader` seam + `FormatChoice.wholesaleSelector`

**Files:**
- Create: `SDMKit/Sources/SDMCore/WholesaleDownloader.swift`
- Modify: `SDMKit/Sources/SDMCore/QualityPreferences.swift` (FormatChoice)
- Test: `SDMKit/Tests/SDMCoreTests/FormatChoiceTests.swift` (create)

**Interfaces:**
- Produces:
  ```swift
  public struct WholesaleProgress: Sendable, Equatable {
      public enum Phase: Sendable, Equatable { case downloading, postProcessing }
      public var downloadedBytes: Int64?
      public var totalBytes: Int64?
      public var fraction: Double?
      public var phase: Phase
      public init(downloadedBytes: Int64? = nil, totalBytes: Int64? = nil,
                  fraction: Double? = nil, phase: Phase = .downloading)
  }
  public enum WholesaleError: Error, Equatable, Sendable {
      case binaryMissing, cancelled, authRequired, unavailable
      case failed(stderrTail: String)
  }
  public protocol WholesaleDownloader: Sendable {
      func download(pageURL: URL, formatSelector: String, destination: URL,
          onProgress: @Sendable @escaping (WholesaleProgress) -> Void) async throws
  }
  ```
- `FormatChoice.wholesaleSelector: String?` (new, defaulted `nil`, last
  init param); `FormatChoice.isWholesale: Bool`; `requiresMux` amended to
  `video != nil && audio != nil && wholesaleSelector == nil`.

- [ ] **Step 1 — failing tests**

```swift
@Test func directChoiceIsNotWholesale() {
    let c = FormatChoice(video: nil, audio: nil, outputContainer: .mp4, estimatedBytes: nil)
    #expect(!c.isWholesale)
}
@Test func wholesaleChoiceReportsItselfAndSkipsMux() {
    let c = FormatChoice(video: nil, audio: nil, outputContainer: .mp4,
        estimatedBytes: nil, wholesaleSelector: "bv*+ba/b")
    #expect(c.isWholesale)
    #expect(!c.requiresMux)
}
```

- [ ] **Step 2** — run → fails.
- [ ] **Step 3 — implement.** Add `wholesaleSelector: String? = nil` as the
  last stored prop + last init param of `FormatChoice`. Add
  `var isWholesale: Bool { wholesaleSelector != nil }`. Amend `requiresMux`.
  Create `WholesaleDownloader.swift` with the three types above.
- [ ] **Step 4** — run tests + `swift build`.
- [ ] **Step 5 — commit**: `feat(core): WholesaleDownloader seam + FormatChoice.wholesaleSelector`

---

## Phase B — `SDMResolve` resolution generalization

### Task 4: `SiteRegistry` + `SitePattern`

**Files:**
- Create: `SDMKit/Sources/SDMResolve/SiteRegistry.swift`
- Test: `SDMKit/Tests/SDMResolveTests/SiteRegistryTests.swift` (create)

**Interfaces:**
- Produces:
  ```swift
  public struct SitePattern: Sendable, Equatable {
      public enum HostMatch: Sendable, Equatable { case exact(String); case suffix(String) }
      public var hosts: [HostMatch]
      public var playlistPathHints: [String]
      public var extraArgs: [String]
  }
  public enum SiteRegistry {
      public static let patterns: [SitePattern]
      public static func match(_ url: URL) -> SitePattern?
  }
  ```
  Matching: host lowercased; `.exact("x.com")` matches `x.com` and
  `www.x.com`; `.suffix(".x.com")` matches any host ending `.x.com` or
  equal to `x.com`. First matching pattern wins.

- [ ] **Step 1 — failing tests** (fixture table)

```swift
import Testing
@testable import SDMResolve
private func u(_ s: String) -> URL { URL(string: s)! }

@Test(arguments: [
    ("https://www.youtube.com/watch?v=a", true),
    ("https://youtu.be/a", true),
    ("https://vimeo.com/12345", true),
    ("https://player.vimeo.com/video/12345", true),
    ("https://www.tiktok.com/@x/video/1", true),
    ("https://vm.tiktok.com/ZM1/", true),
    ("https://clips.twitch.tv/Foo", true),
    ("https://www.twitch.tv/videos/123", true),
    ("https://soundcloud.com/artist/track", true),
    ("https://www.aparat.com/v/abc", true),
    ("https://www.xvideos.com/video123/x", true),
    ("https://www.xnxx.com/video-x/y", true),
    ("https://www.pornhub.com/view_video.php?viewkey=1", true),
    ("https://www.reddit.com/r/x/comments/y/z/", true),
    ("https://x.com/user/status/1", true),
    ("https://twitter.com/user/status/1", true),
    ("https://example.com/video.mp4", false),
    ("https://youtube.com.evil.com/x", false),
    ("ftp://youtube.com/x", false),
    ("https://google.com/", false),
])
func siteRegistryMatchesExpected(_ urlString: String, _ expected: Bool) {
    #expect((SiteRegistry.match(u(urlString)) != nil) == expected)
}

@Test func playlistHintsExposed() {
    let p = SiteRegistry.match(u("https://soundcloud.com/artist/sets/mix"))
    #expect(p?.playlistPathHints.contains("/") == false)  // sanity: has hints, not "/"
}
```

- [ ] **Step 2** — run → fails to compile.
- [ ] **Step 3 — implement.** `match`: `guard let host = url.host?.lowercased()`.
  For each pattern, for each `HostMatch`: `.exact(h)` → `host == h || host == "www." + h`;
  `.suffix(s)` → `host == String(s.dropFirst()) || host.hasSuffix(s)`.
  Populate `patterns` from spec §4.1 (group with `// MARK:` comments:
  general video, live, social, audio, regional, broadcaster, adult).
  `playlistPathHints` set where obvious (`soundcloud`: `["/sets/"]`;
  youtube: `["/playlist", "/@", "/channel/", "/c/", "/user/"]`;
  `bandcamp`: `["/album/"]`; vimeo: `["/album/", "/channels/"]`). `extraArgs`
  empty everywhere for now.
- [ ] **Step 4** — run tests; `swift build`.
- [ ] **Step 5 — commit**: `feat(resolve): SiteRegistry — curated multi-site allowlist`

### Task 5: `YtDlpResolver.canHandle` via registry + `looksLikePlaylist`

**Files:**
- Modify: `SDMKit/Sources/SDMResolve/YtDlpResolver.swift:12-89`
- Test: `SDMKit/Tests/SDMResolveTests/YtDlpResolverTests.swift` (update
  `canHandleYouTubeHostsOnly` → `canHandleUsesSiteRegistry`)

**Interfaces:**
- Consumes: `SiteRegistry.match`.
- Produces: `canHandle` now true for any registry host; `handledHosts`
  deleted; `func looksLikePlaylist(_ url: URL) -> Bool` replaces
  `isPlaylistURL` (keep `isChannelURL` private helper, fold into it).

- [ ] **Step 1 — update test**

```swift
@Test func canHandleUsesSiteRegistry() {
    let r = YtDlpResolver(runner: FakeProcessRunner(), locator: makeLocator())
    #expect(r.canHandle(u("https://www.youtube.com/watch?v=abc")))
    #expect(r.canHandle(u("https://vimeo.com/12345")))
    #expect(r.canHandle(u("https://www.tiktok.com/@a/video/1")))
    #expect(!r.canHandle(u("https://example.com/video.mp4")))
    #expect(!r.canHandle(u("ftp://youtube.com/x")))
}
```

- [ ] **Step 2** — run → `vimeo`/`tiktok` expectations fail.
- [ ] **Step 3 — implement.** Delete `handledHosts`. `canHandle`: keep the
  scheme guard, `return SiteRegistry.match(url) != nil`. Replace
  `isPlaylistURL` body with the `looksLikePlaylist` logic from spec §5.2
  (query `list`, path prefixes, `SiteRegistry.match(url)?.playlistPathHints`).
  Update `refresh`'s `canHandle` guard call site (unchanged name). Splice
  `SiteRegistry.match(url)?.extraArgs ?? []` into `runYtDlp` arguments
  alongside `extraArguments()` — add a stored `perSiteArgs` computed at
  call time from the URL; simplest: pass the resolved pattern's `extraArgs`
  into `resolveSingle`/`resolvePlaylist`/`refresh` where the URL is known
  and append to `args`.
- [ ] **Step 4** — run the full `YtDlpResolverTests` + `swift build`.
- [ ] **Step 5 — commit**: `feat(resolve): route via SiteRegistry, generalize playlist detection`

### Task 6: `YtDlpParser` delivery tagging + `_type` adaptation + entry URLs

**Files:**
- Modify: `SDMKit/Sources/SDMResolve/YtDlpJSON.swift`
- Modify: `SDMKit/Sources/SDMResolve/YtDlpResolver.swift` (resolveSingle/resolvePlaylist)
- Modify: `SDMKit/Sources/SDMCore/ResolvedMedia.swift` (add `sourceURL`)
- Modify: `SDMKit/Tests/SDMResolveTests/Fixtures/` — add `video_direct_vimeo.json`,
  `playlist_soundcloud.json`, `single_but_type_playlist.json`; keep
  `video_hls_only.json`
- Test: `SDMKit/Tests/SDMResolveTests/YtDlpResolverTests.swift`,
  `YtDlpJSONTests.swift`

**Interfaces:**
- Consumes: `MediaDelivery` (Task 1).
- Produces: `YtDlpParser.deliveryFor(proto:) -> MediaDelivery?` (nil ⇒
  reject); `mediaFormat(from:)` now maps hls/dash instead of rejecting;
  `flatEntries(from:) -> [(sourceURL: URL, title: String)]` (was
  `(videoID, title)`); `ResolvedMedia.sourceURL: URL?` (new, last init
  param, defaulted nil); `resolveSingle` re-routes to playlist parsing when
  `dump._type == "playlist"`.

- [ ] **Step 1 — failing tests**

```swift
// YtDlpJSONTests
@Test func deliveryTagging() {
    #expect(YtDlpParser.deliveryFor(proto: "https") == .direct)
    #expect(YtDlpParser.deliveryFor(proto: nil) == .direct)
    #expect(YtDlpParser.deliveryFor(proto: "m3u8_native") == .hls)
    #expect(YtDlpParser.deliveryFor(proto: "http_dash_segments") == .dash)
    #expect(YtDlpParser.deliveryFor(proto: "rtmp") == nil)
}

// YtDlpResolverTests — REPLACES hlsOnlyVideoThrowsUnsupported
@Test func hlsOnlyVideoResolvesWithHlsFormats() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", ok(try fixtureData("video_hls_only")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    let target = try await r.resolve(u("https://youtu.be/hls"))
    guard case .single(let media) = target else { Issue.record("expected .single"); return }
    #expect(media.formats.contains { $0.delivery == .hls })
}

@Test func directVimeoResolves() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", ok(try fixtureData("video_direct_vimeo")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    let target = try await r.resolve(u("https://vimeo.com/12345"))
    guard case .single(let media) = target else { Issue.record("expected .single"); return }
    #expect(media.formats.allSatisfy { $0.delivery == .direct })
}

@Test func flatPlaylistEntriesCarrySourceURL() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("--flat-playlist", ok(try fixtureData("playlist_soundcloud")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    let target = try await r.resolve(u("https://soundcloud.com/artist/sets/mix"))
    guard case .playlist(_, let entries, _) = target else { Issue.record("expected .playlist"); return }
    #expect(entries.first?.sourceURL == u("https://soundcloud.com/artist/track-1"))
}

@Test func noPlaylistSingleThatIsActuallyAPlaylistRoutesToPlaylist() async throws {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", ok(try fixtureData("single_but_type_playlist")))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    let target = try await r.resolve(u("https://vimeo.com/showcase/1"))
    guard case .playlist = target else { Issue.record("expected .playlist"); return }
}
```

- [ ] **Step 2** — run → compile/assert failures.
- [ ] **Step 3 — implement.**
  - `YtDlpJSON`: add `webpageURL` (`webpage_url`) to `YtDlpEntry`; add
    `deliveryFor(proto:)` (switch: `nil,"https","http","https_native"` →
    `.direct`; `"m3u8","m3u8_native"` → `.hls`; `"http_dash_segments"` →
    `.dash`; default → `nil`). In `hasDirectURL` keep for the `.direct`
    check but add: `mediaFormat(from:)` computes
    `guard let delivery = deliveryFor(proto: f.proto) else { return nil }`,
    still rejects `mhtml`, requires non-empty `url`, sets `delivery` on the
    returned `MediaFormat`. Drop the `hasDirectURL` gate (delivery gate
    replaces it).
  - `flatEntries`: return `[(sourceURL: URL, title: String)]` — prefer
    `entry.url` then `entry.webpageURL`; resolve relative against the
    playlist URL; drop entries with no URL. (Signature change ⇒ update
    `resolvePlaylist`.)
  - `ResolvedMedia`: add `public var sourceURL: URL?` last prop + last init
    param `= nil`.
  - `resolvePlaylist`: build entries as
    `ResolvedMedia(extractor: "", videoID: "", title: $0.title, durationSeconds: nil, formats: [], sourceURL: $0.sourceURL)`
    (videoID no longer synthesizable; empty is fine — grabber uses
    `sourceURL`). Keep the `cap` / `prefix|suffix` logic; `isChannelURL`
    becomes a small helper reused by `looksLikePlaylist`.
  - `resolveSingle`: after `decodeDump`, `if dump._type == "playlist" {`
    parse entries via `flatEntries` and `return .playlist(...)` `}`.
  - `resolvePlaylist`: if `dump.entries` nil/empty but `dump.formats`
    present → `return .single(try YtDlpParser.resolvedMedia(from: dump))`.
- [ ] **Step 4** — new fixtures:
  - `video_direct_vimeo.json`: id `v123`, extractor `vimeo`, 2–3 formats
    all `protocol: https`, one progressive + one video-only + one
    audio-only, real `height`/`filesize`.
  - `playlist_soundcloud.json`: `_type playlist`, title `Mix`, 3 entries
    each `{ "url": "https://soundcloud.com/artist/track-1", "title": "Track 1" }` …
  - `single_but_type_playlist.json`: `_type: "playlist"`, 2 entries with
    `url`, no `formats`.
  - Update `playlist_flat.json` entries to include `"url"` fields (keep
    `id` for back-compat of other tests; the parser prefers `url`).
- [ ] **Step 5** — run `SDMResolveTests` + `SDMCoreTests`; `swift build`.
- [ ] **Step 6 — commit**: `feat(resolve): delivery-tagged formats, _type-adaptive resolve, entry source URLs`

Run first: `rg -n "flatEntries|isPlaylistURL|\.sourceURL|ResolvedMedia\(" SDMKit/Sources SDMKit/Tests --glob '*.swift'` to enumerate call sites to fix.

### Task 7: Classifier generic buckets + `ResolveError.drmProtected`

**Files:**
- Modify: `SDMKit/Sources/SDMResolve/YtDlpJSON.swift` (ResolveError) — wait,
  it's in `SDMResolve/ManagedBinariesTypes.swift`? Grep: `ResolveError` is
  in `SDMKit/Sources/SDMCore/LinkResolver.swift:15`. Modify there.
- Modify: `SDMKit/Sources/SDMResolve/YtDlpResolver.swift` (Classifier)
- Test: `SDMKit/Tests/SDMResolveTests/YtDlpResolverTests.swift`

**Interfaces:**
- Produces: `ResolveError.drmProtected` case; classifier maps generic
  login/geo/drm stderr.

- [ ] **Step 1 — failing tests**

```swift
@Test func genericLoginStderrIsAuthRequired() async {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", fail("ERROR: [vimeo] 1: This video requires you to log in."))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    await #expect(throws: ResolveError.authRequired) { try await r.resolve(u("https://vimeo.com/1")) }
}
@Test func geoStderrIsUnavailable() async {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", fail("ERROR: The uploader has not made this video available in your country."))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    await #expect(throws: ResolveError.unavailable) { try await r.resolve(u("https://vimeo.com/1")) }
}
@Test func drmStderrIsDrmProtected() async {
    let runner = FakeProcessRunner()
    runner.responses = [("-J", fail("ERROR: [generic] x: This video is DRM protected."))]
    let r = YtDlpResolver(runner: runner, locator: makeLocator())
    await #expect(throws: ResolveError.drmProtected) { try await r.resolve(u("https://x.com/1")) }
}
```

- [ ] **Step 2** — run → `drmProtected` unknown; others may misclassify.
- [ ] **Step 3 — implement.** Add `case drmProtected` to `ResolveError`.
  In `Classifier.error`, after the YouTube anti-bot block and before the
  `private video` check: `if lower.contains("drm") { return .drmProtected }`;
  add to the auth block `|| lower.contains("requires you to log in") || lower.contains("log in") || lower.contains("login required")`;
  add to unavailable `|| lower.contains("available in your country") || lower.contains("geo") || lower.contains("in your location")`.
- [ ] **Step 4** — run tests + `swift build`.
- [ ] **Step 5 — commit**: `feat(resolve): generic auth/geo/drm error classification`

### Task 8: `FormatSelector` wholesale fallback + `MediaFormatMenu` streamed rows

**Files:**
- Modify: `SDMKit/Sources/SDMResolve/FormatSelector.swift`
- Modify: `SDMKit/Sources/SDMGrabber/MediaFormatMenu.swift`
- Test: `SDMKit/Tests/SDMResolveTests/FormatSelectorTests.swift`,
  `SDMKit/Tests/SDMGrabberTests/MediaFormatMenuTests.swift`

**Interfaces:**
- Consumes: `MediaFormat.delivery`, `FormatChoice.wholesaleSelector`.
- Produces: `FormatSelector.pick` returns a wholesale `FormatChoice` when
  no `.direct` set fits but hls/dash formats exist;
  `FormatSelector.wholesaleSelector(maxHeight:) -> String` helper (pure,
  testable): `"bv*[height<=\(h)]+ba/b[height<=\(h)]/bv*+ba/b"`.
  `MediaFormatOption.isWholesale: Bool` (new); menu appends hls/dash rows.

- [ ] **Step 1 — failing tests**

```swift
// FormatSelectorTests
private func hls(_ id: String, _ h: Int) -> MediaFormat {
    MediaFormat(id: id, kind: .videoOnly, height: h, width: nil, vcodec: .h264, acodec: nil,
        container: .mp4, filesize: nil, filesizeApprox: nil, tbr: Double(h),
        url: URL(string: "https://x/\(id).m3u8")!, delivery: .hls)
}
@Test func directFormatsWinOverHls() {
    let media = ResolvedMedia(extractor: "x", videoID: "1", title: "t", durationSeconds: nil,
        formats: [vf("137", 720, .h264, .mp4), af("140", .aac, .m4a), hls("270", 1080)])
    let choice = FormatSelector.pick(media, .default)
    #expect(choice?.isWholesale == false)
    #expect(choice?.video?.id == "137")
}
@Test func hlsOnlyMediaYieldsWholesaleChoice() {
    let media = ResolvedMedia(extractor: "x", videoID: "1", title: "t", durationSeconds: nil,
        formats: [hls("270", 1080), hls("232", 720)])
    let choice = FormatSelector.pick(media, .default)
    #expect(choice?.isWholesale == true)
    #expect(choice?.wholesaleSelector.contains("height<=1080") == true)
    #expect(choice?.outputContainer == .mp4)
}
@Test func nothingUsableYieldsNil() {
    let media = ResolvedMedia(extractor: "x", videoID: "1", title: "t", durationSeconds: nil, formats: [])
    #expect(FormatSelector.pick(media, .default) == nil)
}
```

- [ ] **Step 2** — run → fails.
- [ ] **Step 3 — implement.**
  - `rankedVideoFormats` / `rankedAudioFormats` gain `.filter { $0.isDirect }`.
  - After the existing logic yields `nil`, add: `let streamed = media.formats.filter { !$0.isDirect && ($0.height ?? 0) <= prefs.maxHeight }`.
    If non-empty → return
    `FormatChoice(video: bestByHeightThenTbr, audio: nil,
      outputContainer: allWebm ? .webm : .mp4, estimatedBytes: best.filesizeEffective,
      wholesaleSelector: wholesaleSelector(maxHeight: prefs.maxHeight))`.
    If empty but `media.formats` has *any* non-direct format (ignoring
    maxHeight) → same with the smallest-height variant (so a
    higher-than-cap-only video still downloads).
  - `MediaFormatMenu.options`: after building direct options, append one
    option per `!isDirect` format: `MediaFormatOption(id: "w:\(f.id)",
    label: "\(f.height.map{"\($0)p"} ?? "auto") · streamed", choice:
    FormatChoice(video: f, audio: nil, outputContainer: .mp4,
    estimatedBytes: f.filesizeEffective, wholesaleSelector: f.id + "+ba/b"),
    matchesPreferences: false)` with `isWholesale: true`. Add `isWholesale`
    to `MediaFormatOption` (defaulted false).
- [ ] **Step 4** — run `FormatSelectorTests` + `MediaFormatMenuTests` + `swift build`.
- [ ] **Step 5 — commit**: `feat(resolve): wholesale FormatChoice fallback for HLS/DASH-only media`

---

## Phase C — `SDMResolve` wholesale downloader

### Task 9: `ProcessRunner.runStreaming`

**Files:**
- Modify: `SDMKit/Sources/SDMResolve/ProcessRunner.swift`
- Modify: `SDMKit/Sources/SDMResolve/SystemProcessRunner.swift`
- Modify: `SDMKit/Tests/SDMResolveTests/Support.swift` (FakeProcessRunner)
- Modify: `SDMKit/Tests/SDMEngineTests/ProcessRunnerSupport.swift` (its Fake)
- Test: `SDMKit/Tests/SDMResolveTests/SystemProcessRunnerTests.swift`

**Interfaces:**
- Produces:
  ```swift
  func runStreaming(executable: URL, arguments: [String], timeout: Duration,
      onLine: @Sendable @escaping (String) -> Void) async throws -> Int32
  ```
  added to `ProcessRunner`. Default extension implements `run` in terms of
  `runStreaming` (accumulating lines into stdout) **only** for conformers
  that don't provide `run` — but since the protocol still requires `run`,
  instead: give the protocol BOTH requirements and a default
  `runStreaming` that calls `run` then splits stdout into lines (good
  enough for fakes); `SystemProcessRunner` overrides with a real streaming
  impl.

- [ ] **Step 1 — failing test**

```swift
@Test func runStreamingForwardsLinesAndExitCode() async throws {
    let script = "/bin/sh"
    let runner = SystemProcessRunner()
    let lines = LineBox()
    let code = try await runner.runStreaming(
        executable: URL(fileURLWithPath: script),
        arguments: ["-c", "echo one; echo two; exit 3"],
        timeout: .seconds(5), onLine: { lines.append($0) })
    #expect(code == 3)
    #expect(lines.all.contains("one"))
    #expect(lines.all.contains("two"))
}
// LineBox: small NSLock-guarded [String] in the test file.
```

- [ ] **Step 2** — run → `runStreaming` unknown.
- [ ] **Step 3 — implement.**
  - Protocol: add `runStreaming` requirement; add a default impl in an
    `extension ProcessRunner` that calls `try await run(...)`, decodes
    stdout+stderr, and replays each `\n`-split line to `onLine`, returning
    `exitCode`. (Fakes inherit this.)
  - `SystemProcessRunner.runStreaming`: same `Process` setup as `run`, but
    the readability handlers buffer into a line-splitter
    (`LineAccumulator`: append bytes, on each `\n` emit the completed line
    via `onLine`), both pipes feeding the same `onLine`. Reuse
    `childEnvironment`, `ExitSignal`, timeout/cancel logic. Return
    `process.terminationStatus`. On flush at end, emit any trailing partial
    line.
  - `run` stays concrete in `SystemProcessRunner` (unchanged).
- [ ] **Step 4** — run `SystemProcessRunnerTests` + `swift build` + full `SDMResolveTests`/`SDMEngineTests` (fakes still compile).
- [ ] **Step 5 — commit**: `feat(resolve): ProcessRunner.runStreaming for line-by-line child output`

### Task 10: `WholesaleProgressParser`

**Files:**
- Create: `SDMKit/Sources/SDMResolve/WholesaleProgressParser.swift`
- Test: `SDMKit/Tests/SDMResolveTests/WholesaleProgressParserTests.swift` (create)

**Interfaces:**
- Consumes: `WholesaleProgress` (Task 3).
- Produces: `enum WholesaleProgressParser { static func parse(_ line: String) -> WholesaleProgress? }`.
  Recognizes `sdm:<status>|<downloaded>|<total>|<estimate>|<percent>` lines
  (from `--progress-template "sdm:%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress._percent_str)s"`)
  and postprocessing markers.

- [ ] **Step 1 — failing tests**

```swift
@Test func parsesByteProgress() {
    let p = WholesaleProgressParser.parse("sdm:downloading|1048576|10485760|NA| 10.0%")
    #expect(p?.downloadedBytes == 1048576)
    #expect(p?.totalBytes == 10485760)
    #expect(p?.phase == .downloading)
}
@Test func parsesEstimateWhenTotalMissing() {
    let p = WholesaleProgressParser.parse("sdm:downloading|500|NA|2000| 25.0%")
    #expect(p?.totalBytes == 2000)   // estimate promoted to total
    #expect(p?.fraction == 0.25)
}
@Test func recognizesPostProcessing() {
    #expect(WholesaleProgressParser.parse("[Merger] Merging formats into \"x.mp4\"")?.phase == .postProcessing)
    #expect(WholesaleProgressParser.parse("[FixupM3u8] Fixing MPEG-TS in MP4 container of \"x.mp4\"")?.phase == .postProcessing)
}
@Test func ignoresUnrelatedLines() {
    #expect(WholesaleProgressParser.parse("[youtube] Extracting URL") == nil)
}
```

- [ ] **Step 2** — run → fails.
- [ ] **Step 3 — implement.** `parse`: if line has prefix `"sdm:"` → split
  on `|`; field 0 after `:` is status, 1 downloaded, 2 total, 3 estimate,
  4 percent (trim, strip `%`). Parse ints (`"NA"`/`""` → nil). total =
  total ?? estimate. fraction = percent/100 (nil if unparseable). status
  `"finished"` still `.downloading` phase (engine treats success via exit
  code). Else if line contains any of `["[Merger]", "Merging formats",
  "[ExtractAudio]", "[FixupM3u8]", "[VideoConvertor]", "[Fixup"]` →
  `WholesaleProgress(phase: .postProcessing)`. Else `nil`.
- [ ] **Step 4** — run tests.
- [ ] **Step 5 — commit**: `feat(resolve): WholesaleProgressParser`

### Task 11: `YtDlpWholesaleDownloader`

**Files:**
- Create: `SDMKit/Sources/SDMResolve/YtDlpWholesaleDownloader.swift`
- Test: `SDMKit/Tests/SDMResolveTests/YtDlpWholesaleDownloaderTests.swift` (create)

**Interfaces:**
- Consumes: `WholesaleDownloader` protocol, `ProcessRunner.runStreaming`,
  `WholesaleProgressParser`, `BinaryLocator`, `CookieSource`, `SiteRegistry`.
- Produces:
  ```swift
  public struct YtDlpWholesaleDownloader: WholesaleDownloader {
      public init(runner: any ProcessRunner, locator: BinaryLocator,
          cookieSource: @escaping @Sendable () -> CookieSource = { .none },
          extraArguments: @escaping @Sendable () -> [String] = { [] },
          timeout: Duration = .seconds(7200))
  }
  ```

- [ ] **Step 1 — failing tests** (use a `FakeStreamingRunner` in the test
  file — records args, replays a scripted line sequence + exit code)

```swift
@Test func emitsProgressAndSucceeds() async throws {
    let runner = FakeStreamingRunner(lines: [
        "sdm:downloading|500|1000|NA| 50.0%",
        "sdm:downloading|1000|1000|NA| 100.0%",
        "[Merger] Merging formats into \"o.mp4\"",
    ], exitCode: 0)
    let d = YtDlpWholesaleDownloader(runner: runner, locator: makeLocator())
    let box = ProgressBox()
    try await d.download(pageURL: u("https://x.com/1"), formatSelector: "bv*+ba/b",
        destination: URL(fileURLWithPath: "/tmp/o.mp4"), onProgress: { box.append($0) })
    #expect(box.all.last?.phase == .postProcessing)
    #expect(box.all.contains { $0.downloadedBytes == 1000 })
    let args = runner.lastArguments
    #expect(args.contains("-f")); #expect(args.contains("bv*+ba/b"))
    #expect(args.contains("--newline")); #expect(args.contains("https://x.com/1"))
}
@Test func nonZeroExitThrowsFailed() async {
    let runner = FakeStreamingRunner(lines: ["ERROR: [x] boom"], exitCode: 1)
    let d = YtDlpWholesaleDownloader(runner: runner, locator: makeLocator())
    await #expect(throws: WholesaleError.failed(stderrTail: "ERROR: [x] boom")) {
        try await d.download(pageURL: u("https://x.com/1"), formatSelector: "b",
            destination: URL(fileURLWithPath: "/tmp/o.mp4"), onProgress: { _ in })
    }
}
@Test func missingBinaryThrows() async {
    let d = YtDlpWholesaleDownloader(runner: FakeStreamingRunner(lines: [], exitCode: 0),
        locator: makeLocator(hasYtDlp: false))
    await #expect(throws: WholesaleError.binaryMissing) {
        try await d.download(pageURL: u("https://x.com/1"), formatSelector: "b",
            destination: URL(fileURLWithPath: "/tmp/o.mp4"), onProgress: { _ in })
    }
}
```

- [ ] **Step 2** — run → fails.
- [ ] **Step 3 — implement.** `download`:
  - `guard let ytdlp = await locator.locate("yt-dlp") else { throw .binaryMissing }`.
  - `guard let binDir = ... ` (ffmpeg dir = `ytdlp.deletingLastPathComponent()`).
  - Build args per spec §6.6 (`-f`, `--no-playlist`, `--newline`,
    `--no-part`, `--no-continue`, `--progress-template "sdm:…"`,
    `--merge-output-format <destination ext>`, `--ffmpeg-location <binDir>`,
    `-o <destination.path>`, `<pageURL>`) + `SiteRegistry.match(pageURL)?.extraArgs`
    + `cookieSource().ytDlpArguments` + `extraArguments()`.
  - Accumulate stderr-ish lines into a rolling tail (last ~2000 chars of
    all lines that start with `ERROR`/`WARNING` or aren't `sdm:`).
  - `let code = try await runner.runStreaming(...) { line in
      if let p = WholesaleProgressParser.parse(line) { onProgress(p) }
      tail.append(line) }`.
  - `catch is CancellationError { throw WholesaleError.cancelled }`.
  - `guard code == 0 else { throw classifyWholesale(tail) }` — map DRM/geo/
    auth via the shared classifier free function (lift `Classifier.error`
    to `internal func classifyResolveError` reused here, translating
    `.authRequired`→`.authRequired`, `.unavailable`/`.drmProtected`→
    `.unavailable`, else `.failed(tail)`).
- [ ] **Step 4** — run tests + `swift build` + full `SDMResolveTests`.
- [ ] **Step 5 — commit**: `feat(resolve): YtDlpWholesaleDownloader (yt-dlp as downloader)`

---

## Phase D — `SDMGrabber`

### Task 12: playlist entry URLs, wholesale handoff, generalized copy

**Files:**
- Modify: `SDMKit/Sources/SDMGrabber/GrabberSession.swift` (expandPlaylist:375, rowState:314, recluster:734)
- Modify: `SDMKit/Sources/SDMGrabber/MediaHandoff.swift`
- Modify: `SDMKit/Sources/SDMGrabber/MediaRow.swift` (displayFilename container for wholesale)
- Test: `SDMKit/Tests/SDMGrabberTests/MediaHandoffTests.swift`,
  `PlaylistExpansionTests.swift`, `MediaRowRoutingTests.swift`

**Interfaces:**
- Consumes: `ResolvedMedia.sourceURL`, `FormatChoice.wholesaleSelector`,
  `ComponentOrigin.wholesale`, `ResolveError.drmProtected`.
- Produces: `MediaHandoff.build` emits a single non-resumable `.wholesale`
  component for a wholesale choice.

- [ ] **Step 1 — failing tests**

```swift
// MediaHandoffTests
@Test func aWholesaleRowBecomesOneNonResumableComponent() {
    let hlsFmt = MediaFormat(id: "270", kind: .videoOnly, height: 1080, width: nil,
        vcodec: .h264, acodec: nil, container: .mp4, filesize: nil, filesizeApprox: nil,
        tbr: nil, url: URL(string: "https://x/m.m3u8")!, delivery: .hls)
    var row = MediaRow(sourceURL: URL(string: "https://www.tiktok.com/@a/video/1")!)
    row.media = ResolvedMedia(extractor: "tiktok", videoID: "1", title: "Clip",
        durationSeconds: 10, formats: [hlsFmt])
    row.choice = FormatChoice(video: hlsFmt, audio: nil, outputContainer: .mp4,
        estimatedBytes: nil, wholesaleSelector: "bv*+ba/b")
    row.state = .resolved
    let (items, held) = MediaHandoff.build(httpLinks: [], mediaRows: [row])
    #expect(held == 0)
    #expect(items[0].components.count == 1)
    #expect(items[0].assembly == .none)
    #expect(items[0].components[0].isResumable == false)
    if case .wholesale(let sel) = items[0].components[0].origin { #expect(sel == "bv*+ba/b") }
    else { Issue.record("expected .wholesale origin") }
    #expect(items[0].components[0].partFilename == items[0].outputFilename)
    #expect(items[0].url == URL(string: "https://www.tiktok.com/@a/video/1")!)
}

// PlaylistExpansionTests — new fixture playlist_soundcloud, resolver stub returns entries with sourceURL
@Test func playlistEntriesUseTheirOwnSourceURL() async {
    // ... expand a soundcloud set; assert a row's sourceURL == the entry url, not youtube.com/watch
}
```

- [ ] **Step 2** — run → fails.
- [ ] **Step 3 — implement.**
  - `MediaHandoff.build`: before the `choice.video`/`choice.audio` block,
    `if let selector = choice.wholesaleSelector { components = [FileComponent(
      url: row.sourceURL,
      partFilename: "\(stem).\(choice.outputContainer.fileExtension)",
      totalBytes: choice.estimatedBytes,
      origin: .wholesale(formatSelector: selector),
      isResumable: false)]
      assembly = .none }` then fall through to the `components.count == 1`
    rename + `DownloadItem(...)` construction (guard `!components.isEmpty`).
  - `GrabberSession.expandPlaylist`: replace
    `let watchURL = URL(string: "https://www.youtube.com/watch?v=\(entry.videoID)")!`
    with `guard let watchURL = entry.sourceURL else { continue }`.
  - `rowState(for:)`: `.authRequired` message → drop "YouTube", say "The
    site blocked the request (sign-in / anti-bot). Set \"Cookies from
    browser\" in Settings → Media Sites, or update yt-dlp."; add
    `case ResolveError.drmProtected: return .unsupported`.
  - `recluster()`: `row.sourceURL.host ?? "youtube.com"` → `?? "media"`.
- [ ] **Step 4** — run `SDMGrabberTests` in full + `swift build`.
- [ ] **Step 5 — commit**: `feat(grabber): wholesale handoff, real playlist-entry URLs, site-neutral copy`

---

## Phase E — `SDMEngine` wholesale

### Task 13: `WholesaleComponentTask` + engine integration

**Files:**
- Create: `SDMKit/Sources/SDMEngine/WholesaleComponentTask.swift`
- Modify: `SDMKit/Sources/SDMEngine/DownloadEngine.swift` (ComponentRun:103,
  init:196, runComponent:1454, runItem, snapshot:968, tick:782, setSegmentCount:822,
  reconcile task construction:1220, scheduler pass-1 eligibility)
- Modify: `SDMKit/Sources/SDMEngine/Scheduler.swift` (non-resumable slot
  reservation already keys on `isResumable == false` — verify wholesale
  items get `isResumable == false` in their snapshot before scheduling)
- Create: `SDMKit/Tests/SDMEngineTests/FakeWholesaleDownloader.swift`
- Test: `SDMKit/Tests/SDMEngineTests/WholesaleDownloadTests.swift` (create)

**Interfaces:**
- Consumes: `WholesaleDownloader`, `WholesaleProgress`, `WholesaleError`,
  `ComponentOrigin.wholesale`.
- Produces: `actor WholesaleComponentTask` exposing the subset `Runner`
  touches:
  ```swift
  init(itemID: UUID, pageURL: URL, formatSelector: String, destinationURL: URL,
       downloader: any WholesaleDownloader)
  func start() async throws -> URL
  func pause() async
  var completedRanges: RangeSet { get }        // synthesized 0..<downloaded
  var expectedTotalBytes: Int64? { get }
  var activeWorkerCount: Int { get }           // 1 while running else 0
  var probedSupportsRanges: Bool? { get }      // always false once started
  var peakWorkerCount: Int { get }             // 1
  var lastCheckpointFailure: String? { get }   // always nil
  var isAssembling: Bool { get }               // true during postProcessing
  func checkpointTick() async                  // no-op
  func setWorkerCount(_ n: Int) async          // no-op
  ```
- `ComponentRun` gains `enum Worker { case segmented(DownloadTask); case wholesale(WholesaleComponentTask) }`
  and forwarding async accessors so existing `componentRun.task.X` call
  sites become `componentRun.X` (or keep a `var task` for segmented-only
  paths + guard).

- [ ] **Step 1 — failing tests** (`FakeWholesaleDownloader`: `enqueue`
  a script of `WholesaleProgress`, then `finish(.success)` /
  `finish(.throwing(WholesaleError))`; `download` awaits a continuation the
  test releases step by step so everything stays tick/checkpoint-free.)

```swift
@Test func wholesaleDownloadReportsSynthesizedProgressAndCompletes() async throws {
    let dir = try makeScratchDirectory(); defer { try? FileManager.default.removeItem(at: dir) }
    let fake = FakeWholesaleDownloader()
    let engine = DownloadEngine(transport: FakeOrigin(payload: Data()),
        stateStore: InMemoryStateStore(),
        settings: EngineSettings(maxConcurrent: 1, segmentsPerItem: 1,
            globalMaxConnections: 8, downloadFolder: dir),
        wholesaleDownloader: fake)
    var comp = FileComponent(url: URL(string: "https://tiktok.com/@a/video/1")!,
        partFilename: "Clip.mp4", totalBytes: nil,
        origin: .wholesale(formatSelector: "bv*+ba/b"), isResumable: false)
    let item = DownloadItem(components: [comp], outputFilename: "Clip.mp4",
        sourceURL: URL(string: "https://tiktok.com/@a/video/1")!, assembly: .none, state: .queued)
    await engine.add(DownloadPackage(name: "P", items: [item]))
    await engine.reconcile()
    fake.emit(.init(downloadedBytes: 500, totalBytes: 1000, phase: .downloading))
    // snapshot: completed == 500, total == 1000
    var snap = await snapshotItem(item.id, in: engine)
    #expect(snap?.completed.totalBytes == 500)
    #expect(snap?.totalBytes == 1000)
    #expect(snap?.isResumable == false)
    fake.emit(.init(phase: .postProcessing))
    snap = await snapshotItem(item.id, in: engine)
    #expect(snap?.isAssembling == true)
    fake.finishWritingFile(at: dir)     // writes the destination file
    fake.finish(.success)
    try await engine.runUntilIdle()
    snap = await snapshotItem(item.id, in: engine)
    #expect(snap?.state == .completed)
}

@Test func wholesalePauseDiscardsPartialAndRestartsFromZero() async throws { /* emit 400 bytes, stopItem, assert completed==0 & no file; start → downloader called again */ }

@Test func runningWholesaleItemReservesItsSlot() async throws {
    // maxConcurrent 1; a running wholesale item + a higher-rank resumable queued item
    // → the resumable one stays .queued (pass-1 reservation), wholesale keeps running.
}

@Test func wholesaleAuthErrorIsPermanentFailure() async throws { /* finish(.throwing(.authRequired)) → item .failed w/ message */ }

@Test func staleWholesaleSidecarLoadsEmpty() async throws { /* write a .sdmpart for the part file, then start → completed starts at 0 */ }
```

- [ ] **Step 2** — run → compile failures (`wholesaleDownloader:` param unknown).
- [ ] **Step 3 — implement (sub-steps, commit once at end):**
  1. `WholesaleComponentTask.swift`: actor per interface. `start()`:
     set `running = true`; `try await downloader.download(pageURL:…,
     formatSelector:…, destination: destinationURL) { p in
        Task { await self.apply(p) } }` — but `onProgress` is sync
     `@Sendable`; instead store progress via an `nonisolated` lock-guarded
     box the actor reads, OR make `apply` update a `nonisolated(unsafe)`
     `Mutex`-guarded struct. Simplest: `final class ProgressState:
     @unchecked Sendable` with `NSLock`, holding `downloaded/total/phase`.
     `completedRanges` computes `RangeSet(ByteRange(0, clampedDownloaded))`.
     On `download` return → `running = false`; verify
     `FileManager.fileExists(destinationURL)` else throw
     `WholesaleError.failed`. Return `destinationURL`. On throw: delete
     `destinationURL` + `destinationURL.path + ".part"`; rethrow.
     `pause()`: cancel the internal `Task`/continuation → `download`
     throws `CancellationError` → mapped `.cancelled`; delete partials.
  2. `ComponentRun`: replace `var task: DownloadTask` with
     `var worker: Worker`; add computed helpers:
     `var destinationURL` stays. Add async forwarders:
     `func start() async throws -> URL`, `func pause() async`,
     `var completedRanges: RangeSet` → `switch worker { ... }` (both types
     expose it), etc. Update the ~10 call sites (`allTasks` becomes
     `func pauseAll()` / a `[Worker]`; `allDestinations` unchanged).
  3. `DownloadEngine.init`: add
     `wholesaleDownloader: (any WholesaleDownloader)? = nil`, store it.
  4. Task construction in `reconcile` (~1222): per component,
     `if case .wholesale(let sel) = componentContext.origin {
        worker = .wholesale(WholesaleComponentTask(itemID: itemID,
          pageURL: item.sourceURL ?? componentContext.sourceURL,
          formatSelector: sel, destinationURL: componentContext.destinationURL,
          downloader: wholesaleDownloader!)) }` else segmented as today.
     Guard: if `wholesaleDownloader == nil` and a component is wholesale →
     fail the item with a clear message (shouldn't happen in the app).
     `ItemRunContext`/`componentContext` must carry `origin` — add it.
  5. `runComponent` (~1454): `if case .wholesale = component.origin {
        do { _ = try await componentRun.start(); return nil }
        catch WholesaleError.cancelled { return CancellationError() as Error }  // handled by retire-intent branch
        catch { return error } }` — skip the `urlExpired`/refresh loop
     entirely for wholesale.
  6. `failureState(for:)` — map `WholesaleError.authRequired` /
     `.unavailable` → permanent; `.failed` → transient/permanent by tail
     via existing rules; `.cancelled` never reaches here (retire-intent).
  7. `snapshot` (~968): the per-component loop calls
     `componentRun.completedRanges` / `.expectedTotalBytes` /
     `.activeWorkerCount` / `.lastCheckpointFailure` (forwarders) —
     unchanged logic. `isAssembling`: OR in
     `runner.components.contains { $0.isAssembling }` alongside the
     `assembling` set.
  8. `tick` (~782): `componentRun.checkpointTick()` (no-op forwarder);
     `completedRanges.totalBytes` forwarder feeds `recordProgress` → speed
     sampler works (bytes rise as yt-dlp reports).
  9. `setSegmentCount` (~822): `componentRun.setWorkerCount` forwarder
     (no-op for wholesale). Fine.
  10. **Scheduler / non-preemption:** a wholesale item's snapshot
      `isResumable` must be `false`. `DownloadItem.isResumable` for a
      wholesale component — set it `false` at handoff (Task 12 does) and
      never let `reconcile`'s `restored[...].isResumable = nil` reset
      matter: pass-1 keys on `isResumable == false`, and `nil` makes it
      preemptible. Fix: in `reconcile`, when building `runningNow` /
      pass-1 input, treat an item whose component 0 origin is `.wholesale`
      as `isResumable == false` regardless of the stored tri-state.
      Add `DownloadItem.hasWholesaleComponent: Bool` and use it in the
      scheduler eligibility mapping.
  11. **Sidecar guard:** wholesale components never write a sidecar
      (`WholesaleComponentTask` has no checkpoint). On `context(for:)` /
      cached-completed assembly, for a `.wholesale` component force
      `cachedCompleted = nil`. Also `resetDownload`/artefact cleanup
      already deletes `partFilename` + `.part`; add `.part` sibling of the
      part file to `itemArtefactURLs` (yt-dlp `--no-part` means usually
      none, but be safe).
  12. `EngineController` (app) passes the real downloader — done in Task 14.
- [ ] **Step 4 — run the FULL engine suite** (`swift test --filter SDMEngineTests`)
  plus `swift test` whole package. Fix regressions. Expect 0 failures.
- [ ] **Step 5 — commit**: `feat(engine): wholesale component task + non-resumable yt-dlp download path`

---

## Phase F — App wiring, Settings, vendored ffprobe

### Task 14: App integration + Settings "Media Sites" + ffprobe vendoring

**Files:**
- Rename: `SDM/YouTubeSettingsStore.swift` → `SDM/MediaSitesSettingsStore.swift`
  (type `YouTubeSettingsStore` → `MediaSitesSettingsStore`; **UserDefaults
  key strings unchanged**)
- Modify: `SDM/SettingsView.swift` (tab label/icon, 3-column codec layout,
  `yt…`→`media…` state names, copy strings)
- Modify: `SDM/EngineController.swift` (construct + inject
  `YtDlpWholesaleDownloader`)
- Modify: `SDM/GrabberController.swift` (no change expected — verify
  resolver already covers new hosts via `canHandle`)
- Modify: `scripts/vendor-binaries.sh` (fetch + pack `ffprobe`)
- Modify: `SDM/…` VendorAsset list (grep `VendorAsset(` in `SDM/`) — add
  `ffprobe`
- Modify: `SDMKit/Sources/SDMResolve/ManagedBinariesTypes.swift`
  (`BinariesManifest.version(for:)` maps `"ffprobe"` → `ffmpegVersion`)
- Modify: `SDMKit/Sources/SDMResolve/ManagedBinaries.swift`
  (`provisionBundledIfNeeded` switch: `case "ffmpeg", "ffprobe":`)
- Modify: `CLAUDE.md`, `todo.md` (check off #2), spec status banner
- Test: `SDMKit/Tests/SDMResolveTests/ManagedBinariesInflationTests.swift`
  (ffprobe inflates), `ManagedBinariesTypesTests.swift`

- [ ] **Step 1 — failing test**

```swift
@Test func ffprobeInflatesFromBundle() async throws {
    // mirror the existing ffmpeg inflation test with an "ffprobe" VendorAsset
    // → assert isExecutableFile at binDir/ffprobe and manifest.ffmpegVersion set
}
@Test func manifestMapsFfprobeToFfmpegVersion() {
    var m = BinariesManifest.empty; m.ffmpegVersion = "7.1"
    #expect(m.version(for: "ffprobe") == "7.1")
}
```

- [ ] **Step 2** — run → fails.
- [ ] **Step 3 — implement.**
  - `ManagedBinariesTypes`: `version(for:)` add `case "ffprobe": ffmpegVersion`.
  - `ManagedBinaries.provisionBundledIfNeeded`: `switch asset.name {
    case "ffmpeg", "ffprobe": manifest.ffmpegVersion = asset.version ...}`.
  - `vendor-binaries.sh`: generalize `fetch_ffmpeg` → `fetch_tool <tool>
    <arch> <sitearch> <out> <build>` that unzips `<tool>` from
    `<tool>.zip` at the same MR build path; call it for `ffmpeg` and
    `ffprobe` per arch. (`ffprobe.zip` exists at the same
    `/download/macos/<arch>/<build>/ffprobe.zip`.)
  - App `VendorAsset` list: add
    `VendorAsset(name: "ffprobe", compressedURL: bundle "ffprobe-<arch>.lzfse", version: manifest ffmpegVersion)`.
  - `SDM/MediaSitesSettingsStore.swift`: `sed`-style rename of the type
    only; keys like `"youtube.maxHeight"` stay.
  - `SettingsView`: `youtubeTab` → `mediaSitesTab`; `.tabItem { Label("Media Sites", systemImage: "film.stack") }`;
    all `yt…` `@State` → `media…`; the three `SettingsSection`s
    (`Video codecs`/`Containers`/`Audio codecs`) wrapped in
    `HStack(alignment: .top, spacing: 16) { … }` each `.frame(maxWidth: .infinity, alignment: .leading)`;
    `"Settings → YouTube"` copy → `"Settings → Media Sites"`; add `ffprobe`
    row to `componentsSection` (mirror the `ffmpeg` row).
  - `EngineController`: build
    `let wholesale = YtDlpWholesaleDownloader(runner: SystemProcessRunner(),
       locator: sharedLocator, cookieSource: { MediaSitesSettingsStore.cookieSource },
       extraArguments: { ytDlpExtraArgs })` and pass
    `wholesaleDownloader: wholesale` to `DownloadEngine(...)`.
  - `CLAUDE.md` "Current state" + Phase 5 spec note: one line on
    multi-site + wholesale. `todo.md`: check `[x]` item 2, note the spec.
    Spec banner → `IMPLEMENTED`.
- [ ] **Step 4 — build the app target:**
  `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build`
  → must succeed. Then `cd SDMKit && swift test` full suite → 0 failures.
- [ ] **Step 5 — regenerate vendored binaries:**
  `./scripts/vendor-binaries.sh` (network; downloads ffmpeg+ffprobe, builds
  qjs). Verify `SDM/Resources/vendor/ffprobe-arm64.lzfse` +
  `ffprobe-x86_64.lzfse` exist and are LFS pointers after `git add`.
  `git add SDM/Resources/vendor && git status` — confirm `.lzfse` show as
  LFS.
- [ ] **Step 6 — commit** (two commits):
  - `feat(app): Media Sites settings, wholesale downloader wiring, ffprobe bundling`
  - `chore(build): vendor ffprobe alongside ffmpeg` (the LFS blobs +
    manifest)

---

## Final verification

- [ ] `cd SDMKit && swift build 2>&1 | tail -5` — clean.
- [ ] `cd SDMKit && swift test 2>&1 | tail -20` — all pass, count ≥ prior
  (~401) + new (~45).
- [ ] `xcodebuild -project SDM.xcodeproj -scheme SDM -destination 'platform=macOS' build 2>&1 | tail -5` — `BUILD SUCCEEDED`.
- [ ] `swift-format lint -r SDMKit/Sources SDM` — no diagnostics (or run
  `swift-format -i -r` and commit).
- [ ] `git log --oneline main..HEAD` — coherent history.
- [ ] Merge: `git checkout main && git merge --no-ff feat/multi-site-resolver`
  then `git branch -d feat/multi-site-resolver`.
- [ ] Final report to user with sample test URLs (NOT committed anywhere).

---

## Self-Review

**Spec coverage:**
- §4 registry → Task 4. §5 generalized resolution → Tasks 5–7. §6.1
  delivery → Task 1/6. §6.2 FormatChoice → Task 3. §6.3 selection → Task 8.
  §6.4 handoff → Task 12. §6.5 seam → Task 3. §6.6 downloader → Tasks
  9–11. §6.7 engine → Task 13. §6.8 unsupported → Task 7/12. §7 grabber →
  Task 12. §8 app/settings → Task 14. §9 ffprobe → Task 14. §10 testing →
  every task's tests. §11 rollout → Final. Covered.

**Placeholder scan:** No "TBD"/"handle errors appropriately". Each task has
real test + impl code or precise mechanical steps against named symbols.

**Type consistency:** `WholesaleProgress`/`WholesaleError`/`WholesaleDownloader`
defined Task 3, consumed Tasks 11/13 with matching signatures.
`MediaDelivery`/`isDirect` Task 1 → Tasks 6/8. `FormatChoice.wholesaleSelector`
Task 3 → Tasks 8/12. `ComponentOrigin.wholesale(formatSelector:)` Task 2 →
Tasks 12/13. `flatEntries` new signature Task 6 → Task 12.
`WholesaleComponentTask` interface Task 13 self-contained.

**Known risk (flagged in spec §12):** Task 13 touches `DownloadEngine`'s
`ComponentRun` — the delicate core actor. Mitigation: `WholesaleComponentTask`
mirrors the exact interface subset; forwarders keep call sites shape-stable;
full engine suite is the gate before commit.
