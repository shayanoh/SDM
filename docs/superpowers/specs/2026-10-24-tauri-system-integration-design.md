# Tauri UI, spec 5: System integration

Status: **DRAFT** (2026-10-24), written with Shayan. Part of [docs/tauri-roadmap.md](../../tauri-roadmap.md);
builds on the [foundation](2026-10-14-tauri-foundation-design.md), the
[main window](2026-10-23-tauri-main-window-design.md), the
[link grabber](2026-10-24-tauri-link-grabber-design.md) and the
[settings window](2026-10-24-tauri-settings-design.md).

## 1. Intent

The Tauri app stores the settings that make SDM feel like a background download manager, and none of them do
anything: launch at login, the Dock / Menu Bar mode, the four notification toggles, clipboard watching and
auto-add-and-start. Closing the main window quits the app. There is no tray.

This spec attaches the behavior. **The bar is the SwiftUI app's behavior, on Windows, Linux and macOS** (Shayan:
"They should work exactly like mac"). The reference code is in `SDM/`: `ClipboardWatcher.swift`,
`NotificationManager.swift`, `MenuBarPopoverView.swift`, `MenuBarRingIcon.swift`, `MenuBarIconController.swift`,
`ActivationPolicyController.swift`, `LaunchAtLoginController.swift` and `SDMApp.swift` (`applicationShouldTerminate`).

The picture Shayan described: copy a link anywhere, SDM grabs it into the Linkgrabber and posts a notification when
it is ready. Close the window, and the downloads keep running with the icon in the tray. Quit from the tray, and if
anything is running, SDM warns first.

## 2. Goals, non-goals, anti-goals

**Goals**

- Clipboard link grabbing on all three platforms, including a GNOME desktop on Wayland.
- The four notifications, each with its existing toggle, and a click that opens the right view.
- A tray icon on all three platforms with the SwiftUI menu bar icon's look and redraw rule.
- The SwiftUI menu bar panel as a popover window on macOS and Windows; a plain menu with status lines on Linux.
- Close-to-tray on Windows and Linux; quit-on-close on a Linux desktop with no tray.
- A quit warning whenever downloads are running, with the SwiftUI non-resumable wording when it applies.
- Launch at login, the macOS Dock / Menu Bar mode, auto-add-and-start, and a single running instance.

**Non-goals**

- The browser extension and its hand-off channel (spec 6).
- Touch Bar, Liquid Glass, and the native macOS popover. The popover here is a positioned window.
- Changes to the SwiftUI app or `sdm-ffi`.
- A Dock / Menu Bar picker on Windows and Linux (S9 in the settings spec stays: it is hidden, the stored value is `both`).
- A popover on Linux (see section 4.3).

**Anti-goals**

- Clipboard content that is not a link is never stored, never logged, never sent anywhere.
- No timer redraws the tray icon. A tick that changes nothing visible costs nothing.
- The app is never left running with no way to reach it (a Linux desktop with no tray must quit on close).
- Closing the window or quitting must never silently throw away a download that cannot be resumed.

## 3. Decisions

| #   | Decision                                                                                                                                                                                                                                                                                                              | Source |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| Y1  | Clipboard watching, notifications and close-to-tray work the same on macOS, Windows and Linux.                                                                                                                                                                                                                        | Shayan |
| Y2  | The tray popover is the SwiftUI panel's content, on macOS and Windows. On Linux the tray icon has a plain menu instead, because Tauri emits no tray click events on Linux (verified in the Tauri docs).                                                                                                               | Shayan |
| Y3  | The Linux menu has Open SDM, Pause all, Quit and (when links wait) Review, then a separator, then disabled status lines: "No downloads running" or "N downloads running", and one line per running download with its progress and speed.                                                                              | Shayan |
| Y4  | The tray icon is the SDM logo with the SwiftUI look: solid when nothing runs; while downloads run, the logo dimmed to 25% with the left part, as wide as the overall progress, at full opacity. It redraws only when the running state flips or progress moves by at least one rendered pixel. Same on all platforms. | Shayan |
| Y5  | macOS tints the icon itself (template image). On Windows and Linux the icon is white or black, chosen from the system's dark/light setting, and redrawn when that setting changes.                                                                                                                                    | Shayan |
| Y6  | Any running download makes quitting ask first. If a running download is non-resumable (`is_resumable == false`; unprobed is not counted), the dialog adds the SwiftUI sentence. Quit paths: tray/popover Quit, the macOS app menu and Cmd+Q, and closing the window on a Linux desktop with no tray.                  | Shayan |
| Y7  | On GNOME under Wayland the clipboard watcher polls through XWayland. Verified on the Fedora 44 / GNOME 50 VM on 2026-10-24: text copied by a Wayland client was read by an X11 client with no SDM window focused, three times in a row.                                                                               | Shayan |
| Y8  | A Windows notification click routes only while the toast is on screen (a limit of `tauri-plugin-notification`). Accepted.                                                                                                                                                                                             | Shayan |
| Y9  | **My call:** the quit warning is a native message dialog, since it must work when no SDM window is visible.                                                                                                                                                                                                           | Claude |
| Y10 | **My call:** on Windows and Linux, a launch-at-login start opens hidden in the tray. On macOS it behaves as the SwiftUI app does today.                                                                                                                                                                               | Claude |
| Y11 | **My call:** starting SDM a second time focuses the running instance (its main window) and exits.                                                                                                                                                                                                                     | Claude |
| Y12 | **My call:** on Windows and macOS, a click on the tray icon (either button) toggles the popover. Nothing else is on a right click.                                                                                                                                                                                    | Claude |

## 4. The tray

### 4.1 The icon

Source: the SwiftUI app's `MenuBarLogo` image (`SDM/Assets.xcassets/MenuBarLogo.imageset`, white, 64 and 128 px).

Two states, from `MenuBarRingIcon`:

- **Idle** (`running_count == 0`): the logo at full opacity.
- **Running**: the whole logo at 25% opacity, with the left `fraction` of its width at full opacity on top
  (`fraction` is the engine's overall progress, `EngineStats.overallFraction` in SwiftUI).

**Redraw rule** (`MenuBarIconController`): re-render and set the tray image only when the running state changes or
`|fraction - last_drawn_fraction| >= 1 / icon_width_px`. SwiftUI uses a 14 pt icon at 2x, so 1/28. The Tauri
icon's pixel width is the builder's choice (Windows tray icons are 16 to 32 px depending on DPI); the delta follows
the width actually drawn.

Driven by the same engine publish that produces patches, never by a timer.

**Color.** macOS: `set_icon_as_template(true)`; draw the logo white. Windows and Linux: draw white on a dark
system setting and black on a light one, and redraw when the setting changes. Which setting is "the system's" is
the builder's to find: on Windows the taskbar theme (`SystemUsesLightTheme`, not the app theme), on Linux the
desktop's color scheme.

### 4.2 The popover (macOS and Windows)

A small frameless window in the same bundle, like the Settings window: always on top, no taskbar or Dock entry,
hidden when it loses focus, 280 px wide, the current theme's colors (colors come from theme roles, never
literals). Its content is `MenuBarPopoverView`:

- Total speed (headline), "N active", and the mini bandwidth graph (32 px tall, the global history).
- Up to five running downloads: a progress ring, the filename (one line), the speed.
- If the Linkgrabber holds links: "N links waiting" and a **Review** button that shows the main window on the
  Linkgrabber.
- **Pause all**, **Open SDM**, **Quit** (Quit goes through the quit guard, section 5).

It reads from the same patch stream as the main window. It must not add per-tick work while hidden.

Position: anchored to the tray icon's rectangle (below it on macOS, above it on Windows with the taskbar at the
bottom; the builder handles other taskbar edges and multiple monitors).

### 4.3 Linux: a plain menu

Tauri emits no tray events on Linux, so a click cannot open a window. The tray icon shows a menu, top to bottom:

1. Open SDM
2. Pause all
3. Review (N links waiting), only when links wait
4. Quit
5. a separator
6. disabled status lines: "No downloads running", or "N downloads running" followed by the total speed, then one
   line per running download: name, percent, speed. Up to five downloads, then "and N more" (my guess; the
   popover also shows five).

A Linux menu cannot update while it is open, so it is rebuilt when its visible text changes, at most once per
tick. The builder checks how the menu is rebuilt without flicker or a leak.

### 4.4 When the tray exists

Windows and macOS always have one. Linux has one only when a StatusNotifier host is running. GNOME needs the
AppIndicator extension, which the Fedora VM does not have installed. The builder decides how to detect it (for
example whether `org.kde.StatusNotifierWatcher` has an owner on the session bus) and does so **when the main
window is closed**, not only at startup, because the extension can be enabled later.

## 5. Closing and quitting

| Platform            | Close the main window                                                                                | Quit                                                       |
| ------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| Windows             | Hides to the tray and leaves the taskbar. Downloads keep running.                                    | Tray popover Quit, through the guard.                      |
| Linux, tray present | Hides to the tray and leaves the taskbar.                                                            | Tray menu Quit, through the guard.                         |
| Linux, no tray      | Quits, through the guard (the dialog is the "closing stops your downloads" warning).                 | The same path.                                             |
| macOS               | Never quits. The Dock / Menu Bar mode (section 7) decides what stays visible, as in the SwiftUI app. | App menu Quit, Cmd+Q, popover Quit, all through the guard. |

**The guard.** A function of the engine snapshot: no running download means quit at once. Otherwise show the
dialog:

> **Quit with active downloads in progress?**
> N downloads are running. Quitting stops them; they resume the next time SDM starts.

and, only if at least one running download has `is_resumable == false` (the three-state field; `None` is not
counted, as in `hasActiveNonResumableDownloads`):

> N of them cannot be resumed. Quitting now will lose their progress permanently.

Buttons: **Cancel**, **Quit**. Cancel keeps the app running.

Hiding the main window also hides the Settings window (it already closes with the main window today; keep that).

## 6. Clipboard watcher and notifications

### 6.1 Clipboard watcher

Behavior from `ClipboardWatcher.swift`, gated by `clipboard_watching_enabled`, live-togglable from Settings:

- Poll about every 0.5 s. Skip a poll when nothing changed since the last one (SwiftUI uses `changeCount`; other
  platforms have no counter, so compare against the previous text without keeping non-link text around: keep a
  hash).
- Read text, run it through the existing URL extractor (the one the Add Links sheet uses) so several links in one
  copy are all caught.
- Ignore content SDM itself just put on the clipboard (`ignoreOwnWrite`), so copying a link out of the app does not
  re-grab it. The builder routes SDM's own copy actions through the same code that records the write.
- Hand the links to the grabber (`ingest`). When auto-add-and-start is on, the existing grab hand-off adds and
  starts them. The builder checks whether `sdm-app` already does this or the setting is only stored.
- Non-link text is dropped immediately: not stored, not logged.

**Linux.** KDE, Sway, Hyprland and Niri expose `wlr-data-control` or `ext-data-control`, which `arboard` (used by
`tauri-plugin-clipboard-manager`, with its `wayland-data-control` feature) can read in the background. GNOME does
not. On GNOME the watcher reads through XWayland (Y7). Whether the Tauri build ends up on the X11 path
automatically or needs to be told is the builder's to find; **it is an acceptance check on the Fedora VM**
(section 10). If that check fails, stop and bring it back to Shayan; do not invent a different fallback.

### 6.2 Notifications

Behavior from `NotificationManager.swift`. The four types, each behind its stored toggle:

| Type              | Title / body                                                      | Click opens |
| ----------------- | ----------------------------------------------------------------- | ----------- |
| Download finished | "Download finished" / filename                                    | Completed   |
| Package finished  | "Package finished" / package name                                 | Completed   |
| Download failed   | "Download failed" / "filename: reason"                            | Downloads   |
| Links grabbed     | "N links grabbed" / "Waiting for confirmation in the Linkgrabber" | Linkgrabber |

Plus a plain informational banner (SwiftUI's `postInfo`, used for "yt-dlp updated to …") if the controller already
emits that event.

- Triggered from engine and grabber state transitions in `sdm-app`, not from the frontend, so they fire with the
  window hidden. Each transition notifies once.
- A notification shows even when SDM is the focused app (SwiftUI opts back in for exactly this reason). My guess,
  not stated by Shayan.
- A click shows the main window (from hidden, minimized or closed-to-tray) on the named view.
- macOS asks for authorization on first use. Windows shows toasts correctly only for an installed app. In
  `tauri dev` they carry the PowerShell name and icon (documented plugin limit).
- Click routing uses `tauri-plugin-notification`'s `on_action` / `onAction`, present in the current plugin source
  (v2.6.0 release notes). The docs page still says actions are mobile only. Verify on each platform; a Windows
  toast routes only while on screen (Y8).

## 7. Launch at login, activation modes, single instance

- **Launch at login** follows the stored `launch_at_login`, applied when it changes and checked at startup (the OS
  state is the truth, as `LaunchAtLoginController` does with `SMAppService`). If registering fails, the toggle
  reverts to what took effect and an error shows in Settings. Mechanism per platform (login item, registry `Run`
  key, autostart `.desktop` file) is the builder's, after reading the current `tauri-plugin-autostart` docs. A
  login start on Windows and Linux opens hidden in the tray (Y10).
- **Dock / Menu Bar mode, macOS only**, exactly the SwiftUI table (spec section 10.2): Menu bar only (no Dock icon
  while no window is open, tray icon always), Dock only (tray icon hidden), Both (default). Changing it applies
  immediately, including with no window open. In Menu bar only mode with no window, Quit lives in the popover.
  Windows and Linux ignore the stored value: the tray icon always shows.
- **Single instance.** A second launch focuses the running instance's main window and exits. If the running
  instance is hidden in the tray, it shows the window.

## 8. Fix first: partial settings updates

From the roadmap: `SettingsStore::update` (`sdm-core/crates/sdm-app/src/settings.rs`) stores exactly what it is
given, and `SettingsDto` has struct-level `#[serde(default)]`, so an update with only `{ launchAtLogin: true }`
resets every other field to its default. This spec adds single-field updates (a tray toggle, a first-run
prompt), so this is built **before** anything else.

Make `update` merge incoming fields onto the stored settings (an omitted field keeps its value), or make every
field except `downloadFolder` required in the export. Add a test that a one-field update leaves the others alone.
`mockBridge.ts` (`updateSettings`) mirrors the current behavior on purpose and changes with it.

## 9. Permissions

Widen the Tauri capability file only as far as these features need. The builder lists exactly which permissions
(tray, notification, autostart, clipboard, dialog, window show/hide/position/focus, single-instance) after reading
each plugin's current docs, and checks that the existing windows (`main`, `settings`) and the new popover window
get only what they use.

## 10. Testing

**Rust, pure and deterministic** (no network, no real clock): the tray icon image for a given state and the redraw
rule (a table of fractions and the draw/skip answer); the guard's decision from a snapshot (none running, running
and resumable, one non-resumable, unprobed); the Linux menu text from a snapshot; each notification trigger fires
once per transition and respects its toggle; the notification route for each type; the clipboard diff, own-write
ignore and multi-link extraction with fixed strings; the settings merge fix.

**Svelte:** the popover component against the mock (idle, running, links waiting); Playwright flow against the mock.

**By hand** (record in the roadmap log which were done):

- macOS: tray icon states, popover, Dock / Menu Bar modes, notification click, quit guard.
- Windows VM: tray icon in light and dark taskbar, popover position above the taskbar, close-to-tray, notification
  click, launch at login, single instance.
- **Fedora VM (GNOME 50, Wayland), without the AppIndicator extension:** closing the window with a download running
  shows the quit warning; closing with none quits. The VM is reachable with `ssh shayanoh@fedora.local` (no sudo).
- **Fedora VM with the extension** (`gnome-shell-extension-appindicator`, enabled, then log out and in): close-to-tray,
  the menu and its status lines, and notification click.
- **Fedora VM clipboard acceptance check:** with the app running and its window not focused, copying a link in
  another app (not `wl-copy`) is grabbed. This is the Y7 check in the real app.

Not testable here: KDE, Sway and other compositors with data-control; say so in the log.

## 11. Left to the builder

- The pixel size, scaling and image library for the tray icon; reading the system dark/light setting per platform.
- How the popover window is created, positioned and dismissed; the multi-monitor and taskbar-edge cases.
- How the Linux menu is rebuilt; how the StatusNotifier host is detected.
- Which plugin or crate handles each of autostart, clipboard, dialog and single instance, after reading current
  docs (do not rely on memory).
- Where notification triggers live in `sdm-app` and how the `Host` trait grows (clipboard, notify with click
  routing, launch at login, tray state).
- Whether `auto_add_and_start_on_grab` already works in the grabber or needs wiring.

## 12. Guesses to check

Items marked "my call" or "my guess" above: Y9 to Y12; the five-download cap and "and N more" on the Linux menu;
notifications showing while SDM is focused; the exact text of the "links waiting" menu entry.
