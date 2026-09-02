# SDM — Todo

The downloads-UI overhaul and all five phases of the design
(`docs/superpowers/specs/2026-08-03-sdm-design.md` + the Phase 5 spec
`docs/superpowers/specs/2026-09-02-phase-5-youtube-resolver-design.md`)
are implemented and merged to `main`. 401 tests, both targets build.

The deferred items below were **all read and reviewed on 2026-09-02** —
sourced from the Phase 5 spec §2 "Deferred" and the Part 1–5 plan
self-reviews under `docs/superpowers/plans/`. Each was either kept as-is
(see "Decided — no change") or turned into a todo item here.

---

## Todo (priority order)

- [ ] **1. Per-component details breakout.** Add `ItemSnapshot.components:
  [ComponentSnapshot]` (per-component url / part-file path / size / progress /
  error) and render the bottom details panel of a muxed item as two rows
  (video, audio) instead of the single "Video + audio, muxed with ffmpeg"
  line. Needs an engine `snapshot()` addition. *(Spec §5.3 / §9.3; deferred
  through Parts 3–5.)*

- [ ] **2. HLS/DASH wholesale fallback.** A video that exposes only segmented
  `.m3u8` / DASH manifests with no single `Range`-capable URL is currently
  marked `unsupported` in the grabber and not handed off (live-stream VODs,
  some brand-new uploads). Add the yt-dlp-as-downloader path: hand the URL to
  yt-dlp to download wholesale, parse progress from its stdout text.
  Non-resumable, degraded path. *(Spec §2; the `unsupported` state + its test
  already exist.)*

- [ ] **3. Proactive URL refresh.** `googlevideo` URLs expire after a few
  hours. Refresh reactively-on-403 already works (`URLRefreshTests`). Add:
  when resuming a `.resolved` item whose stored URL is older than ~5h,
  refresh it *before* spawning workers rather than eating one wasted `403`.
  *(Spec §2 / §7.3(c).)*

- [x] **4. yt-dlp version surfacing / self-update.** DONE 2026-09-03 — see
  `docs/superpowers/specs/2026-09-03-managed-binaries-design.md`. Settings →
  YouTube → Components shows installed + latest version, channel picker, and
  a "Check now" button. Self-update is our own version check + SHA-256
  download (not `yt-dlp -U`).

- [x] **5. Bundle yt-dlp / ffmpeg + self-updating.** DONE 2026-09-03 —
  `SDMResolve/ManagedBinaries.swift` + `ManagedBinariesController`. ffmpeg and
  QuickJS-ng ship as LZFSE blobs (`scripts/vendor-binaries.sh`, Git LFS) and
  inflate on launch; yt-dlp is downloaded and self-updated. One shared
  `BinaryLocator` (managed-dir only) is plumbed through `EngineController` and
  `GrabberController`. The deferred "yt-dlp path / ffmpeg path" Settings
  file-pickers (spec §4.5 / §9.4) are **superseded** — resolution is
  managed-only by design.

- [ ] **6. Overdue Phase 1 decisions** (from
  `memory/sdm-phase-1-followups.md`, owed since before Phase 3):
  - [ ] `RetryPolicy.classify(any Error)` falls through to `.transient` for
    any type outside `TransportError`/`DownloadError`, with no reason string —
    an unrecognized *permanent* failure retries to the attempt cap. Decide the
    classification + reason.
  - [ ] Retry backoff "jitter" is not jitter: the delay is deterministic per
    attempt number, so concurrent retries stay in lockstep — the thundering
    herd spec §6.4 asks jitter to prevent. Likely fix: per-client seeded
    randomness with a test-injected seed.
  - [ ] `URLSessionTransport` continuation race: `setResponseContinuation`
    can re-insert an entry nothing will resume if `didCompleteWithError` wins
    the race against `withCheckedThrowingContinuation`; `fetch()` then never
    returns (bounded only by the 5 s `shutdownBlocking` backstop). Fix with a
    terminal marker on the entry.
  - [ ] `DownloadEngine.flushIfDebounceElapsed` clears
    `ticksSincePendingChange` before awaiting `flush()`; `JSONStateStore`
    keeps `pending` on a failed write, but nothing re-arms the debounce
    window until an unrelated change occurs.
  - [ ] Per-item bookkeeping (`failedAttempts`, `retryHoldTicks`,
    `attemptStartBytes`, `checkpointFailures`) is never pruned — now matters,
    an item-removal API exists.
  - [ ] `URLSessionTransport` uses one serialized delegate `OperationQueue`
    for every transfer (up to maxConcurrent × segments).
  - [ ] `SparseFile.write()` does not check `isClosed` (pre-existing).
  - [ ] `applicationWillTerminate` hook has no automated coverage (AppKit
    offers no test seam) — needs one manual ⌘Q-mid-download check against the
    resume criterion.

---

## Decided — read, reviewed, no change needed (2026-09-02)

- **Distinct `ItemState.assembling` case.** The spec (§7.2) wanted a real
  enum case for the mux step; it is instead represented as `state == .running`
  plus `ItemSnapshot.isAssembling == true`. Functionally identical — the item
  holds its scheduler slot, is never rescheduled during the mux, and the UI
  shows "Assembling…" and a `wand.and.stars` icon. A real enum case would
  touch ~8 exhaustive `switch` statements for zero behavior change.
  **Approved as-is.**

- **Per-component connection-demand precision.** A 2-component muxed YouTube
  item reports connection demand keyed by item (one demand, host from
  component 0), so it counts as one download against the 8-per-host cap
  rather than two. The per-host cap is politeness / rate-limit avoidance, not
  correctness. **Approved as-is.**

---

## Wishlist (not planned)

- SponsorBlock, chapter markers, subtitle download, thumbnail embedding,
  re-encoding — yt-dlp post-processing features, never in scope. *(Spec §2.)*
