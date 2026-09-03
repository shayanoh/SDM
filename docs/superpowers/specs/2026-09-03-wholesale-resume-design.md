# SDM — Resumable Wholesale (HLS/DASH) Downloads: Design

**Status: IMPLEMENTED (2026-09-03).**

Amends `2026-09-03-multi-site-resolver-design.md` §6.6–6.7, which made the
wholesale (yt-dlp-as-downloader) path deliberately **non-resumable**: any
pause, preempt, stop, or crash discarded every byte and the next start
re-ran yt-dlp from zero.

This change makes wholesale downloads **resumable and preemptible** by
leaning on yt-dlp's own fragment-level `--continue`, which is the
mechanism its entire user base relies on for HLS/DASH.

## 1. Why this is safe to lean on

yt-dlp's **native** HLS downloader (`m3u8_native`) and DASH downloader
(`http_dash_segments`) download fragment-by-fragment and maintain, in the
item's package folder alongside `Clip.mp4`:

- `Clip.mp4.part` — fragments concatenated so far
- `Clip.mp4.part-FragN` — the fragment in flight
- `Clip.mp4.ytdl` — JSON state recording the current fragment index,
  rewritten after **every** fragment (not just on exit)

On relaunch with `--continue` (yt-dlp's default) it reads `.ytdl`, skips
every fragment already in `.part`, and resumes. Because SDM re-invokes
with the **page URL**, extraction re-runs and fragment URLs are refreshed
— signed-URL expiry is a non-issue; only the download cursor is restored.
Per-fragment size validation re-fetches a truncated tail fragment. A
SIGTERM (what `SystemProcessRunner` sends on cancellation) lets yt-dlp
flush; even an abrupt death loses ≤1 fragment.

**The one catch:** if yt-dlp routes HLS through **ffmpeg as the
downloader** instead of its native one, there are no fragment files and
resume is impossible. Mitigations:

1. Pass `--hls-prefer-native` to force the resumable path wherever yt-dlp
   can.
2. Detect at runtime whether *this* run is actually fragmented (the
   `fragment_count` progress field is populated only by the native
   fragment downloaders) and only advertise the item as resumable once
   that is confirmed. A non-fragmented wholesale run stays non-resumable
   — exactly today's behavior, never worse.

## 2. Changes

### 2.1 `WholesaleProgress` gains `isFragmented` (`SDMCore`)

```swift
public struct WholesaleProgress {
    // ...
    public var isFragmented: Bool   // NEW, defaulted false
}
```

`WholesaleProgressParser` sets it `true` when it parsed a positive
`fragment_count` (fields 4/5 of the `sdm:` progress line). This is the
signal that yt-dlp's native fragment downloader — and therefore
`--continue` resume — is in play.

### 2.2 `YtDlpWholesaleDownloader` flags (`SDMResolve`)

- **Remove** `--no-part` and `--no-continue`.
- **Add** `--continue` (explicit; guards against a user config that
  disabled it) and `--hls-prefer-native`.

Everything else (progress template, `--merge-output-format`,
`--ffmpeg-location`, site/cookie/extra args) is unchanged. On success
yt-dlp renames `Clip.mp4.part` → `Clip.mp4`, so the caller's
`fileExists(destination)` success check still holds.

### 2.3 `WholesaleComponentTask` (`SDMEngine`)

- **`pause()`** no longer deletes partial output or resets progress. It
  cancels the job (→ SIGTERM) and **keeps every scratch file and the
  synthesized progress state**, so `completedRanges` still reports where
  the download was and the next run's `--continue` picks it up.
- **`start()`** on a **non-cancellation** error still sweeps scratch and
  resets state, then rethrows — a hard failure means the next attempt
  starts clean (matches "if it fails, delete and restart from the
  beginning"). On `CancellationError` it keeps everything and throws
  `WholesaleError.cancelled`.
- **`probedSupportsRanges`** returns `false` until a `WholesaleProgress`
  with `isFragmented == true` has been seen, then `true`. The engine's
  existing `refreshResumability()` mirrors this onto
  `component.isResumable`, so the scheduler sees `false` (slot reserved,
  never preempted) until fragmented progress confirms resume works, then
  `true` (preemptible like any resumable download).

No new engine wiring: `finishItem` already writes each component's
`completedRanges` / `isResumable` back on retire, and `context(for:)`
already hands a rebuild context to any component that is not
`isComplete`. With scratch left in place, the freshly-built
`WholesaleComponentTask` runs yt-dlp `--continue` against it.

### 2.4 `DownloadItem.isResumable` (`SDMCore`)

Drop the hard `if hasWholesaleComponent { return false }` short-circuit in
the getter. The value now falls through to the per-component
`isResumable`, which the task drives (`false` → `nil`-equivalent reserve,
then `true`). `hasWholesaleComponent` stays — it still gates the yt-dlp
scratch sweep in `itemArtefactURLs` (used by remove / reset, which *do*
wipe).

### 2.5 `SystemProcessRunner` graceful-kill wait

On the cancellation path, after `process.terminate()` (SIGTERM), wait for
the child to actually exit — bounded to ~3 s so a process that ignores
SIGTERM cannot hang the caller. This ensures yt-dlp has finished flushing
`.ytdl` / `.part` before a rescheduled run starts a second yt-dlp against
the same files. Applies to both `run` and `runStreaming`.

## 3. What is unchanged

- The wholesale progress bar is still a synthesized contiguous `RangeSet`.
- DASH and non-native HLS still work; they just may not resume (detected,
  advertised honestly).
- DRM / `authRequired` / `unavailable` failures are still permanent.
- Remove / reset still wipe all yt-dlp scratch.
- No test touches the network or a real clock. New coverage:
  `WholesaleProgressParser` fragment detection; `WholesaleComponentTask`
  keeps-scratch-on-pause / wipes-on-failure; engine
  preempt-then-resume-continues and resumable-only-after-fragmented-progress;
  `YtDlpWholesaleDownloader` emits `--continue` / `--hls-prefer-native`
  and not `--no-continue` / `--no-part`.

## 4. Rejected alternatives

- **Hand-roll HLS/DASH in-engine** (parse the media playlist, one worker
  per segment, `ffmpeg -f concat`). Much larger: a new engine task type
  with a durable segment bitmap, separate-audio handling, AES-128
  decryption, `#EXT-X-MAP` / byte-range segments, a full fixture matrix —
  and DASH + DRM still need yt-dlp. Deferred; not needed for the resume
  win.
- **User-initiated pause/resume only, scheduler never preempts wholesale.**
  Lower risk but leaves the "fully preemptible" win on the table; the
  runtime fragmented-detection makes full preemption safe.
