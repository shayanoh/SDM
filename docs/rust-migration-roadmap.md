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
- [x] 1.5 Binding surface and Swift adapter: the app's controllers run on the
      Rust core; Swift `SDMKit` sources removed from the branch (the Swift core
      is kept, frozen, under `legacy/SDMKitSwift`)
- [x] 1.6 App builds in Xcode (Debug and universal Release). The builds are
      done; the manual smoke test is still to be done by Shayan (see "Left for
      Shayan" below) and is not ticked.
- [x] 1.7 Performance comparison against `main`, results recorded below

### Status and how to resume (updated 2026-10-13)

All 27 plan tasks are implemented, committed and reviewed through review gate
F. The Rust core is ported (all 557 Swift tests have Rust twins; the Rust
workspace has 724 tests, all passing, and the Swift bindings and shims have 37
more). The SwiftUI app runs on the Rust core through generated UniFFI
bindings. The Debug and universal Release builds succeed. Nobody has launched
the built app: the app was not run during any task.

What remains is the final whole-branch review (one reviewer reading the full
diff against `main`) and Shayan's manual smoke test, described under "Left for
Shayan". The test counts and how they were taken are in
`docs/rust-migration/test-parity.md`; the benchmark numbers are in
`docs/rust-migration/benchmarks.md`.

To resume: read the execution ledger at
`.superpowers/sdd/2026-10-12-rust-core/progress.md` (git-ignored, on this
machine only). Its last section, "HAND-OFF", lists the steps and the shared
instruction files the implementer and reviewer agents are given. Execution
followed `superpowers:subagent-driven-development` with Sonnet agents and
reviews batched at the plan's gates.

### Decisions

The design decisions, from `docs/superpowers/specs/2026-10-12-rust-core-design.md`:

- D1: the two cores are compared by branch; there is no runtime switch between them.
- D2: "performance" means CPU time and peak memory, with no network; the fake origin runs in-process.
- D3: the Swift core stays on the branch at `legacy/SDMKitSwift/`, linked into nothing, so the benchmark and the test-parity check have it on the same commit.
- D4: `reqwest` with `rustls` and the platform certificate verifier replaces `URLSession`.
- D5: the on-disk formats (`state.json`, resume sidecars, managed-binaries manifest) stay byte-compatible in both directions.
- D6: one Cargo workspace with four library crates mirroring the Swift targets, plus `sdm-ffi`, `sdm-bench` and `uniffi-bindgen`.
- D7: the Swift package keeps its four product names as thin shims, so the Xcode project file and the app's `import` lines are untouched.
- D8: the generated XCFramework and Swift bindings are build outputs and are not committed.

Made during execution:

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
"already exists" failures (gate D), and the probe slot leak above. The final
review added three more, all also in the Swift core:

- A worker whose claim starts past byte 0 accepts any 2xx answer to its ranged
  request. An origin that advertises `Accept-Ranges` but answers with `200` and
  the whole body produces a file of the right size and the wrong content, with
  no error. (The Rust core now refuses it; see the differences list.)
- `..` in a server-declared filename (`Content-Disposition`) or in a package
  name leaves the download folder, and removing or resetting that item then
  moves what is at the escaped path to the Trash.
- Moving a wholesale (HLS/DASH) item to another package leaves its yt-dlp
  scratch files behind in the old folder.

Settled at review gate F and after (the binding surface, the Swift adapter and
the build):

- The types that cross the boundary are declared with `#[uniffi::remote]` in
  `sdm-ffi`, so the library crates stay free of UniFFI. A declaration that
  drifts from the real type is a compile error, so keeping them in step costs
  maintenance, not correctness.
- Every exported function returns an error instead of trapping. A Rust panic
  reaches Swift as `CoreError.panicked`, and the engine keeps answering later
  calls. The engine and grabber locks recover from poisoning.
- A URL the Swift side cannot parse arrives in Rust as a placeholder, and any
  `about:` URL is refused on the way back in, so an unusable URL cannot become
  an `about:invalid` item.
- Pasted text is extracted by the Rust extractor and ingested as Swift `URL`s,
  so the core holds Foundation's spelling of each link from the start and
  deduplication against existing downloads compares like with like. One known
  gap: a raw playlist-entry URL that yt-dlp returns containing a space,
  non-ASCII characters or brackets can miss the "already downloaded"
  comparison. This is low in likelihood and was not fixed.
- The yt-dlp settings (cookie source, playlist cap, extra arguments) are passed
  at construction to both the engine and the grabber, which each hold their own
  copy. The Settings code must update both when a setting changes.
- The shims hold no logic except three per-row figures, `isRetrying`,
  `fractionCompleted` and `RangeSet.totalBytes`. They are computed in Swift for
  speed, because making each an FFI call lowered the whole item snapshot for
  every row on every tick. A shim test pins them to the core's answers over a
  table of cases.
- Only `SystemConfiguration` is linked for the Rust library; the other
  frameworks the skeleton linked were not needed.
- `build.sh --no-install` builds without touching `/Applications`.

### Where the Rust core deliberately differs from the Swift core

One place for all of them; the detailed entries above say why.

- Pausing an HLS/DASH (wholesale) download keeps its partial files and progress
  (gate C).
- A download whose last byte lands as it is stopped or preempted finishes as
  Completed (gate D).
- A stop that interrupts the mux step is resumable: the next start goes
  straight to muxing (gate D).
- Removing or resetting an item hides it from the scheduler until its files
  are dealt with, through an explicit guard (gate D).
- A link must start with `http://` or `https://`; scheme-less links are not
  picked up (Shayan's decision of 2026-10-13, "Made while porting the
  grabber").
- An HTTP header value is decoded as UTF-8 when it is valid UTF-8, otherwise as
  Latin-1 (gate E).
- Grabber calls finish their work even if the caller goes away (gate E).
- A filename, a part filename or a package name is reduced to a single safe
  path component (text after the last `/`; empty, `.` and `..` become
  `download`) wherever it enters and wherever it is joined onto a folder, so a
  link, a server header or a renamed package cannot place a file outside the
  download folder. The last path component of a URL is taken from the decoded
  path, as Foundation does, so `%2F` in a URL no longer yields a name with
  separators. Ordinary names are unchanged. The Swift core reduces none of
  this (final review, FR-1).
- A worker whose claim starts past byte 0 requires `206`. A `200` there fails
  the attempt before any byte is written (error "the server advertised byte
  ranges but answered a ranged request at offset N with HTTP 200 instead of
  206"), and the retry policy treats it as transient. The Swift core writes
  the body at the wrong offset (final review, FR-5). The Rust core does not
  restart such a download as non-resumable; an origin that keeps doing this
  runs out of retries and fails.
- Stopping, removing or quitting while a download is still in its first (probe)
  request abandons that request at once, as the Swift core does (final review,
  FR-3; this was a regression in the port, fixed).
- The engine has no default Trash: without `with_trash` it refuses to move
  anything, and only the `sdm-ffi` production constructor passes the system
  Trash, so no test can reach the real one (final review, FR-2).

### Other differences a user or operator may notice

- Transport (`reqwest_transport.rs`): PAC proxy scripts are not evaluated
  (system proxy settings are read, PAC scripts are not); no HTTP/3 (h2 or
  HTTP/1.1 by ALPN); the `User-Agent` is `SDM`, not CFNetwork's; no
  `Accept-Encoding` is sent (the origin serves the identity encoding) and no
  `Accept-Language` is set.
- The failure reason shown after retries run out is the `reqwest` error chain,
  not Foundation's localized `URLError` text.
- While a running item's data file is being flushed to disk (about once a
  minute per item), every engine call (stop, add, tick) waits for the flush;
  in the Swift core other calls could interleave.
- The link grabber's snapshot is read on the main thread four times a second
  while its tab is open, and the whole snapshot is converted each time. Not
  fixed (final review, FR-6); a few hundred links should cost a few
  milliseconds per read, thousands tens of milliseconds (estimated from the
  engine benchmark, not measured).
- Idle and per-tick CPU through the bindings grew with the length of the list
  because the whole snapshot crossed the boundary every tick. The heartbeat now
  calls `snapshot_if_changed`, which returns nothing when the generation is
  unchanged (see Measurements). While anything is running, every tick still
  crosses in full.

### Measurements

Full tables, every run, what each scenario does and the caveats are in
[`docs/rust-migration/benchmarks.md`](rust-migration/benchmarks.md). Machine:
MacBookPro14,3, Intel Core i7-7820HQ at 2.90 GHz, 8 logical cores, 16 GB
memory, macOS 15.7.7; release builds, 5 runs per cell, the two cores
alternating, medians below, measured from outside with `/usr/bin/time -l`.

Commit d181a54, second full run (the first run's `transfer` spread was over
15 percent, so the script was run twice; the first run's medians are in the
benchmarks file and agree within the spread). `transfer` and both cores now
tick five times a second from the wall clock.

| Scenario | Core | Wall (s) | CPU user+sys (s) | Peak RSS (MB) | Ticks |
|---|---|---:|---:|---:|---:|
| transfer (8 items, 2 GiB) | Rust | 11.08 | 7.17 | 265 | 3 |
| transfer (8 items, 2 GiB) | Swift | 15.47 | 11.67 | 519 | 18 |
| ticks (1,000 items, 600 ticks) | Rust | 2.45 | 2.41 | 30 | 600 |
| ticks (1,000 items, 600 ticks) | Swift | 2.70 | 2.65 | 13 | 600 |
| ffi-snapshot (1,000 items, 600 ticks, through the bindings) | Rust | 15.05 | 15.02 | 23 | 600 |
| ffi-snapshot-if-changed (same, `snapshotIfChanged`) | Rust | 1.17 | 1.18 | 23 | 600 |

`transfer` does not isolate the transfer. The wall time includes building the
256 MB payload, checking the output and deleting 2 GiB, and the tick count
shows the wait for the items lasted only about half a second (Rust) or about
four seconds (Swift). That duration is inferred from the tick count, not
printed by the programs. The wall-time spread was 11.2 percent (Rust) and 26.9
percent (Swift). The `transfer` figures also include the two fake origins (the
Swift fake copies every chunk, the Rust fake slices a shared buffer), which
affects peak memory and some CPU. Neither a faster nor a slower core can be
read from this row; see `docs/rust-migration/benchmarks.md`, "What the
`transfer` tick count and wall time mean". Earlier figures for `transfer`
(tick as fast as the loop spins) are withdrawn.

The `ffi-snapshot` figure is about 25 ms per tick plus snapshot through the
bindings with 1,000 items: the whole snapshot is serialised across the
boundary on every tick, even when nothing changed. With `snapshotIfChanged`,
as the app's heartbeat now calls it, the same 600 idle ticks take 1.17 s
(about 2 ms per tick), 13 times less. While items are running, every tick
still returns a changed snapshot in full. Neither `ffi-snapshot` scenario has
a Swift-core counterpart, and they are not comparable to `ticks`.

Release app sizes: the Rust-core bundle is 171 MB with a 53.8 MB universal main
binary (53,818,256 bytes); the all-Swift app is 133 MB with a 14.8 MB universal
main binary (14,751,824 bytes).

### Final review: findings fixed and minors left as they are

Fixed after the final whole-branch review: FR-1 (filenames and package names
stay inside the download folder), FR-2 (the engine no longer defaults to the
real Trash), FR-3 (stop, remove and quit abandon a first request), FR-5 (a
ranged request answered with `200` fails instead of corrupting the file), FR-4
(the transfer benchmarks tick at the app's rate and print their tick count) and
the per-tick snapshot cost (`snapshot_if_changed`). Not fixed: FR-6 to FR-15
(minors, listed in the review). The 22 deferred minors the reviewer judged fine
to leave:

1. A panicking component leaves the item Running with a dead runner until
   relaunch.
2. File and trash work under the engine mutex in `move_items`; blocking
   `fsync`, `pwrite` and sidecar writes under a std mutex on runtime threads.
3. Atomic-write temp files are not swept after a crash.
4. The churn test's "kill" is a dropped future that still flushes; no crash
   between a write and its checkpoint is simulated.
5. No test for a mid-download 403 (stored refreshed URL, refresh cap, resume
   after refresh).
6. `retry_mux` has no re-entrancy guard and can outlive shutdown.
7. A refresh interrupted by shutdown lands through `failure_state`; a sibling
   mid-refresh is not stopped when another component fails.
8. Moving a wholesale item to another package leaves its yt-dlp scratch files
   behind (also listed as a Swift-core bug above).
9. Two ported assertions accept Queued or Running where Swift pins Queued.
10. Legacy-shape decoding leniencies in `RawItem`.
11. The stale check in `build-rust-core.sh` ignores deleted or renamed sources.
12. Transport differences not tested: redirect cap, `Range` across origins,
    read timeout; `Accept-Language` was missing from the differences list
    (now listed).
13. `SourceUrl::new` keeps surrounding whitespace; no test that playlist entry
    URLs keep their case.
14. Grabber URL and filename details (host case and brackets, extension rules,
    CJK digits, final sigma, `https://a.com` naming), apart from path handling,
    which was FR-1.
15. Leftover split lock scopes in the grabber session.
16. Gate E2 minors M1 to M8 and gate F2 minors M1 to M11 (not read one by one
    by the reviewer).
17. `Duration` lowering traps on a negative or NaN value; `UInt32(...)` of a
    hand-edited setting can trap in Swift.
18. One unparseable extracted URL fails the whole `ingest(text:)` batch; a
    Swift-side filter of `about:` URLs would fix it.
19. Synchronous FFI calls on hot paths: `packageAggregateForItems` on each
    structural publish, `averageRecentSpeed` and `estimatedSecondsRemaining`
    per running row, `effectiveFilename` per link row render.
20. Full-snapshot cost per tick while items run (idle cost is fixed by
    `snapshot_if_changed`); per-package deltas would be the next step.
21. `PackageSnapshot` and `EngineSnapshot` `PartialEq` ignore aggregate,
    revision, figures and generation: a trap for a later skip-if-equal.
22. Timing-sensitive tests: the 3 s elapsed bound in the process runner tests,
    and `removal_tests.rs` lines 241 to 308 if the gate D fix did not gate
    them.

### Left for Shayan

The manual smoke test of the built app. Nothing in the plan launched the app,
so none of the items below has been checked.

Build without installing: `./build.sh --no-bump --no-install` from the repo
root. The Release app lands at `build/Build/Products/Release/SDM.app` (the
script's `DERIVED_DATA_PATH` is `./build`). `--no-install` stops before
anything under `/Applications` is touched, so the installed all-Swift app is
left alone.

Checklist, merged from the gate F review and the Task 24 report:

1. Point the app at a copy of your data. The app has no override for this: it
   takes `~/Library/Application Support/SDM` from
   `FileManager.urls(for: .applicationSupportDirectory)` (`SDM/EngineController.swift`
   and `SDM/ManagedBinariesController.swift`), with no environment variable or
   setting. Whether launching with a different `HOME` redirects it was not
   tested. The safe way is to back up `~/Library/Application Support/SDM` first
   (copy the whole folder, at least `state.json`), and to run the build
   against the real folder only after that. Quit the all-Swift app first so
   the two never write the same files.
2. Launch with your real `state.json`, including a multi-component item and an
   HLS/DASH item: the list, the Completed tab and the package headers (speed,
   ETA, bytes) should look as in the Swift build; the Completed header sums
   only completed items. Compare with the Swift build side by side while a
   download is running.
3. Corrupt state: quit, then replace `state.json` in turn with the text
   `garbage`, with `{}` and with an empty file. The app should launch with an
   empty list and no crash (no banner either, as in Swift). Restore the backup.
4. An old flat-shape `state.json` (version 1, items with `url` and `filename`)
   loads, and after a quit and relaunch it still loads in the Swift baseline.
5. Hand-edit a stored setting to 0 (for example `maxConcurrent` with
   `defaults write`). Launch is expected to die, as in Swift; note it, then
   reset the setting.
6. Download an HTTP file: progress, the segmented bar, speeds, pause, resume,
   stop, retry, reset, reorder by drag, move to a new package, remove with and
   without deleting the file (to the Trash).
7. Per-tick behavior: CPU while idle and while downloading, scrolling, a drag
   reorder that ticks must not interrupt (try it with two other items
   downloading), the menu bar ring and Touch Bar updating, the details panel
   graph with its full history. Also open the details panel with a
   multi-selection of 100 or more items while downloading, and again with one
   item selected, and watch CPU and responsiveness.
8. Perform two actions quickly (pause all, then remove a package) while
   downloading and watch for a one-tick flicker back to an older state.
9. Close the main window with downloads running (menu bar mode) and reopen it:
   rows should show current figures at once, and the menu bar ring and Touch
   Bar should have kept updating while it was closed.
10. Notifications: download finished, package finished and failed each fire
    once, and are not repeated after a relaunch or a package reorder. The
    "yt-dlp updated" banner also appears.
11. Link grabber: paste or clipboard-grab links, check verdict chips and counts,
    recheck, cancel checks, merge, split and rename packages, hand off to
    downloads, auto-add-and-start, and auto-clear after the interval. Paste
    500 or more URLs and watch CPU while probes run; select all and remove.
12. Non-ASCII and space URLs (for example `https://example.com/ü file.zip`):
    grab it, add it to downloads, grab it again and see whether it is badged as
    a duplicate; confirm no `about:invalid` item appears.
13. YouTube and other-site rows: resolve a link, open the format picker, and
    check wholesale HLS/DASH download and its resume (partial files are kept on
    pause), muxing with `ffmpeg`, and a 403 URL refresh on a YouTube item
    mid-download.
14. Settings: change the cookie source and the playlist cap, press OK, and
    resolve a YouTube playlist without relaunching; confirm both apply (the
    yt-dlp command line in a debug log shows them). Change the quality
    preferences, press OK, and open a format picker without relaunching. Press
    Cancel and confirm nothing changed.
15. Settings: switch the yt-dlp channel from stable to nightly and back; the
    "Components" section updates and a banner appears within about a second.
16. First launch with an empty `bin/` (move it aside): the bundled
    `ffmpeg`/`ffprobe`/`qjs` inflate, yt-dlp downloads, and the link grabber
    then offers muxed formats without a relaunch.
17. Quit with a download running and the details panel open: on relaunch the
    progress is restored and no `.incomplete` files are left behind.
18. Remove a package with "delete files" to the Trash, and again with the
    folder read-only: one banner, no crash, and the list stays correct.
19. Behind a proxy or VPN, or against a host with a self-signed or
    corporate-trusted certificate: a download and a link probe both succeed
    (the Rust core uses `reqwest`, not `URLSession`).
20. Launch time: note the time to the first window (three HTTP clients and the
    runtime are built on the main thread).
21. Options on launch: the "auto-start downloads on launch" setting works.

Open questions for Shayan:

- Whether to fix the Swift-core bugs found along the way on `main`, or to
  record them in `todo.md` there. They are the wholesale pause sweep, the
  stop-at-completion and interrupted-mux "already exists" failures, and the
  probe slot leak.
- Whether to do the snapshot-cost follow-up (an export that returns nothing
  when the snapshot generation is unchanged, possibly with per-package revision
  deltas) before judging the Rust core's UI cost.

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
