# SDM — Shayan's Download Manager

A macOS download manager with segmented (multi-connection) transfers, robust resume, clipboard-driven link grabbing, and YouTube support.

> **Status: implemented.** All five design phases are merged; the YouTube
> toolchain (yt-dlp / ffmpeg / QuickJS) is now provisioned automatically. See
> [the design spec](docs/superpowers/specs/2026-08-03-sdm-design.md).

## Features

**Downloading**

- Segmented downloading with a configurable number of parallel connections per file (global setting, per-item override)
- Change the segment count mid-download — drop from 100 segments to 3 and the remaining workers pick up the fragmented gaps
- Resume across pauses, quits, and crashes, guarded by `ETag`/`Last-Modified` validation so a changed remote file never silently corrupts a partial one
- Files download as `name.ext.incomplete` and are renamed on completion
- Per-package output folders under a configurable download directory (default `~/Downloads`)

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

**YouTube**

- Quality, resolution, and codec preference list, applied automatically at grab time
- Per-link override, with filesize updating instantly
- Downloads through the same segmented engine as everything else
- `yt-dlp`, `ffmpeg`, and a JavaScript runtime are **provisioned automatically** — no `brew install` needed (see [Vendored binaries](#vendored-binaries)); generic HTTP downloads work regardless

**Interface**

- Live download speed per item, per package, and globally, with sparklines and a bandwidth graph
- Progress bars visualize the actual byte ranges, so you can see each segment filling in
- Menu bar item with active downloads, pending links, and quick controls
- Notifications for finished downloads and links awaiting confirmation
- Themes: system, light, dark, midnight blue, deep purple, and community palettes (Nord, Dracula, Solarized, Gruvbox, Catppuccin, Tokyo Night, One Dark, Rosé Pine)
- Runs in the dock, the menu bar, or both — your choice

## Requirements

- macOS 15.0 or later (macOS 26 additionally gets Liquid Glass styling)
- Nothing else — the YouTube toolchain is bundled or self-downloaded (see below)

## Building

SDM's core (download engine, scheduler, link grabber, yt-dlp resolver) is
written in Rust and lives in `sdm-core/`. It is exposed to the SwiftUI app
through UniFFI as the Swift module `SDMRust`; the `SDMKit` package targets
(`SDMCore`, `SDMEngine`, `SDMGrabber`, `SDMResolve`) are thin adapters over it.

Prerequisites:

- Xcode with the macOS 15 SDK
- Rust through [rustup](https://rustup.rs), with both macOS targets:
  `rustup target add x86_64-apple-darwin aarch64-apple-darwin`.
  The scripts put `~/.cargo/bin` first on `PATH`, so a Homebrew `cargo`
  (which cannot cross-compile) is not used.
- Git LFS, for the vendored blobs below: run `git lfs install` before
  cloning, or `git lfs pull` after.

`scripts/build-rust-core.sh [debug|release]` builds `sdm-ffi` with cargo
(debug: the host triple; release: x86_64 and arm64, joined with `lipo`),
generates the Swift bindings into `SDMKit/Sources/SDMRust/Generated/`, and
packs the static library into `SDMKit/Artifacts/SDMRust.xcframework`. Both
outputs are git-ignored. The script skips the work when nothing under
`sdm-core/` has changed since the last run. A fresh clone must run it once
before Xcode can open the package cleanly.

To build in Xcode, open `SDM.xcodeproj` and build the `SDM` scheme. The scheme
runs the script as a build pre-action (`debug` for Debug, `release` for
Release), so Rust changes are picked up by `xcodebuild` as well as by the
Xcode UI.

`./build.sh` makes a universal (arm64 + x86_64) Release build, bumps the build
number and installs the app into `/Applications`. It runs the Rust step itself.
Options: `-v <version>` sets the marketing version, `--no-bump` skips the
build-number bump, and `--no-install` stops after the build and leaves the app
in `build/Build/Products/Release/`. Use `./build.sh --no-bump --no-install` to
check that a Release build works without touching the installed app.

## Testing

`./test.sh` runs everything: the Rust workspace tests, then the `SDMKit`
Swift tests (the `SDMRust` bindings and the shim targets) against a Debug
build of the Rust core.

To run the parts separately:

```
cd sdm-core && cargo test --workspace      # Rust core
scripts/build-rust-core.sh debug && (cd SDMKit && swift test)   # Swift side
```

`./lint.sh` runs `swift-format lint` and `cargo clippy --workspace
--all-targets -- -D warnings`. `./format.sh` runs `swift-format`, `prettier`
and `cargo fmt --all`.

The original all-Swift core is kept as a baseline in `legacy/SDMKitSwift/`. It
is its own package and is not linked into the app. To run its tests:
`cd legacy/SDMKitSwift && swift test`.

## Vendored binaries

SDM does not ask you to `brew install` anything. Its three helper binaries
live in `~/Library/Application Support/SDM/bin/`:

| Binary | How it gets there | Updates |
|---|---|---|
| `ffmpeg` | Bundled in the app as a per-arch LZFSE blob, inflated on first launch | Only when a new SDM build ships a newer pinned build |
| `qjs` (QuickJS-ng) | Bundled universal LZFSE blob, inflated on first launch — the JavaScript runtime yt-dlp now needs for full YouTube support | Same as ffmpeg |
| `yt-dlp` | **Downloaded** from GitHub releases on first launch, SHA-256 verified, ad-hoc signed | Self-checks every 6 h (15 min while missing); auto-updates. Stable or nightly channel, chosen in **Settings → YouTube → Components**. Never uses `yt-dlp -U`. |

yt-dlp is invoked with `--extractor-args youtube:jsruntime=quickjs` so it
uses the bundled `qjs`. All of this is implemented in
`sdm-core/crates/sdm-resolve/src/managed_binaries.rs` (the Swift original is `legacy/SDMKitSwift/Sources/SDMResolve/ManagedBinaries.swift`) and driven by the app's
`ManagedBinariesController`.

To refresh the pinned `ffmpeg` / `qjs` versions, edit the version pins at the
top of `scripts/vendor-binaries.sh`, run it, and commit the regenerated
blobs and `vendor-manifest.json`. A fresh clone that has not run the script
carries a placeholder `vendor-manifest.json` (`"0"` versions); the app treats
absent blobs as "nothing to inflate" and yt-dlp still self-downloads.

## Documentation

- [Design spec](docs/superpowers/specs/2026-08-03-sdm-design.md) — architecture, data model, engine design, testing strategy, phasing
- [CLAUDE.md](CLAUDE.md) — working notes for AI agents on this codebase
