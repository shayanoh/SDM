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

Open `SDM.xcodeproj` in Xcode and build the `SDM` scheme.

The vendored `*.lzfse` blobs under `SDM/Resources/vendor/` are stored with
**Git LFS** — run `git lfs install` once before cloning, or `git lfs pull`
after, so they materialize.

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
`SDMKit/Sources/SDMResolve/ManagedBinaries.swift` and driven by the app's
`ManagedBinariesController`.

To refresh the pinned `ffmpeg` / `qjs` versions, edit the version pins at the
top of `scripts/vendor-binaries.sh`, run it, and commit the regenerated
blobs and `vendor-manifest.json`. A fresh clone that has not run the script
carries a placeholder `vendor-manifest.json` (`"0"` versions); the app treats
absent blobs as "nothing to inflate" and yt-dlp still self-downloads.

## Documentation

- [Design spec](docs/superpowers/specs/2026-08-03-sdm-design.md) — architecture, data model, engine design, testing strategy, phasing
- [CLAUDE.md](CLAUDE.md) — working notes for AI agents on this codebase
