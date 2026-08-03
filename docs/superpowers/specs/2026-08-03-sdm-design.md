# SDM — Shayan's Download Manager: Design

Date: 2026-08-03
Status: Approved design, pre-implementation

## 1. Purpose

A macOS download manager with segmented (multi-connection) transfers, robust resume, clipboard-driven link grabbing with automatic package grouping, and YouTube support. Built for personal use first, with a polished native UI.

## 2. Foundational decisions

| Decision | Choice | Rationale |
|---|---|---|
| Distribution | Direct download, Developer ID signed + notarized, **App Sandbox off** | Needs to exec helper binaries and write to arbitrary user-chosen folders without security-scoped bookmark friction. Mac App Store is explicitly out of scope; Apple rejects apps bundling yt-dlp-class downloaders. |
| Deployment target | **macOS 15.0** baseline, enhanced on **macOS 26** | Must run on the user's macOS 15 machine; macOS 26 gets Liquid Glass via availability checks isolated to one modifier file. |
| YouTube | **User-installed yt-dlp, used as a metadata extractor only** | Generic HTTP downloads work with zero dependencies. yt-dlp may be bundled in a later phase; the resolver protocol makes that a drop-in change. |
| Language | Swift 6 language mode, strict concurrency | The engine is heavily concurrent; compiler-enforced data isolation is worth the upfront cost. |
| Persistence | Plain `Codable` + atomic JSON snapshot + per-download sidecar. **No SwiftData.** | See §4. |
| UI | SwiftUI, `NavigationSplitView` + `MenuBarExtra` | Native, and the logic worth testing lives outside the views anyway. |
| Tests | Swift Testing (`@Test` / `#expect`) | See §11. |

The existing Xcode boilerplate (`Item.swift`, the `ModelContainer` in `SDMApp.swift`, the default `ContentView`) is deleted.

## 3. Module structure

A local SPM package with the Xcode app target depending on it. Each target is independently testable without launching an app.

| Target | Owns | Depends on |
|---|---|---|
| `SDMCore` | Domain models (`DownloadItem`, `Package`, `Priority`, `ItemState`, `RangeSet`), value types, no I/O | — |
| `SDMEngine` | Segmented transfer, resume, `.incomplete` files, checkpointing, scheduler, telemetry | `SDMCore` |
| `SDMGrabber` | URL extraction, link probing, verdict rules, package clustering, resolver protocol | `SDMCore` |
| `SDMApp` (Xcode target) | SwiftUI views, menu bar extra, notifications, clipboard watcher, theming, activation policy | all three |

Design constraint: the tuning-prone logic (verdict rules, clustering, scheduler ranking) is expressed as **pure functions over value types**, so it is fixture-testable and can be revised without touching stateful code.

## 4. Persistence

Three tiers, each matched to its write frequency.

### 4.1 Live progress — memory only

Bytes-downloaded, instantaneous speed, and ETA live in engine actors and reach the UI through an `AsyncStream` of coalesced snapshots (~4 Hz, dropping to 1 Hz when the window is hidden). Never written to disk at this rate.

### 4.2 Durable state — one atomic JSON snapshot

Packages, items, URLs, sizes, priorities, ordering, enabled flags, verdicts, selected YouTube format IDs. Written debounced (~2 s after the last change, plus on quit and on `NSApplication` termination) using write-to-temp-then-`replaceItemAt`, so a crash can never leave a torn store.

Item counts are in the thousands at most; a database is not warranted and a zero-dependency store is a genuine benefit. The store sits behind a `StateStore` protocol so swapping in GRDB later is contained.

### 4.3 Resume state — per-download sidecar

Alongside `Foo.mp4.incomplete` sits `Foo.mp4.sdmpart` containing:

- the completed-byte `RangeSet`
- the validator captured at start (`ETag`, `Last-Modified`, `Content-Length`)
- the source URL and, for resolver-backed items, the video ID + format ID needed to refresh an expired URL

Checkpointed every ~8 MB per worker or every 5 s, whichever comes first, plus on pause and quit. It lives next to the file deliberately — file and resume state travel together. A partial file found without a valid sidecar is restarted from zero.

## 5. Download engine

### 5.1 Core model

**A file's progress is a `RangeSet` of completed byte ranges. Segments are ephemeral workers, not stored structure.** This is the load-bearing decision of the whole design.

Each download owns a `RangeSet` and a **worker pool of size N** (the segment-count setting). An idle worker claims work by locating the largest remaining gap and taking a slice of it — halving it if large, taking the whole gap if under a floor of ~1 MB so the system never devolves into thousands of tiny requests. Each worker issues a ranged `GET`, writes bytes at their absolute offset via `pwrite` into the preallocated `.incomplete` file, and reports completed intervals back into the set.

### 5.2 Consequences

- **Raising N** — new workers spawn and claim gaps immediately.
- **Lowering N** (e.g. 100 → 3) — surplus workers finish their current claim, report it, and retire. The remaining workers chew through the fragmented holes left behind, because holes are simply gaps in the interval set; nothing records or cares which worker count produced them.
- **Resume after quit or crash** — identical code path. Load the set from the sidecar, spawn N workers, they claim the gaps. Resume is not a special mode; it is the normal loop starting from a non-empty set.
- **The progress bar is a direct render of the `RangeSet`** (see §9.4).

### 5.3 Supporting behavior

- **Validator guard.** On resume, re-probe and compare `ETag` / `Last-Modified` / `Content-Length` against what was captured at start. Any mismatch discards the partial and restarts, rather than silently producing a corrupt file.
- **No `Range` support** → pool forced to size 1, no checkpointing, `isResumable = false` propagated to the UI badge and to the scheduler.
- **Completion** → verify the set covers `[0, size)` with no gaps, `fsync`, delete the sidecar, atomically rename off the `.incomplete` suffix.
- **URL expiry** → a `403` mid-download triggers the resolver to refresh the URL for the stored video ID + format ID, after which workers resume against the existing set. Invisible to the user.
- **Output layout** → files land in `<downloadFolder>/<packageName>/<filename>`, where `downloadFolder` defaults to `~/Downloads` and is configurable.

### 5.4 Speed measurement

A single **1 Hz tick** in the engine. Each tick folds every worker's byte delta into its item's counter; item speed is an EMA over those samples. Aggregates are computed, never stored: package speed is the sum of its running items, global is the sum of all. One source of truth means the three numbers cannot disagree.

History lives in fixed-size in-memory ring buffers — 60 samples per item and per package for sparklines, 300 globally for the main graph. Nothing is persisted; speeds from a previous session are meaningless.

## 6. Scheduler

### 6.1 One axis: enabled / disabled

"Enable/disable" and "start/stop" are collapsed into a **single** enabled flag; an item's Start/Stop action *is* the enable toggle. Two independent axes would produce four states, two of which are indistinguishable to a user.

Terminology used consistently throughout this document: **disabling** an item is the user-initiated action ("pause" in the UI); **preempting** is the scheduler pausing a still-enabled item to free a slot. A preempted item remains enabled and returns to `queued`.

### 6.2 Ranking

Scheduling is a **pure function re-evaluated on every change** — no queue structure to keep in sync. Order changes, priority changes, additions, and completions all flow through one path.

Sort key, descending: `effective priority → package position → item position within package`.

`effective priority` is the item's own if explicitly set, otherwise inherited from its package. Bumping a package lifts everything under it; a single item can still be pinned above its own package.

The desired running set is the top `maxConcurrent` **enabled, incomplete** items. Items running but absent from it are preempted (paused, partial data intact, returned to queued). Items present but not running are started.

### 6.3 Non-preemptible slot reservation

A running non-resumable item claims its slot unconditionally. The computation therefore reserves slots for currently-running non-resumable items **first**, then fills the remainder by rank. Without that ordering the scheduler would compute an unsatisfiable desired set and thrash.

Manual pause of a non-resumable item is permitted but shows a warning first.

### 6.4 Guards

- **Anti-thrash hysteresis** — an item started within the last ~5 s is not preempted. Without this, dragging a row re-ranks on every frame and produces a pause/resume storm.
- **Global connection ceiling** — `maxConcurrent × segmentsPerItem` can reach absurd numbers (6 × 100 = 600 sockets). A global max-connections setting caps total in-flight workers; worker pools shrink below their configured N when the budget is tight, largest pool yielding first.
- **Per-host cap** — default ~8 concurrent connections per host, for politeness and to avoid rate-limiting or bans.
- **Failure handling** — transient errors retry with exponential backoff and jitter up to a max attempt count, staying queued between attempts. Permanent failures (404, validator mismatch after refresh) enter a terminal `failed` state surfaced with a reason and a manual retry action.

## 7. Linkgrabber

### 7.1 Clipboard watching

`NSPasteboard` has no change notification, so this polls `changeCount` on a ~0.5 s timer, gated behind a settings toggle. Content SDM itself placed on the pasteboard is ignored, so copying a link out of the app does not re-grab it.

**Known risk to verify at implementation time:** recent macOS versions show privacy alerts for programmatic pasteboard reads. `NSPasteboard.detectValues(for: [.probableWebURL])` exists specifically to detect and return URLs without a full content read. Exact prompting behavior on macOS 15 vs 26 must be verified against current documentation rather than assumed; worst case is a one-time system permission grant.

**Privacy stance:** non-URL clipboard content is never stored, never logged, and never leaves the process.

URL extraction uses `NSDataDetector` rather than a regex — it correctly handles URLs embedded in prose, wrapped across lines, and with trailing punctuation.

### 7.2 Checking pipeline

Two stages, both bounded by the same connection budget as the engine (global cap plus ~2–4 per host), so a 200-link paste cannot get the user rate-limited.

**Stage 1 — cheap probe, every link.** `HEAD`, falling back to `Range: bytes=0-0` `GET` when the server rejects or lies about `HEAD`. Captures: final URL after redirects, status, `Content-Length`, `Content-Type`, `Content-Disposition` filename, `ETag` / `Last-Modified`, and whether a `206` came back — which *is* the resume-capable flag, determined here and carried into the engine.

**Stage 2 — deep sniff.** First 64 KB, magic-byte check against the container claimed by the extension and MIME type.

### 7.3 Verdicts

A pure function over the probe result. Four outcomes, because each implies a different user action:

| Verdict | Trigger | User action |
|---|---|---|
| **Online** | 2xx, plausible type and size | Proceed |
| **Offline** | non-2xx after redirects | Delete |
| **Faulty** | `text/html` for a media/archive extension; `Content-Length` implausibly small for its type; magic bytes contradicting the extension; redirect landing on a different host at an error/login-looking path | Inspect or delete |
| **Check failed** | timeout or DNS failure after retries | Retry |

Rules are table-driven pure functions; tuning them is editing data plus a fixture test.

### 7.4 Package clustering

A pure function `[ProbedLink] -> [PackageCandidate]`, run after Stage 1 so it can use real filenames from `Content-Disposition` rather than guessing from URL paths.

1. The paste batch seeds one candidate package.
2. Each filename reduces to a **template**: lowercase, extension stripped, separators normalized, digit runs replaced with a placeholder. `Show.S01E01.1080p.mkv` and `Show.S01E02.1080p.mkv` collapse to the same template and therefore cluster — episode grouping falls out with no episode-specific regex.
3. **Archive part sets** (`.part01.rar`, `.r00`, `.z01`, `.001`) are detected and locked together as an indivisible unit; splitting them across packages is never correct.
4. Surviving templates group by host + directory path as a secondary signal; dissimilar templates split into their own packages.
5. Package name = cleaned longest common prefix of member stems, falling back to the host.

All of it is fully overridable: drag items between packages, right-click → move to existing/new package, rename, merge, split.

### 7.5 Grabber surface

Links appear immediately as pending rows and fill in as probes land — no blocking spinner over a batch of 200. The header shows a determinate `checked / total` bar with live counts of online / faulty / offline / failed, doubling as filter chips. Rows carry per-link state (queued → probing → sniffing → done).

- Manual entry via an **Add links** sheet with a multiline paste field feeding the identical pipeline, plus drag-and-drop of text or URLs onto the window.
- Links already in the download list are badged as duplicates rather than silently dropped; they can still be added deliberately.
- Handoff via **Add to downloads** or **Add and start**, with a setting to auto-add-and-start on grab.

## 8. YouTube and the resolver protocol

`LinkResolver` protocol: `canHandle(URL) -> Bool`, `resolve(URL) -> [ResolvedMedia]`. Generic HTTP is the identity resolver. This is the extension seam for future sites.

**yt-dlp is used as an extractor, not a downloader.** `yt-dlp -J <url>` yields the full format table — resolutions, codecs, bitrates, exact `filesize` or `filesize_approx`, and direct `googlevideo` URLs that honor `Range`. SDM then downloads those with the standard engine: segmented, resumable, priority-scheduled, pausable. YouTube downloads are first-class rather than a second-rate subsystem with bespoke progress parsing.

- **Quality preferences** — an ordered rule list in Settings (resolution ladder, codec preference, container preference, max filesize cap). The Linkgrabber evaluates it against the actual available formats.
- **Per-link override** — a format picker on any YouTube row. Changing it swaps the selected format ID and the displayed filesize updates **instantly** from the already-fetched table; no re-probe, no network round trip.
- **Muxing** — high-resolution YouTube formats are video-only. Video and audio download as two items bound to one output, then mux via `ffmpeg` (a Homebrew dependency of yt-dlp), detected the same way.
- **Fallback** — formats available only as HLS/DASH manifests with no single direct URL are handed to yt-dlp to download wholesale, with progress parsed from its output. This is the degraded path and should be rare.
- **Missing yt-dlp** — YouTube links still grab and display, marked with a clear "requires yt-dlp" state and a copyable `brew install yt-dlp` instruction. Generic HTTP is unaffected.

## 9. UI shell

### 9.1 Structure

`NavigationSplitView`. Sidebar: Downloads / Linkgrabber / Completed, plus a pinned live global stats block (total speed, running average, active count, bandwidth graph). Main area: an `OutlineGroup` list with packages as expandable parents and items as children.

**Completed is a filtered view of the same list, not a separate store.** Finished items remain in their package (so a package reads 8/8 and stays intact); Completed is a predicate over everything.

### 9.2 Rows

Package rows show: name, priority badge, completed/total counts, aggregate size, sparkline, aggregate speed.
Item rows show: state icon, filename, live segment count, segmented progress bar, percentage, sparkline, speed.

Additional row treatments:
- The **faulty reason is the badge text** ("html, not video"), not a generic warning icon requiring hover.
- `no resume` renders as a danger badge, and the same flag is what makes the scheduler refuse to preempt the item.
- Disabled packages stay in place, dimmed and struck through, rather than relocating — the list remains a stable map of user intent.
- Sparklines share the row's right-aligned numeric column so they align vertically down the list.

### 9.3 Reordering

`.draggable` / `.dropDestination` with a custom `Transferable` carrying item IDs. Dropping onto a package row moves items into it; dropping between rows reorders. Reordering writes the position field, which immediately re-triggers the scheduler's ranking function — the pause/resume behavior is automatic, because ordering is not a separate concern from scheduling.

### 9.4 Segmented progress bar

The bar renders the `RangeSet` directly — filled where bytes are complete, empty where gaps remain. A two-worker download shows two growing fills converging; dropping 100 segments to 3 visibly freezes 100 stripes while 3 heads eat through the gaps.

At high segment counts, ranges are sub-pixel and naive rect-drawing aliases badly. The set is therefore **rasterized to the bar's pixel width** — each column receives a coverage fraction (0–1) from overlapping ranges, drawn as alpha in a single `Canvas` pass. Correct at any segment count, and it degrades gracefully into a solid bar near completion. Active worker heads get a brighter tint at their leading edge.

### 9.5 Segment count display

The badge shows **currently active workers**, which can be below the configured N when the global or per-host ceiling is squeezing that item. Rendered as `12/16` when active and configured diverge, plain `16` when they agree. Showing only the configured number would mislead precisely when the truth matters.

Segment count is a **global setting with a per-item override**.

### 9.6 Sparklines and the main graph

Per-row sparklines are drawn in a single `Canvas` — one path, no axes, no legend, y-scaled to that row's own max. A SwiftUI `Chart` per row is far too heavy at hundreds of rows; each builds a full plot with scales and turns scrolling into a slideshow.

Swift Charts is reserved for the one place it earns its cost: the main global bandwidth graph, a filled area with a running-average line over it.

Views never touch engine actors directly; they consume the coalesced snapshot stream.

### 9.7 Menu bar

`MenuBarExtra` with `.menuBarExtraStyle(.window)`, hosting a real SwiftUI popover:

- aggregate speed and mini bandwidth graph
- active downloads with progress rings and per-item speed
- **a pending-links row** when the Linkgrabber holds unconfirmed links — "N links waiting" with a **Review** button that opens the app directly to the Linkgrabber tab. Placed above the actions, since it is the one item there requiring a decision rather than reporting status.
- Pause all / Open SDM / Quit

The menu bar icon shows a determinate ring for overall progress.

### 9.8 Notifications

`UNUserNotificationCenter`, each individually toggleable:
- download finished
- package finished
- download failed (with reason)
- "N links grabbed, waiting for confirmation" — carrying an action that opens the Linkgrabber

### 9.9 macOS 26 Liquid Glass

Isolated in a single `ViewModifier` (`.sdmSurface(.sidebar)`, `.sdmSurface(.toolbar)`) applying `glassEffect` under `if #available(macOS 26)` and falling back to `.regularMaterial` on macOS 15. Views never write availability checks; there is exactly one file to change when the baseline moves. Glass tint derives from the active theme's accent.

## 10. Theming and activation policy

### 10.1 Themes

**Themes are data, not code.** A `Theme` value type maps ~20 semantic roles — surface tiers, text tiers, accent, border, four status colors (online / faulty / offline / failed), progress fill, completed-segment fill, active-head tint, graph stroke, graph average stroke — loaded from JSON in the app bundle. Views reference roles only, never literal colors.

Consequence: adding a theme is adding a JSON file, and a future theme editor or user-theme import needs no new code — the same loader pointed at Application Support.

Built-in: **System**, **Light**, **Dark**, **Midnight Blue**, **Deep Purple**, plus community palettes **Nord**, **Dracula**, **Solarized** (light and dark), **Gruvbox**, **Catppuccin** (Latte / Frappé / Macchiato / Mocha), **Tokyo Night**, **One Dark**, **Rosé Pine**.

Palette values are taken from each project's own published source at implementation time, not from memory, and each carries its license attribution (most are MIT).

Two supporting details:
- Each theme declares whether it is dark, so `NSApp.appearance` is set correctly for native controls — otherwise system menus render light over a dark window.
- **System** resolves live to the light or dark variant, following `effectiveAppearance`.

A test asserts WCAG AA contrast for every text-on-surface pair in every bundled theme, so an attractive palette cannot ship unreadable secondary text.

### 10.2 Window lifecycle and activation policy

Closing the window never quits the app: `applicationShouldTerminateAfterLastWindowClosed` returns false and quitting is explicit.

| Mode | Dock icon | Menu bar icon | On window close | Reopen via |
|---|---|---|---|---|
| **Menu bar only** | only while a window is open | always | policy → `.accessory`, dock icon disappears | menu bar icon (policy → `.regular`) |
| **Dock only** | always | hidden | dock icon stays, app keeps running | dock icon click (`applicationShouldHandleReopen`) |
| **Both** (default) | always | always | dock icon stays | either |

Non-obvious consequence to handle deliberately: in **menu bar only** mode with no window open the app is `.accessory` and therefore has no menu bar of its own, so `⌘Q` is unavailable and Quit must live in the menu bar popover.

Changing the mode applies immediately, including when no window is open.

Quitting with active non-resumable downloads shows a confirmation, since that progress cannot be recovered.

## 11. Testing strategy

Swift Testing (`@Test` / `#expect`). The engine is the part that must be provably correct — a download manager that silently corrupts a 20 GB file is worse than one that does not exist.

### 11.1 No network, no waiting

The engine takes `HTTPTransport` and a `Clock` as injected protocols. Tests run against an in-process fake origin serving a known payload with a fake clock, so the suite finishes in seconds and never flakes on timing.

The fake origin is programmable to misbehave the way real servers do:
- ignores `Range` entirely, or honors it only sometimes
- reports a wrong `Content-Length` (both short and long)
- drops the connection mid-body at a chosen offset
- changes its `ETag` between requests
- returns `403` after N seconds, simulating googlevideo URL expiry
- returns 200 with a tiny HTML error body for a `.mp4` request

### 11.2 The two properties that get hammered

Randomized tests, because a subtle bug here is invisible until it costs a large file:

1. **Byte identity under arbitrary churn.** A randomized schedule of segment-count changes, pauses, kills, and connection drops against a known payload — the finished file must hash-match the source every time. This is the test that proves 100 → 3 re-segmentation is safe.
2. **No lost or duplicated bytes.** `RangeSet` is tested independently: merging, gap-finding, and claim/release must never hand the same range to two workers, and the union of completed ranges must exactly equal what was written.

### 11.3 Resume

Tested as a real restart, not a simulated one: run partway, tear the engine down entirely, reconstruct from the on-disk sidecar, finish, hash-compare. Also covered: missing sidecar, corrupt sidecar, and validator mismatch — each must restart cleanly rather than produce a Frankenstein file.

### 11.4 Scheduler

Pure function, therefore table-driven. Fixtures of `(items, priorities, order, maxConcurrent, connection budget) → expected running set`. Cases pinned: non-resumable items reserving slots before rank-based filling, hysteresis suppressing thrash during a drag, and the invariant that the running set never exceeds `maxConcurrent` under any input.

### 11.5 Grabber

Fixture tables for both pure functions — recorded real-world HTTP responses mapped to expected verdicts, and batches of real filenames mapped to expected package groupings. Both will be tuned over months; fixture tables mean tuning never silently regresses an earlier case.

### 11.6 yt-dlp

Stubbed behind the extractor protocol with recorded `-J` JSON, so format selection and filesize-on-quality-change are tested with no binary installed and no YouTube dependency in CI.

### 11.7 UI

A launch smoke test and nothing more. Snapshot-testing SwiftUI is high-maintenance and low-value here; the logic worth testing has been pushed out of the views. Plus the theme contrast test from §10.1.

## 12. Settings

| Setting | Default |
|---|---|
| Download folder | `~/Downloads` |
| Max concurrent downloads | 3 |
| Segments per file (global) | 8 |
| Per-item segment override | inherits global |
| Global max connections | 32 |
| Max connections per host | 8 |
| Clipboard watching | on |
| Auto-add and start on grab | off |
| Deep sniff (stage 2) | on |
| YouTube quality preference list | 1080p → 720p, prefer H.264 |
| Theme | System |
| Dock / menu bar mode | Both |
| Notifications (per type) | all on |

## 13. Phasing

Engine-first vertical slice. Everything else is a client of the engine, and segmentation plus resume are the hardest things to retrofit.

| Phase | Scope |
|---|---|
| **1** | `SDMCore` + `SDMEngine`: `RangeSet`, worker pool, resume, sidecars, `.incomplete` handling, scheduler, telemetry, full test suite. Plain functional UI to drive it. |
| **2** | `SDMGrabber`: clipboard watcher, URL extraction, two-stage probing, verdict rules, package clustering, Linkgrabber UI. |
| **3** | Menu bar extra, notifications, bandwidth graph, sparklines, segmented progress rendering. |
| **4** | Theming, activation policy modes, Liquid Glass polish pass. |
| **5** | yt-dlp resolver: format tables, quality preferences, per-link override, muxing, URL refresh. |

Each phase gets its own implementation plan.

## 14. Deferred (explicitly out of scope for now)

- Bundling yt-dlp and ffmpeg
- Mac App Store distribution
- Browser extension integration
- Torrent or magnet support
- Bandwidth throttling / scheduling by time of day
- Custom theme editor and theme import (the loader supports it; the UI is deferred)
- Launch at login
- AI-assisted package naming (the clustering interface accommodates it)
