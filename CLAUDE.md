# SDM — Shayan's Download Manager

macOS download manager. Segmented transfers, resume, clipboard link grabbing, YouTube via yt-dlp.

**Read [docs/superpowers/specs/2026-08-03-sdm-design.md](docs/superpowers/specs/2026-08-03-sdm-design.md) before implementing anything.** It is the source of truth for architecture and behavior. This file is a summary plus working conventions.

## Current state

Pre-implementation. The repo still contains unmodified Xcode boilerplate (`SDM/Item.swift`, the `ModelContainer` in `SDM/SDMApp.swift`, the default `ContentView`). All three are slated for deletion in Phase 1.

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
- **Running non-resumable items reserve their slots before rank-based filling.** Skipping this produces unsatisfiable desired sets and thrash.
- **Verdict rules and package clustering are pure functions** over value types, with fixture tables. Tune the data, not the control flow.
- **Views never touch engine actors.** They consume a coalesced snapshot stream (~4 Hz).
- **Colors come from theme roles, never literals.** macOS-26-only APIs live in one `ViewModifier` file, never inline in views.

## Phasing

1. Engine + scheduler + tests (plain UI to drive it)
2. Linkgrabber + clipboard watcher
3. Menu bar, notifications, graphs, segmented progress rendering
4. Theming, activation policy modes, Liquid Glass
5. yt-dlp resolver

Each phase gets its own implementation plan. Currently: **Phase 1 not yet started.**

## Testing

Swift Testing (`@Test` / `#expect`), not XCTest.

The engine takes `HTTPTransport` and `Clock` as injected protocols — tests run against an in-process fake origin with a fake clock. **No test may touch the network or sleep on a real clock.**

The two properties that must never regress:

1. Byte identity under randomized churn (segment-count changes, pauses, kills, drops) — output must hash-match the source.
2. `RangeSet` never hands the same range to two workers, and the union of completed ranges equals what was written.

## Conventions

- Format Swift with `swift-format` before committing.
- Add packages via Xcode or `Package.swift` through SPM tooling — never hand-edit `Package.resolved`.
- Verify library APIs against current documentation (Context7) rather than from memory; SwiftUI, SwiftData, and macOS 26 APIs have all moved recently.

## Git

Shayan has granted blanket consent for **committing** and for **fast-forward pushes** in this repo — no need to ask each time. **Force-pushing still requires explicit authorization.**
