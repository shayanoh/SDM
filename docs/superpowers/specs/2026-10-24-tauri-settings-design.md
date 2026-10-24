# Tauri UI, spec 4: Settings window and themes

Status: **IMPLEMENTED** on branch `tauri-ui` (2026-10-24); see the roadmap log for what was and was not verified. Part of [docs/tauri-roadmap.md](../../tauri-roadmap.md);
builds on the [foundation](2026-10-14-tauri-foundation-design.md), the
[main window](2026-10-23-tauri-main-window-design.md) and the
[link grabber](2026-10-24-tauri-link-grabber-design.md).

## 1. Intent

The Tauri app has no Settings. Every setting the SwiftUI app lets you change is a constant in
`GrabberDefaults`, a hard-coded closure in `production_dependencies`, or a stopgap in the link grabber
view (`TemporaryTools`: a download-folder field and a tools-version line). This spec builds the real
Settings window.

**The bar is parity.** Shayan's words: the Windows and Linux Settings "must look and work exactly the
same" as the SwiftUI one. The reference is the SwiftUI window, captured in
[assets/2026-10-24-swiftui-settings/](assets/2026-10-24-swiftui-settings/) (one screenshot per tab,
Deep Purple theme). The SwiftUI source is `SDM/SettingsView.swift` and the stores it reads
(`EngineSettingsStore`, `GrabberSettings`, `MediaSitesSettingsStore`, `NotificationSettings`,
`ActivationPolicyStore`).

## 2. Goals, non-goals, anti-goals

**Goals**

- A Settings window with the five SwiftUI tabs, behaving as SwiftUI's does (section 4).
- Every setting stored, and every setting that has behavior today actually changing that behavior.
- A theme picker over the bundled themes, applied live.
- `TemporaryTools` gone.

**Non-goals** (decided 2026-10-24)

- **No download-folder control in the UI.** SwiftUI has none. The setting stays stored and validated
  in the Rust crate, so a later field is only a UI change (section 6).
- The roadmap's other extras are dropped: a media-sites list, extra yt-dlp arguments, probe limits,
  override paths for the tools, re-download and download progress UI for the tools. Probe limits and
  the other `GrabberDefaults` constants stay constants.
- No behavior for the spec 5 controls (section 5).

**Anti-goal.** A Settings window that is merely "similar". Layout, labels, order, defaults, ranges,
disabled rules and the buffered-versus-live behavior all follow SwiftUI. Where this document departs
from SwiftUI it says so (section 9).

## 3. Decisions

| #   | Decision                                                                                                                                                                                  | Source         |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------- |
| S1  | Exact parity with the SwiftUI Settings window, tab for tab and control for control.                                                                                                       | Shayan         |
| S2  | The download folder is not shown. It stays in `settings.json` with its validation; nothing in the UI changes it.                                                                          | Shayan         |
| S3  | Controls whose behavior is spec 5's (launch at login, Dock / Menu Bar mode, the four notification toggles, clipboard watching) are shown, saved, and inert until spec 5.                  | Shayan         |
| S4  | Entry points: a gear at the bottom of the sidebar on **every** platform, Cmd+, (macOS) or Ctrl+, (Windows, Linux) from anywhere in the main window, and on macOS the app menu's "Settings…". | Shayan         |
| S5  | The roadmap extras listed in section 2 are dropped; only what SwiftUI has is built.                                                                                                       | Shayan         |
| S6  | The Components rows read "(bundled)" on macOS as in SwiftUI; on Windows and Linux they show the version and a download status instead.                                                    | Shayan         |
| S7  | Settings live in the Rust `sdm-app` crate, in the existing `settings.json`. The frontend holds only the buffered edits. The two apps still share only download state.                     | Claude's call  |
| S8  | The window is a second Tauri window (label `settings`) loading the same frontend bundle, routed by label.                                                                                 | Claude's call  |
| S9  | The Dock / Menu Bar setting is macOS only. On Windows and Linux a taskbar button exists only while a window is open, so the choice is meaningless: the section is hidden, the stored mode is always Both, and (spec 5) the tray icon always shows and closing the main window hides it to the tray, removing it from the taskbar. | Shayan |

## 4. The window

Match the screenshots. Fixed facts, from `SettingsView.swift`:

- Width 560. Height fits the tab's content and changes with the tab (Media Sites is about twice as tall
  as Appearance). Not resizable. Titled "Settings". Opening it again focuses the open one.
- Tab strip with icons in SwiftUI: Appearance, Downloads, Linkgrabber, Media Sites, Notifications.
  On macOS the strip sits in the titlebar as in the screenshots. On Windows and Linux the chrome stays
  OS-native (roadmap T2), so the strip sits directly under the titlebar; same order and labels.
- Each tab is a stack of sections: a bold heading over a rounded, bordered card one step lighter than
  the window. Colors are theme roles, never literals. Window background is `surface-primary`, solid
  (no Mica).
- A divider and an OK / Cancel bar at the bottom. OK is the default button (Enter), Cancel answers
  Esc.
- **Buffered:** Downloads, Linkgrabber, Media Sites and Notifications write nothing until OK.
  **Live, reverted on Cancel:** the theme. Cancel and closing the window by the titlebar are the same
  thing. A change of theme shows in the main window at once and returns to the original on Cancel.
  Launch at login and the Dock / Menu Bar picker are live in SwiftUI too, but have no behavior yet, so
  in spec 4 they are buffered like the rest; spec 5 makes them live and reverting.

### 4.1 Tabs

Defaults are SwiftUI's. Ranges are enforced by the control.

**Appearance**

- General: "Launch SDM at login" (checkbox). Default off.
- Theme: popup, "System", then "Light" and "Dark", then the other bundled themes alphabetically
  (SwiftUI's `orderedThemes`). Default System. Width 220.
- Dock / Menu Bar: popup of Menu Bar Only, Dock Only, Both. Default Both. **macOS only.** On Windows
  and Linux the section is not shown (S9).

**Downloads**

- Concurrency: stepped number fields, label then value: Max concurrent downloads (1..20, default 3),
  Segments per file (1..64, 8), Global max connections (1..256, 32), Max connections per host (1..64,
  8), Minimum segment size (MB) (1..100, 10).
- Startup: "Resume downloads automatically when SDM opens". Default off.

**Linkgrabber**

- Watch clipboard for links (default on), Auto-add and start on grab (off), Deep sniff (stage 2) (on),
  Auto-clear grabbed links: a popup of After 15 minutes, After 30 minutes (default), After 1 hour,
  After 1 day, Never.

**Media Sites**

- Quality: Maximum resolution popup 1920, 1440, 1080 (default), 720, 540, 480, shown as "1,080p" and
  so on (snap a stored value onto the ladder as SwiftUI does); Maximum playlist videos stepper
  (10..200, default 50, with the "10..200" hint).
- Video codecs AV1, VP9, H.264; Containers MP4, WebM; Audio codecs Opus, AAC: three side-by-side
  cards, all on by default. The last checked box of a card is disabled, so an allowlist is never
  empty (the rule from the multi-site spec section 9.4).
- Authentication: "Cookies from browser", a popup of None and the browsers `CookieSource` knows.
  Default None.
- Components: see 4.2.

**Notifications**

- Download finished, Package finished, Download failed, Links grabbed. All default on. The red
  "blocked from system settings" caption is spec 5's (it needs the notification permission state), so
  it is absent in spec 4.

### 4.2 Components

yt-dlp row (version, "not installed" when absent), "latest: X" caption when a newer one is known,
Update channel popup (Stable, Nightly), Check now button, last error in the failed color, a divider,
then ffmpeg, ffprobe and JS runtime ("QuickJS-ng <version>") rows.

- The channel popup applies when changed, not on OK, and runs a check, as in SwiftUI.
- macOS: each tool row ends "(bundled)", as in the screenshot.
- Windows and Linux (S6): the same three rows, but instead of "(bundled)" the row shows the
  installed version, or "not installed" and, while the tool is downloading, "downloading…". The
  status updates by polling `binaries_status` every few seconds, as `TemporaryTools` does today.
  Nothing more (no progress bar, no override, no re-download).
- The version strings are the managed-binaries ones already in `BinariesStatusDto`.

## 5. Spec 5 controls

Launch at login, the Dock / Menu Bar mode, the four notification toggles, Watch clipboard and
Auto-add and start on grab are
read and written like every other setting, and do nothing in spec 4. Spec 5 attaches behavior to the
stored values and makes the first two live. On Windows and Linux spec 5 also owns the S9 behavior
(an always-present tray icon; closing the main window hides it to the tray). It must handle a Linux
desktop with no tray (GNOME without the AppIndicator extension) by quitting on close instead, so the
app is never left running with no way to reach it. Spec 4 does neither: the window still closes and
quits as it does today. This spec must store them under stable names so spec 5
does not migrate anything. Clipboard watching is a link grabber behavior owned by spec 5 even though
its toggle lives on the Linkgrabber tab.

## 6. Storage and application

- One `Settings` struct in `sdm-app` (`settings.rs`), serialized into the existing `settings.json`,
  camelCase. Every field has the default above; a missing field takes its default and an
  out-of-range value is clamped, so an old or hand-edited file never fails to load. The existing
  tests for the corrupt-file and download-folder cases stay.
- `download_folder` remains in the struct, validated exactly as now. `update_settings` from the
  Settings window never sends it, and an update without it leaves it unchanged. Nothing in the UI
  reads or writes it after `TemporaryTools` goes.
- `SettingsDto` grows to carry all the settings; `get_settings` and `update_settings` remain the
  commands (bindings regenerate). One `update_settings` on OK validates every field, then persists
  once and applies. On a validation error nothing changes and the window stays open and says why.
- **Application on OK.** Engine settings (concurrency, segments, connection ceilings, minimum segment
  size) reach the engine as SwiftUI's `applyStoredSettings` does, and the auto-resume flag is read at
  launch right after `restore()`. Grabber settings (deep sniff, auto-clear interval, quality
  preferences) replace the `GrabberDefaults` constants; the session already reads quality preferences
  through a closure and the prune call reads the interval per tick, so changes take effect without a
  restart, and deep sniff reaches the prober. The resolver and wholesale downloader closures in
  `production_dependencies` that are hard-coded today (`CookieSource::None`, the playlist cap 50, the
  yt-dlp channel `Stable`) read the stored values. Auto-add-and-start-on-grab is
  read only by the clipboard flow in SwiftUI (`SDMApp.swift`), so like the clipboard toggle it is
  stored and inert until spec 5.
- Theme: the stored value is a theme id or "system". `theme_css` already takes an optional id; the
  frontend asks for the stored one at startup, and again when the picker changes (preview) and when
  OK or Cancel settles it. Both windows must show the change, so the preview and the revert are
  broadcast, not local to the Settings window.

## 7. Removing the temporary UI

- Delete `TemporaryTools.svelte` and its use in `LinkgrabberView.svelte`.
- The version line it showed becomes the Components section. The download-folder field has no
  successor (S2).
- Before deleting, check the mock bridge, the Playwright specs and the link grabber's layout for
  anything that leans on it, and keep the link grabber view looking as it would without it.

## 8. Entry points

- **Sidebar gear**, bottom of the sidebar, every platform: opens or focuses the Settings window.
- **Shortcut:** Cmd+, on macOS, Ctrl+, on Windows and Linux, from anywhere in the main window, subject
  to the existing rule that shortcuts are ignored while a modal is open or a text field has focus.
- **macOS app menu:** a native "Settings…" item with Cmd+, in the application menu, replacing whatever
  Tauri's default menu has there. This is the one place the app gets a native menu; Windows and Linux
  get none.
- The tray menu's "Settings…" is spec 5's.

## 9. Adaptations and guesses

Departures from SwiftUI, all forced by the platform. Items marked (guess) are Claude's and open to a
veto.

- Tab strip position on Windows and Linux (under the native titlebar). Same for the traffic-light
  buttons: OS-native.
- The Dock / Menu Bar section is hidden on Windows and Linux (S9). Its stored value is Both and is
  not editable there; the settings file keeps the field so one file format serves every platform.
- Components rows on Windows and Linux (S6).
- Cookie browsers: None, Chrome, Firefox, Edge and Brave everywhere; Safari on macOS only (guess).
  The labels are the capitalized names as in SwiftUI.
- The notification "blocked" caption is absent until spec 5.

## 10. Testing

- **Rust** (`cargo test -p sdm-app`): defaults for every field; a round trip through `settings.json`;
  a missing field takes its default; out-of-range values are clamped; a corrupt file gives defaults;
  `update_settings` rejects a bad value without changing anything and never changes the download
  folder unless given one; the existing folder tests; the engine and grabber see a changed setting
  after an update (concurrency, quality preferences, auto-clear, cookie source, playlist cap).
- **Vitest:** the media-sites last-checked-box rule, the resolution ladder snap, the theme ordering
  (System, Light, Dark, then alphabetical), the buffered-edit state (OK commits, Cancel discards and
  reverts the theme).
- **Playwright** against `MockBridge`: each of the five tabs screenshotted next to its reference, at
  the reference size, compared by eye (not an automated pixel diff, since fonts and icons differ by
  design): the gear opens the window, the shortcut opens it, a second open focuses, OK persists,
  Cancel discards, the theme previews in the main window and reverts, last-box disabling, and the
  link grabber view without `TemporaryTools`.
- **Manual (Windows VM, and macOS):** the window next to the SwiftUI one; a changed concurrency
  actually changes the number of running downloads; Components during a first-use tool download.

## 11. Left to the builder

- How the second window and the cross-window theme preview are wired (events versus shared stored
  state), and how the window sizes itself to each tab.
- The exact engine and grabber hooks for applying a changed setting, found by reading how
  `applyStoredSettings` works in SwiftUI and what `sdm-engine` and `sdm-grabber` already expose.
- Icons for the tabs and the gear (Lucide, per the main-window spec's SF Symbol table; the tab icons
  are `paintbrush`, `arrow.down.circle`, `link`, `film.stack`, `bell`).
- The stepped number field's exact look, matched to the screenshot.
