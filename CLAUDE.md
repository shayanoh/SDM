# SDM — Shayan's Download Manager

A download manager for macOS, Windows and Linux: segmented transfers, resume, clipboard link grabbing, a Chrome extension, and media sites through yt-dlp. **This repo is the Rust version only**: a Rust core under a Tauri v2 + Svelte 5 interface. The earlier SwiftUI app was removed from the tree on 2026-10-26 and lives only in git history.

## Read first

- **[docs/tauri-roadmap.md](docs/tauri-roadmap.md)** — status, decisions, and a dated log of everything built (newest first). Update it in the same commit as any change in behavior.
- **[docs/superpowers/specs/2026-08-03-sdm-design.md](docs/superpowers/specs/2026-08-03-sdm-design.md)** — the engine, scheduler and link-grabber design. Written for the Swift app, but the engine design and behavior carry over; where it says Swift or SwiftUI, read the Rust crate or the Svelte component that replaced it. The specs and plans under `docs/superpowers/` are the record of how each part was designed and built and carry "IMPLEMENTED" / "SUPERSEDED" banners; none is an open backlog.
- **`todo.md`** — what is still open.

## Layout

| Path | What it is |
|---|---|
| `sdm-core/` | Cargo workspace. `sdm-core` domain models and `RangeSet`; `sdm-engine` engine, scheduler, transport, persistence; `sdm-grabber` extraction, probing, verdict rules, clustering, the session; `sdm-resolve` yt-dlp, format selection, site registry, managed binaries; `sdm-app` the controller layer (settings, patch stream, notifications, tray, clipboard, grabber view); `sdm-browser` the Chrome extension's native host, cookie file and install helpers; `sdm-bench` a benchmark tool |
| `sdm-tauri/` | The app: `src-tauri/` (Tauri shell: windows, tray, commands), `web/` (Svelte 5 frontend, one `Bridge` interface with a real and a mock implementation), `docker/` (the Linux build image) |
| `SDMChromeExtension/` | The Chrome extension (MV3, loaded unpacked). Its `manifest.json` version is independent of the app's |
| `Assets/` | Logo and icon sources |
| `scripts/` | `build-tauri.sh` (build and publish to `Releases/`), `version.py` (the one version for every crate and package), `check-windows.sh`, `check-linux.sh` |

## Fixed decisions

Do not relitigate these without asking; they were settled during design.

- **No sandbox, no Mac App Store.** The macOS build is signed ad-hoc (`signingIdentity: "-"`); there is no Developer ID certificate on the build machine.
- **No database.** Atomic JSON snapshot for durable state (`state.json`), a per-download sidecar for resume state, memory only for live progress. The state format is still readable from the Swift app's files, and the data directory is the same one, so existing users keep their downloads.
- **yt-dlp is a metadata extractor** — SDM downloads the direct URLs itself. It is auto-provisioned (not user-installed). The one exception is the HLS/DASH wholesale path (`WholesaleComponentTask` / `YtDlpWholesaleDownloader`), where yt-dlp *is* the downloader because there is no single direct URL to hand the engine.
- **Chrome extension: load-unpacked only, no Web Store.** Chrome cookies reach yt-dlp through the extension's site-filtered `cookies.txt`, never by reading Chrome's own database. Several Chrome profiles: the last sync wins.
- **Tauri decisions T1–T9** are in the roadmap.

## The one idea everything rests on

**A download's progress is a `RangeSet` of completed byte ranges. Segments are ephemeral workers, not stored structure.** Workers claim the largest remaining gap, write at absolute offsets, and report intervals back.

Everything else falls out of this: changing the segment count mid-flight, resuming after a crash, and the segmented progress bar are all the same mechanism rather than three features. If a change starts requiring segments to be persistent objects, that's a signal the change is wrong.

## Other load-bearing invariants

- **Scheduling is a pure function** (`sdm-engine/src/scheduler.rs`, `desired_running_set`) re-evaluated on every change, not a queue that's kept in sync.
- **Running non-resumable items reserve their slots before rank-based filling.** Skipping this produces unsatisfiable desired sets and thrash. `is_resumable` is three-state (`Option<bool>`): `None` means not yet probed, and the scheduler keys on `== Some(false)`, so an unprobed item stays preemptible. Collapsing unknown into "not resumable" makes every running item unpreemptible and silently kills preemption — that bug shipped once already.
- **Verdict rules and package clustering are pure functions** over value types, with fixture tables. Tune the data, not the control flow. The same goes for `SiteRegistry`: each site lists its `cookie_domains` as data, and a test fails if a site lacks one.
- **The frontend never touches the engine.** It consumes a coalesced patch stream from `sdm-app` (`attach`, `Patch`), driven by the engine heartbeat (`AppTiming::TICKS_PER_SECOND`). A second consumer must not call `attach` again (the app keeps one patch sink): the tray popover listens to the `tray-status` event instead.
- **Colors come from theme roles, never literals** (`var(--surface-primary)` and friends, supplied by `theme_css`).
- **Windows are created hidden and shown by their page** (`window_ready`) once the theme is painted and the size fitted, so nothing flashes white or unstyled. A new window must follow that.
- **The Tauri binary is also the Chrome native host** (Chrome starts it with `chrome-extension://…` as its first argument and `main` branches on that before Tauri starts).
- **Translation rules from the Swift era still apply to the Rust core:** state behind a mutex gets one lock scope per stretch that has no `await` and never a lock held across `.await`; cancellation is an explicit token; time is driven by `tick()` and there is no clock.

## Testing

Rust: `cargo test` in `sdm-core/` (per crate, or `--workspace --exclude sdm-tauri`; the whole workspace is slow). Frontend: `pnpm test` (vitest) and `pnpm check` (the real type check), run inside `sdm-tauri/web/` (pnpm only switches to the project's pinned version from inside the project). Extension: `node --test SDMChromeExtension/cookies.test.js SDMChromeExtension/downloads.test.js`. Version script and bundle contents: `python3 -m unittest scripts/test_version.py scripts/test_bundle_resources.py`. `./test.sh` runs all of it plus the Windows and Linux compile checks.

The engine takes `HTTPTransport` as an injected trait — tests run against `FakeOrigin`, an in-process fake server programmable to misbehave (ignores `Range`, lies about `Content-Length`, drops or truncates the body, changes its `ETag`). **No test may touch the network or sleep on a real clock.**

There is **no injected clock.** Time-dependent behavior is tick-driven from outside: `DownloadEngine::tick()` is called by the app and drives telemetry, rescheduling, checkpoint staleness, retry backoff, and the debounced state flush. Tests advance time by calling `tick()` directly, which pins "fires on tick N and not before" far more precisely than a fake clock would. Keep new time-dependent behavior on the tick rather than reaching for a clock.

The two properties that must never regress:

1. Byte identity under randomized churn (segment-count changes, pauses, kills, drops) — output must hash-match the source (`sdm-engine/tests/churn_tests.rs`).
2. `RangeSet` never hands the same range to two workers, and the union of completed ranges equals what was written.

## Conventions

- Format Rust with `cargo fmt`, TypeScript and Svelte with `prettier` (`./format.sh`); `./lint.sh` runs clippy and the Svelte check.
- Add packages with the package manager (`cargo add`, `pnpm add`), never by hand-editing manifests or lockfiles. Versions only through `scripts/version.py`.
- A change to the Chrome extension that is incompatible with older copies gets a major version bump in its `manifest.json` (2.0.0 was the cookie sync); the app's launch prompt compares it with the version the extension reports.
- `ffmpeg`/`ffprobe`/`qjs` are downloaded at run time on every platform and never bundled (`platform_tools.rs`); a Chrome extension file that is not listed in `bundle.resources` of `tauri.conf.json` is not shipped (`scripts/test_bundle_resources.py` checks).
- The project is MIT-licensed; keep `LICENSE` and the metadata in `tauri.conf.json` in step when anything about the licence or the downloaded tools changes. `sdm-tauri/src-tauri/licenses.txt` (the licenses of every compiled-in Rust crate and web library, shipped in every package) is generated: after adding or updating a dependency run `python3 scripts/generate_licenses.py` (needs `cargo install --locked cargo-about --features cli`) and commit the result; a dependency with a license outside `accepted` in `sdm-core/about.toml` fails the run.
- Verify library APIs against current documentation (Context7) rather than from memory.

## Git

Shayan has granted blanket consent for **committing** and for **fast-forward pushes** in this repo — no need to ask each time. **Force-pushing still requires explicit authorization.**
