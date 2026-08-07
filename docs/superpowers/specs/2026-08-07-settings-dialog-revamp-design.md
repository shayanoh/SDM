# Settings dialog revamp — design

Status: approved, implementing directly on `main`.

## Problem

The current `SettingsView` is a `Form` inside SwiftUI's native `Settings { }`
scene. It has four problems: sections aren't visually separated, the
hardcoded `.frame(width: 420)` was commented out because it truncated
content and the width is now unbounded (Pickers blow it out), `Stepper`
controls aren't keyboard-editable and don't align with the rest of the
fields, and every change applies immediately with no way to back out.

## Approach

**Presentation.** Replace the `Settings { }` scene with a dedicated
`WindowGroup(id: "settings")`, non-resizable, opened via `openWindow(id:
"settings")`. Intercept `CommandGroup(replacing: .appSettings)` so ⌘, and the
app-menu "Settings…" item both route to this window instead of the default
scene. Fixed width **560pt**; height sized to content per tab via
`.fixedSize(horizontal: false, vertical: true)`.

**Draft state.** One local `@State` var per setting, same shape as today.
Two categories:

- *Buffered* (Downloads, Linkgrabber, Notifications tabs): no side effects
  while editing. `applyAll()` writes every var to its backing store
  (`EngineSettingsStore`, `GrabberSettings`, `NotificationSettings`,
  `EngineController.applyStoredSettings()`) only when OK is pressed.
- *Live-preview* (Appearance tab — theme, Dock/Menu Bar mode): still write
  straight through via `.onChange`, same as today, so the effect is visible
  behind the dialog immediately. On `.onAppear`, the view snapshots
  `themeStore.selectedID` and `activationPolicyController.mode`. Cancel
  restores both snapshots before dismissing; OK just dismisses (the values
  are already live).

Cancel and OK both dismiss the window via `\.dismiss`.

**Tabs.** `TabView` with four tabs, matching the requested grouping:
Appearance (theme + Dock/Menu Bar), Downloads, Linkgrabber, Notifications.

**Section styling.** A `SettingsSection` wrapper (title + content) rendered
as a card: `RoundedRectangle` stroke in `theme.borderColor`, fill in
`theme.surfaceSecondaryColor.opacity(0.5)`, corner radius 8, replacing plain
`Form`/`Section` chrome. Uses the same theme-role color vocabulary as the
rest of the app (`ThemeColor.swift`) instead of system-default colors, so it
matches the main window in both light and dark.

**Numeric fields.** A reusable `SteppedNumberField(label:value:range:)`:
right-aligned editable `TextField` (bound through a `String` draft,
`NumberFormatter`-backed) plus an adjoining label-less `Stepper`, both rows
laid out in a `Grid` so labels and controls align vertically across the
section. On commit (return key or focus loss), text is parsed and clamped to
`range`; anything unparseable or out-of-range reverts to the last valid
value rather than being accepted. This covers `maxConcurrent`,
`segmentsPerItem`, `globalMaxConnections`, `maxConnectionsPerHost` — the four
fields currently using bare `Stepper`.

**Bottom bar.** Cancel / OK buttons, right-aligned, OK is the default
(`.keyboardShortcut(.defaultAction)`), Cancel is `.keyboardShortcut(.cancelAction)`.

**Theme ordering.** The Appearance tab's theme picker builds its item list
as `System, Light, Dark, then the remaining themeStore.catalog entries
sorted alphabetically (excluding light/dark)`. This is picker-local
presentation logic — `ThemeCatalog.builtInThemes()`'s own alphabetical sort
(depended on elsewhere, e.g. fixed preview indices in `PackagesListView`)
is untouched.

## Out of scope

- No change to what settings exist or their persistence keys.
- No change to `ThemeCatalog`'s canonical ordering.
- No new settings.
