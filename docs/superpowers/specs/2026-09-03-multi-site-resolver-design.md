# SDM — Multi-Site Resolver & HLS/DASH Wholesale Fallback: Design

Date: 2026-09-03
Status: **IMPLEMENTED — merged to `main` 2026-09-03 on branch
`feat/multi-site-resolver`.** Plan:
`docs/superpowers/plans/2026-09-03-multi-site-resolver.md`. ~496 package
tests, app target builds.

**Superseded (2026-09-03):** §6.7's "a `.wholesale` component's
`isResumable` is always `false` … next start re-runs yt-dlp from zero" no
longer holds. Wholesale downloads now resume via yt-dlp's fragment-level
`--continue` and become preemptible once a fragmented progress report
confirms the native downloader is in use. See
`2026-09-03-wholesale-resume-design.md`.

**Post-merge fixes (2026-09-03, from real Twitch-VOD testing):**
- Wholesale progress template now emits `fragment_index` / `fragment_count`
  instead of `_percent_str`; `WholesaleProgressParser` parses yt-dlp's
  **float**-formatted `total_bytes_estimate` (it was silently dropped, so
  HLS downloads showed total 0 / progress stuck at 0%).
  `WholesaleComponentTask` derives the item total as `downloaded / fraction`
  from the fragment ratio (re-evaluated every report, so the UI tracks
  yt-dlp's moving estimate) and falls back to the reported estimate before
  the first fragment.
- `YtDlpArtifacts` sweeps the scratch files a killed yt-dlp leaves
  (`Clip.mp4.ytdl`, `Clip.mp4-FragN`, `Clip.fNNN.*`, `Clip.temp.mp4`) on
  pause / failure / item removal — the old cleanup only knew `.part` and
  `.ytdl`.
- **Codec-less formats no longer rejected.** xnxx / xvideos (and others)
  omit `vcodec` / `acodec` on every format. `YtDlpParser` now infers the
  kind from resolution / container (a direct video container ⇒ progressive;
  a real height ⇒ a video variant; a codec-less HLS entry with no height ⇒
  skipped, it is usually a bare audio manifest). `FormatSelector` treats a
  `nil` codec as unknown, not disqualifying.
- **Direct and HLS/DASH formats are now ranked together by quality**, so a
  1080p HLS variant beats a metadata-less direct 360p; a real direct 1080p
  still beats an equivalent HLS one (delivery is the tiebreak at equal
  resolution). Was: direct always preferred, HLS only as a last resort.
- **The picker no longer double-lists HLS formats.** `MediaFormatMenu` was
  emitting a plain-download row for every progressive/video-only format
  regardless of `delivery`; a Twitch quality thus appeared both as a
  "streamed" row and as a bogus direct row whose URL was the `.m3u8`.
Supersedes: nothing. Extends
`docs/superpowers/specs/2026-09-02-phase-5-youtube-resolver-design.md` (the
resolver seam) and closes `todo.md` item **#2 (HLS/DASH wholesale fallback)**.

## 1. Purpose

Today SDM routes exactly five YouTube hostnames to `yt-dlp`. Everything the
resolver pipeline does downstream — `-J` parsing, format tables, direct-URL
segmented download, `ffmpeg -c copy` muxing, `403` URL refresh — is already
site-agnostic. This change:

1. Opens the resolver gate to a curated set of ~40–60 popular sites
   (Vimeo, Twitch, TikTok, SoundCloud, Reddit, Aparat, the major adult
   tubes, …) via a data-driven **site registry**.
2. Removes the YouTube-shaped assumptions still baked into playlist
   detection and playlist-entry URL reconstruction.
3. Adds the **HLS/DASH wholesale fallback** (`todo.md` #2): a video that
   offers only segmented manifests — no single `Range`-capable URL — is
   handed to `yt-dlp` *as a downloader*, with progress parsed from its
   stdout. Non-resumable, degraded path. This makes TikTok, Twitch VODs,
   Instagram, YouTube live-VODs, and similar first-class instead of
   `unsupported`.
4. Bundles `ffprobe` alongside `ffmpeg` (yt-dlp's HLS fixup postprocessors
   need it).

Non-goals unchanged from Phase 5: SponsorBlock / chapters / subtitles /
re-encoding, DRM, proactive URL refresh, per-component details breakout.
`yt-dlp` remains a **metadata extractor** for direct-URL sites; the
wholesale path is the *only* place it acts as a downloader, and only when
there is no direct URL to hand the engine.

## 2. Fixed decisions (settled during brainstorming 2026-09-03)

- **Curated, code-only allowlist.** No runtime "extractor list" matching, no
  user-editable host field. Adult sites are in the list with no toggle and
  no runtime partitioning (grouped only by source comment).
- **Wholesale downloads are non-resumable.** Pause / stop / crash kills the
  `yt-dlp` process and discards its partial output; restart is from zero.
  yt-dlp's own fragment resume is explicitly *not* used — the one-resume-
  mechanism invariant (progress is a `RangeSet`) stays intact.
- **`WholesaleDownloader` is an injected `SDMCore` protocol** (approach A1),
  mirroring `LinkResolver`. `SDMEngine` gains no subprocess code and no
  dependency on `SDMResolve`.
- **Wholesale progress is a synthesized contiguous `RangeSet`** (approach
  B1): `0..<downloadedBytes`, derived from yt-dlp's reported progress. One
  documented spot where a range comes from an external reporter rather than
  from bytes SDM wrote. No new snapshot / telemetry / UI code path.
- **Settings "YouTube" tab → "Media Sites".** Codec / container / audio
  allowlists laid out in three columns. Cookies-from-browser stays one
  global setting.

## 3. Module structure

No new target. Additions per existing target:

| Target | Adds |
|---|---|
| `SDMCore` | `MediaDelivery` enum; `MediaFormat.delivery`; `ComponentOrigin.wholesale(formatSelector:)`; `FormatChoice.wholesale` shape (via new optional field, see §6); `WholesaleDownloader` protocol + `WholesaleProgress` value type |
| `SDMResolve` | `SiteRegistry` + `SitePattern`; generalized `YtDlpResolver` (playlist detection, entry URLs, classifier); `YtDlpParser` delivery tagging; `FormatSelector` delivery-aware fallback; `ProcessRunner.runStreaming`; `SystemProcessRunner` streaming impl; `YtDlpWholesaleDownloader`; `WholesaleProgressParser` |
| `SDMEngine` | wholesale branch in `DownloadTask`; `DownloadEngine` takes an injected `WholesaleDownloader?`; sidecar guard (a `.wholesale` component always loads empty); scheduler treats a running wholesale item as non-preemptible (slot reserved) |
| `SDMGrabber` | `MediaHandoff` wholesale component; playlist-entry `sourceURL` plumbing; `rowState(for:)` copy generalized off "YouTube"; `recluster()` host fallback |
| `SDMApp` | wire `YtDlpWholesaleDownloader` into `EngineController`; Settings tab rename + 3-column layout; `MediaSitesSettingsStore` (renamed `YouTubeSettingsStore`, keys unchanged) |

Dependency rule from Phase 5 holds: neither `SDMEngine` nor `SDMGrabber`
depends on `SDMResolve`; both take injected protocols. The app is the only
place `YtDlpResolver` / `YtDlpWholesaleDownloader` are constructed.

## 4. The site registry (`SDMResolve`)

```swift
struct SitePattern: Sendable {
    enum HostMatch: Sendable, Equatable {
        case exact(String)     // "xvideos.com" — also matches "www.xvideos.com"
        case suffix(String)    // ".twitch.tv" — matches vod./www./clips. subdomains
    }
    var hosts: [HostMatch]
    /// Path prefixes that mean "this URL is a playlist / channel / set".
    /// Only feeds the pre-check in §5; the parser still trusts yt-dlp `_type`.
    var playlistPathHints: [String] = []
    /// Per-site escape hatch spliced into every yt-dlp invocation for this
    /// site (e.g. ["--impersonate", "chrome"] , ["--referer", "https://…"]).
    /// Empty for almost every entry.
    var extraArgs: [String] = []
}

enum SiteRegistry {
    static let patterns: [SitePattern]          // ~40–60 entries, grouped by comment
    static func match(_ url: URL) -> SitePattern?   // exact host wins over suffix
}
```

- `HostMatch.exact("x.com")` matches host `x.com` and `www.x.com`.
  `.suffix(".x.com")` matches any `*.x.com` including bare `x.com`.
- `SiteRegistry.match` walks `patterns` in order; first match wins. Exact
  patterns are ordered before suffix patterns for the same domain.
- Pure, no I/O. Fixture-table tested (`SiteRegistryTests`): a table of
  `(urlString, expectedMatch: Bool)` rows plus a few `playlistPathHints`
  assertions.

### 4.1 Initial pattern list

Grouped by source comment only. Not exhaustive; the list is expected to
grow by code change.

- **General video:** youtube.com / youtu.be / *.youtube.com, vimeo.com,
  dailymotion.com, *.dailymotion.com, rumble.com, odysee.com, *.bitchute.com,
  *.bilibili.com, tv.kick.com / kick.com, ok.ru, *.vk.com / vk.com,
  streamable.com, loom.com, ted.com
- **Live / streaming:** *.twitch.tv
- **Social:** tiktok.com / *.tiktok.com, instagram.com, *.facebook.com /
  fb.watch, x.com / twitter.com / *.x.com, reddit.com / *.reddit.com /
  redd.it, bsky.app, tumblr.com
- **Audio:** soundcloud.com / *.soundcloud.com, bandcamp.com /
  *.bandcamp.com, mixcloud.com
- **Regional:** aparat.com / *.aparat.com
- **Broadcaster (direct-URL, generally reliable):** *.arte.tv,
  *.bbc.co.uk (iplayer), *.pbs.org
- **Adult:** xvideos.com / *.xvideos.com, xnxx.com, *.xhamster.com,
  pornhub.com / *.pornhub.com, youporn.com, redtube.com, spankbang.com,
  *.eporner.com

`extraArgs` starts empty everywhere. Known likely exceptions to fill in
during implementation testing: Instagram / TikTok may need
`--impersonate`; some tubes need a `--referer`. These are data edits, never
control-flow.

## 5. Generalized resolution (`YtDlpResolver` / `YtDlpParser`)

### 5.1 `canHandle`

```swift
public func canHandle(_ url: URL) -> Bool {
    guard let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https"
    else { return false }
    return SiteRegistry.match(url) != nil
}
```

`handledHosts` is deleted.

### 5.2 Playlist vs. single

`isPlaylistURL(_:)` becomes a cheap heuristic that only chooses which
yt-dlp invocation to *try first*:

```swift
func looksLikePlaylist(_ url: URL) -> Bool {
    if URLComponents(url: url, resolvingAgainstBaseURL: false)?
        .queryItems?.contains(where: { $0.name == "list" }) == true { return true }
    let path = url.path.lowercased()
    if path.hasPrefix("/playlist") || path.hasPrefix("/sets/")
        || path.hasPrefix("/@") || path.hasPrefix("/channel/")
        || path.hasPrefix("/c/") || path.hasPrefix("/user/")
        || path.hasSuffix("/videos") { return true }
    return SiteRegistry.match(url)?.playlistPathHints
        .contains(where: { path.hasPrefix($0) }) ?? false
}
```

The **parser then keys on `dump._type`**, not on the guess:

- `resolveSingle` runs `-J --no-playlist`. If the returned dump has
  `_type == "playlist"` (some sites ignore `--no-playlist`), route the
  entries through the playlist path instead of throwing.
- `resolvePlaylist` runs `-J --flat-playlist`. If the dump is a single
  video (`entries` nil/empty, `formats` present), fall back to treating it
  as `.single`.

So a wrong guess self-corrects.

### 5.3 Playlist entries carry their own URL

`YtDlpEntry` already decodes `url` + `ie_key`. `YtDlpParser.flatEntries`
returns `[(sourceURL: URL, title: String)]` — `url` resolved against the
playlist URL when relative; entries with no usable URL are dropped.

`ResolvedMedia` (used as a not-yet-resolved playlist entry) gains
`var sourceURL: URL?` — set for playlist entries, `nil` for a directly
resolved single. `expandPlaylist` in `GrabberSession` uses
`entry.sourceURL` instead of building `https://www.youtube.com/watch?v=…`.

`resolvePlaylist` stops hardcoding `extractor: "youtube"` — entries carry
no extractor until they are individually resolved (their `-J` supplies it).

### 5.4 Error classifier

Existing YouTube-worded anti-bot strings stay (harmless; still fire for
YouTube). Add generic buckets, checked after the YouTube-specific ones:

- `"log in"` / `"login required"` / `"account"` / `"authentication"` →
  `.authRequired`
- `"geo"` / `"not available in your country"` / `"geo-restricted"` /
  `"in your location"` → `.unavailable`
- `"drm"` / `"protected by drm"` → new `ResolveError.drmProtected`
  (grabber maps it to `.unsupported`)

Everything else still falls through to `.ytDlpFailed(stderrTail:)` with the
real stderr tail.

## 6. HLS/DASH wholesale fallback

### 6.1 `MediaDelivery` (`SDMCore`)

```swift
public enum MediaDelivery: String, Codable, Sendable, Equatable {
    case direct   // single Range-capable http(s) URL — engine downloads it
    case hls      // m3u8 / m3u8_native
    case dash     // http_dash_segments
}
// MediaFormat gains:  public var delivery: MediaDelivery   (defaulted .direct in init for call-site compat)
```

`YtDlpParser.mediaFormat(from:)` stops rejecting non-direct protocols:

| yt-dlp `protocol` | `delivery` |
|---|---|
| `https`, `http`, `https_native`, `nil` | `.direct` |
| `m3u8`, `m3u8_native` | `.hls` |
| `http_dash_segments` | `.dash` |
| anything else (`rtmp`, `ism`, …), `mhtml` | rejected (unchanged) |

`.hls` / `.dash` formats still map their `height` / `tbr` / `vcodec` /
`acodec` / `container` from the `-J` entry. Their `url` is the manifest URL
(not used by the engine for a wholesale component — see §6.4).

### 6.2 `FormatChoice` gains a wholesale shape

Rather than a parallel enum, `FormatChoice` gets one new optional field:

```swift
public struct FormatChoice {
    public var video: MediaFormat?
    public var audio: MediaFormat?
    public var outputContainer: MediaContainer
    public var estimatedBytes: Int64?
    /// Non-nil ⇒ this choice is a wholesale (yt-dlp-as-downloader) job. The
    /// string is a yt-dlp `-f` selector (a format id, or an expression).
    /// `video`/`audio` are still populated for the picker label, but the
    /// engine ignores them for a wholesale component.
    public var wholesaleSelector: String?   // NEW, defaulted nil

    public var isWholesale: Bool { wholesaleSelector != nil }
    public var requiresMux: Bool { video != nil && audio != nil && wholesaleSelector == nil }
}
```

### 6.3 Format selection

`FormatSelector.pick` gains a delivery-aware final stage:

1. Run today's logic over `.direct` formats only. If it yields a choice →
   return it unchanged (progressive, or video+audio mux).
2. Otherwise, consider `.hls` / `.dash` formats. Pick the best variant that
   fits `prefs.maxHeight` (rank by height, then `tbr`). Build a wholesale
   choice:
   - `wholesaleSelector`: the chosen format's `id` if it is a
     self-contained variant (progressive HLS), else `"<id>+bestaudio/best"`
     when the best variant is video-only.
   - Simplest robust default when the site's HLS table is muddy:
     `"bv*[height<=\(maxHeight)]+ba/b[height<=\(maxHeight)]/bv*+ba/b"`.
     `pick` returns this expression form; the picked `MediaFormat` is kept
     only for the label / estimated size.
   - `outputContainer`: `.mp4` (yt-dlp `--merge-output-format mp4`), unless
     every candidate is webm → `.webm`.
   - `estimatedBytes`: chosen variant's `filesizeEffective` (usually nil
     for HLS — that's fine, the item shows an indeterminate size until
     yt-dlp reports totals).
3. Nothing usable at all → `nil` (grabber row → `.unselected` /
   `.unsupported` as today).

`MediaFormatMenu.options` additionally lists `.hls` / `.dash` variants as
rows tagged `streamed` (a `MediaFormatOption.isWholesale` flag drives a
small badge). Picking one produces a `FormatChoice` with
`wholesaleSelector` set to that variant's id (`+ba/b` appended if
video-only).

### 6.4 Handoff (`MediaHandoff`)

```swift
if let selector = choice.wholesaleSelector {
    let container = choice.outputContainer.fileExtension
    components = [FileComponent(
        url: row.sourceURL,                    // the PAGE url; yt-dlp re-resolves
        partFilename: "\(stem).\(container)",
        totalBytes: choice.estimatedBytes,     // often nil
        origin: .wholesale(formatSelector: selector),
        isResumable: false)]                   // ALWAYS non-resumable
    assembly = .none
    // single component → partFilename set to the final output name, as today
}
```

`ComponentOrigin` gains:

```swift
case wholesale(formatSelector: String)
```

Codable via the same `kind`-tagged encoder the enum already uses
(`{"kind":"wholesale","formatSelector":"…"}`), with the legacy-tolerant
decoder returning `.http` for anything unrecognized.

### 6.5 `WholesaleDownloader` seam (`SDMCore`)

```swift
public struct WholesaleProgress: Sendable, Equatable {
    public enum Phase: Sendable, Equatable { case downloading, postProcessing }
    public var downloadedBytes: Int64?
    public var totalBytes: Int64?
    public var fraction: Double?     // yt-dlp's own %, when byte counts unavailable
    public var phase: Phase
}

public protocol WholesaleDownloader: Sendable {
    /// Download `pageURL` wholesale to `destination` (a final file, already
    /// muxed by yt-dlp). Emits progress. Honors task cancellation by
    /// terminating the child; the *caller* deletes partial output.
    func download(
        pageURL: URL,
        formatSelector: String,
        destination: URL,
        onProgress: @Sendable @escaping (WholesaleProgress) -> Void
    ) async throws
}

public enum WholesaleError: Error, Equatable, Sendable {
    case binaryMissing
    case cancelled
    case failed(stderrTail: String)
    case authRequired
    case unavailable
}
```

### 6.6 `YtDlpWholesaleDownloader` (`SDMResolve`)

Concrete impl over a **streaming** process runner.

`ProcessRunner` gains:

```swift
func runStreaming(
    executable: URL, arguments: [String], timeout: Duration,
    onLine: @Sendable @escaping (String) -> Void      // one stdout OR stderr line
) async throws -> Int32                               // exit code
```

`SystemProcessRunner.runStreaming` reads both pipes line-by-line on a
background task, forwarding each line; same child-environment and
kill-on-cancel/timeout logic as `run`. A default protocol extension can
implement `run` in terms of `runStreaming` for fakes that only bother with
one; `SystemProcessRunner` keeps both concrete.

`YtDlpWholesaleDownloader.download`:

```
yt-dlp
  -f <formatSelector>
  --no-playlist
  --newline
  --no-part                       # write straight to destination; no .part
  --no-continue
  --progress-template "sdm:%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress._percent_str)s"
  --merge-output-format <container>
  --ffmpeg-location <managed bin dir>
  -o <destination>
  <pageURL>
+ SiteRegistry.match(pageURL)?.extraArgs
+ cookieSource().ytDlpArguments
+ app extraArguments()   (jsruntime=quickjs etc.)
```

- Lines starting `sdm:` → parsed by `WholesaleProgressParser` into
  `WholesaleProgress` (phase `.downloading`).
- Lines containing `[Merger]`, `[ExtractAudio]`, `Merging formats`,
  `[FixupM3u8]`, `[VideoConvertor]` → emit `phase == .postProcessing`.
- Non-zero exit → `WholesaleError` mapped from the accumulated stderr tail
  via the shared `Classifier` (reused from `YtDlpResolver`, lifted to a
  small `internal` free function).
- Task cancelled mid-run → `runStreaming` kills the child → throw
  `WholesaleError.cancelled`.
- `--no-part` + `-o destination`: on any failure the engine deletes
  `destination`; on success it is the finished file.

`WholesaleProgressParser` is pure and unit-tested against captured stdout
samples (`sdm:downloading|1048576|10485760||10.0%`, estimate-only lines,
`sdm:finished|…`).

### 6.7 Engine integration (`SDMEngine`)

`DownloadEngine.init` gains `wholesaleDownloader: (any WholesaleDownloader)? = nil`.
Threaded into `DownloadTask`.

In `DownloadTask`, a component whose `origin` is `.wholesale(selector)`
does **not** create worker pools. Instead:

1. Build `destination` = the item's part-file path for that component.
2. `try await wholesaleDownloader.download(pageURL: component.url,
   formatSelector: selector, destination: destination, onProgress: …)`.
3. **Progress (B1).** Each `WholesaleProgress`:
   - `totalBytes` present → set `component.totalBytes`.
   - else `totalBytes` nil but `fraction` present and `component.totalBytes`
     known → derive `downloaded = Int64(fraction * Double(total))`.
   - Replace `component.completed` with a single range
     `RangeSet(0..<downloaded)` (clamped to `totalBytes`). This is the one
     place a `RangeSet` is synthesized from an external reporter — a
     doc-comment says so.
   - `phase == .postProcessing` → set the task's assembling flag so
     `ItemSnapshot.isAssembling == true` (reuse the existing "Assembling…"
     treatment; no new state).
4. On success: `destination` is the final file. `Assembly.none` moves it
   into place exactly like a single progressive stream.
5. On `throws`:
   - `WholesaleError.cancelled` → treated as a pause/stop (no failure
     surfaced), partial `destination` deleted, `component.completed`
     cleared.
   - `.authRequired` / `.unavailable` → permanent failure with the mapped
     message.
   - `.failed(tail)` → transient or permanent per the same rules
     `RetryPolicy` already applies (`.failed` maps through
     `DownloadError`-style classification with the tail as reason).
   - Any exit → `destination` deleted, `component.completed` cleared.

**Non-resumable handling.** A `.wholesale` component's `isResumable` is
always `false`. The scheduler already reserves slots for running
non-resumable items before rank-based filling (load-bearing invariant) —
a wholesale item therefore holds its slot and is never preempted. On
pause / stop / reschedule-away / crash: `completed` cleared, `destination`
+ any yt-dlp leftovers (`*.part`, fragment dir) removed; next start
re-runs yt-dlp from zero.

**No `refresh`.** `LinkResolver.refresh` is never called for a
`.wholesale` component (yt-dlp re-resolves internally each run). The `403`
path in `DownloadTask` is gated on `origin == .resolved`.

**Sidecar.** No format bump — the new `ComponentOrigin` case rides the
existing v2 encoder. The v2 *loader* gains a guard: a `.wholesale`
component always loads with `completed = RangeSet()` and
`isResumable = false`, whatever the sidecar says.

**Telemetry / snapshot / segmented bar.** Unchanged — they read
`component.completed` / `component.totalBytes`, which the synthesized range
keeps consistent (one solid growing segment). ETA is indeterminate until
yt-dlp reports totals; the existing "unknown size" rendering covers it.

### 6.8 What stays `.unsupported`

Only: `ResolveError.drmProtected`; a resolve that yields zero formats of
any delivery; a `WholesaleError` that is clearly permanent on first
attempt. The `.unsupported` `MediaRowState` and its test stay — they just
fire far less often.

## 7. Grabber changes (`SDMGrabber`)

- `MediaHandoff.build` — wholesale branch per §6.4.
- `GrabberSession.expandPlaylist` — use `entry.sourceURL` (§5.3); the
  playlist `seenURLs` bookkeeping is unchanged.
- `GrabberSession.rowState(for:)` — the `.authRequired` message drops
  "YouTube", reads: *"The site blocked the request (sign-in / anti-bot).
  Set \"Cookies from browser\" in Settings → Media Sites, or update
  yt-dlp."* `.drmProtected` → `.unsupported`. The
  "the page needs to be reloaded" hint stays (YouTube-specific but
  harmless).
- `recluster()` — the media-row `host` fallback `?? "youtube.com"` becomes
  `?? "media"`; `directoryPath` fallback `/watch` stays (clustering only
  needs it stable, not accurate).
- `applyResolvedMedia` — unchanged; `FormatSelector.pick` now may return a
  wholesale choice, which flows through exactly like any other choice
  (`choice.requiresMux` is false for wholesale, so the `needsFfmpeg` gate
  is skipped — yt-dlp does its own muxing with the bundled ffmpeg).

## 8. App & Settings (`SDMApp`)

- `EngineController` constructs `YtDlpWholesaleDownloader(runner:
  SystemProcessRunner(), locator: sharedLocator, cookieSource:, extraArgs:)`
  and passes it to `DownloadEngine.init`.
- `YouTubeSettingsStore` → **`MediaSitesSettingsStore`** (type rename;
  every `UserDefaults` key string unchanged, so no migration and no reset).
  All `yt…`-prefixed `@State` in `SettingsView` renamed `media…` for
  clarity; no behavior change.
- `SettingsView`:
  - Tab label `"YouTube"` → `"Media Sites"`, icon
    `play.rectangle.on.rectangle` → `film.stack`.
  - The `Video codecs` / `Containers` / `Audio codecs` sections become one
    row of **three columns** (`HStack(alignment: .top)` of three
    `SettingsSection`s, or a `Grid`), cutting the tab's height.
  - `"Settings → YouTube"` mentions in any surfaced copy → `"Settings →
    Media Sites"`.
  - `Authentication` section label / picker unchanged (still global).
  - `Components` section unchanged, plus a third bundled row `ffprobe`
    (see §9).
- No allowlist UI.

## 9. Bundled ffprobe

- `scripts/vendor-binaries.sh` — fetch `ffprobe.zip` from the same
  martin-riedl.de build alongside `ffmpeg.zip` (`fetch_ffmpeg` generalized
  to a `fetch_tool` that takes the tool name); pack
  `ffprobe-{arm64,x86_64}.lzfse`. `vendor-manifest.json` gains no new key
  (ffprobe shares `ffmpegVersion`).
- `ManagedBinariesTypes` — `VendorAsset` list in the app gains an
  `ffprobe` entry; `BinariesManifest.version(for:)` maps `"ffprobe"` →
  `ffmpegVersion`.
- `ManagedBinaries.provisionBundledIfNeeded` already loops `vendorAssets()`
  generically — inflating `ffprobe` needs only the new asset entry and a
  `case "ffprobe": manifest.ffmpegVersion = asset.version` line (or fold
  into the ffmpeg case).
- LFS: the two new `*.lzfse` blobs are added under the existing
  `SDM/Resources/vendor/*.lzfse` LFS rule.
- yt-dlp is invoked with `--ffmpeg-location <managed bin dir>`, which is a
  *directory* — yt-dlp finds both `ffmpeg` and `ffprobe` there
  automatically. No code change to the invocation beyond what already
  passes the dir.

## 10. Testing

Swift Testing. No network, no real clock — unchanged constraints.

- **`SiteRegistryTests`** — fixture table: ~60 `(url, matches)` rows across
  every listed site + negative cases (bare hosts, unrelated domains,
  `ftp://`); `playlistPathHints` / `looksLikePlaylist` rows.
- **`YtDlpResolverTests`** — new recorded `-J` fixtures via
  `FakeProcessRunner`:
  - a non-YouTube **direct** site (Vimeo-shaped JSON) → `.single` with
    direct formats.
  - an **HLS-only** site (Twitch/TikTok-shaped: all formats
    `protocol: m3u8_native`) → `.single` whose `FormatSelector.pick` yields
    a wholesale choice.
  - a non-YouTube **playlist** (SoundCloud set-shaped) → `.playlist` with
    entry `sourceURL`s taken from `entries[].url`.
  - `_type == "playlist"` returned from a `--no-playlist` single resolve →
    routed to playlist path.
  - classifier: login / geo / drm stderr samples → correct `ResolveError`.
- **`YtDlpParserTests`** — `mediaFormat` delivery tagging table (protocol →
  delivery / rejected).
- **`FormatSelectorTests`** — direct-available → unchanged choice;
  direct-absent + HLS present → wholesale choice with expected selector /
  container; nothing usable → nil.
- **`WholesaleProgressParserTests`** — captured stdout line samples →
  `WholesaleProgress`; malformed lines ignored.
- **`MediaHandoffTests`** — wholesale choice → one non-resumable
  `.wholesale` component, `assembly == .none`, part filename = output name.
- **`SDMEngine` — `FakeWholesaleDownloader`** (in-process, mirrors
  `FakeOrigin`): scripted to emit a sequence of `WholesaleProgress` on
  demand, or to throw a chosen `WholesaleError`, or to hang until released.
  Tests, all tick-driven:
  - progress math: synthesized `RangeSet` tracks reported bytes; item
    fraction / snapshot correct.
  - `.postProcessing` → `isAssembling == true`.
  - success → file moved into place, item `.completed`.
  - pause mid-download → child cancelled, `destination` gone, `completed`
    empty; restart from zero.
  - crash simulation (task dropped) → same.
  - scheduler: a running wholesale item reserves its slot and is not
    preempted by a higher-rank resumable item.
  - `.authRequired` → permanent failure with message; `.failed` transient
    → retried per `RetryPolicy`.
  - sidecar with stale `.wholesale` data → loads empty.
- **Property tests** — the byte-identity / no-overlap properties exclude
  `.wholesale` components (bytes are not SDM-written). A one-line filter +
  a comment in the property test explains why.
- **`SystemProcessRunner.runStreaming`** — one integration test against a
  tiny shell script that prints known lines then exits N (guarded to skip
  where `/bin/sh` is unavailable — it always is on macOS CI). Kept minimal;
  the parser has the real coverage.

Target: no regression in the existing suite (~401), ~40–55 new tests.

## 11. Rollout

Single branch `feat/multi-site-resolver`, merged to `main` when green.
`todo.md` #2 checked off. This spec's status banner flips to IMPLEMENTED.
`CLAUDE.md` "Current state" + the Phase 5 spec note updated to mention
multi-site + wholesale.

## 12. Risks

- **HLS variety.** Different sites' HLS tables differ wildly. Mitigation:
  the `bv*+ba/b` selector expression form leans on yt-dlp's own selection
  rather than SDM picking a specific variant; the picker still lets the
  user override.
- **Wholesale progress gaps.** Some extractors don't report `total_bytes`.
  Item shows indeterminate size — acceptable, matches existing "unknown
  size" HTTP downloads.
- **`--no-part` on failure.** A killed yt-dlp can leave a partial file at
  `destination`. The engine unconditionally deletes `destination` +
  siblings on any non-success exit from a wholesale component.
- **ffprobe build source.** Same provider/build as ffmpeg, already trusted
  and notarized; SHA-256 checked in the vendor script like ffmpeg.
- **Adult-site extractors** are community-maintained and break more often
  than YouTube. Acceptable — failures surface as normal resolve errors
  with the real yt-dlp stderr, and nightly yt-dlp self-update already
  covers the fixes.
