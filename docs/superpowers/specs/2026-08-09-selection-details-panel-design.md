# SDM — Selection Details Bottom Panel: Design

Date: 2026-08-09
Status: Approved design, pre-implementation

## 1. Purpose

A closable, resizable bottom panel in the main window's Downloads and Completed tabs that shows details about the current selection. A single selected item gets its full metadata grid (URL, on-disk path, sizes, state, speed, ETA, segments, resumability, warnings). A package or multi-item selection gets a rolled-up aggregate (total/completed size, state counts, aggregate speed + ETA, a wide speed graph, and a progress bar). This is a pure UI addition — it reads existing snapshot and telemetry data and touches **no code in SDMKit**.

## 2. Placement and mechanics

The panel lives inside `PackagesListView`, which is the one shared list component both tabs already render through — so building it there gives both tabs the panel with no per-tab copies.

```
VStack(spacing: 0) {
    list
    if isDetailsOpen {
        Divider()
        SelectionDetailsPanel(...)          // resize handle + scrollable content
    }
    Divider()
    PackagesBottomBar(..., isDetailsOpen: $isDetailsOpen)   // gains the chevron toggle
}
```

- **Toggle.** A chevron button (▴ / ▾) on the right side of the existing bottom bar, next to the "N packages" count. The same control opens and closes the panel; it only ever reflects a user choice (no auto-open on selection). The existing Pause All button, live speed, and package count are untouched.
- **Resize.** A drag handle along the panel's top edge (vertical-resize cursor). Height is clamped to a minimum of ~120 px and a maximum of about half the list area; default ~200 px.
- **Scroll.** The panel's content sits in a `ScrollView` so content taller than the panel scrolls instead of stretching it.
- **Persistence.** Open/closed state and the last height are remembered **per tab** and stored in `UserDefaults` (e.g. `detailsPanel.downloads.open` / `.height`, and `.completed.` variants). Closing it once keeps it closed next launch; the two tabs are independent.
- **Empty selection.** With the panel open and nothing selected, a subtle placeholder reads "Select a download to see details."

## 3. Selection model and resolution

Selection is the existing `selectedItemIDs` / `selectedCompletedItemIDs` — a `Set<UUID>` holding both item ids and package ids (package headers are tagged into the same set). The panel resolves that set to the items it actually shows.

**Resolution (a pure function):** the resolved items are the explicitly selected item ids that still exist in the presented packages, **plus** every item inside any selected package. Selecting a package means "everything in it," regardless of which rows are collapsed.

**Mode by resolved count:**

| Resolved count | Mode |
|---|---|
| 0 | placeholder |
| 1 | single-item details |
| ≥ 2 | aggregate details |

A package with at least one item always resolves to ≥ 1 item (empty packages are dropped by the engine, so a selected package always contributes at least one), so single-item mode genuinely means "one item, no packages."

## 4. Single-item view

A key/value metadata grid, then a full-width speed graph, then the segmented progress bar directly below it.

**Metadata grid** (label → value; all from the snapshot / telemetry, nothing new):

| Field | Source |
|---|---|
| Filename | `ItemSnapshot.filename` |
| Location (on-disk path) | `EngineController.destinationURL(for:inPackage:)`, truncated middle |
| Source URL | `ItemSnapshot.url`, truncated middle, clickable to copy |
| State | `ItemSnapshot.state` (+ "disabled" suffix when `!isEnabled`; failed reason when failed) |
| Size | completed / total bytes (live via telemetry), or "Unknown" when total is `nil` |
| Progress | percent |
| Speed | live bytes/sec |
| ETA | derived, remaining ÷ speed, "—" when speed ≤ 0 or total unknown |
| Segments | active / configured |
| Resumable | Yes / "not resumable" (warning) / "not yet probed" for `nil` |
| Retries remaining | `remainingAttempts` when non-`nil` |
| Warnings | `checkpointFailure`, `fileMissing` |

**Graph + progress.** A `BandwidthGraph` (filled area + average line, already used in the sidebar) renders the item's live speed history across the panel width, with the item's **segmented** progress bar (`SegmentedProgressBar`, rendering its `RangeSet`) directly below it.

## 5. Aggregate view

Rolled-up totals only — no per-item breakdown (the list above already shows per-item progress).

**Header.** "N files", plus "· M packages" when at least one package is in the selection.

**Grid.**

| Field | Source |
|---|---|
| Total size | completed / total, summed |
| Speed | sum of live item speeds |
| ETA | remaining ÷ speed over the summed figures, "—" when no running items / speed 0 / totals unknown |

**State-counts table.** One row per state — Completed, Running, Queued, Stopped, Failed — with counts (zero counts rendered dimmed). *Stopped is included even though the original request listed four states, because it is a real, observable state; all five rows keep the table complete.*

**Graph + progress.** A `BandwidthGraph` of the summed speed history (aligned to the trailing edge and front-padded with zeros, the same algorithm `PackageSnapshot.bytesPerSecondHistory` uses) across the panel width, with a **solid** (non-segmented) roll-up progress bar of aggregate completed/total directly below it. Segmented rendering is per-file only, so it stays in single-item mode.

## 6. Data sources

- **Structural** (`ItemSnapshot` / `PackageSnapshot`): filename, url, state, enabled, resumable, checkpoint failure, retries, missing-file flag, and the per-item fields used as a telemetry fallback.
- **Live telemetry** (`EngineController.itemTelemetry`): completed `RangeSet`, total bytes, active/configured segments, bytes/sec, speed history — read with a fallback to the structural item's own value, exactly like the existing `ItemTelemetryFields` leaves.
- **Derived in the UI**: on-disk path (via existing `destinationURL`), ETA (remaining ÷ speed, guarded), and every aggregate (counts, size sums, summed history).
- **No new snapshot fields.** No `priority` — it is not surfaced anywhere in the UI yet, so the engine and snapshots stay untouched.

## 7. Code organization (app target only)

Everything lives in the SDM app target; SDMKit is not modified.

- **`SDM/SelectionDetails.swift`** (new): a pure `SelectionSummary` value type and a `selectionSummary(packages:selection:telemetry:)` builder that resolves the selection, picks the mode, and computes counts / sizes / speed / ETA / aligned history. Plus the panel views: `SelectionDetailsPanel` (resize handle + `ScrollView` container), `SingleItemDetails`, and `AggregateDetails`.
- **Live reads are leaf subviews.** The panel's ticking fields (speed, graph, progress, completed/total sizes) read `controller.itemTelemetry` in their own `body` — the same pattern as `ItemTelemetryFields` / `PackageHeaderSparkline` — so the panel never forces `PackagesListView.body` (and hence the `List`, and any in-flight drag) to re-evaluate per tick. The structural wrapper reads only the selection set and the structural packages.
- **`PackagesBottomBar`** gains the chevron toggle (a binding to open state). **`PackagesListView`** owns the per-tab `@State` open/height, seeded from and written to `UserDefaults`.

## 8. Testing

The pure aggregation is unit-testable with no network and no clock, and `SDMTests` is an app-hosted Swift Testing target (`@testable import SDM`), so the tests live there:

- Selection resolution: single item; package expands to its items; multiple items across packages; mixed item + package; empty selection; ids that no longer exist are filtered out.
- Counts by state; total/completed size sums; aggregate speed sum; history alignment/front-padding.
- ETA guards (unknown total, zero speed → "—").
- The panel views themselves get no snapshot tests, consistent with spec §11.7; they are verified manually.

## 9. Out of scope

- Priority in the details grid (not implemented in the UI yet; revisit when it lands).
- Per-item breakdown table in aggregate mode (pure aggregate chosen deliberately).
- The panel in the Linkgrabber tab or the menu-bar popover.
- A persisted "always show panel" preference — the toggle and remembered state cover this.
