# Tauri link grabber and platform tools (spec 3)

Status: **approved 2026-10-24** (Shayan approved the design including all "my call" decisions, G5 to G10).
Roadmap: [docs/tauri-roadmap.md](../../tauri-roadmap.md), spec 3 of 6. Builds on
[spec 1](2026-10-14-tauri-foundation-design.md) and [spec 2](2026-10-23-tauri-main-window-design.md).

## 1. Intent

Two things ship together, because the first is where the second first hurts:

1. **The link grabber view**, replacing the Linkgrabber placeholder from spec 2. It must look and behave
   **exactly like the SwiftUI view** (`SDM/LinkGrabberView.swift`, `SDM/AddLinksSheet.swift`,
   `SDM/GrabberController.swift`), the way spec 2 did for the main window. No redesign.
2. **`ffmpeg`, `ffprobe` and `qjs` for Windows and Linux** (spec 1 plan task 16, decision T7). A YouTube
   row in the grabber says "requires ffmpeg" until `ffmpeg` is on disk, and yt-dlp needs `qjs` to solve
   YouTube's player; without both, the grabber's most valuable rows do nothing on Windows and Linux. macOS in
   the Tauri build keeps sharing `<data dir>/bin` with the SwiftUI app, which bundles the blobs.

## 2. Decisions

| # | Decision | By |
|---|---|---|
| G1 | The view is a straight port of the SwiftUI one: same header, stat pills, buttons, rows, package header, context menus, Add Links sheet, drag and drop. Where SwiftUI and the web differ (icons, font, tooltips) spec 2's section 2 rules apply (Lucide icons, system UI font). | Shayan |
| G2 | The view **replaces** the Linkgrabber placeholder, as a sidebar view inside the main window, not a separate window. | Shayan |
| G3 | Binary sources: BtbN `FFmpeg-Builds` for `ffmpeg` and `ffprobe`, `quickjs-ng/quickjs` releases for `qjs`. **x64 only** (Windows x64, Linux x86_64). | Shayan |
| G4 | The binaries get a **minimal** UI in this spec (section 7); the full "managed binaries" Settings tab is spec 4, and the deferral is recorded in section 9, the roadmap and memory so it is not forgotten. | Shayan |
| G5 | *(my call)* Grabber state travels on the **existing patch stream**: `Patch` gains an optional `grabber` field carrying the complete grabber view whenever it changed. One ordered transport, one `seq`, no second channel. A batch of links is hundreds of rows, not thousands, so a full replacement beats a diff protocol; the frontend windows the list as spec 2 does. | me |
| G6 | *(my call)* **All grabber logic lives in `sdm-app`** (T6): the hand-off to the engine (`MediaHandoff::build`, removing the handed-off rows), the "held back" message, format-picker options (`MediaFormatMenu::options`), the "addable" rule, package grouping for a selection. The frontend renders the view and sends ids. | me |
| G7 | *(my call)* The session is the native Rust `sdm_grabber::GrabberSession`, built in `sdm-app` with the production prober and the same resolver `production_dependencies` already builds. Not the UniFFI wrapper in `sdm-ffi` (that is SwiftUI's seam; it needs `Toolchain` and `GrabberSettings` plumbing the controller does not). | me |
| G8 | *(my call)* Until spec 4 adds the settings, the grabber runs on the SwiftUI defaults: deep sniff on, auto-clear after 30 minutes, default quality preferences, no cookie source, 50 playlist videos, no extra yt-dlp arguments, probe limits 8 global / 4 per host (the engine's defaults) and 4 concurrent resolves. They are constants in one `GrabberDefaults` struct that spec 4 replaces with stored settings. | me |
| G9 | *(my call)* The "known downloads" duplicate marking is wired: after every engine publish whose set of item URLs changed, the controller calls `set_known_download_urls`. | me |
| G10 | *(my call)* `ffmpeg`, `ffprobe` and `qjs` archives are downloaded as **pinned releases with SHA-256 verification**, never a floating `latest` URL (section 6). | me |

## 3. Goals, non-goals, anti-goals

**Goals.** Parity with the SwiftUI grabber for everything listed in section 5; `ffmpeg`/`ffprobe`/`qjs`
provisioned on Windows and Linux with verified checksums, reflected in the grabber's `requires ffmpeg` rows
and header; the spec 1 and 2 performance gates still pass (an idle grabber costs nothing; the list is windowed).

**Non-goals.** The clipboard watcher and "auto add and start on grab" (spec 5). Every grabber setting
(deep sniff, auto-clear interval, quality preferences, cookie source, playlist cap, binary overrides,
clipboard toggle): spec 4. The managed-binaries Settings tab, a yt-dlp channel picker, a manual "check now"
button (spec 4). OS file-drop of `.txt` or URL files (only text and URL drops, as SwiftUI). Linux aarch64,
Windows arm64. Redistributing `ffmpeg` (it is downloaded at run time, which is why T7 exists).

**Anti-goals.** Business logic in the frontend (G6). A snapshot that re-renders every row when one row
changes (rows are keyed by id and compared by value). A checksum that is read from the same server the
binary came from without being pinned in the source (an attacker who controls the release then controls both).
Anything that makes the SwiftUI app or `sdm-ffi` behave differently.

## 4. Phase A: controller (`sdm-core/crates/sdm-app`)

A new module `grabber.rs` owns a `GrabberController`: the `GrabberSession`, the last published view, and the
"held back" notice. `App` holds an `Option<Arc<GrabberController>>` set from `Dependencies` (None in the
existing tests, so they do not change). Every method is `async`, publishes after it finishes, and has a Rust
test against the fake probe origin (`sdm-grabber`'s `fake_probe_origin`) and a fake resolver; no network, no
real clock.

### 4.1 Wire types (`protocol.rs`)

`Patch` gains `grabber: Option<GrabberView>`. Present when the view changed, and always on a `full` patch.

```text
GrabberView {
  checkedCount, totalCount,                       // header progress
  summary { online, faulty, offline, failed, isChecking, recheckable },
  rows:     [GrabberRow],                          // links and media rows, session order
  packages: [GrabberPackage],                      // { id, name, note?, rowIds }
  heldBack: number | null,                         // "N items held back — choose a format first"
  tools:    ToolsNotice | null                     // section 7
}
GrabberRow = Link { id, filename, finalUrl, stage, verdict?, sizeBytes?, isDuplicate, removable }
           | Media { id, title, sourceUrl, state, stateText?, summary?, options[], isDuplicate, sizeBytes? }
```

- `Link.verdict` is `online | offline | checkFailed | faulty{reason}`; `stage` is `queued | probing | sniffing | done`.
  `removable` is true when a verdict exists (SwiftUI shows the trash button only then).
- `Media.state` is `resolving | resolved | unselected | unsupported | needsYtDlp | needsFfmpeg | failed`;
  `stateText` carries the full failure reason (first line is shown, the whole text on hover).
- `Media.summary` is the SwiftUI `formatSummary` text (`Streaming · 1080p · avc1 · mp4`); computed in Rust.
- `Media.options` is `MediaFormatMenu::options` for rows in `resolved | unselected | needsFfmpeg`, each
  `{ id, label, matchesPreferences, isWholesale, selected }`. Others get an empty list. Choosing sends the option
  `id`, so the frontend never holds a `FormatChoice`.
- Rows that belong to no package come first (SwiftUI: links, then media rows, then packages); `rowIds` order
  is the order inside a package (links, then media rows).
- Ids are lowercase hyphenated UUIDs and byte counts `f64`, as in the rest of the protocol (every `f64` reaches
  TypeScript as `number | null`; the store reads `null` as zero).

### 4.2 Actions

| `App` method | Does | SwiftUI source |
|---|---|---|
| `grabber_ingest_text(text)` | Extract links with `URLExtractor`, ingest, kick the managed binaries check (section 7) | `GrabberController.ingest(text:)` |
| `grabber_remove_links(ids)` / `grabber_remove_package(id)` / `grabber_clear()` | Session edits | same names |
| `grabber_move_link(id, package_id)` / `grabber_rename_package` / `grabber_merge_packages(source, destination)` / `grabber_split_package` | Session edits | same names |
| `grabber_recheck(ids)` / `grabber_recheck_failed()` / `grabber_cancel_checks()` | Re-probe or cancel | same names |
| `grabber_set_format(id, option_id)` | Looks the option up again from the row's media and applies it | `setFormatChoice` |
| `grabber_copy_url(id) -> String` | The one command that returns a value: a link's `finalUrl` or a media row's source page | `url(for:)` |
| `grabber_add_to_downloads(target, start)` | `target` is `{rows: ids}`, `{package: id}` or `{all}`. Builds one engine package per source package (standalone rows each under their own filename), calls the engine's add with `start`, removes only the rows that handed off, sets `heldBack` | the three `addToDownloads` / `addAllToDownloads` / `addSelectedToDownloads` paths |

Rules carried over from SwiftUI, each with a test: an offline or check-failed link is not addable; a media
row is addable only when `resolved`; held-back rows stay in the grabber; `addAll` resolves every package's
items and handoff ids **before** removing anything, because removal reclusters; the engine call comes
before the removal, so a failed hand-off loses nothing; `start` maps to the engine's start-immediately flag.

`App::tick` also (a) prunes rows older than the auto-clear age once a minute, (b) calls
`set_known_download_urls` when the engine's URL set changed, (c) refreshes the grabber view and includes it in
the patch when it changed (value equality against the last sent view, so an idle grabber sends nothing).

### 4.3 Add Links and the old direct-add

`add_url` stays (the main window's own "add" entry points and tests use it); only its **UI**,
`TemporaryTools`, is retired. Add Links sends text through `grabber_ingest_text`, exactly as SwiftUI.

## 5. Phase B: the view (`sdm-tauri/web`)

Reference: `SDM/LinkGrabberView.swift` and `SDM/AddLinksSheet.swift`. As in spec 2, the acceptance targets
are screenshots of the real SwiftUI view, taken once from a fixture (section 8) and stored under
`docs/superpowers/specs/assets/2026-10-24-swiftui-link-grabber/`.

### 5.1 Structure

- **Header** (a top bar under the toolbar, theme `surfaceSecondary` background, bottom divider, 16 px padding):
  a determinate progress bar (`checked / total`); below it one row: `N / M checked` caption, spacer, four stat
  pills (**Online**, **Faulty**, **Offline**, **Check failed**, each `Label count` in its verdict color on a
  `surfaceTertiary` capsule; static readouts, not filters), **Add all to downloads**, **Add all and start**
  (both disabled with no packages), **Cancel Checks** (destructive, only while checking) or **Recheck All**
  (disabled when nothing is recheckable), **Clear** (destructive, disabled with no packages). Then the held-back
  line in the faulty color when `heldBack` is set.
- **Toolbar**: the main window's toolbar keeps its entries; in this view it gains **Add Links** (link-plus
  icon) as its primary action, opening the sheet.
- **List**: rows with no package, then for each package a header row followed by its rows. The package header
  is a plain row (title3 bold name, spacer, **Add to downloads**, **Add and start**, trash), `surfaceSecondary`
  background. Windowed like spec 2's `PackageList` (two row heights).
- **Link row**: filename (one line, tooltip = final URL), a `duplicate` tag in the faulty color, spacer, verdict
  badge (`online`, `offline`, `check failed` in their colors; a faulty link shows its reason in the faulty color;
  no verdict yet shows a small spinner and the stage word `queued | probing | sniffing | done`), size
  (`1.5 GiB`-style, binary units, empty when unknown), trash button when removable.
- **Media row**: play-rectangle icon, title (tooltip = source URL), `duplicate` tag, spacer, then either the
  **format picker** (a borderless menu button labelled with the summary in the online color when resolved, else
  `choose a format` / `requires ffmpeg` in the faulty color) or a badge (`resolving` with a spinner, `not
  supported yet`, `requires yt-dlp`, `requires ffmpeg`, or the first line of the failure in the offline color,
  full text on hover), then the size. The picker lists matching options, a divider, then non-matching ones
  prefixed with `⚠ `.
- **Context menu** on a row or a multi-selection: Recheck; Copy URL (one row only); divider; Add to Downloads;
  Add and Start (both disabled when no selected row is addable); divider; Remove (destructive). **Package header
  menu**: Rename… (a modal with a name field), Merge into ▸ (other packages, or a disabled "No Other
  Packages"), Split.
- **Selection** follows spec 2's pure `selection` module (click, Ctrl/Cmd, Shift). Selection is cleared of
  removed ids after Remove and after a hand-off.
- **Drag and drop**: dropping text or a URL anywhere on the view ingests it (`grabber_ingest_text`); dragging
  a row onto a package header moves it into that package (`grabber_move_link`). Uses spec 2's `dragdrop` helpers.
- **Add Links sheet**: a modal titled "Add links" with a monospaced multi-line field (min 420 × 200), Cancel and
  **Add** (default action; disabled for blank text). On Add it closes first, then ingests.
- **Empty state**: the list area is empty; the folder field and tools line from section 7 stay reachable here.

### 5.2 Store and bridge

`store.svelte.ts` gains a `grabber` state replaced wholesale by each `patch.grabber`. The `Bridge` interface
gets one method per action in 4.2; `tauriBridge.ts`/`bindings.ts` follow the generated commands; `MockBridge`
gets a grabber that behaves like the controller (probes advance per simulated tick; one fixture of ~10 links,
two packages, one resolving, one resolved, one needs-ffmpeg and one failed media row). Pure logic gets its own
module with a fixture table: `grabberRows.ts` (grouping rows by package, the SwiftUI text for badges, size text,
the first line of a failure).

## 6. Platform tools (`sdm-core/crates/sdm-resolve`)

### 6.1 What gets downloaded

| Tool | Source | Platforms |
|---|---|---|
| `ffmpeg`, `ffprobe` | BtbN `FFmpeg-Builds` GPL static build archive (`.zip` on Windows, `.tar.xz` on Linux) from a **tagged** release | Windows x64, Linux x86_64 |
| `qjs` | `quickjs-ng/quickjs` release asset (the single `qjs` executable for the platform) | Windows x64, Linux x86_64 |

The exact tags, URLs, archive layouts and SHA-256 values are decided **while planning** (spec 1's plan said the
same), recorded in a doc comment at the top of `platform_tools.rs` with the date, and re-verified against the
release pages. If BtbN does not publish a usable pinned tag, the fallback is gyan.dev (Windows) and John Van
Sickle's static builds (Linux), and the plan stops and asks. Licences: the GPL build is downloaded by the
user's machine at run time, not redistributed.

### 6.2 Mechanism

`platform_tools.rs` holds a table `PlatformTool { name, url, sha256, archive: Raw | Zip{member} | TarXz{member} }`
per supported target, compiled in with `cfg(all(target_os, target_arch))`; on any other target the table is
empty and nothing downloads. `ManagedBinaries` gets the step "ensure platform tools": for each tool whose file
is missing or whose recorded version (`ffmpegVersion` / `qjsVersion` in the manifest) differs from the pinned
one, download through the existing `BinaryFetching` seam, verify the SHA-256 **before** extracting, extract the
one member, write atomically (temp file then rename, executable bit on Linux), and update the manifest.
`ffmpeg` and `ffprobe` come from one archive, downloaded once. Failures set `lastError` (as yt-dlp's do) and
retry on the existing absent-interval cadence; a partial archive is never left in `bin/`. Time stays
tick-driven; no clock, no sleeping tests.

Archive handling needs new dependencies (`zip`, `tar`, an xz decoder). They are added with `cargo add` and
chosen for being pure Rust, so the Windows cross-build and the Docker Linux build need no system library.

### 6.3 When it runs

At launch, `provision_binaries` (already called by the shell) runs the check, which now includes the tools; and
`grabber_ingest_text` kicks a check, as SwiftUI's `managedBinaries?.kick()` does. When a tool lands,
the `on_binaries_changed` hook invalidates the locator **and** tells the grabber session that `ffmpeg` is available
(the session cannot discover that itself), after which `needsFfmpeg` rows can be rechecked.

## 7. Minimal tools UI (the part spec 4 finishes)

- `GrabberView.tools` is `null` normally and, while a platform tool is downloading or after a failed attempt, a
  `ToolsNotice { text, isError }` such as `Fetching ffmpeg…` or `Could not fetch ffmpeg: transport("offline")`.
  The grabber header shows it as one caption line under the progress row (same style as the held-back line).
- The `TemporaryTools` component loses its Add field (Add Links replaces it) and keeps the **download folder**
  field and the **tools status** line (now yt-dlp, ffmpeg and qjs versions from `BinariesStatusDto`), shown in the
  grabber's empty state and nowhere else. It is marked `TODO(spec 4)` and removed there.
- There is **no** channel picker, no "check now", no override path and no re-download button. A re-download is
  "delete the file from `bin/` and restart".

## 8. Testing

- **Rust (`sdm-app`)**: every action in 4.2; the add-to-downloads rules; the view diffing (an idle grabber emits
  no `grabber` field; a changed row does); auto-clear pruning on the tick; known-download marking. Fake probe
  origin and fake resolver only.
- **Rust (`sdm-resolve`)**: the platform-tool step against a fake `BinaryFetching` serving a generated zip and a
  generated tar.xz: checksum accepted, checksum mismatch rejected and nothing written, truncated archive leaves
  `bin/` clean, version bump re-downloads, unsupported target does nothing. One `#[ignore]`d test per platform
  downloads the real pinned artifact and runs `ffmpeg -version` / `qjs --help` (run by hand).
- **Compile gates**: `scripts/check-windows.sh` and `scripts/check-linux.sh` cover the new code and dependencies.
- **Frontend**: unit tests for `grabberRows.ts` and the new store behavior, component tests for the header, rows,
  context menus, the Add Links sheet and the picker; Playwright against the mock for the full flows (paste links,
  watch them probe, pick a format, add one package and add all) and the parity screenshots.
- **Parity**: SwiftUI reference screenshots of the real view from a fixture (spec 2's section 11 generator, with a grabber batch added) compared by eye with `pnpm exec playwright test parity`.
- **By hand (Shayan, Windows VM and Linux)**: tools download on a clean data directory, a real YouTube link
  resolves, muxes and downloads, and the "requires ffmpeg" row clears itself.

## 9. Deferred to spec 4 (do not forget)

Recorded here, in the roadmap's spec 3 and spec 4 rows, and in the project memory:

1. The grabber settings: deep sniff, auto-clear interval, clipboard watching toggle (the watcher itself is spec 5),
   quality preferences (and the allowlists the picker's `matchesPreferences` reads), cookie source, max playlist
   videos, extra yt-dlp arguments, probe and resolve limits. Replaces `GrabberDefaults` (G8).
2. The full managed-binaries Settings tab: versions, yt-dlp channel, check now, binary override paths, a visible
   re-download, and the download-progress display. Replaces section 7's notice, tools line and the temporary
   download-folder field.
3. Re-wiring the grabber session when those settings change (SwiftUI's `applyStoredSettings`).

## 10. Risks

| Risk | Handling |
|---|---|
| BtbN keeps only recent tagged builds, so a pinned URL may disappear | Pin to a tagged release asset, re-verify at planning time, record the date; if it breaks, a hand-updated table row is the fix (one place) |
| ~150 MB `ffmpeg` archive on a slow link | The download runs in the background with a visible notice and retries; the grabber works for plain links meanwhile |
| Full-view replacement at the tick rate for a very large batch | Send only when the view changed; windowed list. Measure with a 2,000-link fixture; if the patch is too heavy, the fallback is a row-level diff on the same `grabber` field |
| `FormatChoice` ids unstable across snapshots | Option ids are derived from the format ids, not positions; the action re-derives from the row's media, and a stale id is a no-op with an error toast |
| Windows HTML5 drag inside WebView2 (unverified since spec 2) | Same helpers as spec 2; the link-onto-package drag is the one new target |

## 11. Left to the builder

The exact release pins (6.1); the module split inside `grabber.rs`; the option-id scheme; how the mock simulates
probing; Lucide icon picks for the three icons not in spec 2's table (link-plus, play-rectangle, trash is
reused); the exact toolbar slot of Add Links in the other views (hidden there, as in SwiftUI).
