# Tauri UI roadmap

Started 2026-10-14. **This is the file to read first in a new session.** It
tracks the Tauri v2 UI for SDM from foundation to full parity, and is updated
as each spec is written, planned and delivered. When a decision changes or a
spec finishes, change this file in the same commit.

Related: [docs/rust-migration-roadmap.md](rust-migration-roadmap.md) covers the
earlier Rust-core migration. Its "Step 2 — Tauri UI" and "Step 3 — .NET UI"
sections are superseded by this document.

## Goal

A Tauri v2 app that has the same layout and features as the SwiftUI app,
running on **Windows first**, then Linux, and on macOS for testing. It sits on
the existing Rust core (`sdm-core/`) in-process. Success is feature parity with
the SwiftUI app, minus the items under "Out of scope".

## Decisions

Settled during brainstorming on 2026-10-14. Do not relitigate without asking.

| #   | Decision                                                                                                                                                                        |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T1  | Tauri v2 with the Rust core in-process. No sidecar, no UniFFI on this path.                                                                                                     |
| T2  | UI fidelity: same layout and features, **Mac-styled** on every OS. Liquid Glass is approximated with translucent blur (Mica/Acrylic on Windows). Window chrome stays OS-native. |
| T3  | The **SwiftUI app stays the macOS app**. Tauri on macOS is a dev/test host and optional alternative.                                                                            |
| T4  | **No Touch Bar** in the Tauri app: not spiked, not ported.                                                                                                                      |
| T5  | The **browser-extension native host is spec 6**. Clipboard link grabbing is not deferred.                                                                                       |
| T6  | App-level controller logic lives in a new Rust crate, `sdm-app`. The frontend renders patches and holds no business logic.                                                      |
| T7  | On Windows and Linux, `ffmpeg`, `ffprobe` and `qjs` are downloaded on first use from the **latest** release (no pinned build, Shayan 2026-10-24) and SHA-256 verified against the digest the release publishes, like `yt-dlp`. macOS bundles the LZFSE blobs (arm64 and x86_64 ffmpeg/ffprobe, universal qjs) as Tauri resources from `SDM/Resources/vendor` (`tauri.macos.conf.json`) and inflates the matching architecture into the shared `bin/` on launch.                       |
| T8  | Frontend: TypeScript, Vite, pnpm, Svelte 5. One `Bridge` interface with a real and a mock implementation, so the UI can be checked in a plain browser with Playwright.          |
| T9  | Windows and Linux builds are made on the Mac: Windows by cross-compiling with `cargo-xwin` and NSIS, Linux inside a Docker container. No separate build machine.                |

## Specs

Each spec gets a design under `docs/superpowers/specs/`, then a plan under
`docs/superpowers/plans/`, then implementation. Status values: _not started_,
_spec draft_, _spec approved_, _plan written_, _in progress_, _done_.

| #   | Spec                    | Contents                                                                                                                                | Status                                                                                                                                                                                                                                                                                                                                                                                                            |
| --- | ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | **Foundation**          | `sdm-app` crate, Tauri shell, patch protocol, a minimal live list, Windows/Linux core portability, build tooling, the performance gate  | built (tasks 1–15 except the Windows VM check; task 16, the ffmpeg/qjs downloads, was done in spec 3) — [design](superpowers/specs/2026-10-14-tauri-foundation-design.md), [plan](superpowers/plans/2026-10-14-tauri-foundation.md), [performance](tauri-performance.md). Windows VM checks 1–4 confirmed by the user 2026-10-23; the Windows CPU/memory measurement (check 5) remains, so not yet marked done |
| 2   | **Main window**         | Package and item list, toolbar, details panel, graphs, drag and drop, selection, context menus, Mica/Acrylic window effects             | built on branch `tauri-ui` (2026-10-24; Windows VM visuals unverified) — [design](superpowers/specs/2026-10-23-tauri-main-window-design.md), [plan](superpowers/plans/2026-10-23-tauri-main-window.md), [performance](tauri-performance.md) |
| 3   | **Link grabber**        | Grabber view (replaces the placeholder), verdicts and clustering display, format picker, playlists, Add Links sheet; plus the ffmpeg/ffprobe/qjs downloads for Windows and Linux (spec 1 task 16) with a minimal tools UI. Deferred to spec 4: all grabber settings and the full managed-binaries tab | built on branch `tauri-ui` (2026-10-24; Windows VM, a real YouTube download and the SwiftUI side-by-side unverified) — [design](superpowers/specs/2026-10-24-tauri-link-grabber-design.md), [plan](superpowers/plans/2026-10-24-tauri-link-grabber.md), [performance](tauri-performance.md)                                                                                                                                                                                                                                                                                                                                                                                                       |
| 4   | **Settings and themes** | All Settings tabs, theme roles as CSS variables, media-sites list, YouTube/managed-binaries tab. **Owed from spec 3:** every link-grabber setting (deep sniff, auto-clear interval, quality preferences, cookie source, playlist cap, extra yt-dlp arguments, probe limits; today constants in `GrabberDefaults`), the full managed-binaries tab (versions, yt-dlp channel, check now, override paths, re-download, progress) and removing `TemporaryTools` (the download-folder field and tools line) | not started                                                                                                                                                                                                                                                                                                                                                                                                       |
| 5   | **System integration**  | Tray icon and panel, notifications, activation (tray-only/taskbar) modes, clipboard watcher, launch at login, widened Tauri permissions | not started                                                                                                                                                                                                                                                                                                                                                                                                       |
| 6   | **Browser extension**   | Native-messaging host and install flow on Windows, Linux and macOS, hand-off channel to the running app                                 | not started                                                                                                                                                                                                                                                                                                                                                                                                       |

Order rationale: spec 1 proves the architecture and the cross-build before any
UI is invested in. Specs 2–4 are the visible UI, in the order a user meets it.
Spec 5 needs the window and settings to exist. Spec 6 is last because it adds a
separate binary and per-OS registration.

### Dependencies between specs

- 2, 3, 4, 5 each depend on 1 (the protocol, the bridge, the shell).
- 3 needs the main window's toolbar entry points from 2.
- 5 adds methods to the `Host` trait that spec 1 defines (clipboard, launch at
  login, open-with) and reads settings from 4.
- 6 needs 5's tray/window activation to show the app when a link arrives.

## Out of scope

- Touch Bar (T4).
- Liquid Glass and the native macOS popover (the menu bar panel is a
  positioned window).
- Mac App Store. Windows code signing is decided before the first distributed
  release and is not part of any spec above.
- Changes to the SwiftUI app or `sdm-ffi`, beyond keeping them building.

## Where things live

```
docs/tauri-roadmap.md                    this file
docs/superpowers/specs/2026-10-14-tauri-foundation-design.md
docs/superpowers/plans/                  one plan per spec, written after approval
sdm-core/crates/sdm-app/                 controller layer (new in spec 1)
sdm-tauri/src-tauri/                     Tauri shell (new in spec 1)
sdm-tauri/web/                           Svelte frontend (new in spec 1)
sdm-core/crates/sdm-ffi/                 UniFFI surface used by SwiftUI; untouched
```

## Environment notes

- The development Mac is **Intel (`x86_64`)**. A Windows VM runs x64 natively,
  so Windows builds target `x86_64-pc-windows-msvc` only.
- `cargo-xwin` and NSIS were **not installed** on 2026-10-14. Docker is
  (`/usr/local/bin/docker`). The first plan task installs them with
  `cargo install --locked cargo-xwin` and `brew install nsis`.
- Two `cargo` binaries exist: Homebrew's (`/usr/local/bin`) and rustup's
  (`~/.cargo/bin`). Cross-target builds need rustup's.
- The SwiftUI app keeps its settings in `UserDefaults`. The Tauri app has its
  own settings file in the data directory; the two apps share the download
  state (`state.json`, sidecars) and nothing else.
- The engine tick rate is `AppTiming::TICKS_PER_SECOND` (currently 5).
- Tauri WebDriver does not support WKWebView on macOS, so the real window
  cannot be automated there. Visual checks run against the `MockBridge` in a
  browser.

## Library facts verified on 2026-10-14

From the current docs through Context7. Re-check before relying on them in a
later session.

- Tauri v2: `tauri::ipc::Channel<T>` streams typed (serde) events to the
  frontend; in TypeScript, `new Channel<T>()` with `onmessage`, passed to
  `invoke`. Commands run on the async runtime when declared `async`.
- `tauri-specta` v2 generates TypeScript for commands, events and `Channel<T>`
  arguments. The docs pin `tauri-specta` and `specta` to `=2.0.0-rc.25` (a
  release candidate) with `specta-typescript` `0.0.12`.
- Svelte 5: class fields declared with `$state` are reactive; `$state` on
  objects is deeply reactive; `SvelteMap` (from `svelte/reactivity`) is the
  reactive map.
- Tauri builds Windows installers from macOS or Linux with
  `--runner cargo-xwin --target x86_64-pc-windows-msvc` (NSIS output). Linux
  bundles should be built in a container or CI on the oldest base system to
  support, because of glibc compatibility.

## Spec 4 next (the facts spec 3 also leaned on)

Facts a later session needs (spec 2 added the main window; its design and plan are linked in the table):

- The controller API is `sdm-core/crates/sdm-app/src/app.rs` (`App`, `Dependencies`); the wire types are
  `protocol.rs`; every f64 reaches TypeScript as `number | null` (a non-finite value becomes `null`) and the
  frontend store reads `null` as zero.
- All patches, including the result of a user action, arrive on the channel registered by `attach`;
  commands return `()`.
- The frontend store is `sdm-tauri/web/src/lib/store.svelte.ts`; bridges are `tauriBridge.ts` and
  `mockBridge.ts`; `bindings.ts` is generated by `cargo test -p sdm-tauri export_bindings` (and by every debug
  start) and is excluded from prettier.
- The controller now also has list-based actions (`start_items`, `stop_items`, `retry_items`, `retry_mux_items`,
  `reset_items`, `set_items_enabled`, `remove_items`), `remove_package`, `move_items`,
  `move_items_to_new_package`, `reorder_items`, `reorder_packages`, `describe_deletion`, `open_item`,
  `reveal_item`, `reveal_package` (tests: `sdm-app/tests/actions_tests.rs`, `deletion_tests.rs`). `Host` gained
  `open_path`. A frontend `Bridge` method exists for each; `MockBridge` follows the engine's rules and holds the
  five-package reference fixture (`?theme=deep-purple`, `?vibrancy=on`, `?simulate=500:50` in a browser).
- The window: `src/components/Shell.svelte` (sidebar, toolbar, `PackageList`, `DetailsPanel`, `BottomBar`, modals,
  `ContextMenu`); pure logic in `src/lib/` (`eligibility`, `selection`, `dnd`, `menu`, `actions`, `shortcuts`,
  `details`, `rowText`, `views`, `format`); state in `ui.svelte.ts`. Reference screenshots of the SwiftUI window are in
  `docs/superpowers/specs/assets/2026-10-23-swiftui-main-window/`; `pnpm exec playwright test parity` regenerates ours
  into `web/test-results/parity/` for comparing by eye.
- Add Links is still `TemporaryTools.svelte` (inside the Linkgrabber placeholder); spec 3 replaces it, spec 4 takes
  the download folder and yt-dlp status.
- Run: `cd sdm-tauri && pnpm tauri dev` (this reads the real data directory), or
  `pnpm --dir web dev` in a browser against the mock. Load harness: a debug build, or a release build with
  `--features simulate,tauri/custom-protocol`, takes `--simulate <items> <active>` (scratch folder, fake origin;
  `SDM_SIMULATE_IDLE=1` pauses everything).
- Build: `scripts/build-tauri.sh mac|windows|linux`; compile gates: `scripts/check-windows.sh`,
  `scripts/check-linux.sh [--shell]`. Windows installers are unsigned.
- The shell crate lives outside the Cargo workspace root, so it declares `workspace = "../../sdm-core"`.

## Log

Newest first. One line per event: date, what happened, where to look.

- 2026-10-24 (spec 3) — Link grabber and platform tools built inline on branch `tauri-ui` ([design](superpowers/specs/2026-10-24-tauri-link-grabber-design.md), [plan](superpowers/plans/2026-10-24-tauri-link-grabber.md)). **Tools (closes spec 1 task 16):** `ManagedBinaries` installs `ffmpeg`/`ffprobe` (BtbN `latest`, only when missing) and `qjs` (quickjs-ng newest release, when the tag changes) on Windows x64 and Linux x86_64 from GitHub release assets, checked against the release's published digest, unpacked with pure-Rust `zip`/`tar`/`lzma-rs`. A first pinned-build version was replaced the same day on Shayan's instruction ("only use latest"). The real Linux download, unpack and run passed in the Docker image (`platform_tools_real_tests`, 109 s). **Controller:** `sdm-app::grabber` (native `GrabberSession`, view on the patch stream as `Patch.grabber`, hand-off, formats, duplicates, auto-clear), 28 new Rust tests; workspace 898 tests pass. **View:** `LinkgrabberView` and parts (header, windowed list, rows, format picker, menus, Add Links sheet, rename prompt, drop to ingest, drag a row onto a package); 334 Svelte tests, 4 Playwright flows against the mock, parity screenshots in `web/test-results/parity/grabber-*.png`. Verified: Rust workspace, Windows and Linux compile of the portable crates, `sdm-app` and the Tauri crate. **Not verified:** the Windows VM (the real tool download, WebView2 drag and drop), a real YouTube link end to end (resolve, mux, 403 refresh), a side-by-side with the SwiftUI grabber (no SwiftUI reference screenshots were taken for this view), the WebView-side cost of a 2,000-row patch. Package hand-off sends every link of a package, offline ones too, as the SwiftUI app does (only a row selection filters to addable rows). Package ids change on every recluster, so the frontend always acts on ids from the latest view. Owed to spec 4: see its row.

- 2026-10-24 (later) — Spec 2 checked by hand and polished. **Windows (user):** the build runs, normal-use CPU is low, drag and menus work; fixed afterwards: the app icon is now the Mac icon on every platform (`tauri icon` from the SwiftUI app's 1024 px icon; `build.rs` now watches `icons/` because tauri-build did not, so the first Windows build kept the old exe icon), and a package header's context menu is the item menu acting on all its items, as in SwiftUI (package-only entries dropped). The sidebar uses the built-in Dark theme (pure black, `#0A84FF` selection) like SwiftUI's Dark; the user kept it as is; there is no theme picker until spec 4. **Linux (user):** `scripts/build-tauri.sh linux` now builds `.deb`, `.rpm` and `.AppImage` in the Docker image (Node and pnpm are installed in it; node_modules, the cargo registry and the pnpm store stay out of the Mac's tree; output in `sdm-core/target/linux/release/bundle/`); the user installed the Linux build and it works. The RPM also installs with `dnf` in a fresh Fedora container with all libraries resolving. Still open: a quiet-machine macOS CPU re-run, the Windows WebView2 CPU table row in [tauri-performance.md](tauri-performance.md), spec 1 task 16.
- 2026-10-24 — Spec 2 (main window) built inline on branch `tauri-ui`: plan tasks 1–21 ([plan](superpowers/plans/2026-10-23-tauri-main-window.md)). Phase A (controller): list actions, organize actions, `describe_deletion`, open/reveal, the folder/assembly/remaining-attempts protocol fields, commands, bridges and a mock that follows the engine. Phase B (window): formatting and menu/selection/drag logic as tested pure modules, the windowed two-height list, sidebar, toolbar, context menus, details panel, delete/pause/new-package modals, drag and drop, shortcuts, window-state persistence, Mica/Acrylic on Windows. Verified: Rust workspace tests, `cargo xwin check` for the Tauri crate, 272 Svelte unit and component tests, Playwright (mock, parity screenshots and real-browser drag). macOS release: idle 0.2 % CPU, 50 active 12.9 to 15.8 % (median of three 14.5 %) on a busy machine: [tauri-performance.md](tauri-performance.md). **Not verified:** the Windows VM (Mica/Acrylic, HTML5 drag inside WebView2, shortcuts, WebView2 CPU), the Linux build (Docker was not running), a quiet-machine CPU re-run. Dropped from SwiftUI on purpose: the breathing icon animations (CPU), the Linkgrabber view (spec 3).
- 2026-10-23 — Spec 1 built on branch `tauri-ui` (plan tasks 1–15; macOS checks done). Verified: Rust workspace tests, Windows and Linux compile checks gated in `test.sh`, Svelte unit tests (22) and Playwright e2e against the mock (3), `scripts/build-tauri.sh mac` (`SDM.app`) and `windows` (`SDM_0.1.0_x64-setup.exe`, unsigned), `scripts/check-linux.sh --shell`. macOS release measurements pass the spec 10 thresholds (idle 0.3 % CPU, 50 active 10.3 % median, 206 MB): [tauri-performance.md](tauri-performance.md); screenshot against the mock: `docs/tauri-skeleton-mock.png`. **Windows VM (user, same day):** installer and stand-alone exe both work; adding a download, pause/resume, and restore after a restart all confirmed. The NSIS installer installs per user under `%LOCALAPPDATA%` (Tauri's default `installMode: currentUser`); the user chose to keep that. **Not done:** the Windows CPU/memory measurement (check 5, needs a build with the `simulate` feature) and a real `pnpm tauri dev` download on the Mac; task 16 (per-platform ffmpeg/ffprobe/qjs downloads) is still open. Spec synced to the five plan deviations.
- 2026-10-14 — Plan for spec 1 written: `docs/superpowers/plans/2026-10-14-tauri-foundation.md` (16 tasks: portability 1–5, `sdm-app` 6–11, shell 12, frontend 13, load harness 14, cross-builds and closing 15, ffmpeg/qjs downloads 16). The portability survey found more Unix-only code than the spec first listed (positional file I/O, process environment, yt-dlp asset name); the spec's section 7 was corrected. Plan deviates from spec in five small ways, listed at the top of the plan; task 15 syncs the spec. Awaiting plan review and the choice of execution method.
- 2026-10-14 — Brainstorming done; decisions T1–T9 settled. Spec 1 drafted:
  `docs/superpowers/specs/2026-10-14-tauri-foundation-design.md`. Core
  portability survey found Unix-only code in `sdm-engine` and `sdm-resolve`
  (see spec section 7). Spec approved by Shayan the same day; next is the implementation plan.
