# Touch Bar controls

**Status: IMPLEMENTED.** A learning/experimental feature — Touch Bar hardware
only exists on 2016–2020 Intel MacBook Pros, so this has a tiny real audience,
but the app must still run correctly (Touch Bar API calls are inert no-ops) on
every other Mac.

## Goal

Show live download status and a pause/resume control in the Touch Bar
whenever SDM is the active app, without needing a window focused.

## Scope

One new file, `SDM/TouchBarController.swift`, plus a few lines of wiring in
`SDMApp.swift`. No engine or `SDMKit` changes — this is a pure read/command
consumer of `EngineController.snapshot`, `pauseAll()`, and `resumeAll()`.

## Where the bar lives

`NSApp.touchBar` (a `var` on `NSResponder`, which `NSApplication` inherits) is
set directly, rather than overriding `touchBar` on any window or view
controller. None of SDM's windows currently provide a custom `touchBar`, so
`NSApp`'s is the one AppKit falls back to at the tail of the responder chain
— it becomes the effective Touch Bar regardless of which SDM window (if any)
is key, satisfying "show whenever SDM has focus, not tied to one window."

Setting `NSApp.touchBar` on a Mac with no Touch Bar hardware (the normal case
today) and in the Touch Bar Simulator both work identically — the OS either
renders it or ignores it. No availability guards are needed.

## Content: one `NSHostingView`, not custom `NSView` drawing

Rather than hand-drawing the ring/sparkline with Core Graphics, the single
`NSCustomTouchBarItem`'s view is an `NSHostingView` wrapping a small SwiftUI
view (`TouchBarContentView`) that reuses the exact views/formulas the sidebar
already uses:

- the system circular `ProgressView` (same as `MainWindowView`'s sidebar row)
- `Sparkline` (`SDM/Sparkline.swift`), fed `controller.snapshot.globalHistory`
- `formatted(_:)` / `formattedBytes(_:)` (`PackagesListView.swift`) for the
  speed and byte-count text

`TouchBarContentView` reads `EngineController` and `ThemeStore` via
`.environment(_:)`, injected once when the hosting view is built — the same
`@Observable` machinery that already drives the sidebar keeps it live with no
manual per-tick redraw code. `TouchBarController.update()` is only
responsible for deciding whether the bar should be installed at all (see
below); once installed, its content updates itself.

Layout, left to right, one `HStack` at a fixed width (480pt — the Touch Bar
has no scrolling and no true "fill parent," so a generous fixed width stands
in for "full width"):

1. Pause/resume icon button (SF Symbol only, no text)
2. Circular progress ring
3. "`x MB / y MB`" downloaded/total text
4. Sparkline (`maxWidth: .infinity` — the one flexible element)
5. Current averaged speed text

## Three display states

Re-evaluated in `TouchBarController.update()`, called from the same
`.onChange(of: controller.snapshot)` hook `SDMApp.swift` already uses to
refresh the menu bar icon (`SDMApp.swift:271-273`) — no separate timer.

1. **Downloading** — any item `.running`. Full content: pause icon, ring,
   byte-count text, sparkline, speed.
2. **Idle, work pending** — nothing running, but at least one item is
   `isEnabled && state != .completed`. Resume icon + "`N packages, M items
   waiting`" text, where N/M are counted over exactly that filter (a
   `.failed` item still counts as waiting; a disabled item never does).
3. **Fully idle** — neither of the above. `NSApp.touchBar = nil`, releasing
   the strip back to the system Control Strip default.

The pause/resume icon's shown state and its action are independent of which
of states 1/2 is active: it mirrors `PackagesBottomBar` in
`PackagesListView.swift:1130-1194` exactly — `downloadableItems` is every
item in `{.queued, .running, .stopped}`; the button shows "resume" (calls
`controller.resumeAll()`) when all of those are `.stopped`, otherwise "pause"
(calls `controller.pauseAll()`); disabled when `downloadableItems` is empty.

The ring's fraction and the "`x MB / y MB`" text both sum over the same item
set as `SDMApp.overallFraction` (`SDMApp.swift:310-317`): items in
`{.running, .queued, .completed}` with `totalBytes > 0`. The ring averages
`fractionCompleted` unweighted (matching the sidebar); the byte text sums
`completed.totalBytes` and `totalBytes` across that same set.

## Wiring

`SDMApp` gets one more `@State private var touchBarController =
TouchBarController()`, configured once in `init()` the same way
`appDelegate.controller = engine` is (weak refs to `EngineController` and
`ThemeStore`, both already app-lifetime `@State`). The existing
`.onChange(of: controller.snapshot)` block that updates
`menuBarIconController` gets one more call: `touchBarController.update()`.

## Testing

No new unit tests — this is AppKit/Touch-Bar glue with no new business
logic (every filter/formula here already exists and is tested via the
sidebar's equivalents). Manual verification (Touch Bar Simulator and real
hardware) is left to the user, not automated as part of this change.
