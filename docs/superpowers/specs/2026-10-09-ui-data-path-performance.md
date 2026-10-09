# SDM — UI Data-Path Performance: Findings and Refactor Plan

**Status: IN PROGRESS (started 2026-10-09).** Steps 1, 2 and 5 are
implemented on branch `ui-data-path-perf` (2026-10-09), step 7 likewise;
steps 3, 4 and 6 are open. Per-step status is in §4.

> Steps 1, 2 and 5 were tried live by Shayan on 2026-10-09 and are a clear
> improvement; they have not been measured in Instruments. Step 7 is
> verified by a clean build only — closing and reopening the main window
> during a download is the check still owed.

## 1. Symptom

High CPU in the UI while downloads are active, and sluggish scrolling of the
downloads list when it holds many items. The UI must keep refreshing at
`AppTiming.ticksPerSecond` (5 Hz).

## 2. Verdict

**Refactor the data path; do not rewrite it.** The overall shape — engine
actor → immutable `EngineSnapshot` → main-actor `EngineController` → views —
is sound, and so is the `RangeSet` progress model. The problem is
granularity: everything is rebuilt, compared and observed as a whole, five
times a second, regardless of how little changed.

None of the steps below touches the load-bearing invariants: progress stays
a `RangeSet`, scheduling stays a pure function, views still never touch
engine actors.

> The ranking in §3 comes from reading the code, not from a profiler run.
> One Instruments session (SwiftUI template, one download active in a long
> list) should confirm the order before the engine-side steps are started.

## 3. Findings

File/line references are as of commit `dd9c9f9` (v2.4.1).

| # | Problem | Where | Cost per tick |
|---|---|---|---|
| 1 | `itemTelemetry` is one observable dictionary. Observation tracks it as a single property, so one active download invalidates every telemetry leaf in every realised row (4 per row, 2 per package header, plus the bottom bar and the details panel). | `SDM/EngineController.swift:64` | Row leaf bodies and two `Canvas` draws per row, for rows whose data did not change |
| 2 | `MainWindowView.realBody` reads `controller.snapshot` for the sidebar badge and `overallFraction`, so the whole window body re-evaluates every tick. (The comment at line 152 says this was avoided; it was only avoided for the `.onChange`.) | `SDM/MainWindowView.swift:82`, `:53` | Whole window body |
| 3 | `completedPackages` builds fresh arrays on every body evaluation, so on the Completed tab the `List` gets a new data source every tick. | `SDM/MainWindowView.swift:312` | Full list re-diff |
| 4 | Each package header sums the full speed history of every member item to draw a 60 pt sparkline. | `SDM/PackagesListView.swift:1107` | Up to 3,000 samples × items in the package |
| 5 | Package header and bottom bar make several passes over all items in `body`, with dictionary lookups and `RangeSet.totalBytes` reductions. | `SDM/PackagesListView.swift:1039-1082`, `:1171` | O(items), several times over |
| 6 | `formatted` / `formattedBytes` allocate a new `ByteCountFormatter` on every call. | `SDM/PackagesListView.swift:1804` | 2–3 allocations per row |
| 7 | `publish` deep-compares the whole snapshot, builds a new telemetry dictionary for all items and compares it; `notifyChanges` builds two more dictionaries. | `SDM/EngineController.swift:202`, `:212`, `:270` | O(items) allocations on the main thread |
| 8 | `snapshot()` rebuilds every `ItemSnapshot` and calls `FileManager.fileExists` for every completed item. | `SDMKit/Sources/SDMEngine/DownloadEngine.swift:1142` | One `stat` per completed item, 5×/s |
| 9 | `tick()` calls `itemState(for:)` — a linear scan — once per sampler, and every item has a sampler. | `DownloadEngine.swift:1008`, `:964` | O(items²) |
| 10 | Each item keeps 10 minutes of speed history (3,000 samples) and ships it in every snapshot; a row sparkline draws 48 of them. | `SDMKit/Sources/SDMEngine/SpeedSampler.swift:35` | Copy + trim of the array per active item |
| 11 | `snapshot()` makes about five separate actor hops per running component. | `DownloadEngine.swift:1087-1095` | Minor |

The App scene, menu bar, Touch Bar and sidebar each also re-derive running
counts and the overall fraction by flat-mapping every item
(`SDM/SDMApp.swift:273`, `:314`).

### Why scrolling feels slow

Two things combine; the code alone does not say which dominates:

- **Main-thread bursts at 5 Hz.** Findings 1–7 all run on the main thread,
  so each tick can land in the middle of a scroll frame.
- **Heavy rows.** Each row carries two `Canvas` views, a palette symbol with
  a continuous effect, `.draggable`, `.dropDestination`, a `GeometryReader`
  background and an overlay, all inside a `DisclosureGroup` in a `List`.
  Each row also receives the whole `ItemSnapshot`, including its history
  array.

## 4. Refactor plan

| Step | What | Layer | Status |
|---|---|---|---|
| 1 | Per-item observable models | App | **Done** |
| 2 | Aggregates computed once per tick | App + Engine (pure helpers) | **Done** |
| 3 | Display-sized data in the snapshot | Engine + App | Open |
| 4 | Incremental engine snapshot | Engine | Open |
| 5 | Window-level leaks | App | **Done** |
| 6 | Lighter rows | App | Open |
| 7 | Skip per-item publishing when no window is visible | App | **Done** |

### Step 1 — Per-item observable models

Replace the `itemTelemetry` dictionary with one stable `@Observable` object
per item. `publish` writes only the fields that actually changed, so only
the rows of items that are transferring re-render. Addresses finding 1.

**Done.** `ItemLiveModel` (`SDM/LiveModels.swift`) holds an item's hot
figures; `EngineController.publish` updates each model field only when it
differs and keeps the models in an `@ObservationIgnored` dictionary, so
adding or removing an item invalidates nothing by itself —
`structuralPackages` changing is what tells the list its rows changed.
`ItemRow` looks its model up (an unobserved read) and passes it to the
telemetry leaves, which no longer take a `fallback` snapshot. The details
panel reads the same models. `ItemTelemetry` and `itemTelemetry` are gone.

`publish` now returns immediately when the new snapshot equals the previous
one, which also removes the per-tick telemetry dictionary build from
finding 7. The whole-snapshot compare and `notifyChanges`' dictionaries
remain; those go with steps 3 and 4.

### Step 2 — Aggregates computed once per tick

Package totals, speed, ETA and a windowed history go into a per-package
model; running/queued counts, overall fraction, global speed and history go
into one global stats model. Sidebar, menu bar, Touch Bar, bottom bar and
package headers read those instead of recomputing inside `body`. Addresses
findings 4, 5 and the duplicated flat-maps.

**Done.** The arithmetic is pure and lives in
`SDMKit/Sources/SDMEngine/SnapshotAggregates.swift` (`PackageAggregate`,
`EngineFigures`, `averageRecentSpeed`, `estimatedSecondsRemaining`), covered
by `SnapshotAggregatesTests`. The observable holders are `PackageLiveModel`
and `EngineStats` in `SDM/LiveModels.swift`.

- A package's aggregate is recomputed only when one of its items changed,
  the list structure changed, or it has a running item (its ETA depends on
  the global rate).
- The header sparkline sums the trailing `LiveWindow.packageSparklineSamples`
  (64) samples instead of every item's full history. The drawn result is
  unchanged: the sparkline is 60 pt wide and draws one sample per point.
- `EngineStats` properties are gated individually, so a reader of
  `runningCount` is not invalidated by `overallFraction` moving.
- Readers moved to `stats`: sidebar label and overview block, bottom bar
  speed, menu bar popover header, Touch Bar content, and the App scene's
  menu-bar-icon/Touch-Bar refresh (now three narrow `.onChange` triggers in
  place of one on the whole snapshot).
- The Completed tab's headers sum only the completed items they are handed
  (`showsLiveAggregates: false`), as before; that runs on structural
  changes only.

Still reading `controller.snapshot` directly, deliberately: the menu bar
popover's active-item list, `TouchBarController`, the deletion sheet and the
quit check.

### Step 3 — Display-sized data in the snapshot

The snapshot carries completed bytes as a number and a short history
(about 64 samples). Full history is fetched only for the item selected in
the details panel. Addresses finding 10 and shrinks 7.

### Step 4 — Incremental engine snapshot

Cache `ItemSnapshot` for non-running items and invalidate on mutation. Move
the file-missing check to a slow cadence. Add an id → item index. Read one
telemetry struct per component in a single actor hop. Addresses findings 8,
9, 11.

### Step 5 — Window-level leaks

Move the sidebar's snapshot reads into a leaf view, memoise
`completedPackages`, share one byte-count formatter. Addresses findings 2,
3, 6.

**Done.** The sidebar row is `DownloadsSidebarLabel`, so
`MainWindowView.realBody` no longer reads anything that changes per tick.
`EngineController.completedPackages` is stored and reassigned only when the
filtered list structurally changes. `formatted` / `formattedBytes` share one
main-actor `ByteCountFormatter`.

### Step 6 — Lighter rows

Draw the progress bar and sparkline in one `Canvas`, use a fixed row height
in place of the per-row `GeometryReader`, pass rows only the fields they
show. If scrolling is still slow at thousands of rows after this, the next
move is an `NSOutlineView`-backed list — not before.

### Step 7 — Skip per-item publishing when no window is visible

With the main window closed only the menu bar aggregate is needed.

**Done.** `MainWindowView` reports `scenePhase != .background` — the signal
it already uses to tear its content down — through
`EngineController.setListVisible(_:)`. While hidden, `publish` skips the
per-item and per-package live models; `snapshot`, `stats`,
`structuralPackages` and `completedPackages` stay current, since the menu
bar icon, its popover and the Touch Bar read those. Becoming visible again
refreshes every model from the current snapshot immediately, including
dropping models of items removed in the meantime. The engine still builds a
full snapshot every tick while hidden (notifications need it); trimming that
belongs to step 4.
