# Downloads UI overhaul — progress tracking

Decisions locked in with Shayan:
- Deleting a file on disk moves it to Trash (recoverable), never a permanent unlink.
- When the last item in a package is removed, the empty package row disappears from the list too.

## Engine (SDMKit/SDMEngine) — DONE
- [x] `DownloadEngine.removeItem(_:deleteFile:)` — stop runner if running, remove sidecar, trash file if requested, remove item, drop package if now empty, trash empty package folder if requested
- [x] `DownloadEngine.removePackage(_:deleteFiles:)` — stop all runners in package, trash whole package folder if requested, remove package
- [x] `DownloadEngine.resetDownload(_:)` — stop runner, clear sidecar + partial file, reset progress/state to queued
- [x] `DownloadEngine.reorderPackages(_:)` — reorder `packages` array by given id order
- [x] `DownloadEngine.setEnabledForAllItems(_:)` — global pause/resume
- [x] `ItemSnapshot.fileMissing` — true when a `.completed` item's destination file no longer exists on disk
- [x] `PackageSnapshot.completedBytes` — sum of item completed bytes, for package-level "x MB/y MB"
- [x] Engine unit tests for the above (removeItem/removePackage/resetDownload/reorderPackages/fileMissing) — all pass, 253/253 suite green

## EngineController (SDM app) — DONE
- [x] Wrap the new engine calls: `removeItem`, `removePackage`, `resetDownload`, `reorderPackages`, `setAllEnabled`
- [x] Batch variant for multi-select delete (`removeItems`)

## Downloads list UI (ContentView.swift) — DONE
- [x] Remove the URL textbox + Add button from the downloads tab (adding now only happens via Linkgrabber)
- [x] Redesign list as collapsible packages (DisclosureGroup): larger package header, light/dark banding between packages, zebra striping between item rows (macOS's native alternating-content colors)
- [x] Package header shows aggregate progress "x MB / y MB"
- [x] Item row shows its own "downloaded / total" size, no start/stop button — a state icon instead (queued/stopped/running/completed/failed)
- [x] Completed items: no stop/start button anywhere — only a state icon + a missing-file badge when `fileMissing`; restart is "Reset Download" in the context menu
- [x] Right-click context menu per item: Start/Stop, Retry (when failed), Reset Download, Remove from List, Remove and Delete File — via `.contextMenu(forSelectionType:)`, so it scopes to the full selection automatically
- [x] Right-click context menu per package: Remove from List, Remove and Delete Files
- [x] Multi-selection: `List(selection:)` gives native click/shift/cmd-click; Cmd-A selects everything currently listed; Cmd-Backspace deletes the selection **with file trashing** (Finder convention — confirmed with Shayan's earlier Trash decision, not the non-destructive variant)
- [x] Package reordering via drag (`.onMove` on the package-level `ForEach`)
- [x] Global Pause All / Resume All button in the bottom bar

## Linkgrabber integration — DONE
- [x] After "Add to downloads" / "Add and start", clear those links from the Linkgrabber list

## Verification — DONE
- [x] `swift test` in SDMKit passes — 253/253, including 7 new removal/reorder/pause tests
- [x] Manual smoke test in the running app (real persisted data): context menu (single + selection), Cmd-A, Cmd-Backspace confirmation dialog, Pause All/Resume All toggle, missing-file badge, collapsible packages — all confirmed working live
- [x] `swift-format` run on all touched files

## Note for Shayan
Manual smoke-testing ran against the real app (your actual `state.json`, not a sandbox). I toggled **Pause All** then **Resume All** to check the button — that flips every item's enabled flag, so any items you'd individually paused before this session are now enabled again. Nothing was deleted (I cancelled every destructive confirmation dialog before confirming). Worth a glance at your download list to re-pause anything that should stay stopped.

## Round 2 — DONE (per your follow-up requests, not visually re-tested per your instruction)
- [x] Backspace alone now removes the selection from the list only (no file touched); Cmd-Backspace removes *and* trashes the file (unchanged from before)
- [x] Both shown as shortcut hints on their context-menu items ("Remove from List" / "Remove and Delete File(s)")
- [x] **Package delete bug**: found the likely cause — the view had two independent `.confirmationDialog` modifiers (one for item deletion, one for package deletion) stacked on the same view, a known SwiftUI pattern where the second one can silently never present. Unified both into one `PendingDeletion` enum (`.items`/`.package`) driving a single `.sheet(item:)`, so there is exactly one confirmation surface now.
- [x] Confirmation dialog rewritten as a bigger custom sheet (`DeletionConfirmationView`): shows item count, total size, and a scrollable list of filenames before you confirm
- [x] New setting **"Resume downloads automatically when SDM opens"** (Settings → Downloads), off by default. When off, `EngineController.startHeartbeat()` forces every restored item's enabled flag off right after `restore()`, so nothing pulls bytes just because the app launched.
- [x] Start/Stop context-menu items are now disabled when meaningless (Start disabled unless the item is genuinely stopped; Stop disabled unless it's running or queued-and-enabled)
- [x] Grammar fix: multi-selection now shows "Start All" / "Stop All" instead of "Starts" / "Stops"
- [x] Completed tab rows now have a right-click menu: Reset Download, Remove from List, Remove and Delete File

Build succeeds and the 253-test SDMKit suite still passes. Not re-verified visually in the running app per your request — over to you.
