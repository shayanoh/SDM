# SDM — Shayan's Download Manager

macOS download manager. Segmented transfers, resume, clipboard link grabbing, YouTube via yt-dlp.

**Read [docs/superpowers/specs/2026-08-03-sdm-design.md](docs/superpowers/specs/2026-08-03-sdm-design.md) before implementing anything.** It is the source of truth for architecture and behavior. This file is a summary plus working conventions.

## Current state

**All five phases are complete and merged to `main`** (Phase 1 merged 2026-08-04, Phase 5 merged 2026-09-02). ~549 tests, no network and no real clock anywhere in the suite.

`SDMKit/` is a local SPM package with **four library targets** — `SDMCore`, `SDMEngine`, `SDMGrabber`, `SDMResolve` — linked into the `SDM` app target. The engine, scheduler, persistence, linkgrabber, menu bar, notifications, theming, activation-policy modes, and the yt-dlp/YouTube feature (multi-component downloads, `ffmpeg` muxing, silent 403 URL refresh, playlists, format picker, YouTube Settings tab) all work end to end.

**What's still open lives in `todo.md` at the repo root** — a couple of consciously-deferred Phase 5 items (proactive URL refresh, per-component details panel) and the small Phase 1 follow-ups owed since before Phase 3. Nothing in the design specs or plan docs is an open backlog; they carry "IMPLEMENTED" / "SUPERSEDED" status banners.

**Managed binaries (2026-09-03):** `yt-dlp` / `ffmpeg` / `ffprobe` / `qjs` (QuickJS-ng) are provisioned into `~/Library/Application Support/SDM/bin/` — ffmpeg/ffprobe/qjs bundled as LZFSE blobs and inflated on launch, yt-dlp downloaded + SHA-256-verified + self-updated by `SDMResolve/ManagedBinaries.swift`. No `brew install` required. Spec: `docs/superpowers/specs/2026-09-03-managed-binaries-design.md`.

**UI data path (2026-10-10):** the engine → UI path was reworked for CPU and scrolling: per-item / per-package `@Observable` live models and whole-engine `EngineStats` (`SDM/LiveModels.swift`), aggregates computed once per tick (`SDMEngine/SnapshotAggregates.swift`), engine snapshots reused when unchanged, a 64-sample speed history per item with the full one fetched only for the details panel. Findings, what was done and what was deliberately left: `docs/superpowers/specs/2026-10-09-ui-data-path-performance.md`.

**Multi-site resolver + HLS/DASH wholesale (2026-09-03):** the yt-dlp gate is a curated ~50-site `SiteRegistry` (`SDMResolve`), not just YouTube — Vimeo, Twitch, TikTok, SoundCloud, Reddit, Aparat, the major adult tubes, etc. HLS/DASH-only videos (no single `Range`-capable URL) download via `YtDlpWholesaleDownloader` (yt-dlp as a downloader) driven by `WholesaleComponentTask` in the engine; that path is non-resumable and its progress is a synthesized `RangeSet`. Spec: `docs/superpowers/specs/2026-09-03-multi-site-resolver-design.md`.

## Rust migration branch

This section applies to the `rust-migration` branch only. The sections here that describe `SDMKit`'s Swift targets describe the baseline, which now lives under `legacy/SDMKitSwift`.

The experiment ports the Swift core to Rust and keeps the SwiftUI app on top. Status, decisions, measurements and what is left are in [docs/rust-migration-roadmap.md](docs/rust-migration-roadmap.md); the design is in [docs/superpowers/specs/2026-10-12-rust-core-design.md](docs/superpowers/specs/2026-10-12-rust-core-design.md).

- `sdm-core/` is a Cargo workspace. `sdm-core` holds the domain models and `RangeSet`; `sdm-resolve` holds yt-dlp, format selection and managed binaries; `sdm-engine` holds the engine, scheduler, transport and persistence; `sdm-grabber` holds extraction, probing, clustering and the session; `sdm-ffi` is the UniFFI export surface; `sdm-bench` and `uniffi-bindgen` are tools.
- The SwiftUI app reaches the core through generated UniFFI bindings (`SDMKit/Sources/SDMRust`) and thin shims in `SDMKit/Sources/SDM*` that keep the four product names.
- Translation rules (spec section 4): an actor becomes one mutex, with one lock scope per stretch that has no `await` and never a lock held across `.await`; Swift's implicit task cancellation becomes an explicit cancellation token; time is driven by `tick()` and there is no clock.
- Build and test: `scripts/build-rust-core.sh` builds the Rust library, `./test.sh` runs the Rust and Swift tests, and `./build.sh --no-install` builds the app without touching `/Applications`.
- `legacy/SDMKitSwift` is a frozen baseline for the tests and benchmarks. Do not edit it.

## Fixed decisions

Do not relitigate these without asking; they were settled during design.

- **Unsandboxed**, Developer ID signed + notarized. Not going to the Mac App Store.
- **macOS 15.0 baseline**, macOS 26 enhancements behind `if #available`.
- **Swift 6 language mode**, strict concurrency.
- **No SwiftData, no database.** Atomic JSON snapshot for durable state, per-download sidecar for resume state, memory only for live progress.
- **yt-dlp is a metadata extractor** — SDM downloads the direct URLs itself. It is auto-provisioned (not user-installed). The one exception is the HLS/DASH wholesale path (`WholesaleComponentTask` / `YtDlpWholesaleDownloader`), where yt-dlp *is* the downloader because there is no single direct URL to hand the engine.

## Architecture

Local SPM package, **five targets** (the spec's original four plus `SDMResolve`, added in Phase 5):

| Target | Owns |
|---|---|
| `SDMCore` | Domain models (`DownloadItem`/`FileComponent`, `RangeSet`, `MediaFormat`, `QualityPreferences`, `LinkResolver` protocol), value types, no I/O |
| `SDMEngine` | Worker pools, resume, checkpointing, scheduler, telemetry, per-component parallel download, `ffmpeg` muxing (`Muxer`), 403 URL refresh |
| `SDMGrabber` | URL extraction, probing, verdict rules, package clustering, YouTube media-row routing, playlist expansion, format-picker list |
| `SDMResolve` | yt-dlp process invocation (`ProcessRunner`), binary discovery (`BinaryLocator`), `-J` parsing, `FormatSelector`, `YtDlpResolver`, `SiteRegistry`, `YtDlpWholesaleDownloader` |
| `SDMApp` (Xcode target) | SwiftUI views, menu bar, notifications, clipboard, theming, Settings |

### The one idea everything rests on

**A download's progress is a `RangeSet` of completed byte ranges. Segments are ephemeral workers, not stored structure.** Workers claim the largest remaining gap, write at absolute offsets, and report intervals back.

Everything else falls out of this: changing the segment count mid-flight, resuming after a crash, and the segmented progress bar are all the same mechanism rather than three features. If a change starts requiring segments to be persistent objects, that's a signal the change is wrong.

### Other load-bearing invariants

- **Scheduling is a pure function** re-evaluated on every change, not a queue that's kept in sync. Reordering, priority changes, additions, and completions all flow through it.
- **Running non-resumable items reserve their slots before rank-based filling.** Skipping this produces unsatisfiable desired sets and thrash. `isResumable` is three-state (`Bool?`): `nil` means not yet probed, and the scheduler keys on `== false`, so an unprobed item stays preemptible. Collapsing unknown into "not resumable" makes every running item unpreemptible and silently kills preemption — that bug shipped once already.
- **Verdict rules and package clustering are pure functions** over value types, with fixture tables. Tune the data, not the control flow.
- **Views never touch engine actors.** They consume a coalesced snapshot stream published off the engine heartbeat (`AppTiming.ticksPerSecond`).
- **Per-tick figures reach views through per-object live models, not one shared observable.** `EngineController.publish` writes a model's field only when it changed (`@Observable` notifies on assignment, not on value change); `structuralPackages` is what tells the `List` its rows changed. A view that reads `controller.snapshot` in its `body` re-evaluates every tick — keep such reads in small leaf views. `SDMApp.body` must read nothing that changes per tick (no scene-level `.onChange` on such a value either): re-running the scene body made SwiftUI re-apply the window toolbar every tick. Non-view state that follows the engine (menu bar icon, Touch Bar) hangs off `EngineController.onPublish`.
- **Nothing may sit between the `List` and a row's `.tag`, `.listRowBackground` and `.draggable`.** Wrapping a row in `.equatable()`, or putting `.onGeometryChange` above those, hides them from the `List` (selection, row color and drag all broke that way once). A skip-if-unchanged boundary goes *inside* the row — see `ItemRowBody`.
- **`DownloadEngine.snapshot()` reuses unchanged values.** Every stored property it reads bumps `snapshotGeneration` in its `didSet`; a new snapshot input must do the same or an idle engine will serve a stale snapshot.
- **Colors come from theme roles, never literals.** macOS-26-only APIs live in one `ViewModifier` file, never inline in views.

## Phasing

1. Engine + scheduler + tests (plain UI to drive it)
2. Linkgrabber + clipboard watcher
3. Menu bar, notifications, graphs, segmented progress rendering
4. Theming, activation policy modes, Liquid Glass
5. yt-dlp resolver (rebrainstormed 2026-09-02; delivered as 5 part-plans)

Each phase got its own implementation plan under `docs/superpowers/plans/`. **All five phases are complete** — Phase 5 merged 2026-09-02. Follow-up work is in `todo.md`.

## Testing

Swift Testing (`@Test` / `#expect`), not XCTest.

The engine takes `HTTPTransport` as an injected protocol — tests run against `FakeOrigin`, an in-process fake server programmable to misbehave (ignores `Range`, lies about `Content-Length`, drops or truncates the body, changes its `ETag`). **No test may touch the network or sleep on a real clock.**

There is **no injected `Clock`, and no `FakeClock`.** Time-dependent behavior is tick-driven from outside instead: `DownloadEngine.tick()` is called once per second by the app and drives telemetry, rescheduling, checkpoint staleness, retry backoff, and the debounced state flush. Tests advance time by calling `tick()` directly, which pins "fires on tick N and not before" far more precisely than a fake clock would. Keep new time-dependent behavior on the tick rather than reaching for a clock.

The two properties that must never regress:

1. Byte identity under randomized churn (segment-count changes, pauses, kills, drops) — output must hash-match the source.
2. `RangeSet` never hands the same range to two workers, and the union of completed ranges equals what was written.

## Conventions

- Format Swift with `swift-format` before committing.
- Add packages via Xcode or `Package.swift` through SPM tooling — never hand-edit `Package.resolved`.
- Vendored `ffmpeg`/`ffprobe`/`qjs` blobs (`SDM/Resources/vendor/*.lzfse`) are Git LFS; regenerate with `scripts/vendor-binaries.sh` and commit the results.
- Verify library APIs against current documentation (Context7) rather than from memory; SwiftUI, SwiftData, and macOS 26 APIs have all moved recently.

## Git

Shayan has granted blanket consent for **committing** and for **fast-forward pushes** in this repo — no need to ask each time. **Force-pushing still requires explicit authorization.**
