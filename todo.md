# SDM — Todo

What is still open. The status of everything built, and a dated log, is in
[docs/tauri-roadmap.md](docs/tauri-roadmap.md).

The items below came from the Swift app's backlog (read and reviewed
2026-09-02) and were carried over when the Rust version replaced it. They were
**not all re-checked against the Rust port**, so look at the Rust code before
treating one as a confirmed bug.

## Todo

Nothing open.

## Parked

- **Retry classifier** (`sdm-engine/src/retry_policy.rs`): an error type it
  does not know falls through to "transient" with no reason, so an unrecognized
  *permanent* failure retries to the attempt cap. Not seen in daily use; fix
  when one shows up (Shayan, 2026-10-27).

## Decided — no change needed

- **A distinct `assembling` item state.** The mux step is `running` plus an
  `is_assembling` flag instead of its own enum case. Functionally identical
  (the item keeps its scheduler slot, is not rescheduled during the mux, and
  the UI shows "Assembling…"). Approved as is.
- **Per-component connection demand.** A two-component muxed item reports one
  connection demand (host from component 0), so it counts as one download
  against the per-host cap. The cap is politeness, not correctness. Approved
  as is.
- **Per-component details breakout** (video and audio as two rows in the
  details panel): not needed (Shayan, 2026-10-27).
- **Proactive URL refresh** of an old `googlevideo` URL before resuming: extra
  code and tests to save one `403`, which refresh-on-403 already handles
  (Shayan, 2026-10-27).
- **Spec 5 leftovers** in the roadmap (second-monitor DPI, vertical taskbar,
  `Info` banners, a vanished theme id, out-of-range numbers): ignored and
  closed (Shayan, 2026-10-27). The by-hand checks of spec 5 on all three
  systems pass.

## Wishlist (not planned)

- SponsorBlock, chapter markers, subtitle download, thumbnail embedding,
  re-encoding — yt-dlp post-processing features, never in scope. *(Spec §2.)*
- Developer ID signing and notarization for the macOS build, and Windows code
  signing (decide before the first distributed release).
- Cookies for Edge and Brave (they still go through yt-dlp's own reader, which
  has the same encrypted-database problem on Windows).
