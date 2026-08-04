# SDM — Shayan's Download Manager

macOS download manager. Segmented transfers, resume, clipboard link grabbing, YouTube via yt-dlp.

**Read [docs/superpowers/specs/2026-08-03-sdm-design.md](docs/superpowers/specs/2026-08-03-sdm-design.md) before implementing anything.** It is the source of truth for architecture and behavior. This file is a summary plus working conventions.

## Current state

**Phase 1 is complete** (merged 2026-08-04). The engine, scheduler, persistence, and a plain driver UI all work end to end: 163 tests, no network and no real clock anywhere in the suite. The Xcode boilerplate is gone — `SDM/Item.swift`, the `ModelContainer`, and the default `ContentView` were all deleted.

`SDMKit/` is a local SPM package with `SDMCore` and `SDMEngine`, linked into the app target. `SDMGrabber` does not exist yet — it arrives with Phase 2.

Carried but deliberately not enforced yet, all Phase 3: `globalMaxConnections` and per-host caps (settings only), hysteresis (`Scheduler` supports `startedRecently`; `DownloadEngine` passes an empty set), and the full retry design (`RetryPolicy` classifies and computes backoff, and the engine now caps attempts and holds items in backoff, but nothing refreshes an expired URL). Signed-URL refresh on 403 waits on Phase 5's resolver.

## Fixed decisions

Do not relitigate these without asking; they were settled during design.

- **Unsandboxed**, Developer ID signed + notarized. Not going to the Mac App Store.
- **macOS 15.0 baseline**, macOS 26 enhancements behind `if #available`.
- **Swift 6 language mode**, strict concurrency.
- **No SwiftData, no database.** Atomic JSON snapshot for durable state, per-download sidecar for resume state, memory only for live progress.
- **yt-dlp is user-installed and used as a metadata extractor only** — SDM downloads the direct URLs itself. Not bundled (for now), not used as the downloader.

## Architecture

Local SPM package, four targets:

| Target | Owns |
|---|---|
| `SDMCore` | Domain models, `RangeSet`, value types, no I/O |
| `SDMEngine` | Worker pools, resume, checkpointing, scheduler, telemetry |
| `SDMGrabber` | URL extraction, probing, verdict rules, package clustering, resolvers |
| `SDMApp` (Xcode target) | SwiftUI views, menu bar, notifications, clipboard, theming |

### The one idea everything rests on

**A download's progress is a `RangeSet` of completed byte ranges. Segments are ephemeral workers, not stored structure.** Workers claim the largest remaining gap, write at absolute offsets, and report intervals back.

Everything else falls out of this: changing the segment count mid-flight, resuming after a crash, and the segmented progress bar are all the same mechanism rather than three features. If a change starts requiring segments to be persistent objects, that's a signal the change is wrong.

### Other load-bearing invariants

- **Scheduling is a pure function** re-evaluated on every change, not a queue that's kept in sync. Reordering, priority changes, additions, and completions all flow through it.
- **Running non-resumable items reserve their slots before rank-based filling.** Skipping this produces unsatisfiable desired sets and thrash. `isResumable` is three-state (`Bool?`): `nil` means not yet probed, and the scheduler keys on `== false`, so an unprobed item stays preemptible. Collapsing unknown into "not resumable" makes every running item unpreemptible and silently kills preemption — that bug shipped once already.
- **Verdict rules and package clustering are pure functions** over value types, with fixture tables. Tune the data, not the control flow.
- **Views never touch engine actors.** They consume a coalesced snapshot stream (~4 Hz per spec §4.1; the Phase 1 driver UI republishes at 1 Hz off the heartbeat — raise it when the real UI lands).
- **Colors come from theme roles, never literals.** macOS-26-only APIs live in one `ViewModifier` file, never inline in views.

## Phasing

1. Engine + scheduler + tests (plain UI to drive it)
2. Linkgrabber + clipboard watcher
3. Menu bar, notifications, graphs, segmented progress rendering
4. Theming, activation policy modes, Liquid Glass
5. yt-dlp resolver

Each phase gets its own implementation plan. Currently: **Phase 1 done, Phase 2 not yet started.**

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
- Verify library APIs against current documentation (Context7) rather than from memory; SwiftUI, SwiftData, and macOS 26 APIs have all moved recently.

## Git

Shayan has granted blanket consent for **committing** and for **fast-forward pushes** in this repo — no need to ask each time. **Force-pushing still requires explicit authorization.**
