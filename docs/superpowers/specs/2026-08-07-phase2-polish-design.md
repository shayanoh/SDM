# Phase 2 polish — design

Seven independent fixes/features, grouped here because they landed together, not because they share mechanism. Each section is self-contained.

## 1. Open file on double-click / context menu

**Behavior:** Double-clicking a `.completed` item row (in either the Downloads or Completed tab) opens the downloaded file with the default application, via `NSWorkspace.shared.open(_:)`. A matching "Open File" entry is added to the item's right-click context menu in `PackagesListView.itemsContextMenu`.

**Scope:** Both affordances are enabled only when `item.state == .completed && !item.fileMissing` — a missing or non-final file has nothing to open. The context-menu entry follows the same single/multi-selection pattern as `Retry`/`Reset Download`: shown once, acts on every eligible selected item (opens each file) when more than one completed item is selected.

**Wiring:** Both live in `PackagesListView.swift` since that's the one shared list component for both tabs (per its existing doc comment) — no duplication between Downloads and Completed needed.

## 2. Minimum segment size setting

**Behavior:** A new setting, "Minimum segment size," in Settings → Downloads. Unit: MB. Range: 1–100. Default: 10 MB.

**Mechanism:** `EngineSettings` gains `minSegmentSizeBytes: Int64` (default `10 * 1024 * 1024`). `DownloadEngine.reconcile()` passes it as `DownloadTask.Configuration.minChunk` instead of the current hardcoded `64 * 1024`. No other code changes: `DownloadTask.stealableRemainder()` already refuses to steal (split) a busy worker's remaining claim unless `remainder.length > minChunk * 2`, which guarantees neither resulting half drops below `minChunk` — that's the exact "don't split below the minimum" rule asked for. Unclaimed free gaps are always handed out whole via `RangeSet.nextClaim`, regardless of size, so a small leftover gap is still claimed by an idle worker even if it's under the minimum — splitting is what's restricted, not claiming.

**Settings UI:** `EngineSettingsStore` gets a `minSegmentSizeMB` key (stored in MB, converted to bytes when building `EngineSettings`), and `SettingsView`'s `downloadsTab` gets a `SteppedNumberField(label: "Minimum segment size (MB)", value: $minSegmentSizeMB, range: 1...100)` alongside the existing concurrency fields.

## 3. Singleton main window and settings window

**Behavior:** At most one main window and at most one settings window may exist at a time. Triggering "Open SDM" (menu bar) or the dock reopen gesture while the main window is already open brings it to the front instead of creating a duplicate.

**Mechanism:** Change both `WindowGroup(id: "main")` and `WindowGroup(id: "settings")` in `SDMApp.swift` to `Window(id: "main")` / `Window(id: "settings")`. `Window` is SwiftUI's single-instance scene type — unlike `WindowGroup`, it has no "New Window" command and `openWindow(id:)` against an already-open `Window` scene activates the existing window rather than opening another. `.windowResizability(.contentSize)` on the settings scene carries over unchanged.

## 4. Non-resumable stop/resume restart correctness

**Verification finding:** `DownloadTask.prepare()` already handles this correctly at the byte level — it only trusts a resume sidecar when `acceptsRanges == true`; for a non-resumable origin it unconditionally deletes the `.incomplete` file and any sidecar and starts the actual on-disk bytes at zero. This part needs no change.

**Bug found and fixed:** `DownloadEngine.finish()` writes the old task's partial `completedRanges` into `item.completed` when a running non-resumable item is user-stopped (the only way a non-resumable running item's runner retires, since the scheduler's pass 1 gives it an unconditional claim on its slot). That stale, non-zero `item.completed` then:
- Misrepresents the stopped item's progress bar as "resumable from partway," when restarting will actually discard it.
- Seeds `DownloadEngine.reconcile()`'s `sampledBytes[itemID]` baseline (via `completedBytes(of:)`) at that stale non-zero value on the next start, so the speed sampler reports 0 B/s until the new attempt's real bytes climb back past the old total — an artificial stall on the graph.

**Fix:** In `finish()`, when the resolved `isResumable == false` and the runner is landing in a non-running terminal state (`.stopped` or `.failed`), reset `item.completed` to an empty `RangeSet` instead of the task's reported partial ranges. `sampledBytes` then naturally re-seeds at 0 on the next `reconcile()` since it reads through `completedBytes(of:)`, which reads `item.completed`. No separate bookkeeping needed.

## 5. Reset Download preserves state; disabled while running

**Bug:** `DownloadEngine.resetDownload()` currently sets `$0.state = $0.isEnabled ? .queued : .stopped` unconditionally — an enabled item is always re-queued (auto-started) regardless of what state it was reset from.

**Fix:** Capture the item's state before mutating, and preserve it: `.queued` stays `.queued`, `.stopped` stays `.stopped` (disabled-item behavior — landing on `.stopped` — is unchanged). `.running` items are stopped first as today (existing `stopRunnerIfRunning` call), but the UI should prevent triggering Reset on a running item in the first place (see below), so a running item reaching this code path shouldn't normally happen via the UI.

**UI:** Add a `canReset` predicate to `PackagesListView.swift` (same shape as `canStart`/`canStop`) that excludes `.running` items, and disable the "Reset Download" context-menu button when no selected item satisfies it — matching the existing per-action `.disabled(...)` pattern used for Start/Stop/Disable/Enable.

## 6. Remove empty ModuleInfo.swift placeholders

`SDMCore/ModuleInfo.swift`, `SDMGrabber/ModuleInfo.swift`, and `SDMEngine/ModuleInfo.swift` are each a single empty enum, explicitly commented as scaffolding "so the target compiles before real types land" for tasks that have since shipped (all three modules have substantial real source now). Delete all three files. Verify `swift build` (and the Xcode project build, since the app target links `SDMKit`) still succeeds with no other file referencing them.

## 7. Clipboard watcher runs independent of window visibility

**Bug:** In `SDMApp.swift`, `clipboardWatcher.start()`/`.stop()` are wired to the main `WindowGroup`'s content `.onAppear`/`.onDisappear`. Closing the main window (which the app is designed to allow, staying resident via the menu bar) stops clipboard watching entirely — silently breaking the core Linkgrabber feature whenever the window isn't open.

**Fix:** Move the watcher's lifecycle out of the window view and into the app scene itself, gated solely on `GrabberSettings.clipboardWatchingEnabled`:
- On launch (in `SDMApp.init` or an app-level `.task`, not tied to any window's appearance), start the watcher if the setting is on.
- When the setting changes in `SettingsView.commit()`, start or stop the watcher to match the new value immediately (today `commit()` only writes the UserDefaults value; it needs to also reach the running `ClipboardWatcher` instance).
- `onLinksDetected` wiring is unaffected — it already checks `GrabberSettings.clipboardWatchingEnabled` itself, so no change there beyond removing the `.onAppear`/`.onDisappear` calls that gated `start()`/`stop()`.

## Testing

Items 2, 4, and 5 touch `SDMKit` (`DownloadEngine`/`DownloadTask`), which has full `FakeOrigin`-based test coverage and the two invariants noted in `CLAUDE.md` (byte identity under churn, no double-claimed ranges) — new/adjusted tests belong in `SDMEngineTests`. Items 1, 3, 6, 7 are app-target UI/wiring changes with no existing test harness (per `CLAUDE.md`, the driver UI has none) — verified manually.
