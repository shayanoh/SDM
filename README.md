# SDM — Shayan's Download Manager

A download manager for macOS, Windows and Linux with segmented (multi-connection) transfers, robust resume, clipboard-driven link grabbing, a Chrome extension that captures downloads, and media-site support through yt-dlp.

SDM is a Rust core (`sdm-core/`) under a Tauri v2 + Svelte 5 interface (`sdm-tauri/`). The earlier SwiftUI app is gone from the tree; it lives in git history (see [the roadmap](docs/tauri-roadmap.md) for how the Rust version came about).

## Features

**Downloading**

- Segmented downloading with a configurable number of parallel connections per file (global setting, per-item override)
- Change the segment count mid-download — drop from 100 segments to 3 and the remaining workers pick up the fragmented gaps
- Resume across pauses, quits, and crashes, guarded by `ETag`/`Last-Modified` validation so a changed remote file never silently corrupts a partial one
- Files download as `name.ext.incomplete` and are renamed on completion
- Per-package output folders under a configurable download directory (default: your Downloads folder)

**Link grabbing**

- Optional clipboard watching — copy anything, and URLs are extracted automatically
- Every link is probed for size, availability, filename, and resume support
- Faulty-link detection catches the cases a status code misses: an `.mp4` URL that returns a 2 KB HTML error page, magic bytes that contradict the extension, redirects landing on a login page
- Links are auto-grouped into **packages** by filename similarity, with archive part sets (`.part01.rar`, `.z01`, `.001`) kept together
- Full manual control — drag between packages, merge, split, rename

**Queue**

- Configurable parallel download limit, with the rest queued
- Priority per package and per item; higher priority starts regardless of list position
- Drag to reorder, with immediate effect on what's running
- Non-resumable downloads are never auto-paused, and warn before a manual pause

**Media sites**

- About fifty sites (YouTube, Vimeo, Twitch, TikTok, SoundCloud, Reddit and more) through yt-dlp, which only extracts metadata: SDM downloads the direct URLs itself, through the same segmented engine
- HLS/DASH-only videos are downloaded by yt-dlp itself, resumably
- Quality, resolution, and codec preference list, applied automatically at grab time, with a per-link override
- `yt-dlp`, `ffmpeg`, and a JavaScript runtime are **provisioned automatically** — nothing to install (see [Helper binaries](#helper-binaries))

**Chrome extension**

- Captures Chrome's downloads and hands them to SDM (starting SDM if it is not running)
- Shares Chrome's cookies with SDM for the media sites it supports, so signed-in and age-restricted videos work on every system, without a password prompt. Only the cookies of those sites leave Chrome.
- Loaded unpacked from Settings → Downloads → Chrome Extension (or the macOS app menu); no store listing

**Interface**

- Live download speed per item, per package, and globally, with sparklines and a bandwidth graph
- Progress bars visualize the actual byte ranges, so you can see each segment filling in
- Tray icon with a popover (macOS and Windows) or a status menu (Linux); notifications for finished downloads and links awaiting confirmation
- Themes: system, light, dark, midnight blue, deep purple, and community palettes (Nord, Dracula, Solarized, Gruvbox, Catppuccin, Tokyo Night, One Dark, Rosé Pine)
- Closes to the tray, launches at login, and on macOS runs in the Dock, the menu bar, or both

## Requirements

- macOS 10.13 or later (universal build), Windows (x64), or a recent Linux desktop (x64; `.deb`, `.rpm` and `.AppImage`)
- Nothing else — the media toolchain is bundled or self-downloaded (see below)

## Building

Prerequisites:

- Rust through [rustup](https://rustup.rs). The scripts put `~/.cargo/bin` first on `PATH`, so a Homebrew `cargo` (which cannot cross-compile) is not used. For the macOS universal build also `rustup target add x86_64-apple-darwin aarch64-apple-darwin`.
- Node and [pnpm](https://pnpm.io) for the Svelte frontend.
- Windows builds are cross-built from a Mac with `cargo-xwin`; Linux builds run in Docker (`sdm-tauri/docker/`).

Run the app while developing:

```
cd sdm-tauri && pnpm tauri dev                 # real data directory
cd sdm-tauri/web && pnpm dev                   # the UI in a browser, against an in-memory mock
```

Build and package:

```
scripts/build-tauri.sh mac | windows | linux   # builds, then copies to Releases/<version>/<os>/
scripts/build-tauri.sh mac --publish-only      # copy an existing build without building
```

The macOS app is signed ad-hoc (no Developer ID), so other Macs show the unidentified-developer prompt. `Releases/` is not in git; if `Releases/<version>/<os>` already exists the script warns and copies nothing.

Versioning is one number for every crate and every package:

```
scripts/version.py            # show the version, and whether everything agrees
scripts/version.py 1.2.3      # set it everywhere (workspace, path dependencies, Cargo.lock, tauri.conf.json)
```

## Testing

`./test.sh` runs everything: the Rust workspace tests, the Tauri crate's tests, the Svelte tests, the extension's tests, the version script's tests, and the Windows and Linux compile checks.

To run the parts separately:

```
cd sdm-core && cargo test --workspace --exclude sdm-tauri   # Rust core
cd sdm-core && cargo test -p sdm-tauri                      # the Tauri shell
(cd sdm-tauri/web && pnpm test)                             # Svelte
node --test SDMChromeExtension/cookies.test.js SDMChromeExtension/downloads.test.js              # extension
python3 -m unittest scripts/test_version.py scripts/test_bundle_resources.py   # version script, bundle contents
```

`./lint.sh` runs `cargo clippy --workspace --all-targets -- -D warnings` and the Svelte type check. `./format.sh` runs `cargo fmt --all` and `prettier`.

## Helper binaries

SDM does not ask you to install anything, and does not ship the helpers either. They are downloaded on first launch, on every platform, into the `bin/` folder of SDM's data directory (`~/Library/Application Support/SDM/bin/` on macOS, `%APPDATA%\SDM\bin\` on Windows, `~/.local/share/SDM/bin/` on Linux), each checked against a published SHA-256 before it is installed:

| Binary              | Where from                                                                                                                                                                                                                        | Updates                                                                                                                                                           |
| ------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ffmpeg`, `ffprobe` | Windows and Linux: the BtbN FFmpeg-Builds `latest` release, checked against the digest GitHub publishes. macOS: the static builds of ffmpeg.martin-riedl.de for the machine's CPU, checked against the `.sha256` beside the build | Only when missing                                                                                                                                                 |
| `qjs` (QuickJS-ng)  | The JavaScript runtime yt-dlp needs for full YouTube support, from the quickjs-ng GitHub release                                                                                                                                  | When the release tag changes                                                                                                                                      |
| `yt-dlp`            | GitHub releases                                                                                                                                                                                                                   | Self-checks every 6 h (15 min while missing); auto-updates. Stable or nightly channel, chosen in **Settings → Media Sites → Components**. Never uses `yt-dlp -U`. |

All of this is implemented in `sdm-core/crates/sdm-resolve/src/managed_binaries.rs`; the table of sources is `platform_tools.rs`. They are separate programs with their own licenses (yt-dlp: Unlicense, FFmpeg: GPL, QuickJS-ng: MIT); SDM runs them but does not ship or link them.

## License

SDM is open source under the [MIT License](LICENSE). Copyright © 2026 Shayan Ostadhassan.

## Documentation

- [Roadmap and status](docs/tauri-roadmap.md) — where the Rust/Tauri version stands, the decisions behind it, and a dated log; read this first
- [Design spec](docs/superpowers/specs/2026-08-03-sdm-design.md) — architecture, data model, engine design, testing strategy (written for the original Swift app; the engine design carries over)
- [CLAUDE.md](CLAUDE.md) — working notes for AI agents on this codebase
