# SDM — Shayan's Download Manager

A macOS download manager with segmented (multi-connection) transfers, robust resume, clipboard-driven link grabbing, and YouTube support.

> **Status: pre-implementation.** The design is complete and documented; the code is still Xcode boilerplate. See [the design spec](docs/superpowers/specs/2026-08-03-sdm-design.md).

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
- Requires [yt-dlp](https://github.com/yt-dlp/yt-dlp) (`brew install yt-dlp`); generic HTTP downloads work without it

**Interface**

- Live download speed per item, per package, and globally, with sparklines and a bandwidth graph
- Progress bars visualize the actual byte ranges, so you can see each segment filling in
- Menu bar item with active downloads, pending links, and quick controls
- Notifications for finished downloads and links awaiting confirmation
- Themes: system, light, dark, midnight blue, deep purple, and community palettes (Nord, Dracula, Solarized, Gruvbox, Catppuccin, Tokyo Night, One Dark, Rosé Pine)
- Runs in the dock, the menu bar, or both — your choice

## Requirements

- macOS 15.0 or later (macOS 26 additionally gets Liquid Glass styling)
- `yt-dlp` and `ffmpeg` for YouTube downloads only

## Building

Open `SDM.xcodeproj` in Xcode and build the `SDM` scheme.

## Documentation

- [Design spec](docs/superpowers/specs/2026-08-03-sdm-design.md) — architecture, data model, engine design, testing strategy, phasing
- [CLAUDE.md](CLAUDE.md) — working notes for AI agents on this codebase
