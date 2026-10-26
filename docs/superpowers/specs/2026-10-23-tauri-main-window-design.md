# Tauri main window (spec 2)

Status: **built 2026-10-24** (spec approved 2026-10-23; Shayan: "go on with planning and then execute it in-line in current branch", two phases; see the roadmap log for what is unverified).
Roadmap: [docs/tauri-roadmap.md](../../tauri-roadmap.md), spec 2 of 6. Builds on
[spec 1](2026-10-14-tauri-foundation-design.md).

## 1. Intent

The Tauri main window must look and behave **as exactly as possible like the SwiftUI main window**
(`SDM/MainWindowView.swift`, `SDM/PackagesListView.swift`), on Windows first, then Linux and macOS (test host).
Same layout, same states, same menus, same shortcuts, same drag and drop. Where the two platforms differ
(icons, font, file-manager wording, modifier key, window effects) section 2 says how.

The reference is eleven screenshots of the real SwiftUI window in
[assets/2026-10-23-swiftui-main-window/](assets/2026-10-23-swiftui-main-window/), taken at 1300 × 960 pt
with the Deep Purple theme and fake data (the fixture generator is described in section 11). They are the
acceptance targets.

## 2. Decisions

| # | Decision | By |
|---|---|---|
| M1 | **Two phases in one spec.** Phase A extends the Rust controller (`sdm-app`) and the bridge; Phase B builds the window. Phase B starts only when A's tests pass. | Shayan |
| M2 | Icons are **Lucide** (open licence), one per SF Symbol (table in section 6.1). The font is the system UI font (`system-ui`: Segoe UI on Windows). SF Symbols and San Francisco cannot ship off Apple platforms. | Shayan |
| M3 | Only the default theme (`dark` / `light`, following the OS) is wired in spec 2; every color is still a `var(--role)`. The theme picker and the other themes are spec 4. The reference screenshots use Deep Purple, so parity checks load `deep-purple` through the existing `themeCss(themeId)` call. | Shayan |
| M4 | The toolbar is an in-content bar directly under the native title bar (window chrome stays OS-native, T2), with the same 13 buttons in the same order. | Shayan |
| M5 | The Linkgrabber sidebar row is a placeholder in spec 2 (it is selectable and shows an empty-state message). The view is spec 3. | Shayan |
| M6 | Shortcuts use Ctrl on Windows and Linux, Cmd on macOS. "Show in Finder" reads "Show in Explorer" (Windows) or "Show in File Manager" (Linux). Delete goes to the Recycle Bin / Trash (the engine's `Trash`). | Shayan |
| M7 | The three native alerts (delete confirmation, "Pause non-resumable download?", "Move to New Package" name prompt) become in-app modals styled like the macOS ones. | Shayan |
| M8 | *(builder's call)* The Completed view is **derived in the frontend** from the same structure and figures patches (completed items only, packages with no completed items hidden, header sums over just those items). It is not a second controller query. | me |
| M9 | *(builder's call)* Window size and position persist (`tauri-plugin-window-state`). Panel height, panel visibility, collapsed packages and selection do not (they reset on restart, as in SwiftUI's `@State`). | me |
| M10 | *(builder's call)* Mica (Windows 11) with Acrylic fallback (Windows 10) on the sidebar and toolbar regions via `window-vibrancy`; Linux and macOS get solid theme colors. This is the last task in the plan and is only compile-checked here: the visual check happens in the user's Windows VM. | me |
| M11 | *(builder's call)* The Tauri window sets `dragDropEnabled: false` so HTML5 drag and drop works inside the webview on Windows. OS file drops are not used until spec 3 revisits it. | me |
| M12 | *(builder's call)* Item actions in the controller take **lists of ids**, one call per user gesture, so one gesture is one engine publish and one patch burst (SwiftUI loops `await` per item). The single-id commands from spec 1 are replaced by the list versions. | me |

## 3. Goals, non-goals, anti-goals

**Goals.** Visual and behavioral parity with the 11 screenshots; every action reachable from the
toolbar, the context menus and the shortcuts that SwiftUI offers; the spec-1 performance gate still
passes (idle ≈ 0 CPU, 50 active items below the recorded threshold, the list windowed so thousands of rows cost nothing).

**Non-goals.** Linkgrabber view, Add Links sheet, Settings and the theme picker, tray and notifications,
clipboard watcher, activation modes, the Touch Bar (T4), the Chrome-extension setup dialog, "ignore own
clipboard write" (spec 5; spec 2 copies through the webview clipboard API).

**Anti-goals.** A frontend that holds business logic (T6): eligibility rules that SwiftUI keeps in the
view (`canStart`, `canStop`, …) are pure functions in one TypeScript module with a fixture table, and
everything that touches the engine or the disk goes through the controller. A list that re-renders every row per tick
(spec 1 solved that; do not regress it). A color literal anywhere in a component.

## 4. Phase A: controller (`sdm-core/crates/sdm-app`)

All new `App` methods are `async`, parse ids with `parse_id`, call the engine, then `publish(false)`,
exactly like the existing ones. Every one gets a Rust test in `sdm-app` against the fake origin or the
engine's existing test doubles (no network, no real clock).

### 4.1 Actions

| `App` method | Engine call | Notes |
|---|---|---|
| `start_items(ids)` | `start_item` per id | |
| `stop_items(ids)` | `stop_item` per id | |
| `retry_items(ids)` | `retry` per id | |
| `retry_mux_items(ids)` | `retry_mux` per id | |
| `reset_items(ids)` | `reset_download` per id | Refused by the engine for a running item; the controller skips such ids rather than failing the batch. |
| `set_items_enabled(ids, enabled)` | `set_enabled` per id | |
| `remove_items(ids, delete_files)` | `remove_item` per id | Replaces `remove_item`. Also drops the ids from `detail_items`. |
| `remove_package(id, delete_files)` | `remove_package` | |
| `move_items(ids, package_id, at_index: Option<u32>)` | `move_items_to_package` | `at_index` is against the pre-move list, as in SwiftUI. |
| `move_items_to_new_package(ids, name)` | `move_items_to_new_package` | Name trimmed; empty is `AppError::invalid`. |
| `reorder_items(ids, package_id)` | `reorder_items` | "Sort Items by Name" computes the order in the frontend (natural, case-insensitive compare) and sends this. |
| `reorder_packages(ids)` | `reorder_packages` | |

A bad id rejects the whole call and changes nothing (same rule as `set_detail_items`).
`start_item`, `stop_item`, `retry_item`, `remove_item` from spec 1 are replaced by the list versions
(commands, bindings, `TauriBridge`, `MockBridge`, tests and the spec-1 skeleton UI all follow).

### 4.2 Disk and OS

- `describe_deletion(target: DeletionTarget) -> DeletionInfo` where `DeletionTarget` is
  `{ kind: "items", ids } | { kind: "package", id }` and `DeletionInfo { title, totalBytes, names }`.
  Port of `MainWindowView.deletionInfo(for:)`: walk the output file and every part file of each item
  (`part_filenames`) plus each `.incomplete` sibling in the package's destination folder, sum the **actual**
  on-disk sizes, list the actual file names sorted. `title` is `"Remove From List?"` when no file exists, else
  `"Remove From List And Delete Files?"`; a package target with no matching package returns
  `"Delete Package From Disk?"` with nothing. Pure over a directory, so it is tested with a temp dir.
- `open_item(id)`: opens the destination file with the OS default application. No-op unless the item is `Completed` and not `file_missing`.
- `reveal_item(id)` and `reveal_package(id)`: show the item's file / the package folder in the file manager (existing `Host::reveal_in_file_manager`; for a package, the folder itself is opened).
- `Host` gains `open_path(&Path)`; `TauriHost` uses `tauri_plugin_opener::open_path`; `FakeHost` records calls.
- Copy to clipboard stays in the webview (`navigator.clipboard.writeText`).

### 4.3 Protocol additions (`protocol.rs`)

- `StructurePackage.folder: String`: the absolute destination folder (download folder joined with the package name), shown in the details panel and used by Open/Show.
- `StructureItem.assembly: AssemblyDto` (`none` | `mux`), for the panel's "Assembly" row and "Retry Mux" eligibility.
- `ItemFigures.remaining_attempts: Option<u32>`, for the row's "Retrying in Ns · N attempts left".
- `EngineStats` already carries everything the sidebar and bottom bar need.

`bindings.ts` is regenerated (`cargo test -p sdm-tauri export_bindings`); `Bridge`, `TauriBridge` and `MockBridge` gain every new call, and `MockBridge` implements them with the same semantics so the UI is checkable in a plain browser (including reorder and move).

## 5. Phase B: window (`sdm-tauri/web`)

### 5.1 Structure

```
Shell            NavigationSplit: Sidebar | detail
  Sidebar        Downloads (badge + ring), Completed, Linkgrabber (placeholder), Overview block
  Toolbar        13 icon buttons + Info Panel toggle (title bar region)
  Detail         PackageList + (DetailsPanel, drag handle) + BottomBar   (Downloads and Completed share it)
Modals           DeleteConfirm, PauseConfirm, NewPackage   (one slot, one modal at a time)
ContextMenu      one component, item / selection / package variants
```

Downloads and Completed render through the **same** `PackageList` component with three props
(`allowsReordering`, `showsPauseResume`, `showsLiveAggregates`), exactly like SwiftUI, so they cannot drift.

### 5.2 Measurements (CSS px = SwiftUI pt)

Tuned against the screenshots during the plan's parity task; the starting values come from the SwiftUI source.

- Window: default 1000 × 640, minimum 760 × 480. Sidebar 200 min, 220 ideal, draggable divider.
- Fonts: body 13, caption 10, title3 15 (package name bold, bottom-bar speed), headline 13 bold.
- Item row: icon column 20 wide, gap 8, padding 4 vertical and 4 horizontal inside the list inset; three lines
  (name + telemetry, 6 px progress bar, status + ETA + bytes), about 61 px tall.
- Package header: name (title3 bold), bytes caption, ETA, speed text and a 60 × 20 sparkline on the right, about 53 px tall.
- Zebra: item rows alternate `surface-primary` / `surface-secondary`; package headers use `surface-secondary` at 0.5 / 0.9 opacity by package index; selected rows use `selection-tint` mixed with black 20 % (items and headers), the selected sidebar row plain `selection-tint`.
- Details panel: default 150 high, clamped 100 to 600, resized by a 40 × 5 capsule handle at its top edge (resize cursor); background `surface-secondary`; the content is a two-column grid with 10 px column gap and 4 px row gap.
- Bottom bar: padded 16, `surface-secondary`, Pause All / Resume All button, divider, global speed (title3, monospaced digits), `N packages, M items` on the right in `text-secondary`.

The list is windowed with **two row heights** (header, item): `windowing.ts` generalizes from one `rowHeight` to a prefix-sum over per-row heights. Selection, hover, drag targets and the context menu live on the row slot, so the rule "nothing between the list and a row's selection and drag wiring" holds by construction.

### 5.3 Behavior

- **Selection** is one set of ids holding item ids and package ids, as in SwiftUI. Click selects one; Ctrl/Cmd-click toggles; Shift-click selects the range between the anchor and the clicked row in list order; clicking a package header selects the package (its items count as selected for actions and the panel). Selection is cleared when the tab changes. Ctrl/Cmd+A selects every item in the current tab.
- **Eligibility** (pure module `eligibility.ts`, ported from `canStart` … `canReset`): start = stopped and enabled; stop = running or queued; enable = disabled; disable = enabled and not completed; retry = failed; retry mux = failed and `assembly = mux`; open = completed and not missing; reset = not running. A menu entry is disabled unless some selected item qualifies; the action runs on the qualifying subset.
- **Context menu**: right-click an unselected row selects just it; on a selected row it acts on the whole selection. Item menu (with dividers in this order): Open File (Enter), Show in Explorer (Ctrl+Enter), Copy URL(s) to Clipboard | Start, Pause | Enable/Enable All, Disable/Disable All, Retry Failed Item(s), Retry Mux, Reset Download(s) | Move to New Package, Remove from List (Delete), Remove and Delete File(s) (Ctrl+Delete). Right-clicking a package header opens this same menu and acts on all of the package's items (the header is selected like a row, as the SwiftUI app does). The package-only entries (Sort Items by Name, Remove Package) were dropped on 2026-10-24 at Shayan's request. The toolbar's 12 action buttons are the same items without dividers, then Info Panel; they act on the selection and grey out the same way.
- **Pause of a non-resumable running item** asks first ("Pause non-resumable download?", body "One or more downloads cannot be resumed. Pausing them now will lose their progress permanently.", Pause / Cancel). The decision is a pure function over the selection; no controller call.
- **Remove and Delete** opens the delete modal with `describe_deletion`; confirm sends `remove_items(ids, true)` or `remove_package(id, true)`; Cancel does nothing. The modal shows the trash icon, the title, `N files · SIZE will be moved to the Trash.` (or `No files will be moved to the Trash`), a scrolling file list (max 180 high, middle truncation), Cancel (Esc) and Move to Trash / Remove from List (Enter).
- **Remove from List** (Delete key, menu) sends `remove_items(ids, false)` and clears the selection.
- **Drag and drop** (HTML5): every item row and package header is draggable and a drop target when `allowsReordering`. Payload kinds are item and package; a mixed payload is refused. Item drop on an item row inserts before it (top half) or after it (bottom half), into that row's package (`move_items(ids, package, index)`); item drop on a header appends to that package; package drop on a header reorders (before the target on the top half, after it on the bottom half). Dragging a selected row drags the whole selection, ordered as it appears in the list. The target shows a 2 px `accent` rounded outline (a whole-row highlight, as in SwiftUI, not an insertion line). Only `queued`-eligible moves succeed in the engine; the UI offers drag only on the Downloads tab.
- **Keyboard**: Enter opens the selected completed items, Ctrl/Cmd+A, Delete, Ctrl/Cmd+Delete, Esc closes a modal or menu, arrow keys move the selection by one row, Ctrl/Cmd+Enter shows in the file manager. Shortcuts are ignored while a modal is open or a text field has focus.
- **Sidebar**: Downloads row shows, while anything runs, a capsule badge `running/(running+queued)` (`status-online` background) and a circular progress ring of `overallFraction` (`accent`); (the SwiftUI icon breathes while downloading; the Tauri build dropped that animation and the one on running rows, because continuous CSS animations cost about 7 points of CPU in WKWebView, see [tauri-performance.md](../../tauri-performance.md)). Completed and Linkgrabber (count badge, 0 in spec 2) are plain rows. Overview: `No running downloads` (headline, `text-secondary`) or the global speed (headline, monospaced digits), a 40 px bandwidth graph (area at 25 % opacity under a 1.5 px running-average line, windowed to its pixel width, same math as `BandwidthGraph.swift`), `N active` (caption).
- **Details panel**: none selected, "Select an item to view its details here"; one item, the sparkline (48 high) with the speed, the segmented bar, then Status, Filename (Open), Assembly (mux only), Destination Folder (Show in Explorer), URL (Copy URL), Media Info, Progress (`DONE/TOTAL - P.P%`), Segments, Can Resume?; several items, the summed sparkline, segmented bar of the union of ranges, Item States (colored counts), Destination Folder when they share one, Total Progress, Total Segments. Status text and colors are ported from `statusDescription` / `statusColor`. The panel calls `setDetailItems` for exactly the items it shows and clears it when hidden.
- **Row text**: ported verbatim from `ItemRowBody.describe`, `resumabilityBadge` (`not resumable`), the `file missing` label, strikethrough and 0.55 opacity for disabled items, the state icon table (section 6.1) with a ring whose arc is the completed fraction for running, queued and stopped, the segment readout `A/C seg`, speed, 48 × 16 sparkline, ETA, `DONE / TOTAL`.
- **Formatting** matches `ByteCountFormatter` binary style: 1024 base; `Zero KB` for 0; bytes under 1 KB as `N bytes`; KB with 0 fraction digits, MB with 1, GB and TB with 2, trailing zeros dropped (`5.59 GB`, `629.4 MB`, `4 MB`); speed appends `/s`. ETA is `formatTimeIntervalForEta` ported exactly (days/hours/minutes/seconds rules, `A few seconds`, `Less than a minute`, `Soon`).
- **Segmented progress bar**: a canvas, 6 px high, fully rounded track in `surface-tertiary` at 0.6 opacity, completed ranges as rectangles (minimum 0.5 px) in `progress-fill`, clipped.
- **Sparkline**: one 1 px `graph-stroke` path, y-scaled to its own max, windowed to its pixel width and left-padded with zeros, as `Sparkline.swift`.
- **Window title** "Shayan's Download Manager". `setListVisible` follows `document.hidden` (already wired).

### 5.4 Persistence and window effects

`tauri-plugin-window-state` (size, position, maximized; label `main`). Vibrancy per M10: on Windows the shell calls `apply_mica` (fallback `apply_acrylic`, then nothing) after window creation, sets `data-vibrancy="on"` on `<html>` through a small command, and the CSS then lets the sidebar and toolbar backgrounds be `color-mix(in srgb, var(--sidebar-background) 80%, transparent)`; with the attribute absent the same backgrounds are opaque. The window is created transparent only on Windows.

## 6. Reference material

### 6.1 SF Symbol → Lucide

| Use | SF Symbol | Lucide |
|---|---|---|
| Sidebar Downloads / row running | `arrow.down.circle` | `circle-arrow-down` |
| Completed / row completed | `checkmark.circle` | `circle-check` |
| Linkgrabber | `link` | `link` |
| Row queued | `clock.circle` | `clock` (in a ring) |
| Row stopped | `pause.circle` | `circle-pause` |
| Row failed / retrying | `exclamationmark.arrow.trianglehead.counterclockwise.rotate.90` | `rotate-ccw` with `circle-alert` fallback |
| Row assembling | `wand.and.stars` | `wand-sparkles` |
| Open File | `eye` | `eye` |
| Show in Finder | `folder` | `folder-open` |
| Copy URL | `square.and.arrow.up.on.square` | `copy` |
| Start | `play` | `play` |
| Pause | `pause` | `pause` |
| Enable / Disable | `lightswitch.off` / `lightswitch.on` | `toggle-left` / `toggle-right` |
| Retry Failed Item | `exclamationmark.arrow.trianglehead.2.clockwise.rotate.90` | `refresh-cw` |
| Retry Mux | `arrow.trianglehead.clockwise` | `rotate-cw` |
| Reset Download | `arrow.left.to.line` | `arrow-left-to-line` |
| Move to New Package | `folder.badge.plus` | `folder-plus` |
| Remove from List | `eraser.line.dashed` | `eraser` |
| Remove and Delete | `trash` | `trash-2` |
| Info Panel | `info.circle.text.page` | `panel-bottom` |
| Pause All / Resume All | `pause.fill` / `play.fill` | `pause` / `play` filled |

### 6.2 Reference screenshots

| File | State |
|---|---|
| `01-downloads-running.png` | Downloads, 3 running, queued, disabled (struck through), failed, no selection |
| `02-item-selected-details.png`, `03-details-panel-tall.png` | one running item selected, panel at default and at 380 |
| `04-context-menu-item.png`, `05-context-menu-package.png` | the two menus |
| `06-multi-select-details.png` | three running items selected, aggregate panel |
| `07-delete-sheet.png` | delete confirmation |
| `08-completed-tab.png`, `09-completed-item-details.png` | Completed view, panel with a completed item |
| `10-package-collapsed.png` | first package collapsed |
| `11-info-panel-hidden.png` | panel hidden |

Not captured (built from the SwiftUI source): the drag-target outline, the two other alerts, the hidden-sidebar state, light mode.

## 7. Testing

- **Rust** (`cargo test -p sdm-app`): one test per `App` method in 4.1, `describe_deletion` against a temp directory (output file, `.incomplete`, part files, missing files, package target, unknown package), `open_item` / `reveal_*` through `FakeHost`, protocol additions in the differ tests (a changed `folder` or `assembly` produces a structure patch). Windows and Linux compile gates stay green.
- **Vitest**: formatting (`ByteCountFormatter` table from the screenshots: `0 → Zero KB`, `5.59 GB`, `629.4 MB`, `3.7 MB/s`, `Zero KB/s`; ETA cases from `Helpers.swift`), eligibility fixture table, selection logic (click, toggle, range, package select), drag payload ordering and drop-index math, windowing with two heights, sparkline and bandwidth windowing, details-panel aggregate math, the Completed derivation, the delete-info title rule.
- **Playwright** against `MockBridge`: the 11 reference states reproduced and screenshotted, compared by eye against the references at the same size (the check is a recorded step in the plan's final task, not an automated pixel diff, because fonts and icons differ by design); context menu contents and disabled states; shortcuts; drag reorder and move; delete modal flow; pause-confirm flow.
- **Performance**: the `--simulate` harness numbers from spec 1 are re-measured on macOS and must still meet the spec-1 thresholds ([tauri-performance.md](../../tauri-performance.md)).

## 8. Build order (the plan follows it)

Phase A: protocol additions → list actions → package/organize actions → `describe_deletion` → host open/reveal → commands, bindings, bridges, mock → spec-1 skeleton UI migrated to the new calls.
Phase B: formatting and eligibility modules → theme/typography/icons → windowed two-height list → rows, headers, selection → sidebar and overview → toolbar and context menus → details panel → modals → drag and drop → shortcuts → window state → vibrancy → parity pass against the screenshots and the performance re-measure → roadmap and docs.

## 9. Risks

- HTML5 drag and drop inside WebView2 needs `dragDropEnabled: false` (M11); verify early on the Windows VM.
- Lucide has no variable-value symbols; the state ring is drawn by hand (small SVG).
- Mica needs a transparent window and breaks if any ancestor paints an opaque background; isolated in the last task.
- The Windows VM cannot be driven from here; Windows-specific behavior (drag, Mica, shortcuts) is built to compile and is checked by the user, as in spec 1.

## 10. Left to the builder

Exact CSS values until they match the screenshots, component file split, the Lucide package choice (verified against current docs), test names, and the internal shape of `DeletionTarget` on the wire.

## 11. Fixture used for the reference screenshots

A throttled, Range-capable fake HTTP origin plus a generated `state.json` in the legacy flat item shape (five
packages, 22 items: running, queued, stopped, disabled, failed, non-resumable, completed with real files) was
run against the installed SwiftUI release build with `autoStartDownloadsOnLaunch` on. It is not committed (it
touches `~/Downloads` and the app's real data directory); `--simulate` in the Tauri app is the equivalent for
this side.
