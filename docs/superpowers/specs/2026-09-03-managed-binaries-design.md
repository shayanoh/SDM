---
status: IMPLEMENTED (2026-09-03)
supersedes: todo.md items 4 and 5; parent spec §2 "yt-dlp bundling + self-update"
---

# Managed Binaries — yt-dlp / ffmpeg / QuickJS provisioning & self-update

## Problem

SDM currently requires the user to `brew install yt-dlp ffmpeg`. Three gaps:

1. **yt-dlp goes stale.** YouTube breaks extraction every few weeks; a user with
   an old `yt-dlp` silently gets anti-bot walls or missing formats.
2. **ffmpeg is a hard external dependency** for muxed YouTube downloads.
3. **A JavaScript runtime is now mandatory** for full YouTube support (late-2025
   change: `nsig`/signature challenges need a real JS engine). `yt-dlp_macos`
   bundles none.

## Decision summary (settled in brainstorming 2026-09-03)

| Binary | Source | Provisioning |
|---|---|---|
| `yt-dlp` | GitHub releases — `yt-dlp/yt-dlp` (stable) or `yt-dlp/yt-dlp-nightly-builds` (nightly). The `yt-dlp_macos` asset is **universal2** (one file, both arches). | **Downloaded at runtime** into a managed dir. We do our own version check + download + atomic replace. **Not** `yt-dlp -U`. SHA-256 verified against the release's `SHA2-256SUMS`. |
| `ffmpeg` | martin-riedl.de **LGPL** static builds, native `arm64` + `x86_64`, pinned by version. | **Bundled** in the `.app` as per-arch LZFSE blobs (`Compression` framework, no third-party dep). Inflated to the managed dir on first run and whenever the app ships a newer pinned version. Never auto-updated at runtime. |
| `qjs` (QuickJS-ng) | Built locally by `scripts/vendor-binaries.sh` from a pinned tag, `lipo`'d universal. ~1 MB. | **Bundled** and inflated exactly like ffmpeg. |

- **App is not notarized / not sent to Apple.** Personal + friends; source is on
  GitHub. This removes the Developer-ID re-signing and notary steps: the
  maintainers' signatures on the bundled ffmpeg/qjs survive the copy (copying a
  signed Mach-O does not invalidate it), and the downloaded `yt-dlp_macos` is
  already ad-hoc signed by PyInstaller. We still `xattr -c` + `codesign -s -
  --force` the downloaded yt-dlp defensively.
- **Binary resolution policy: managed-only** (brainstorming Q1 = A). The
  `BinaryLocator` Homebrew search path is dropped from the app wiring; only the
  managed dir is consulted. The manual override remains as a test seam.
- **Large vendored binaries are tracked with Git LFS.**

## Layout

```
~/Library/Application Support/SDM/bin/
    yt-dlp          # downloaded, universal2, self-updated
    ffmpeg          # inflated from bundled asset
    qjs             # inflated from bundled asset
    manifest.json   # installed versions + channel + last-check + last-error
```

`manifest.json` (Codable):

```
{
  "ytDlpVersion": "2026.08.19",      // nil until first successful download
  "ytDlpChannel": "stable",
  "lastCheckTick": 123456,           // monotonic tick counter, not wall clock
  "ffmpegVersion": "7.1",            // last inflated
  "qjsVersion": "0.10.0",
  "lastError": null                  // human-readable string of the last failed check/download
}
```

Bundled assets live at `SDM.app/Contents/Resources/vendor/`:

```
vendor/
    ffmpeg-arm64.lzfse
    ffmpeg-x86_64.lzfse
    qjs.lzfse                 # universal
    vendor-manifest.json      # { "ffmpegVersion": "7.1", "qjsVersion": "0.10.0", ... }
    ffmpeg-COPYING.txt        # LGPL text + link to martin-riedl.de source
    qjs-LICENSE.txt           # QuickJS-ng MIT/BSD
```

## Component: `ManagedBinaries` actor (new, `SDMResolve`)

Owns the managed dir, the manifest, and all provisioning. No `Bundle`
assumptions — the app injects everything.

```swift
public actor ManagedBinaries {
    public init(
        binDirectory: URL,
        fetcher: any BinaryFetching,
        runner: any ProcessRunner,                       // for codesign / xattr
        vendorAssets: @Sendable () -> [VendorAsset],      // app supplies bundled blobs + versions
        channel: @Sendable () -> YtDlpChannel,            // reads YouTubeSettingsStore
        onBinariesChanged: @Sendable () async -> Void,    // app calls locator.invalidate()
        notify: @Sendable (String) -> Void = { _ in }     // "yt-dlp updated to …"
    )

    /// Inflate bundled ffmpeg/qjs if missing or older than the vendor manifest.
    /// Cheap, synchronous-ish, safe to call on every launch.
    public func provisionBundledIfNeeded() async

    /// Advance the internal tick counter by one. Called ~1 Hz by the app.
    /// When a check becomes due and none is running, kicks one off.
    public func tick()

    /// Force a check now (launch, Settings "Check now", grabber-needs-yt-dlp).
    /// Coalesces with any in-flight check — never runs two at once.
    @discardableResult
    public func checkNow(reason: CheckReason) async -> CheckOutcome

    public func status() async -> ManagedBinariesStatus   // for the Settings UI
}

public protocol BinaryFetching: Sendable {
    /// GETs a URL, follows redirects, returns the body. Throws on non-2xx.
    func data(from url: URL) async throws -> Data
}
```

### Cadence (tick counter at 1 Hz — no wall clock, house style)

| Condition | Re-check interval |
|---|---|
| launch | immediately (first `tick()` / explicit `checkNow`) |
| `yt-dlp` present | every 6 h (21 600 ticks) |
| `yt-dlp` absent (was offline) | every 15 min (900 ticks) |
| grabber about to resolve links & `yt-dlp` absent | `checkNow(.resolveNeeded)` — coalesced |

### Update algorithm

1. Resolve the release-info URL for the active channel:
   `https://api.github.com/repos/<repo>/releases/latest` where `<repo>` is
   `yt-dlp/yt-dlp` or `yt-dlp/yt-dlp-nightly-builds`.
2. `fetcher.data(from:)` → parse `tag_name`.
3. If `bin/yt-dlp` missing **or** `remoteTag > manifest.ytDlpVersion` (component-wise
   `[Int]` compare; stable `YYYY.MM.DD`, nightly `YYYY.MM.DD.HHMMSS`):
   a. Download `yt-dlp_macos` and `SHA2-256SUMS` from that release.
   b. Verify the SHA-256 of the binary bytes against the `yt-dlp_macos` line in
      `SHA2-256SUMS`. Mismatch → abort, record `lastError`, keep the current
      binary, retry next cycle.
   c. Write to `bin/yt-dlp.new`, `chmod 0755`.
   d. `runner.run` → `xattr -c bin/yt-dlp.new` then `codesign -s - --force
      bin/yt-dlp.new` (best-effort; log but don't fail on codesign error).
   e. `FileManager.replaceItemAt` (atomic) `bin/yt-dlp.new` → `bin/yt-dlp`.
      Safe even while a `yt-dlp` process is running (the running process keeps
      the old inode).
   f. Update manifest (`ytDlpVersion`, `lastCheckTick`, clear `lastError`).
   g. `await onBinariesChanged()` (→ `BinaryLocator.invalidate()`), `notify(...)`.
4. On any thrown error: record `lastError`, leave everything else intact.

### Single-flight

The actor holds `private var inFlight: Task<CheckOutcome, Never>?`. `checkNow`
returns `await inFlight.value` when one exists; otherwise creates it, awaits,
clears it. `tick()` calls `checkNow` only when `inFlight == nil` and the
interval elapsed. This is what dedupes launch + timer + grabber triggers.

### Bundled-asset inflation

`provisionBundledIfNeeded()`:
- Read `bin/manifest.json` (may be absent).
- For ffmpeg: if `bin/ffmpeg` missing or `manifest.ffmpegVersion != vendor.ffmpegVersion`,
  pick the blob for the current arch (`#if arch(arm64)` at build → but the app
  binary itself is per-arch; use `ProcessInfo`/`uname` is unnecessary — the
  running process's arch is fixed, so compile-time `#if arch(arm64)` selects
  `ffmpeg-arm64` vs `ffmpeg-x86_64`), LZFSE-decode to `bin/ffmpeg.new`, `chmod
  0755`, atomic replace, update manifest.
- For qjs: same, single universal blob.
- Errors are recorded in `lastError` but never crash; a missing ffmpeg surfaces
  later as `MuxError.ffmpegMissing`.

## Wiring changes

### `BinaryLocator` (`SDMResolve`)
- No API change. The app stops passing `defaultSearchPaths` and passes
  `searchPaths: [binDir]` instead. Doc comment updated to note the app uses
  managed-only resolution; `defaultSearchPaths` retained for tests.

### `YtDlpResolver` (`SDMResolve`)
- New `init` param `extraArguments: @Sendable () -> [String] = { [] }`.
- `runYtDlp(...)` splices `extraArguments()` into every invocation (covers
  `-J`, `--flat-playlist`, `refresh`).
- App passes:
  `{ ["--extractor-args", "youtube:jsruntime=quickjs", "--ffmpeg-location", binDir.path] }`
  (QuickJS is off-by-default in yt-dlp, so the `--extractor-args` is mandatory.
  Exact token reverified against current yt-dlp docs during implementation.)

### `SystemProcessRunner`
- Already prepends the executable's own directory to `PATH`, so a `yt-dlp` in
  `bin/` automatically finds `ffmpeg`/`qjs` siblings. The explicit
  `--ffmpeg-location` / `--extractor-args` are belt-and-braces. Homebrew dirs
  stay in the fallback `PATH` (harmless; managed dir wins by being first).

### App target
- **One shared** `BinaryLocator(searchPaths: [binDir])` and one
  `ManagedBinaries`, constructed in `SDMApp.init` and handed to both
  `EngineController` and `GrabberController` (replacing their private
  `BinaryLocator()` instances).
- `SDMApp` starts a 1 Hz `Task` calling `managedBinaries.tick()` (mirrors
  `GrabberController.startAutoClearLoop`), and calls
  `provisionBundledIfNeeded()` + `checkNow(.launch)` once at startup.
- `GrabberController.ingest(urls:)` / `ingest(text:)` fire
  `Task { await managedBinaries.checkNow(.resolveNeeded) }` (not awaited).
- `GrabberController.ffmpegOnDisk` → checks `binDir/ffmpeg`.
- `VendorAsset` list built from `Bundle.main` in `SDMApp`.

### Settings — YouTube tab
- New "Components" `GroupBox`:
  - **yt-dlp**: installed version · latest-known version · channel `Picker`
    (Stable / Nightly) · "Check now" `Button` · last-checked (relative) ·
    status / error line.
  - **ffmpeg**: bundled version + "bundled" caption.
  - **JS runtime**: "QuickJS-ng <version> (bundled)".
- `YouTubeSettingsStore` gains `ytDlpChannel: YtDlpChannel` (`.stable` /
  `.nightly`, `UserDefaults` key `sdm.yt.ytDlpChannel`, default `.stable`).
- Changing the channel triggers `managedBinaries.checkNow(.channelChanged)`.

## `scripts/vendor-binaries.sh`

Run manually when bumping versions (there is no CI). Idempotent.

1. `FFMPEG_VERSION`, `QJS_TAG` pinned at the top of the script.
2. Download ffmpeg `arm64` + `x86_64` static builds from martin-riedl.de
   (JSON API at `https://ffmpeg.martin-riedl.de/api/v1/…` — resolve exact
   asset URLs in-script), verify the site's published SHA-256, extract the
   `ffmpeg` binary.
3. `git clone --depth 1 --branch "$QJS_TAG" https://github.com/quickjs-ng/quickjs`,
   `cmake` build for both arches, `lipo -create` → universal `qjs`.
4. LZFSE-compress each (`scripts/lzfse-tool` — a tiny Swift `Compression`
   one-file CLI built on demand, or `xcrun`/`ditto`). Output:
   `SDM/Resources/vendor/{ffmpeg-arm64,ffmpeg-x86_64,qjs}.lzfse`.
5. Write `SDM/Resources/vendor/vendor-manifest.json`, copy license texts.
6. Print a reminder to `git add` (LFS-tracked) and bump the Xcode resource.

`.gitattributes`:

```
SDM/Resources/vendor/*.lzfse filter=lfs diff=lfs merge=lfs -text
```

## Testing (`SDMResolveTests`, Swift Testing, no network / no real sleep)

- **`FakeBinaryFetcher`**: canned release JSON per URL + canned binary bytes +
  a matching / deliberately-wrong `SHA2-256SUMS`.
- **`FakeProcessRunner`** (exists in test support) records `codesign`/`xattr`
  invocations.
- Cases:
  - fresh install: `bin/yt-dlp` absent → one download, manifest written, binary
    at `bin/yt-dlp`, `onBinariesChanged` fired once.
  - up-to-date: remote tag == manifest → no download.
  - newer remote: `2026.09.01 > 2026.08.19` → download + atomic replace; old
    file gone.
  - nightly tag compare: `2026.08.19.120000 > 2026.08.19`.
  - SHA mismatch: no replace, `lastError` set, current binary untouched.
  - single-flight: 5 concurrent `checkNow` → fetcher hit once.
  - cadence: `tick()` ×21 599 → no check; ×21 600 → check. Absent-retry at 900.
  - offline: `fetcher` throws → `lastError` set, retries at the 900-tick cadence,
    succeeds once `fetcher` recovers.
  - manifest round-trips; corrupt manifest → treated as empty.
  - `provisionBundledIfNeeded`: LZFSE blob → decoded binary on disk, `chmod`
    0755, manifest version recorded; unchanged version → no rewrite; bumped
    version → rewrite.
  - LZFSE round-trip helper test.
- **`YtDlpResolverTests`**: `extraArguments` closure output appears in the args
  the `FakeProcessRunner` receives, for `-J`, `--flat-playlist`, and `refresh`.

## Docs to update on completion

- `README.md` — new "Vendored binaries" section: what's bundled vs downloaded,
  how `scripts/vendor-binaries.sh` works, LFS note, that yt-dlp self-updates.
- `CLAUDE.md` — "Current state" + a line under conventions about
  `scripts/vendor-binaries.sh` and LFS.
- `todo.md` — tick items 4 and 5; move item 5's "yt-dlp path / ffmpeg path
  Settings fields" note to done (managed-only supersedes the file pickers).
- Parent spec §2 — mark "yt-dlp bundling + self-update" resolved.

## Out of scope (unchanged)

HLS/DASH wholesale via yt-dlp-as-downloader (todo #2), proactive URL refresh
(todo #3), per-component details (todo #1). `ffprobe` is not bundled — `Muxer`
does not use it.
