# SDM Phase 5 — YouTube & the Resolver Seam: Design

Date: 2026-09-02
Status: Approved design, pre-implementation
Supersedes: §8 of `2026-08-03-sdm-design.md` (kept as the high-level intent; this
document is the authority on behavior and structure for Phase 5).

## 1. Purpose

Add first-class YouTube (and, via the same seam, other extractor-backed sites)
support. yt-dlp is used **only as a metadata extractor** — `yt-dlp -J` yields the
format table and direct `googlevideo` URLs, and SDM downloads those with the
existing segmented engine: resumable, priority-scheduled, pausable. High-quality
YouTube formats are video-only, so a download can now consist of more than one
file, assembled into a single output with `ffmpeg -c copy`.

## 2. Scope

### In scope

- `LinkResolver` protocol seam (`SDMCore`) and a concrete `YtDlpResolver`
  (`SDMResolve`).
- Multi-component downloads: one `DownloadItem` owns 1–N `FileComponent`s, each
  a separately-downloaded file, presented as one row and assembled on completion.
- Full muxing: video-only + audio-only streams downloaded in parallel, then
  `ffmpeg -c copy` into one file.
- Quality preferences (max resolution + codec/container/audio allowlists) as a
  pure selection function, with a per-row format-picker override.
- Playlists and channels, expanded into one package, capped and showing the
  newest N.
- Cookies-from-browser for age-restricted / private / members-only videos.
- Silent URL refresh on `403` mid-download, via `LinkResolver.refresh`.
- yt-dlp / ffmpeg discovery with Settings path overrides and "requires yt-dlp" /
  "requires ffmpeg" row states.

### Deferred (explicitly out of scope for this phase)

- **HLS/DASH wholesale fallback.** A video offering only segmented manifests with
  no single direct URL is marked `unsupported` in the grabber and not handed off.
  A later phase adds the yt-dlp-as-downloader path with text progress parsing.
- **Proactive URL refresh.** URLs are refreshed reactively on `403` only; a
  long-paused item eats one `403` on resume and refreshes then.
- **yt-dlp version surfacing / self-update.** The user maintains their own
  yt-dlp.
- **SponsorBlock, chapters, subtitles, thumbnail embedding, re-encoding.**
- **Bundling yt-dlp / ffmpeg** (still deferred per parent §14).

## 3. Module structure

One new target added to the existing four.

| Target | Adds |
|---|---|
| `SDMCore` | `LinkResolver` protocol; value types `ResolvedTarget`, `ResolvedMedia`, `MediaFormat`, `QualityPreferences`, `FormatChoice`; `FileComponent` and the reworked `DownloadItem`; `DownloadPackage.note` |
| `SDMEngine` | per-component worker pools, the `assembling` state and `Muxer`, `403` → `refresh` in the error path, multi-component sidecar (format v2), one-shot v1→v2 migration |
| `SDMGrabber` | resolver routing in `GrabberSession`, the `GrabberRow` two-case model, `MediaRow`, playlist expansion |
| `SDMResolve` *(new; depends on `SDMCore`)* | `ProcessRunner`, `BinaryLocator`, `YtDlpResolver`, `FormatSelector` |
| `SDMApp` | format-picker menu, media/playlist row treatments, new Settings, binary-missing surfaces |

`SDMResolve` depends only on `SDMCore`, mirroring `SDMEngine` and `SDMGrabber`.
Neither `SDMEngine` nor `SDMGrabber` depends on `SDMResolve`; both take an
injected `any LinkResolver`. The app is the only place the concrete
`YtDlpResolver` is constructed and wired into all three consumers.

Tuning-prone logic stays pure: `FormatSelector` is a pure function over value
types with a fixture table, exactly like `VerdictRules` and `PackageClustering`.

## 4. The resolver seam

### 4.1 Protocol (`SDMCore`)

```swift
public protocol LinkResolver: Sendable {
    /// Cheap, synchronous, no I/O — a host/path check only.
    func canHandle(_ url: URL) -> Bool

    /// Grab-time resolution. May shell out; may take seconds.
    func resolve(_ url: URL) async throws -> ResolvedTarget

    /// Mid-download URL refresh for one expired component.
    func refresh(extractor: String, videoID: String,
                 formatID: String) async throws -> RefreshedFormat
}
```

- Generic HTTP has **no** resolver. `GrabberSession` and `DownloadEngine` treat a
  `nil` resolver, or `canHandle == false`, as "use the existing HTTP path."
- `resolve` and `refresh` are `async` and allowed to block on a subprocess.
  `refresh` is called from `DownloadTask`'s error path where that component's
  workers are already stopped, so the latency is acceptable (the sibling
  component keeps downloading).

### 4.2 Value types (`SDMCore`)

```swift
public enum ResolvedTarget: Sendable {
    case single(ResolvedMedia)
    case playlist(title: String, entries: [ResolvedMedia], totalAvailable: Int)
}

public struct ResolvedMedia: Sendable, Equatable, Codable {
    public var extractor: String        // "youtube"
    public var videoID: String
    public var title: String
    public var durationSeconds: Double?
    public var formats: [MediaFormat]
}

public struct MediaFormat: Sendable, Equatable, Codable, Identifiable {
    public enum Kind: Sendable, Codable { case progressive, videoOnly, audioOnly }
    public var id: String               // yt-dlp format_id, e.g. "137"
    public var kind: Kind
    public var height: Int?             // nil for audioOnly
    public var width: Int?
    public var vcodec: VideoCodec?      // .av1 / .vp9 / .h264 / .other(String)
    public var acodec: AudioCodec?      // .opus / .aac / .other(String)
    public var container: Container     // .mp4 / .webm / .m4a / .other(String)
    public var filesize: Int64?         // exact, else nil → filesizeApprox used
    public var filesizeApprox: Int64?
    public var tbr: Double?             // total bitrate, kbps — tie-breaker/estimate
    public var url: URL
}

public struct RefreshedFormat: Sendable, Equatable {
    public var url: URL
    public var filesize: Int64?
    public var formatID: String         // must equal the requested id
}

public struct QualityPreferences: Sendable, Equatable, Codable {
    public var maxHeight: Int                 // e.g. 1080
    public var videoCodecs: Set<VideoCodec>   // which are acceptable
    public var containers: Set<Container>     // which are acceptable
    public var audioCodecs: Set<AudioCodec>   // which are acceptable
    // Priority orders are fixed in code, NOT user-editable:
    //   video:     av1 > vp9 > h264
    //   container: mp4 > webm > (others, alphabetical)
    //   audio:     opus > aac > (others, alphabetical)
}

public struct FormatChoice: Sendable, Equatable, Codable {
    public var video: MediaFormat?      // nil only for an audio-only download
    public var audio: MediaFormat?      // nil when `video` is progressive
    public var outputContainer: Container
    public var estimatedBytes: Int64
}
```

`filesizeEffective` on `MediaFormat` is `filesize ?? filesizeApprox`; when both
are nil it is estimated from `tbr × duration` and the row marks the size as
approximate.

### 4.3 `FormatSelector` (pure, `SDMResolve`)

`FormatSelector.pick(_ media: ResolvedMedia, _ prefs: QualityPreferences) -> FormatChoice?`

1. **Eligible video formats** = those with `height <= prefs.maxHeight`,
   `vcodec ∈ prefs.videoCodecs`, `container ∈ prefs.containers`, of kind
   `progressive` or `videoOnly`.
2. **Rank** eligible video formats by, in order: highest `height`, then fixed
   codec priority, then fixed container priority, then higher `tbr`.
3. Take the top-ranked video format `V`.
   - If `V.kind == .progressive` → `FormatChoice(video: V, audio: nil,
     outputContainer: V.container)`.
   - Else pick the best **eligible audio** format `A` (`acodec ∈
     prefs.audioCodecs`, ranked by fixed audio-codec priority then higher `tbr`).
     If no eligible audio exists → **return nil** (row goes `unselected`).
     `outputContainer = V.container` (av1/vp9 → webm, h264 → mp4).
4. If step 1 yields nothing → **return nil** (`unselected`).
5. An audio-only site/URL (no video formats at all) → best eligible audio, kind
   `audioOnly`, `outputContainer = A.container` (`.m4a` / `.webm`).

`estimatedBytes` = sum of the chosen formats' `filesizeEffective`.

The picker UI's ordering reuses the same rank comparator so the auto-pick is
always the first *matching* entry in the list.

### 4.4 `YtDlpResolver` (`SDMResolve`)

- **`canHandle`** — host in a static set (`youtube.com`, `youtu.be`,
  `m.youtube.com`, `music.youtube.com`, plus `www.` variants). Deliberately
  narrow for this phase; widening it later is a one-line change.
- **`resolve`**
  - Single video → `yt-dlp -J --no-warnings <url>` (+ cookies args, see §8).
    Parse the `formats` array into `[MediaFormat]`; drop storyboard/`mhtml` and
    formats with no usable `url`. `.single(ResolvedMedia)`.
  - Playlist/channel (URL matches a playlist/channel/tab pattern, or the `-J`
    top level is a playlist) → **two steps**:
    1. `yt-dlp --flat-playlist -J <url>` — cheap, returns the full entry index
       (ids + titles) even for thousands of entries.
    2. Keep the newest `maxPlaylistVideos` (§7): channel video listings are
       newest-first natively; playlist order has new additions at the tail, so
       take the tail. *(Exact `yt-dlp` flag/ordering behavior — `-I`,
       `--playlist-items`, reversal — verified against current yt-dlp docs at
       implementation time, not assumed.)*
    3. Return `.playlist(title:, entries:, totalAvailable:)` with `entries`
       holding **id + title only, `formats: []`**. Each entry's format table is
       extracted lazily by `GrabberSession` (§6.2).
  - A single-video `resolve` that finds no format with a direct URL (HLS/DASH
    only) throws `ResolveError.unsupported` → the row is marked `unsupported`.
- **`refresh`** — `yt-dlp -J --no-warnings <canonical watch URL for videoID>`,
  locate the format whose `format_id == formatID`, return its fresh `url` +
  `filesize`. Throw `ResolveError.formatGone` if absent.
- **Errors** — `ResolveError`: `.binaryMissing`, `.unsupported`,
  `.formatGone`, `.authRequired` (yt-dlp signals login/age wall),
  `.private`, `.unavailable`, `.timeout`, `.ytDlpFailed(stderrTail:)`.

### 4.5 `ProcessRunner` & `BinaryLocator` (`SDMResolve`)

- **`ProcessRunner`** — `run(_ executable: URL, _ args: [String], timeout:
  Duration) async throws -> (stdout: Data, stderr: Data, exitCode: Int32)`.
  Plain `Foundation.Process` (unsandboxed). Kills the process on timeout or task
  cancellation. Never inherits a shell; `PATH` is irrelevant because callers pass
  absolute paths from `BinaryLocator`.
- **`BinaryLocator`** — for each of `yt-dlp`, `ffmpeg`:
  1. Settings override path, if set and executable.
  2. First hit scanning `/opt/homebrew/bin`, `/usr/local/bin`, `/usr/bin`,
     `~/.local/bin`.
  3. Otherwise `.notFound`.
  Result cached; re-scanned when the app becomes active and when the override
  setting changes, so installing yt-dlp while SDM is open is picked up without a
  relaunch.

## 5. Core model: multi-component items

### 5.1 `FileComponent`

```swift
public struct FileComponent: Identifiable, Equatable, Codable, Sendable {
    public let id: UUID
    public var url: URL
    public var partFilename: String       // "<title> [<id>].f137.mp4"
    public var totalBytes: Int64?
    public var completed: RangeSet
    public var validator: String?
    public var origin: ComponentOrigin
    public var isResumable: Bool?          // moved here from DownloadItem
    public var lastError: ComponentError?  // surfaced in the details panel
}

public enum ComponentOrigin: Equatable, Codable, Sendable {
    case http
    case resolved(extractor: String, videoID: String, formatID: String)
}
```

### 5.2 Reworked `DownloadItem`

`url`, `totalBytes`, `completed`, `validator`, `isResumable` **move into
`FileComponent`**. `DownloadItem` becomes:

```swift
public struct DownloadItem: Identifiable, Equatable, Codable, Sendable {
    public let id: UUID
    public var outputFilename: String        // "<title> [<id>].webm" — the final file
    public var components: [FileComponent]    // invariant: count >= 1
    public var assembly: Assembly             // .none | .mux
    public var state: ItemState
    public var isEnabled: Bool
    public var priority: Priority?
    public var position: Int
}

public enum Assembly: Equatable, Codable, Sendable {
    case none                 // 1 component, already the final container → rename
    case mux                  // ffmpeg -c copy the components together
}
```

- **Generic HTTP download** = one `FileComponent` (`origin: .http`), `assembly:
  .none`. Every existing behavior is a one-component special case.
- **Progressive YouTube** = one component (`origin: .resolved`), `assembly:
  .none`.
- **Muxed YouTube** = two components (video + audio), `assembly: .mux`.

### 5.3 Item-space concatenation (derived, never stored)

Component `k` occupies item-space byte range
`[base_k, base_k + size_k)` where `base_k = Σ_{i<k} size_i`. Sizes are known up
front (format table for resolved, probe for http), so bases are stable.

`ItemSnapshot` gains:
- `totalBytes: Int64?` — `Σ size_k` (nil if any component size is unknown).
- `completed: RangeSet` — the union of each component's `completed` shifted by
  its `base_k`. The progress bar, percentage, and sparkline all read this one
  synthetic set. §5.4's speed aggregation already sums worker deltas per item —
  unchanged.
- `components: [ComponentSnapshot]` — per-component url, part path, size,
  fraction, active worker count, `lastError` — for the details panel.
- `activeWorkers` — summed across components (drives the `12/16` badge).

Package-level aggregates (`PackageSnapshot`) sum the item-space totals, so a
package still reads `8/8` and a correct byte total.

### 5.4 `DownloadPackage.note`

`public var note: String?` — an optional freeform line shown under the package
name. Set by the grabber for a truncated playlist (`"50 of 320 videos"`), carried
into the durable snapshot so it persists. Nil for ordinary packages.

## 6. Grabber changes (`SDMGrabber`)

### 6.1 `GrabberRow` two-case model

`GrabberSnapshot.links: [ProbedLink]` becomes `rows: [GrabberRow]`:

```swift
public enum GrabberRow: Identifiable, Sendable, Equatable {
    case http(ProbedLink)
    case media(MediaRow)
}
```

`GrabberSession.ingest` routes each extracted URL:
- `resolver?.canHandle(url) == true` → resolve path (§6.2).
- else → existing HTTP probe path, producing `.http(ProbedLink)`.

Clustering (`PackageClustering`) runs over a uniform
`ClusterableLink` derived from either case (media rows use `outputFilename`,
host `youtube.com`). Manual override / move / merge / split are unchanged.

### 6.2 `MediaRow` and resolution lifecycle

```swift
public struct MediaRow: Identifiable, Sendable, Equatable {
    public let id: UUID
    public let sourceURL: URL
    public var state: MediaRowState
    public var media: ResolvedMedia?          // nil until resolved
    public var choice: FormatChoice?          // auto-picked or user-set
    public var playlistGroup: UUID?           // set for playlist members
}

public enum MediaRowState: Sendable, Equatable {
    case resolving
    case resolved            // choice != nil, ready for handoff
    case unselected          // resolved, but auto-pick returned nil — user must choose
    case unsupported         // HLS/DASH only, no direct URL
    case needsYtDlp          // yt-dlp not found
    case needsFfmpeg         // choice requires a mux but ffmpeg not found
    case failed(String)      // resolve error, with reason; recheckable
}
```

- **Single URL** → one `.resolving` row; `resolve` → `FormatSelector.pick` →
  `.resolved` or `.unselected`.
- **Playlist URL** → `resolve` returns the flat entry list; the session creates
  one `.resolving` `MediaRow` per entry, all sharing a `playlistGroup`, in one
  `PackageCandidate` named from the playlist title, with `note` set when
  `entries.count < totalAvailable`. Each entry's full format table is then
  fetched by the session under the **existing probe concurrency budget**
  (`GrabberSession.Budget`), updating rows to `.resolved` / `.unselected` as they
  land.
- Changing quality settings after grab **does not** re-pick any row. It only
  changes the picker's sort/marking (§9.2). `unselected` rows stay `unselected`.
- `recheckFailed` also re-resolves `.failed` media rows.

### 6.3 Handoff

`urls(inPackage:)` / the handoff path becomes "build `[DownloadItem]`":
- `.http` rows → one-component items (today's behavior).
- `.media` rows in `.resolved` → one `DownloadItem`; `choice.video`/`choice.audio`
  become 1–2 `FileComponent`s with `origin: .resolved`; `assembly` is `.mux` iff
  both are present; `outputFilename = "<sanitized title> [<videoID>].<container>"`.
- `.media` rows **not** in `.resolved` (`unselected`, `unsupported`,
  `needsYtDlp`, `needsFfmpeg`, `failed`, `resolving`) are **held back** — skipped
  by handoff, left in the grabber. HTTP siblings in the same package still go
  through. A toast / count reports "N items held back."

Filename sanitization: strip `/`, control chars, leading dots; collapse
whitespace; trim to a filesystem-safe length preserving the `[<videoID>]` suffix
and extension.

## 7. Engine changes (`SDMEngine`)

### 7.1 Per-component worker pools

Each **incomplete** component of a running item runs the existing claim/steal
loop over its own `RangeSet` with its own pool of size `segmentsPerItem`. To the
scheduler the item is still exactly one slot. The two pools of a mux download
draw from the same `globalMaxConnections` and `maxConnectionsPerHost` budgets as
any other download — both `googlevideo` streams share the per-host cap, which the
existing `ConnectionAllocator` already handles (largest pool yields first). No
scheduler ranking change.

`isResumable` for the **item** (used by §6.3 non-preemptible slot reservation) is
`false` if **any** component is `isResumable == false`, `nil` if any is unprobed
and none is `false`, else `true`.

### 7.2 Completion & assembly

When **every** component's `RangeSet` covers `[0, size_k)`:
- `assembly == .none` → `fsync`, delete sidecar, atomic rename off `.incomplete`
  (today's path).
- `assembly == .mux` → item enters new `ItemState.assembling`. `Muxer` runs
  `ffmpeg -y -i <video.part> -i <audio.part> -c copy -movflags +faststart
  <output>.<container>` (flags per container, verified against current ffmpeg
  docs). On success: delete the component part-files and the sidecar, item →
  `completed`. On failure: item → `failed(reason:)` with the ffmpeg stderr tail;
  **all component files kept**; a manual retry re-runs only the mux (components
  are already complete).

`assembling` is not schedulable and holds its slot until it resolves (it is
brief and CPU/IO-bound, not a network slot — acceptable for this phase).

### 7.3 URL refresh on 403

In `DownloadTask`'s fetch error path, a `403` (or a `410`) on a component with
`origin == .resolved(extractor, videoID, formatID)`:
1. Stop that component's workers (the sibling component keeps running).
2. `await resolver.refresh(extractor:videoID:formatID:)`.
3. Verify `refreshed.formatID == formatID` and, when both sizes are known,
   `refreshed.filesize == component.totalBytes`. Mismatch or `formatGone` →
   component `lastError`, item → `failed(reason:)`.
4. On success: set `component.url = refreshed.url`, restart the component's
   workers against the **existing** `RangeSet`. No validator re-check beyond the
   size compare (a `googlevideo` refresh keeps the same bytes).
5. Refresh attempts are **not** a separate budget — they consume the existing
   `RetryPolicy` attempt cap for that component, and a `403` with no resolver
   (an `http` component) stays a normal `serverError(403)` failure.

### 7.4 Sidecar — format v2

`ResumeSidecar` becomes multi-component and bumps to `formatVersion = 2`:

```swift
public struct ResumeSidecar: Codable, Equatable, Sendable {
    public static let currentFormatVersion = 2
    public var formatVersion: Int
    public var outputFilename: String
    public var assembly: Assembly
    public var components: [ComponentResumeState]   // url, totalBytes, validator,
                                                    // completed, origin
}
```

- One `.sdmpart` per **item**, named from the output file
  (`<output>.<container>.sdmpart`), sitting alongside the component `.incomplete`
  files in the package folder.
- Checkpointed on the same cadence (§4.3): per-worker byte interval or 5 s,
  whichever first, plus on pause/quit — now folding every component's workers.
- Missing / corrupt / unreadable → restart **all** components from zero.
- **v1 → v2 migration:** a one-shot shim on load. A `formatVersion == 1` sidecar
  (single `sourceURL` / `totalBytes` / `validator` / `completed`) is wrapped into
  a one-element `components` array with `origin: .http`, `assembly: .none`,
  `outputFilename` from the existing `.incomplete` name. Same shim path for the
  durable `state.json`: an old single-URL `DownloadItem` decodes into a
  one-component item.

### 7.5 `DownloadEngine.init`

Gains `resolver: (any LinkResolver)? = nil`. Only used by §7.3. Tests inject a
fake resolver with recorded refresh responses; the existing engine tests that
don't touch resolved components pass `nil` and are otherwise unchanged (modulo
the `DownloadItem` shape).

## 8. Cookies / authenticated videos

- Settings: **Cookies from browser** — `None` (default) / `Safari` / `Chrome` /
  `Firefox` / `Edge` / `Brave`.
- When set, every `yt-dlp` invocation (`resolve` and `refresh`) gets
  `--cookies-from-browser <name>`.
- Known risks, verified at implementation time, surfaced to the user rather than
  silently swallowed:
  - Safari cookie access needs Full Disk Access for SDM; a failure surfaces as
    `.authRequired` with a hint.
  - Chromium App-Bound cookie encryption on current macOS can make
    `--cookies-from-browser chrome` fail; the failure is reported, not hidden.
- A video that still fails an auth wall → row `state = .failed("sign-in
  required")`, recheckable after the user adjusts the setting.

## 9. UI (`SDMApp`)

### 9.1 Media & playlist rows

- A `.media` row shows: title, state, chosen format summary
  (`1080p · av1 · webm · ~82 MB`, "~" when size is approximate), a format-picker
  menu, and combined size.
- `resolving` → spinner + title (from flat-playlist, available immediately).
- `unselected` → a distinct badge ("choose a format"), row cannot be added.
- `unsupported` / `needsYtDlp` / `needsFfmpeg` → badge with the copyable hint
  (`brew install yt-dlp` / `brew install ffmpeg`).
- A truncated playlist package shows `DownloadPackage.note` under its name.

### 9.2 Format-picker menu

Flat list of every selectable option (progressive formats + every
video+best-eligible-audio combination). Ordering: **matching options first**
(pass all of max-res / codec / container / audio filters), then non-matching
options, **each group** sorted by height → codec priority → container priority →
tbr. Non-matching rows are dimmed with a ⚠ marker. Selecting one sets
`MediaRow.choice`; the displayed size updates **instantly** from the already
-fetched table — no network round trip. Only the selected `formatID`(s) persist
(in `state.json` via the resulting `FileComponent`s after handoff); the full
format table is never persisted.

### 9.3 Download list & details panel

- The item row is unchanged in layout — one segmented bar, one %, one sparkline,
  one speed — all now reading the §5.3 concatenated snapshot.
- The **details panel** for a multi-component item lists each component: role
  (video / audio), format id, url, part-file path, per-component progress, and
  `lastError`. `assembling` shows an ffmpeg-running indicator.
- `failed` mux → details show the ffmpeg stderr tail and a **Retry mux** action.

### 9.4 Settings (additions to §12)

| Setting | Default | Range / options |
|---|---|---|
| Max resolution | 1080 | 144 … 4320 |
| Video codecs accepted | av1, vp9, h264 (all) | toggle each; priority fixed av1>vp9>h264 |
| Containers accepted | mp4, webm (all) | toggle each; priority fixed mp4>webm>… |
| Audio codecs accepted | opus, aac (all) | toggle each; priority fixed opus>aac>… |
| Max playlist videos | 50 | 10 … 200 |
| Cookies from browser | None | None / Safari / Chrome / Firefox / Edge / Brave |
| yt-dlp path | *(auto)* | file picker override |
| ffmpeg path | *(auto)* | file picker override |

If every video codec (or every container, or every audio codec) is toggled off,
the UI blocks it — at least one must stay selected — so `FormatSelector` can
never be handed an all-empty allowlist.

## 10. Testing

Extends parent §11; same rules — **no network, no real clock, no yt-dlp binary
in CI.**

### 10.1 `FormatSelector` — fixture table (`SDMResolveTests`)

`(recorded ResolvedMedia, QualityPreferences) -> expected FormatChoice?`.
Cases pinned: progressive-only video (no mux); video-only at max res with
eligible / no-eligible audio (→ choice / nil); codec allowlist excluding the top
format; container allowlist forcing webm→mp4 fallback; max-res cap dropping 4K;
audio-only URL; empty eligible set → nil; approximate-size estimation from tbr.

### 10.2 `YtDlpResolver` — recorded JSON (`SDMResolveTests`)

`ProcessRunner` is injected as a protocol; tests feed recorded `-J` /
`--flat-playlist -J` stdout and stderr fixtures for: a normal video, a
video-only-formats video, an HLS-only video (→ `.unsupported`), an
age-restricted video without cookies (→ `.authRequired`), a private video, a
200-entry playlist (→ capped to N, newest kept), a channel URL, and a malformed
JSON blob (→ `.ytDlpFailed`). `refresh` fixtures: fresh URL same itag/size (ok),
format gone (throws), size changed (caller fails the component).

### 10.3 Multi-component engine (`SDMEngineTests`)

Against `FakeOrigin` serving two distinct payloads on two paths:
- **Byte identity, muxed:** two components download in parallel under randomized
  churn (segment-count changes, pauses, kills, drops per component); each
  component's `.incomplete` must hash-match its source before the mux step.
- **Concatenated snapshot:** the item-space `completed` set exactly equals the
  union of shifted component sets at every tick; `totalBytes` = sum;
  `fractionCompleted` monotonic.
- **403 refresh:** `FakeOrigin` returns `403` on one component's path after N
  bytes; an injected fake resolver returns a fresh path; the component resumes
  against the existing set and the final file still hash-matches. Format-gone and
  size-changed both drive the item to `failed`.
- **Mux step:** injected fake `Muxer` (real ffmpeg not run in unit tests) —
  success deletes parts + sidecar and completes; failure keeps files and enters
  `failed`; **Retry mux** from `failed` re-runs only the mux. One real-ffmpeg
  smoke test, skipped when `BinaryLocator` reports `.notFound`.
- **Sidecar v2 + migration:** partway, tear down, reconstruct from the v2
  sidecar, finish, hash-compare — for both a 1-component and a 2-component item.
  A synthetic v1 sidecar + v1 `state.json` load and migrate to a one-component
  item that then completes cleanly.

### 10.4 Grabber (`SDMGrabberTests`)

Injected fake resolver: a YouTube URL produces a `.media` row that reaches
`.resolved`; a playlist URL produces N clustered `.media` rows sharing a group
with `note` set when truncated; `canHandle == false` still yields `.http`;
handoff builds a 2-component item for a mux choice and holds back `unselected` /
`unsupported` rows while letting HTTP siblings through.

### 10.5 UI

Launch smoke test only (parent §11.7). Plus a fixture test that the picker's
match/non-match ordering equals `FormatSelector`'s rank comparator, so the
auto-pick is always the first matching entry.

## 11. Migration & compatibility

- `state.json` and `.sdmpart` both gain a one-shot v1→v2 shim on load; no
  separate migration pass, no user action. A downgrade (v2 store opened by a v1
  build) is not supported — acceptable for a single-user app.
- The parent spec's §8 remains as intent; §12 Settings table is extended by §9.4
  here; §4.3 sidecar contents are superseded by §7.4.

## 12. Open risks carried into implementation

- Exact `yt-dlp` flags for "newest N of a playlist/channel" and for the flat vs.
  full extraction split — verify against current yt-dlp, don't assume.
- `ffmpeg -c copy` container/codec compatibility edge cases (e.g. opus-in-mp4)
  and the right `-movflags` per container.
- Chromium App-Bound cookie encryption behavior on the user's macOS version.
- `assembling` holding a scheduler slot during a large-file mux — revisit if mux
  times become long enough to starve the queue.
