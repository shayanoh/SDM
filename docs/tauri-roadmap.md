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
| T7  | On Windows and Linux, `ffmpeg`, `ffprobe` and `qjs` are downloaded and SHA-256 verified on first use, like `yt-dlp`. macOS keeps its bundled LZFSE blobs.                       |
| T8  | Frontend: TypeScript, Vite, pnpm, Svelte 5. One `Bridge` interface with a real and a mock implementation, so the UI can be checked in a plain browser with Playwright.          |
| T9  | Windows and Linux builds are made on the Mac: Windows by cross-compiling with `cargo-xwin` and NSIS, Linux inside a Docker container. No separate build machine.                |

## Specs

Each spec gets a design under `docs/superpowers/specs/`, then a plan under
`docs/superpowers/plans/`, then implementation. Status values: _not started_,
_spec draft_, _spec approved_, _plan written_, _in progress_, _done_.

| #   | Spec                    | Contents                                                                                                                                | Status                                                                         |
| --- | ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| 1   | **Foundation**          | `sdm-app` crate, Tauri shell, patch protocol, a minimal live list, Windows/Linux core portability, build tooling, the performance gate  | spec draft — [design](superpowers/specs/2026-10-14-tauri-foundation-design.md) |
| 2   | **Main window**         | Package and item list, toolbar, details panel, graphs, drag and drop, selection, context menus, Mica/Acrylic window effects             | not started                                                                    |
| 3   | **Link grabber**        | Grabber window, link verdicts and clustering display, format picker, playlists, Add Links sheet                                         | not started                                                                    |
| 4   | **Settings and themes** | All Settings tabs, theme roles as CSS variables, media-sites list, YouTube/managed-binaries tab                                         | not started                                                                    |
| 5   | **System integration**  | Tray icon and panel, notifications, activation (tray-only/taskbar) modes, clipboard watcher, launch at login, widened Tauri permissions | not started                                                                    |
| 6   | **Browser extension**   | Native-messaging host and install flow on Windows, Linux and macOS, hand-off channel to the running app                                 | not started                                                                    |

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

## Log

Newest first. One line per event: date, what happened, where to look.

- 2026-10-14 — Brainstorming done; decisions T1–T9 settled. Spec 1 drafted:
  `docs/superpowers/specs/2026-10-14-tauri-foundation-design.md`. Core
  portability survey found Unix-only code in `sdm-engine` and `sdm-resolve`
  (see spec section 7). Awaiting spec review.
