# Rust core under the SwiftUI app — design

**Status:** APPROVED 2026-10-12 (design agreed in conversation; step 1 of
`docs/rust-migration-roadmap.md`). Branch `rust-migration`.

## 1. Purpose

An experiment, to learn from and compare. The four `SDMKit` targets are ported
to Rust; the existing SwiftUI app runs on the Rust core through generated
bindings; the two cores are compared for CPU and memory. `main` stays all-Swift
and is the baseline. Later steps (Tauri UI, possibly a .NET UI) reuse this core,
so nothing in it may depend on Apple frameworks except behind a platform
module.

**Done means:**

1. Every Swift test in `SDMKit/Tests` has a Rust counterpart that passes, or a
   written reason it does not apply.
2. The SwiftUI app builds in Xcode (Debug) and through `build.sh` (universal
   Release) against the Rust core, with no Swift engine code linked in.
3. A benchmark runs the same scenarios on both cores and its CPU-time and
   peak-memory results are recorded in the roadmap.

**Not in scope:** any UI change, any behavior change, Windows or Linux builds,
Liquid Glass. Where the Swift code and an older spec disagree, **the Swift code
is the specification** — this is a port, not a redesign.

## 2. Decisions

| # | Decision | Why |
|---|---|---|
| D1 | Compare by branch, no runtime switch between cores | A switch doubles the adapter work |
| D2 | "Performance" = CPU time and peak memory, no network | Agreed with Shayan; the fake origin is in-process |
| D3 | The Swift core stays on the branch at `legacy/SDMKitSwift/`, linked into nothing | The benchmark and the test-parity check need it on the same commit |
| D4 | `reqwest` + `rustls` + the platform certificate verifier replace `URLSession` | Streaming every body chunk back through Swift would defeat the port |
| D5 | The on-disk formats (`state.json`, resume sidecars, managed-binaries manifest) stay byte-compatible in both directions | The experimental app must open the real download list and hand it back undamaged |
| D6 | One Cargo workspace, four library crates mirroring today's targets, plus `sdm-ffi`, `sdm-bench`, `uniffi-bindgen` | Crate boundaries keep the dependency rule `core ← resolve ← engine, grabber` enforced by the compiler |
| D7 | The Swift package keeps its four product names as thin re-export shims | The Xcode project file and the app's `import` lines stay untouched |
| D8 | The generated XCFramework and Swift bindings are build outputs, not committed | They are large and derived |

## 3. Layout

```
sdm-core/                      Cargo workspace
  Cargo.toml
  crates/
    sdm-core/                  domain models, RangeSet, themes — no I/O
    sdm-resolve/               process runner, yt-dlp, site registry, managed binaries
    sdm-engine/                transport, download task, scheduler, persistence, engine
    sdm-grabber/               extraction, probing, verdicts, clustering, session
    sdm-ffi/                   UniFFI surface: staticlib + cdylib, owns the tokio runtime
    sdm-bench/                 benchmark binary
    uniffi-bindgen/            the bindgen binaries UniFFI needs
SDMKit/                        Swift package: binary target + generated bindings + shims
legacy/SDMKitSwift/            the original Swift package, baseline only
scripts/build-rust-core.sh     Rust → static libs → bindings → XCFramework
```

Each Rust source file mirrors one Swift source file (`RangeSet.swift` →
`range_set.rs`), and each Swift test file becomes one Rust test file under the
crate's `tests/` (or a `#[cfg(test)]` module for private-API tests) with the
same test names in snake case. A reader should be able to diff the two cores
file by file.

## 4. Translating Swift concurrency

These rules are what make the port faithful. Every porting task follows them.

**Actors become a struct holding `std::sync::Mutex<State>` behind an `Arc`.**
A Swift actor is reentrant: between two `await`s a method runs atomically, and
at each `await` other calls may interleave. The same holds if the mutex is
locked only for each synchronous stretch and **never held across `.await`**.
`clippy::await_holding_lock` is denied workspace-wide. Each stretch of Swift
code between suspension points becomes one lock scope; the comments in the
Swift source that explain why a section must not suspend carry over verbatim.

A `tokio::sync::Mutex` held across awaits is **not** an acceptable substitute:
`shutdown()` awaits a job that itself needs the engine, and would deadlock.

**`Task { }` becomes `tokio::spawn`; cancellation is cooperative.**
`Task.cancel()` only sets a flag in Swift. It maps to a
`tokio_util::sync::CancellationToken` checked where Swift checks
`Task.isCancelled` / `checkCancellation()` — never `JoinHandle::abort`, which
drops a future mid-write.

**`withTaskGroup` becomes `tokio::task::JoinSet`.**
**`AsyncThrowingStream<Data, Error>` becomes
`Pin<Box<dyn Stream<Item = Result<Bytes, TransportError>> + Send>>`.**
**Protocols injected for testing (`HTTPTransport`, `StateStore`,
`ProcessRunner`, `LinkResolver`, `Muxer`, `WholesaleDownloader`,
`ProbeTransport`, `BinaryFetching`) become `async_trait` traits held as
`Arc<dyn Trait>`.**

**Time stays tick-driven.** No clock is injected and no test sleeps. Tests run
under `#[tokio::test]`; the handful of process-runner timeout tests use
`tokio::time::pause()`.

**`didSet { snapshotGeneration &+= 1 }`** has no Rust equivalent. The engine
state keeps its snapshot inputs private to one module and exposes them only
through accessors that bump the generation on mutable access, so the invariant
"a new snapshot input must bump the generation" is still enforced by structure
rather than by remembering.

## 5. Types and formats

- `UUID` → `uuid::Uuid`; `URL` → `url::Url`; `Data` → `bytes::Bytes` or
  `Vec<u8>`; `Date` → `std::time::SystemTime`; `TimeInterval` → `f64` seconds;
  `Duration` → `std::time::Duration`.
- `isResumable: Bool?` stays `Option<bool>` and the scheduler keeps keying on
  `== Some(false)`.
- Persistence uses `serde` with field names, enum encodings, date encoding and
  optional-field omission matching what Swift's `Codable` writes today (D5).
  Compatibility is proven by fixtures: `state.json` and sidecar files written
  by the Swift core are checked in under the Rust tests, and must load and
  re-save to semantically equal JSON.
- `url::Url` normalizes more eagerly than `Foundation.URL`. Wherever a URL is
  persisted or compared (known-download de-duplication, clustering), the port
  must keep the original string form available; each such place gets a test.
- Files are written at absolute offsets with `FileExt::write_at` behind a
  small `SparseFile` type, as today. Writes stay synchronous inside the
  download task, as in Swift, so the comparison measures like for like.
- Trash uses the `trash` crate. LZFSE inflation uses a pure-Rust decoder,
  proven against a fixture compressed by macOS.
- Themes ship as `include_str!` JSON from the existing resource files.

## 6. The binding surface (`sdm-ffi`)

Domain records and enums derive the UniFFI traits in the crate that defines
them, under their existing names, so the generated Swift types are called what
the app already calls them. `Uuid` and `Url` are UniFFI custom types mapped to
Foundation's `UUID` and `URL`.

`sdm-ffi` exports three objects, each constructed with production dependencies
and exposing the same operations as today's actors:

| Object | Replaces | Notes |
|---|---|---|
| `DownloadEngine` | `SDMEngine.DownloadEngine` | all current public methods; `async` |
| `GrabberSession` | `SDMGrabber.GrabberSession` | all current public methods |
| `ManagedBinaries` | `SDMResolve.ManagedBinaries` + `BinaryLocator` | paths of bundled blobs passed in from the app bundle |

plus the pure functions the views call directly (site registry lookup, format
menu, aggregates helpers, theme catalog, contrast ratio).

Settings the Swift app currently supplies as closures (`cookieSource`,
`maxPlaylistVideos`, `extraArguments`) become setter methods the app calls when
a setting changes; no foreign callbacks are needed in step 1.

**Async:** `sdm-ffi` owns one multi-thread tokio runtime. Exported `async`
functions run on it. The generated async Swift code is known not to be fully
`Sendable`-clean, so the generated sources compile in their own Swift module in
Swift 5 language mode; the hand-written shims and the app stay in Swift 6.

**Snapshots:** `DownloadEngine.snapshot()` returns the whole `EngineSnapshot`
by value, as today. Two additions keep the app's skip-if-unchanged logic
working across a copying boundary:

- `PackageAggregate` and `EngineFigures` are computed in Rust during
  `snapshot()` and carried in the snapshot.
- `EngineSnapshot` carries a `generation`, and each `PackageSnapshot` a
  `revision` that changes only when that package's content changed (the same
  comparison `packageSnapshotCache` does today). The app compares integers
  where it compared storage identity.

Per-item diffs are deliberately not built. If step 1.7's measurement shows the
snapshot copy is a meaningful share of CPU, that becomes a follow-up.

**The Swift side:** `SDMKit/Package.swift` declares a binary target for the
XCFramework, one target holding the generated Swift, and four shim targets —
`SDMCore`, `SDMEngine`, `SDMGrabber`, `SDMResolve` — each re-exporting the
generated module and holding whatever small extensions keep the app source
close to unchanged (`Identifiable`, argument-label overloads, trivial computed
properties). Anything with logic lives in Rust.

## 7. Build

`scripts/build-rust-core.sh [debug|release]`:

1. `cargo build -p sdm-ffi` for the host architecture (debug) or both
   `x86_64-apple-darwin` and `aarch64-apple-darwin` (release), using rustup's
   toolchain explicitly, with `MACOSX_DEPLOYMENT_TARGET=15.0`.
2. `lipo` the release pair into one static library.
3. `uniffi-bindgen-swift` generates the Swift sources, headers and an
   XCFramework-compatible modulemap.
4. `xcodebuild -create-xcframework` writes `SDMKit/Artifacts/SDMRust.xcframework`.

The script skips work when no Rust source changed. It runs from a pre-build
action on the shared `SDM` scheme (pre-actions are not subject to user-script
sandboxing, which stays on for the project) and from `build.sh`. A fresh clone
must run it once before opening the project; `README.md` says so.

`test.sh` runs `cargo test --workspace`, then the Swift shim tests.

## 8. Testing

- The rules do not change: no network, no real clock, time advanced by
  `tick()`. `FakeOrigin`, `FakeProbeOrigin`, the fake wholesale downloader and
  the scripted process runner are ported first, since most tests need them.
- The transport tests replace `URLProtocol` stubs with a server bound to
  `127.0.0.1:0` inside the test process. Loopback is not "the network".
- The two properties that must never regress are ported before the engine is
  called done: byte identity under randomized churn, and `RangeSet` never
  handing one range to two workers.
- **Parity ledger:** `docs/rust-migration/test-parity.md` lists every Swift
  test file with its Swift test count, its Rust test count, and a reason for
  any difference. A part is not complete until its rows are filled in.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt
  --check` are part of every task's verification.

## 9. Benchmark

`sdm-bench` (Rust) and a matching executable target in `legacy/SDMKitSwift`
run identical scenarios against their own in-process fake origin, writing real
files to a temporary directory. Both origins serve bodies by slicing one
pre-generated buffer, so body generation costs the same on each side.

| Scenario | What it stresses |
|---|---|
| `transfer` — 8 items × 256 MB, 8 segments each, run to completion | the hot path: stream → claim → write at offset → checkpoint |
| `ticks` — 1,000 items, 8 running, 600 × (`tick()` + `snapshot()`) | scheduler, telemetry and snapshot building at list scale |
| `ffi-snapshot` (Rust branch only) — Swift calling `snapshot()` on a 1,000-item engine 600 times | the cost of copying the snapshot across the boundary |

Each run is measured from outside with `/usr/bin/time -l` (wall, user + system
CPU, peak resident memory), release builds, five runs, median reported. Results
go in the roadmap under Step 1 → Measurements, with the machine noted.

## 10. Risks

| Risk | Handling |
|---|---|
| Multi-crate UniFFI, XCFramework and Xcode integration is fiddly | Part 1.0 proves the whole pipeline with one record and one async function before anything is ported |
| Generated Swift fails Swift 6 strict concurrency | Generated code compiles in its own Swift 5-mode module |
| A subtle behavior is lost in translation | File-by-file port with the tests ported alongside; parity ledger; batched reviews against the Swift source |
| `url::Url` vs `Foundation.URL` differences change de-duplication or clustering | Explicit tests wherever URLs are compared or persisted (§5) |
| `reqwest` differs from `URLSession` on proxies, redirects, HTTP/2 or timeouts | The transport port lists each `URLSessionTransport` behavior and tests it against the loopback server; system proxy support is checked in the manual smoke test |
| The experimental app damages the real `state.json` | D5's round-trip fixtures; the smoke test runs against a copy first |
| Intel-only dev machine | The Apple Silicon slice is built but can only be verified to link, not run; noted in the roadmap |
