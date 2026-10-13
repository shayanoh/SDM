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
- [x] 1.3 `SDMEngine` port: transport, `FakeOrigin`, download task, scheduler,
      persistence, engine, muxing, wholesale task — with tests
- [x] 1.4 `SDMGrabber` port: extraction, probing, verdicts, clustering, session
      — with tests
- [ ] 1.5 Binding surface and Swift adapter: the app's controllers run on the
      Rust core; Swift `SDMKit` sources removed from the branch
- [ ] 1.6 App builds in Xcode (Debug and universal Release), manual smoke test
- [ ] 1.7 Performance comparison against `main`, results recorded below

### Status and how to resume (updated 2026-10-13)

Plan Tasks 1–22 of 27 are implemented, reviewed and committed: all four Swift
targets are ported (557 Swift tests accounted for; 663 Rust tests). Review
gate E (Tasks 20–22, the grabber) is done and its fixes end at `94120b5`.
**Next: Task 23, the UniFFI surface**, then Tasks 24–25 (Swift adapter, build)
and review gate F.

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

Settled at review gate C:

- The reqwest transport keeps an in-memory cookie store per transport, as the
  Swift transport's ephemeral `URLSession` does.
- Atomic writes (state file, resume sidecar) `fsync` the temp file before the
  rename, as Foundation's `.atomic` write does (plain `fsync`, not
  `F_FULLFSYNC`).
- A network failure keeps its cause (`TransportError::Network`), so the
  message shown after retries run out says what failed. Retry classification
  is the same in both cores.
- **Deliberate difference from the Swift core:** pausing an HLS/DASH
  (wholesale) download keeps its partial files and progress in Rust. The Swift
  core intends the same but sweeps them: `YtDlpWholesaleDownloader` throws
  `WholesaleError.cancelled` while `WholesaleComponentTask` only treats
  `CancellationError` as a pause. That is a bug on `main`, not fixed here.

Settled at review gate D (all three are deliberate differences from the Swift
core, which has the bug in each case):

- A download whose last byte lands just as it is stopped or preempted finishes
  as Completed. In Swift it stays Stopped next to an already-finished file and
  the next start fails with "A file already exists at the destination".
- A stop that interrupts the mux step is resumable: the next start goes
  straight to muxing. In Swift it re-downloads the first part and then fails
  the same way.
- Removing or resetting an item hides it from the scheduler until its files
  are dealt with. Swift's actor made that sequence atomic; the Rust engine
  needs an explicit guard.

Made while porting the grabber:

- Link extraction uses the `linkify` crate in place of `NSDataDetector`.
  **Deliberate difference from the Swift core (Shayan, 2026-10-13):** a link
  must start with `http://` or `https://`; a scheme-less `example.com/file.zip`
  is not picked up, so dotted words with slashes are never grabbed. Trimming of
  trailing punctuation may differ in rare cases.

Settled at review gate E:

- **Deliberate difference from the Swift core:** an HTTP header value is
  decoded as UTF-8 when it is valid UTF-8, otherwise as Latin-1. Foundation
  always decodes as Latin-1, so a raw-UTF-8 `Content-Disposition` filename is
  mojibake in the Swift app and correct here. Side effect: a non-ASCII `ETag`
  saved by the Swift app will not match what the Rust engine reads, which
  restarts that one download.
- The grabber session's calls finish their work even if the caller goes away
  (as a Swift task group does); a dropped call cannot leave rows stuck in
  "Probing".
- Kept as in Swift: a probe whose link is removed while it is in flight holds
  its per-host slot for the rest of that ingest, so that host's remaining
  probes can stay queued until a recheck. Rare; a candidate for `todo.md` on
  `main`.

Bugs found in the Swift core along the way (not fixed on `main`): the
wholesale pause sweep (gate C), the stop-at-completion and interrupted-mux
"already exists" failures and the unguarded removal window does not apply
(gate D), and the probe slot leak above.

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
