# SDM — Todo

What is still open. The status of everything built, and a dated log, is in
[docs/tauri-roadmap.md](docs/tauri-roadmap.md); its "small ones" list (spec 5
leftovers: the tray icon on a second monitor with another DPI, a vertical
taskbar edge, `Info` banners such as "yt-dlp updated to …" that are never
posted, a stored theme id that no longer exists, hand-edited out-of-range
numbers that reset instead of clamping) is open too.

The items below came from the Swift app's backlog (read and reviewed
2026-09-02) and were carried over when the Rust version replaced it. Items
that only made sense for the Swift code (`URLSessionTransport`, AppKit hooks)
were dropped; the rest were **not re-checked against the Rust port**, so look
at the Rust code before treating one as a confirmed bug.

## Todo

- [ ] **Per-component details breakout.** Report per-component url / part-file
  path / size / progress / error in the engine snapshot, and render the details
  panel of a muxed item as two rows (video, audio) instead of the single "Video
  + audio, muxed with ffmpeg" line. *(Spec §5.3 / §9.3.)*

- [ ] **Proactive URL refresh.** `googlevideo` URLs expire after a few hours.
  Refresh-on-403 already works. Add: when resuming a resolved item whose stored
  URL is older than about 5 h, refresh it *before* spawning workers rather than
  eating one wasted `403`. *(Spec §2 / §7.3(c).)*

- [ ] **Retry policy** (`sdm-engine/src/retry_policy.rs`):
  - An error type the classifier does not know falls through to "transient"
    with no reason, so an unrecognized *permanent* failure retries to the
    attempt cap. Decide the classification and the reason text.
  - Retry backoff "jitter" was not jitter in the Swift code: the delay was
    deterministic per attempt number, so concurrent retries stayed in lockstep
    (the thundering herd spec §6.4 asks jitter to prevent). Check the Rust
    port; the likely fix is per-client seeded randomness with a test-injected
    seed.
- [ ] **Engine bookkeeping:** per-item counters (failed attempts, retry hold
  ticks, attempt start bytes, checkpoint failures) were never pruned when an
  item is removed; the debounced state flush clears its tick counter before the
  write, so a failed write is not retried until an unrelated change re-arms it;
  `SparseFile::write` does not check that the file is open.

## Decided — no change needed

- **A distinct `assembling` item state.** The mux step is `running` plus an
  `is_assembling` flag instead of its own enum case. Functionally identical
  (the item keeps its scheduler slot, is not rescheduled during the mux, and
  the UI shows "Assembling…"). Approved as is.
- **Per-component connection demand.** A two-component muxed item reports one
  connection demand (host from component 0), so it counts as one download
  against the per-host cap. The cap is politeness, not correctness. Approved
  as is.

## Wishlist (not planned)

- SponsorBlock, chapter markers, subtitle download, thumbnail embedding,
  re-encoding — yt-dlp post-processing features, never in scope. *(Spec §2.)*
- Developer ID signing and notarization for the macOS build, and Windows code
  signing (decide before the first distributed release).
- Cookies for Edge and Brave (they still go through yt-dlp's own reader, which
  has the same encrypted-database problem on Windows).
