# Rust migration roadmap

Branch: `rust-migration`. Started 2026-10-12.

**Why:** an experiment to learn from and compare — a Rust core under the
existing SwiftUI app, then the same UI in Tauri, then possibly a .NET UI for
Windows and Linux. `main` keeps the all-Swift app as the baseline every step is
measured against. Nothing here is merged to `main` until Shayan decides the
experiment is worth keeping.

This file is the single place that tracks where the experiment stands. Tick
items as they land and record decisions and measurements under the step they
belong to. Each step gets its own design spec and plan(s) under
`docs/superpowers/`.

## Step 1 — Rust core under the SwiftUI app

Port the four `SDMKit` targets (~9,700 lines, 557 tests) to Rust, keep the
SwiftUI app on top through generated bindings, and compare against the Swift
core.

- [x] Branch and roadmap
- [x] Design spec (`docs/superpowers/specs/2026-10-12-rust-core-design.md`) and plan (`docs/superpowers/plans/2026-10-12-rust-core.md`)
- [x] 1.0 Walking skeleton: Cargo workspace, UniFFI bindings, XCFramework, the
      app calling one Rust function, building in Xcode and with `build.sh`
- [x] 1.1 `SDMCore` port: domain models, `RangeSet`, themes — with tests
- [x] 1.2 `SDMResolve` port: process runner, yt-dlp JSON, format selection,
      site registry, managed binaries, wholesale downloader — with tests
- [ ] 1.3 `SDMEngine` port: transport, `FakeOrigin`, download task, scheduler,
      persistence, engine, muxing, wholesale task — with tests
      (in progress: plan Tasks 11–13 of 11–19 implemented)
- [ ] 1.4 `SDMGrabber` port: extraction, probing, verdicts, clustering, session
      — with tests
- [ ] 1.5 Binding surface and Swift adapter: the app's controllers run on the
      Rust core; Swift `SDMKit` sources removed from the branch
- [ ] 1.6 App builds in Xcode (Debug and universal Release), manual smoke test
- [ ] 1.7 Performance comparison against `main`, results recorded below

### Status and how to resume (updated 2026-10-13)

Plan Tasks 1–13 of 27 are implemented and committed (last commit `cab722c`).
Tasks 1–10 are reviewed; Tasks 11–13 are implemented but not yet reviewed
(review gate C comes after Task 15). **Next: Task 14, the download task.**

The app does not run on the Rust core yet (that is Task 24) and there are no
benchmark numbers yet (Task 26). Xcode Debug builds with the Rust skeleton
linked in; the universal release build of the Rust library succeeds.

To resume: read the execution ledger at
`.superpowers/sdd/2026-10-12-rust-core/progress.md` (git-ignored, on this
machine only). Its last section, "HAND-OFF", lists the next steps in order, the
notes each remaining task needs, and the shared instruction files the
implementer and reviewer agents are given. Execution follows
`superpowers:subagent-driven-development` with Sonnet agents and reviews
batched at the plan's gates.

### Decisions

Made during execution (the design decisions D1–D8 are in the spec):

- Swift's implicit task cancellation is an explicit `CancellationToken`
  parameter on the trait methods that observe it (`ProcessRunner::run` /
  `run_streaming`, `WholesaleDownloader::download`, `LinkResolver::refresh`).
  A cancelled process call returns only after the child has exited.
- URLs that are persisted or forwarded keep their original string
  (`SourceUrl`), including refreshed URLs and the URLs passed to yt-dlp. It
  does not imitate Foundation re-encoding `%20` in strings that also contain
  non-ASCII.
- Release builds keep integer overflow checks on, as Swift does.
- reqwest 0.13 with features `rustls,http2,system-proxy` (plus `stream` in the
  engine): rustls, the OS trust store, no OpenSSL.
- LZFSE inflation uses the pure-Rust `lzfse_rust` crate, checked against a
  real vendored `ffmpeg` blob.

Open questions for review gate C:

- The reqwest transport has no cookie jar; check what `URLSessionTransport`
  does with cookies.
- Whether Foundation's atomic write fsyncs (the Rust state store does not).

### Measurements

_None yet._

## Step 2 — Tauri UI

Rebuild the UI in Tauri on the Rust core, matching the SwiftUI app screen for
screen, to see how it differs in look, feel and resource use.

- [ ] Design spec
- [ ] Touch Bar prototype (the riskiest piece — do it first)
- [ ] Main window: package/item list, toolbar, details panel, graphs
- [ ] Linkgrabber window, format picker
- [ ] Settings, themes
- [ ] Menu bar icon and panel, activation-policy modes, notifications
- [ ] Touch Bar
- [ ] Comparison against the SwiftUI app, results recorded here

Known up front: Liquid Glass is dropped; the menu bar panel is a positioned
window rather than a native popover.

## Step 3 — .NET UI for Windows and Linux (optional)

Undecided. An Avalonia UI over the Rust core through C# bindings.

- [ ] Decide whether to do it
- [ ] Core builds for Windows and Linux (per-platform binaries, clipboard,
      tray, notifications)
- [ ] Design spec

## Environment notes

- The development Mac is Intel (`x86_64`). A universal build needs the
  `aarch64-apple-darwin` Rust target added through `rustup`.
- Two `cargo` binaries are installed: Homebrew's (`/usr/local/bin`) and
  rustup's (`~/.cargo/bin`). Cross-target builds need rustup's.
