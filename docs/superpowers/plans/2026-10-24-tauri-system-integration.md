# Tauri system integration (spec 5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Attach real behavior to the stored system settings: clipboard link grabbing, notifications with click routing, a tray icon (popover on macOS and Windows, menu on Linux), close-to-tray, a quit guard, launch at login, macOS Dock / Menu Bar modes, single instance.

**Architecture:** Everything that can be decided without a window stays in `sdm-app` as pure, tested functions (notification texts and routes, the clipboard watcher, the tray status, the quit guard, the icon redraw rule, the Linux menu lines), driven by `App::tick`. The Tauri shell (`sdm-tauri/src-tauri`) only does I/O: it draws the tray icon, builds the menu and popover window, reads the clipboard, shows OS notifications and dialogs. The popover is a third webview window on the same bundle (like Settings). It gets its data from a Rust-side `tray-status` event, **not** from `attach`, because `App` keeps one patch sink and a second `attach` would steal the main window's stream.

**Tech Stack:** Rust (`sdm-app`, Tauri v2.12), Svelte 5 + TypeScript, `tauri-plugin-notification`, `-clipboard-manager`, `-autostart`, `-dialog`, `-single-instance`.

**Spec:** [docs/superpowers/specs/2026-10-24-tauri-system-integration-design.md](../specs/2026-10-24-tauri-system-integration-design.md). Executors read both. Reference behavior: `SDM/ClipboardWatcher.swift`, `NotificationManager.swift`, `MenuBarPopoverView.swift`, `MenuBarRingIcon.swift`, `MenuBarIconController.swift`, `SDMApp.swift`.

## Global Constraints

- Tests: Rust `cd sdm-core && cargo test -p sdm-app` (and `-p sdm-tauri` for the shell); web `cd sdm-tauri/web && pnpm test`, `pnpm check`, `pnpm exec playwright test`. No test touches the network, the real clipboard or a real clock; time is the tick.
- Format Rust with `cargo fmt`, TypeScript/Svelte with `prettier -w <file>`. Clippy clean (`cargo clippy --workspace --all-targets`). Never hand-edit a manifest or lockfile: use `cargo add`, `pnpm add`.
- `bindings.ts` is generated: `cd sdm-core && cargo test -p sdm-tauri export_bindings`; never edit it by hand.
- Colors come from theme roles, never literals (the popover uses the same CSS variables as the main window).
- Library behavior is verified against current docs (Context7), not memory, before each plugin is used.
- Notification click `route` strings are exactly `downloads`, `completed`, `linkgrabber` (the web `Tab` values).
- Quit dialog title `Quit with active downloads in progress?`; non-resumable sentence `N of them cannot be resumed. Quitting now will lose its progress permanently.`; buttons `Cancel`, `Quit`. A download counts as non-resumable only when `is_resumable == Some(false)` and its state is running.
- Tray icon: logo from `SDM/Assets.xcassets/MenuBarLogo.imageset/icon-white-logo-128.png` (copy to `sdm-tauri/src-tauri/icons/tray-logo.png`). Running look: whole logo at 25% alpha, left `fraction` of the width at full alpha. Redraw only when running flips or `|fraction - drawn| >= 1 / width_px`.
- Windows and Linux ignore the stored `activation_mode` (tray always shown, mode treated as `both`).
- Commits are allowed freely (project CLAUDE.md); pushes only fast-forward. Commit messages end with the attribution lines from the session.

## Review Focus

- A copy that holds several links, prose around a link, or no link at all (nothing grabbed, nothing stored).
- The same text copied twice in a row, and a link SDM itself copied (each grabs at most once / never).
- Quitting with a running download whose `is_resumable` is `None` (counts as running, not as non-resumable).
- A notification for a transition that already happened before the app attached (no repeat on restart or re-attach).
- The Linux menu with zero, one and nine running downloads (text, cap and "and N more").
- Closing the main window with no tray on Linux and nothing running (quits at once, no dialog).
- A failed launch-at-login registration (toggle reverts, error shown).

## File Structure

| File                                                                                          | Responsibility                                                                       |
| --------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| `sdm-app/src/settings.rs`, `protocol.rs`                                                      | settings wire fix (Task 1)                                                           |
| `sdm-app/src/notify.rs`                                                                       | `Notification` variants, texts, routes, gating (Task 2)                              |
| `sdm-app/src/autoadd.rs` (new)                                                                | pure auto-add decision (Task 3)                                                      |
| `sdm-app/src/clipboard.rs` (new)                                                              | pure clipboard watcher (Task 4)                                                      |
| `sdm-app/src/tray.rs` (new)                                                                   | `TrayStatus`, quit guard, menu lines, icon tracker (Task 5)                          |
| `sdm-app/src/app.rs`, `host.rs`                                                               | wiring: `tray_status`, `clipboard_changed`, notification detection, `auto_add_ready` |
| `src-tauri/src/notify.rs` (new)                                                               | OS notifications and click routing (Task 6)                                          |
| `src-tauri/src/clipboard.rs` (new)                                                            | poll loop (Task 7)                                                                   |
| `src-tauri/src/tray_icon.rs` (new)                                                            | pure icon compositing (Task 8)                                                       |
| `src-tauri/src/tray.rs` (new)                                                                 | tray icon, Linux menu, refresh per tick (Tasks 8, 9)                                 |
| `src-tauri/src/popover.rs` (new)                                                              | popover window (Task 10)                                                             |
| `src-tauri/src/lifecycle.rs` (new)                                                            | close/quit guard, tray-host detection (Task 11)                                      |
| `src-tauri/src/autostart.rs` (new), `lib.rs`                                                  | launch at login, single instance (Task 12)                                           |
| `src-tauri/src/activation.rs` (new, macOS)                                                    | Dock / Menu Bar mode (Task 13)                                                       |
| `web/src/components/TrayPopover.svelte`, `ProgressRing.svelte` (new), `main.ts`, bridge files | popover UI, `navigate` event (Tasks 6, 10)                                           |

---

### Task 1: Required settings fields (the partial-update hazard)

**Files:** Modify `sdm-core/crates/sdm-app/src/protocol.rs:176-178`, `sdm-core/crates/sdm-app/src/settings.rs`, `sdm-tauri/web/src/lib/mockBridge.ts` (`updateSettings`). Test: `sdm-core/crates/sdm-app/tests/settings_tests.rs`.

**Interfaces:** Produces: `SettingsDto` deserializes only when every field except `download_folder` is present. `lenient()` is unchanged (it already builds a full object from defaults).

- [ ] **Step 1: Failing test** `a_partial_settings_object_is_rejected` in `settings_tests.rs`: `serde_json::from_value::<SettingsDto>(json!({"launchAtLogin": true}))` is `Err`; and `a_full_object_without_download_folder_parses` (serialize the default, remove `downloadFolder`, parse OK, `download_folder == None`).
- [ ] **Step 2:** `cd sdm-core && cargo test -p sdm-app --test settings_tests` fails (the first test).
- [ ] **Step 3:** In `protocol.rs` change `#[serde(rename_all = "camelCase", default)]` to `#[serde(rename_all = "camelCase")]` on `SettingsDto`. Keep `Default`. Fix any test or helper in the crate that deserializes a partial object (compile and test failures show them).
- [ ] **Step 4:** Regenerate bindings (`cargo test -p sdm-tauri export_bindings`); `pnpm check` shows every TS site that built a partial DTO. In `mockBridge.ts` make `updateSettings` store exactly the given full DTO (it already replaces); delete any defaults-merging it does.
- [ ] **Step 5:** `cargo test -p sdm-app`, `pnpm test`, `pnpm check` pass. Commit `Require every settings field on the wire`.

### Task 2: Notification texts, routes, gating, new triggers

**Files:** Modify `sdm-app/src/notify.rs`, `sdm-app/src/app.rs` (`publish`, around lines 331-430), `sdm-app/src/host.rs` (`FakeHost` unchanged signature). Test: `notify.rs` unit tests and `sdm-app/tests/notification_tests.rs` (new).

**Interfaces:** Produces:

```rust
pub enum Notification { Finished{filename:String}, PackageFinished{name:String}, Failed{filename:String,reason:String}, LinksGrabbed{count:u32} }
#[derive(Clone,Copy,PartialEq,Eq,Debug)] pub enum Route { Downloads, Completed, Linkgrabber }
impl Route { pub fn as_str(self) -> &'static str }            // "downloads" | "completed" | "linkgrabber"
impl Notification {
    pub fn title_body(&self) -> (String, String);
    pub fn route(&self) -> Route;
    pub fn enabled(&self, settings: &SettingsDto) -> bool;    // the matching notify_* toggle; LinksGrabbed also needs count > 0
}
```

Texts: `Download finished`/filename; `Package finished`/name; `Download failed`/`{filename}: {reason}`; `{count} links grabbed`/`Waiting for confirmation in the Linkgrabber`. Routes: Finished, PackageFinished -> Completed; Failed -> Downloads; LinksGrabbed -> Linkgrabber.

- [ ] **Step 1: Failing tests:** `title_body` and `route` for each variant; `enabled` false for each when its toggle is off and true when on; `notifications_between` yields `PackageFinished{name}` once when a package's items all become `Completed` (not when it was already all-completed, not for an empty package, not on a move between packages); order: item notifications first, then package ones (SwiftUI order).
- [ ] **Step 2:** Run `cargo test -p sdm-app notify` (compile failure / FAIL).
- [ ] **Step 3:** Add the variants and methods; extend `notifications_between` with the package pass (`isFinished` = non-empty and all `ItemState::Completed`; compare changed packages against their old revision as the item pass does).
- [ ] **Step 4:** In `app.rs` `publish`, filter `due` through `notification.enabled(&settings.get())` before `host.notify`. Integration test in `notification_tests.rs` using `FakeHost`: complete an item with `notify_download_finished` off -> no notification; on -> one; a second `tick()` adds none.
- [ ] **Step 5:** All `sdm-app` tests pass; `cargo clippy -p sdm-app`. Commit.

**Links grabbed (same task):** Add `GrabberController::take_fresh_rows(&self) -> u32` in `grabber.rs`: compares the ids of the session snapshot's link rows and media rows against an internal remembered set and returns how many are new (remembering them; ids that left stay forgotten so a row is never announced twice). Call it from `publish` once per tick when a grabber exists and push `Notification::LinksGrabbed{count}` into `due` when `> 0`. It must cost nothing when the session did not change (check whether the session exposes a change counter; if not, compare row counts first). Test in `notification_tests.rs`: ingest two links (fake probe) -> one `LinksGrabbed{count:2}`; a recheck of the same rows -> none; toggle `notify_links_grabbed` off -> none delivered.

### Task 3: Auto-add and start

**Files:** Create `sdm-app/src/autoadd.rs`; modify `app.rs` (`tick`), `lib.rs`. Test: unit tests in `autoadd.rs`, `sdm-app/tests/grabber_handoff_tests.rs` (extend).

**Interfaces:** Produces `pub(crate) fn ready_packages(snapshot: &GrabberSnapshot, already: &HashSet<Uuid>) -> Vec<Uuid>` (package ids, in order) and `App::auto_add_ready(&self) -> Result<(), AppError>`.
Rule (from `SDMApp.swift`): a package is ready when it has at least one row, none of its link ids are in `already`, every link row's verdict is online and every media row is resolved. `auto_add_ready` does nothing unless `auto_add_and_start_on_grab`; for each ready package it records its row ids in `already` (an `App` field) and calls `grabber_add_to_downloads(HandoffTarget::Package{id}, true)`. `tick` calls it after `publish`.

- [ ] **Step 1: Failing tests:** `ready_packages` for: all online (ready), one faulty (not), a media row unresolved (not), empty package (not), ids already added (not). Integration: setting off -> nothing added; setting on -> the package becomes an engine item and starts; a second `tick()` adds nothing more.
- [ ] **Step 2-4:** Implement, run `cargo test -p sdm-app`.
- [ ] **Step 5:** Commit.

### Task 4: Clipboard watcher core

**Files:** Create `sdm-app/src/clipboard.rs`; modify `app.rs`, `lib.rs`. Test: unit tests in `clipboard.rs`; `sdm-app/tests/clipboard_tests.rs` (new).

**Interfaces:** Produces:

```rust
pub struct ClipboardWatcher { /* last hash, ignored hash */ }
impl ClipboardWatcher {
    pub fn new() -> Self;
    /// `Some(text)` to ingest when it changed since the last call and is not our own write.
    pub fn changed<'a>(&mut self, text: Option<&'a str>) -> Option<&'a str>;
    pub fn ignore_own_write(&mut self, text: &str);
}
```

plus `App::clipboard_changed(&self, text: Option<String>) -> Result<(), AppError>` (does nothing unless `clipboard_watching_enabled`; otherwise `grabber_ingest_text`) and `grabber_copy_url` calling `ignore_own_write` with the URL it returns. State is hashes only: no clipboard text is kept or logged.

- [ ] **Step 1: Failing tests:** first poll with text returns it; the same text again returns `None`; changed text returns it; `None` (empty or non-text clipboard) returns `None`; text equal to the last `ignore_own_write` returns `None` once, and the same text copied again later after something else was copied returns `Some`. Integration (`FakeHost`, fake probe): two links in one string -> two rows; prose with no link -> zero rows; setting off -> zero rows; `grabber_copy_url` then `clipboard_changed(same url)` -> no new row.
- [ ] **Step 2-4:** Implement (hash with `std::hash::DefaultHasher`), run `cargo test -p sdm-app`.
- [ ] **Step 5:** Commit.

### Task 5: Tray status, quit guard, menu lines, icon tracker

**Files:** Create `sdm-app/src/tray.rs`; modify `app.rs` (`tray_status`), `protocol.rs` (export types), `lib.rs`. Test: unit tests in `tray.rs`; `sdm-app/tests/tray_tests.rs` (new).

**Interfaces:** Produces:

```rust
#[derive(Clone, PartialEq, Serialize, Type)] #[serde(rename_all="camelCase")]
pub struct TrayItem { pub name: String, pub fraction: f64, pub bytes_per_second: f64 }
#[derive(Clone, PartialEq, Serialize, Type)] #[serde(rename_all="camelCase")]
pub struct TrayStatus {
    pub running_count: u32, pub non_resumable_count: u32, pub bytes_per_second: f64,
    pub history: Vec<f64>, pub overall_fraction: f64,
    pub items: Vec<TrayItem>,           // every running item
    pub links_waiting: u32,
}
impl TrayStatus {
    pub fn from_snapshot(s: &EngineSnapshot, links_waiting: u32) -> Self;
    pub fn quit_warning(&self) -> Option<String>;           // None when running_count == 0
    pub fn menu_status_lines(&self) -> Vec<String>;
}
pub struct IconTracker { /* drawn running, drawn fraction */ }
impl IconTracker { pub fn new()->Self; pub fn needs_redraw(&mut self, running: bool, fraction: f64, width_px: u32) -> bool; }
```

`App::tray_status(&self) -> TrayStatus` uses the last published snapshot (`state.previous`) and the grabber's total row count (links + media rows, the SwiftUI `totalCount`); it takes no engine await. `overall_fraction` and counts come from `snapshot.figures` (the same source as `EngineStats`).
Text: `quit_warning` = `"{n} download(s) are running. Quitting stops them; they resume the next time SDM starts."` (singular: `1 download is running.`), plus, when `non_resumable_count > 0`, a second paragraph `"{k} of them cannot be resumed. Quitting now will lose its progress permanently."` (singular `1 of them`). `menu_status_lines`: `["No downloads running"]`, or `["{n} download(s) running · {speed}", "{name} — {pct}% — {speed}" x min(n,5), "and {n-5} more"]`; speeds use binary units with one decimal (`12.4 MB/s`, `0 B/s`).

- [ ] **Step 1: Failing tests:** `quit_warning` for none running, 3 resumable, 1 of 3 non-resumable, a running item with `is_resumable: None` (counts as running, not non-resumable); `menu_status_lines` for 0, 1, 5, 9 running (9 -> five lines then `and 4 more`); `needs_redraw` table: first call true; same state false; fraction +0.01 at width 28 false; +0.04 true; running flips true; flip back true; `fraction` NaN treated as 0. Integration: `App::tray_status` after a tick with two running items reports them and `links_waiting` after an ingest.
- [ ] **Step 2-4:** Implement; `cargo test -p sdm-app`.
- [ ] **Step 5:** Commit.

### Task 6: OS notifications, click routing, `navigate` event

**Files:** Create `src-tauri/src/notify.rs`; modify `src-tauri/src/host.rs:63-76` (`notify`), `lib.rs` (plugin init, `show_main` command), `commands.rs`, `capabilities/default.json`; web: `lib/bridge.ts`, `tauriBridge.ts`, `mockBridge.ts`, `App.svelte`. Test: `sdm-tauri` unit test for payload mapping; web `lib/bridge` test with the mock.

**Interfaces:** Consumes `Notification::{title_body, route}` (Task 2). Produces command `show_main(route: Option<String>) -> Result<(), AppError>` (shows, unminimizes, focuses the `main` window, then emits `navigate` with the route string), and `Bridge.onNavigate(handler: (tab: Tab) => void): () => void`.

- [ ] **Step 1:** Look up `tauri-plugin-notification` (Context7) for `Notification::on_action`, how an action reports which notification was clicked (id or `extra`), Windows installed-app requirement, and macOS permission request. Add: `cd sdm-tauri/src-tauri && cargo add tauri-plugin-notification`. If an action carries no payload, keep an in-memory map `notification id -> Route`.
- [ ] **Step 2: Failing test:** a pure `fn payload(n: &Notification) -> (String, String, &'static str)` in `src-tauri/src/notify.rs` returns title, body, route string (reuse the Task 2 methods; the test pins the four route strings). Web test: `MockBridge.onNavigate` handler receives a route emitted by a test helper; `App.svelte` sets `ui.setTab` (extend `shell.test.ts`: emit `completed` -> Completed tab is shown).
- [ ] **Step 3:** Implement `TauriHost::notify` (show the notification, register the click handler once at startup that calls `show_main` logic with the route), the `show_main` command (register in `specta_builder`), `Bridge.onNavigate` (Tauri: `listen("navigate")`), mock, and the `App.svelte` subscription. On macOS request notification permission at startup.
- [ ] **Step 4:** `cargo test -p sdm-tauri`, `cargo test -p sdm-tauri export_bindings`, `pnpm test`, `pnpm check`. Manual (macOS): `pnpm tauri dev -- -- --simulate 20 3`, finish an item, click the banner -> window shows Completed.
- [ ] **Step 5:** Commit.

### Task 7: Clipboard poll loop

**Files:** Create `src-tauri/src/clipboard.rs`; modify `lib.rs` (spawn in `setup`), `Cargo.toml` via `cargo add`. Test: none for the OS read; the logic is Task 4.

**Interfaces:** Consumes `App::clipboard_changed(Option<String>)`.

- [ ] **Step 1:** Look up `tauri-plugin-clipboard-manager` (Context7) for its Rust read API and whether it must run on the main thread; `cargo add tauri-plugin-clipboard-manager`. Check how `arboard` picks Wayland versus X11 when `WAYLAND_DISPLAY` is set and the compositor has no data-control protocol.
- [ ] **Step 2:** Implement `pub fn spawn(app: AppHandle, core: Arc<App>)`: every 500 ms read the clipboard text (error or non-text -> `None`; log only the error kind, never content) and call `clipboard_changed`. Settings changes need no restart because `clipboard_changed` reads the live setting.
- [ ] **Step 3:** Verify in `tauri dev` on macOS: copy a link anywhere -> a Linkgrabber row appears with the window hidden behind others; copy prose -> nothing.
- [ ] **Step 4 (Fedora VM acceptance, spec Y7):** build for Linux with `scripts/build-tauri.sh linux`, install the `.rpm`/`.AppImage` on the VM (`ssh shayanoh@fedora.local`; no sudo, so use the AppImage), run it in the desktop session with the window unfocused, copy a link in another app (Firefox) -> it is grabbed. If it is not, **stop and ask Shayan** (spec 6.1).
- [ ] **Step 5:** Commit.

### Task 8: Tray icon (all platforms)

**Files:** Create `src-tauri/src/tray_icon.rs`, `src-tauri/icons/tray-logo.png`, `src-tauri/src/tray.rs`; modify `lib.rs`, `Cargo.toml` (tauri feature `image-png`, via `cargo add tauri --features image-png` if not already enabled). Test: unit tests in `tray_icon.rs`.

**Interfaces:** Produces

```rust
pub enum Tint { White, Black, Template }                       // Template = draw white; macOS tints it
pub fn compose(logo_rgba: &[u8], width: u32, height: u32, running: bool, fraction: f64, tint: Tint) -> Vec<u8>;
pub fn system_tint() -> Tint;                                  // macOS Template; Windows taskbar theme; Linux color scheme
```

and in `tray.rs`: `pub fn install(app:&AppHandle)->tauri::Result<()>` and `pub fn refresh(app:&AppHandle, status:&TrayStatus)`.

- [ ] **Step 1: Failing tests** on a 4x1 fully opaque white logo: idle -> all alpha 255; running at fraction 0.5 -> alpha `[255,255,64,64]` (25% of 255 rounded); fraction 0 -> all 64; fraction 1 -> all 255; `Tint::Black` -> RGB 0 with the same alphas; NaN fraction -> as 0; `fraction` clamped to 0..1.
- [ ] **Step 2-3:** Implement `compose`; `system_tint` (Windows: registry `SystemUsesLightTheme` under `HKCU\...\Themes\Personalize` via the `windows-registry` or `winreg` crate added with `cargo add --target 'cfg(windows)'`; Linux: the desktop's color scheme through the GSettings/portal value, falling back to White). Build the `TrayIconBuilder` with id `main`, set the image, and on macOS `icon_as_template(true)`.
- [ ] **Step 4:** In the heartbeat loop (`lib.rs`), after `app.heartbeat()` call `tray::refresh(&handle, &core.tray_status())`. `refresh` asks a held `IconTracker` (`needs_redraw(running, fraction, width_px)`), and only then composes and sets the icon. Re-tint on a system theme change (Tauri `ThemeChanged` on the main window, plus a slow re-check of `system_tint()` once per minute for the taskbar theme).
- [ ] **Step 5:** `cargo test -p sdm-tauri`; manual macOS: icon dims and fills as `--simulate 20 3` runs. Commit.

### Task 9: Linux tray menu

**Files:** Modify `src-tauri/src/tray.rs`. Test: pure builder test in `tray.rs`.

**Interfaces:** Consumes `TrayStatus::menu_status_lines`. Produces `fn menu_spec(status:&TrayStatus) -> Vec<MenuEntry>` where `MenuEntry` is `Item{id,label,enabled} | Separator`; ids `open`, `pause`, `review`, `quit`, and `status-N`.

- [ ] **Step 1: Failing tests:** idle status -> `[Open SDM, Pause all, Quit, Separator, "No downloads running"(disabled)]`; with `links_waiting = 4` a `Review (4 links waiting)` entry sits before Quit; 7 running -> 5 item lines + `and 2 more`, all status entries disabled.
- [ ] **Step 2-3:** Implement `menu_spec` and, on Linux only, install the menu on the tray, rebuilding it when the spec differs from the last one (compare the `Vec<MenuEntry>`), at most once per `refresh`. Menu events: `open` -> `show_main(None)`, `review` -> `show_main(Some("linkgrabber"))`, `pause` -> `pause_all`, `quit` -> `lifecycle::request_quit` (Task 11).
- [ ] **Step 4:** Check on the Fedora VM with the AppIndicator extension enabled (spec 10): menu shows, status lines change while a download runs, no flicker/leak over a minute (watch RSS).
- [ ] **Step 5:** Commit.

### Task 10: Popover window (macOS and Windows)

**Files:** Create `src-tauri/src/popover.rs`, `web/src/components/TrayPopover.svelte`, `ProgressRing.svelte`, tests `web/src/components/popover.test.ts`; modify `tray.rs` (click handler), `lib.rs` (command `tray_status`, event emit), `capabilities/default.json` (add window label `tray`), `main.ts` (`?window=tray` -> `TrayPopover`), `bridge.ts`, `tauriBridge.ts`, `mockBridge.ts`.

**Interfaces:** Produces commands `tray_status() -> TrayStatus` and `quit_requested() -> ()` (calls `lifecycle::request_quit`); event `tray-status` (payload `TrayStatus`) emitted to the `tray` window from `refresh` only when the popover is visible and the status changed; `Bridge.trayStatus()`, `Bridge.onTrayStatus(handler)`, `Bridge.showMain(route?)`, `Bridge.requestQuit()`. `popover::toggle(app, anchor: Rect)`.

- [ ] **Step 1: Failing web tests** (`popover.test.ts`, mock bridge): idle shows `0 B/s`, `0 active` and no items; running shows speed, `3 active`, at most five item rows with name and speed; `links_waiting: 4` shows `4 links waiting` and a Review button that calls `showMain("linkgrabber")`; links 0 hides the row; Pause all calls `pauseAll`; Open SDM calls `showMain()`; Quit calls `requestQuit`. Layout width 280 px; colors only from CSS variables (no hex literal in the component: grep test).
- [ ] **Step 2:** `pnpm test popover` fails.
- [ ] **Step 3:** Build `TrayPopover.svelte` from `MenuBarPopoverView.swift` (headline speed, `N active`, `BandwidthGraph` 32 px with `history`, `ProgressRing` + name + speed rows, links row, buttons), subscribed through the bridge; `startTheming` as the Settings window does. Rust: create the window lazily (label `tray`, `index.html?window=tray`, frameless, `always_on_top`, `skip_taskbar`, not resizable, 280 px wide, height fitted by the page like `fitWindow` in the Settings window), hide on `WindowEvent::Focused(false)`, position from the tray `Rect` (below the icon on macOS, above it on Windows, clamped to the monitor work area). Tray click (left or right, button up) toggles it; Linux does not use it.
- [ ] **Step 4:** `pnpm test`, `pnpm check`, `cargo test -p sdm-tauri`; Playwright flow against the mock (`?window=tray`) for idle and running; manual macOS click. Windows VM check recorded as unverified until run.
- [ ] **Step 5:** Commit.

### Task 11: Close, quit and the guard

**Files:** Create `src-tauri/src/lifecycle.rs`, `src-tauri/src/linux_tray.rs` (cfg linux); modify `lib.rs` (`on_window_event`, `RunEvent::ExitRequested`, menu event for Quit on macOS), `Cargo.toml` (`cargo add tauri-plugin-dialog`; on Linux `cargo add zbus --target 'cfg(target_os = "linux")'`). Test: pure decision function.

**Interfaces:** Consumes `TrayStatus::quit_warning`. Produces

```rust
pub enum CloseAction { HideToTray, QuitViaGuard, MacOs }       // by platform and tray presence
pub fn close_action(platform: Platform, tray_present: bool) -> CloseAction;
pub fn request_quit(app: &AppHandle);                          // runs the guard, then app.exit(0)
pub fn tray_host_present() -> bool;                            // linux: owner of org.kde.StatusNotifierWatcher on the session bus; elsewhere true
```

- [ ] **Step 1: Failing tests:** `close_action` table: Windows -> HideToTray; Linux with tray -> HideToTray; Linux without -> QuitViaGuard; macOS -> MacOs. (The guard text is tested in Task 5.)
- [ ] **Step 2-3:** Implement. Main-window `CloseRequested`: `api.prevent_close()` then act: `HideToTray` hides the window and the Settings window and removes the taskbar entry (`set_skip_taskbar(true)`; `show_main` undoes it); `QuitViaGuard` calls `request_quit`; `MacOs` hides (mode rules in Task 13). `request_quit` asks `App::tray_status().quit_warning()`; `None` -> exit; `Some(text)` -> native message dialog (`Quit with active downloads in progress?`, buttons `Quit`/`Cancel`) on a spawned thread, exit on Quit. Cmd+Q and the macOS app-menu Quit go through `RunEvent::ExitRequested` with the same guard (prevent the exit, run the guard, then `app.exit`). Evaluate `tray_host_present()` at close time.
- [ ] **Step 4:** Manual: macOS Cmd+Q with a simulated running download shows the dialog and Cancel keeps it; Fedora VM without the extension: close with a download running shows the dialog, close with none quits. Note Windows VM as unverified.
- [ ] **Step 5:** Commit.

### Task 12: Launch at login and single instance

**Files:** Create `src-tauri/src/autostart.rs`; modify `lib.rs`, `commands.rs` (`update_settings`), `capabilities/default.json`. Test: pure parsing of the `--hidden` flag.

**Interfaces:** Produces `pub fn is_hidden_launch(args: &[String]) -> bool`, `pub fn apply(app:&AppHandle, enabled: bool) -> Result<(), AppError>`, `pub fn is_enabled(app:&AppHandle) -> bool`.

- [ ] **Step 1:** Look up `tauri-plugin-autostart` and `tauri-plugin-single-instance` (Context7); `cargo add` both (autostart and single-instance are desktop-only targets). Register autostart with the arg `--hidden` on Windows and Linux.
- [ ] **Step 2: Failing test:** `is_hidden_launch(["sdm","--hidden"])` true, `["sdm"]` false, `["sdm","--simulate","20","3"]` false.
- [ ] **Step 3:** Implement. `update_settings` command: after the core accepts the DTO, if `launch_at_login` changed call `apply`; on failure restore the stored value (second `update_settings` with the previous DTO) and return the error. At startup, read `is_enabled` and, if it differs from the stored value, write the OS state into settings (the OS is the truth). A hidden launch on Windows and Linux hides the main window right after setup (use `"visible": false` in the window config and show it unless hidden, if flicker-free; otherwise hide in setup). Single instance: the callback calls `show_main(None)`.
- [ ] **Step 4:** Manual: toggle in Settings, check the OS entry (macOS Login Items; registry `Run` key and `~/.config/autostart/*.desktop` noted as unverified until run on the VMs); a second `open -n` focuses the first.
- [ ] **Step 5:** Commit.

### Task 13: macOS Dock / Menu Bar mode

**Files:** Create `src-tauri/src/activation.rs` (cfg macOS); modify `lib.rs`, `commands.rs` (`update_settings`), `tray.rs`. Test: pure function.

**Interfaces:** Produces `pub fn plan(mode: ActivationMode, has_visible_window: bool) -> Plan { dock_icon: bool, tray_visible: bool }`: `MenuBarOnly` -> dock only while a window is visible, tray always; `DockOnly` -> dock always, tray hidden; `Both` -> both always.

- [ ] **Step 1: Failing test** for the table above (six combinations).
- [ ] **Step 2-3:** Implement `apply(app)` setting `set_activation_policy(Regular|Accessory)` and `TrayIcon::set_visible`; call it at startup, after `update_settings` (immediately, including with no window open), and on main-window show/hide. In Menu bar only mode with no window open, the popover's Quit is the way out (Task 10).
- [ ] **Step 4:** Manual on macOS: each mode, with and without a window, switching live from Settings.
- [ ] **Step 5:** Commit.

### Task 14: Close-out

- [ ] **Step 1:** Full verification: `./test.sh` Rust part (`cd sdm-core && cargo test --workspace --exclude sdm-tauri`, then `cargo test -p sdm-tauri`), `cargo clippy --workspace --all-targets`, `pnpm test && pnpm check && pnpm exec playwright test`, `scripts/check-windows.sh`, `scripts/check-linux.sh`, `scripts/build-tauri.sh linux`.
- [ ] **Step 2:** Re-run the spec 10 manual list; record which were done (macOS, Fedora no-tray, Fedora with extension, clipboard acceptance) and which were not (Windows VM unless Shayan ran it; KDE/Sway).
- [ ] **Step 3:** Update `docs/tauri-roadmap.md`: spec 5 row to "built", add a log entry, remove the "Spec 5 next" section's done items, update `CLAUDE.md`'s Tauri paragraph (specs 1-5 built). Set the spec's Status to IMPLEMENTED. Commit.
