# Tauri Settings and Themes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Tauri Settings window (five tabs, exactly the SwiftUI one), store and apply every setting that has behavior today, add a theme picker, and remove `TemporaryTools`.

**Architecture:** One `Settings` struct in `sdm-app` is the single source of truth, persisted in `settings.json` and shared (`Arc<RwLock>`) with the engine, grabber and resolver closures so changes apply without a restart. The frontend holds only buffered edits; OK sends one `update_settings`. The window is a second Tauri window (`settings`) loading the same bundle, routed by label.

**Tech Stack:** Rust (`sdm-app`, `sdm-tauri`, tauri 2, tauri-specta), Svelte 5 + TypeScript + Vite, Vitest, Playwright, pnpm.

**Spec:** [docs/superpowers/specs/2026-10-24-tauri-settings-design.md](../specs/2026-10-24-tauri-settings-design.md). Reference screenshots: `docs/superpowers/specs/assets/2026-10-24-swiftui-settings/`.

## Global Constraints

- Parity with SwiftUI `SDM/SettingsView.swift`: five tabs in the order Appearance, Downloads, Linkgrabber, Media Sites, Notifications; width 560; OK/Cancel bar; OK is the default button (Enter), Cancel answers Esc.
- Defaults and ranges: max concurrent 3 (1..20), segments per file 8 (1..64), global connections 32 (1..256), per host 8 (1..64), min segment MB 10 (1..100), auto-resume off; clipboard on, auto-add-and-start off, deep sniff on, auto-clear 30 minutes (15 min, 30 min, 1 h, 1 day, never); max height 1080 on the ladder 1920/1440/1080/720/540/480, playlist cap 50 (10..200), all codecs/containers on, cookies None, channel Stable; four notification toggles on; launch at login off; mode Both; theme "system".
- Colors are theme roles (`var(--role)`), never literals.
- No download-folder control in the UI. `update_settings` leaves the stored folder unchanged unless a folder is sent; the Settings window never sends one.
- Dock / Menu Bar section is macOS only. Windows and Linux store "both" and hide the section.
- Launch at login, Dock / Menu Bar mode, the four notification toggles, Watch clipboard and Auto-add-and-start are stored and inert until spec 5.
- The last checked box of Video codecs, Containers and Audio codecs is disabled.
- Shortcuts: Cmd+, on macOS, Ctrl+, elsewhere; ignored while a modal is open or a text field has focus.
- Format Rust with `cargo fmt`, TypeScript and Svelte with `prettier -w`. Run Rust tests from `sdm-core/` (`cargo test -p sdm-app`) and web tests from `sdm-tauri/web/` (`pnpm test`, `pnpm check`). No new packages are expected; if one is needed add it with `pnpm add`.
- Commit after each task (Shayan's blanket consent for commits in this repo), ending messages with the attribution lines from the session reminder.

## Review Focus

- A settings file from before this spec (only `downloadFolder`) loads with every new field at its default.
- A hand-edited file with out-of-range numbers, an unknown cookie source or theme id, or all codecs off loads clamped/defaulted, never failing and never giving the selector an empty allowlist.
- OK with an invalid value changes nothing and leaves the window open with the reason shown.
- Cancel (and closing the window) reverts a previewed theme in the main window.
- A stored theme id that no longer exists falls back to System without an error banner.
- Opening Settings twice focuses the existing window; Ctrl/Cmd+, while Settings is already open does the same.

---

### Task 1: The `Settings` model and store

**Files:**
- Modify: `sdm-core/crates/sdm-app/src/settings.rs`, `sdm-core/crates/sdm-app/src/protocol.rs` (the `SettingsDto`, `BinariesStatusDto`)

**Interfaces:**
- Produces: `pub struct Settings` (serde camelCase, `#[serde(default)]` per field, `Default` = the Global Constraints values) with the fields `download_folder: PathBuf`, `max_concurrent`, `segments_per_item`, `global_max_connections`, `max_connections_per_host`, `min_segment_size_mb` (all `usize`), `auto_start_downloads_on_launch: bool`, `clipboard_watching_enabled`, `auto_add_and_start_on_grab`, `deep_sniff_enabled: bool`, `auto_clear_interval: AutoClearInterval`, `yt_max_height: u32`, `allow_av1/vp9/h264/mp4/webm/opus/aac: bool`, `max_playlist_videos: usize`, `cookie_source: CookieSource` (string, from `sdm-resolve`), `yt_dlp_channel: YtDlpChannel`, `notify_download_finished/package_finished/download_failed/links_grabbed: bool`, `launch_at_login: bool`, `activation_mode: ActivationMode`, `theme_id: String`. `AutoClearInterval { FifteenMinutes, ThirtyMinutes, OneHour, OneDay, Never }` with `fn duration(self) -> Option<Duration>`; `ActivationMode { MenuBarOnly, DockOnly, Both }`; both serde camelCase strings.
- Produces: `impl Settings { pub fn clamped(self) -> Settings }` (ranges above, resolution snapped onto the ladder as `MediaSitesSettingsStore.maxHeight` does, an empty codec/container/audio set restored to all, empty `theme_id` to "system").
- Produces: `pub type SharedSettings = Arc<RwLock<Settings>>`; `SettingsStore::shared(&self) -> SharedSettings`; `SettingsStore::get(&self) -> SettingsDto`; `SettingsStore::update(&mut self, dto: SettingsDto) -> Result<(), AppError>` (validates, clamps nothing silently: an out-of-range value is `AppError::invalid(field, reason)`; folder validated only when `dto.download_folder` is `Some`; persists atomically; updates the shared value last).
- Produces: `SettingsDto` with the same fields as `Settings` (camelCase) except `download_folder: Option<String>`; `BinariesStatusDto` gains `bundled: bool` and `downloading: bool`.

- [ ] **Step 1: Write failing tests** in `settings.rs`'s test module: `an_old_file_with_only_the_folder_loads_with_defaults`, `out_of_range_numbers_in_the_file_are_clamped`, `an_unknown_cookie_source_falls_back_to_none`, `an_empty_codec_set_is_restored_to_all`, `the_resolution_snaps_onto_the_ladder` (1000 becomes 720, 5000 becomes 1920), `update_rejects_out_of_range_values_and_changes_nothing` (each numeric field, one case per range), `update_without_a_folder_keeps_the_folder`, `update_persists_and_reloads_every_field`, `the_shared_value_follows_an_update`. Keep the existing four folder tests.
- [ ] **Step 2: Run** `cargo test -p sdm-app settings` from `sdm-core/`; expected: compile errors or FAIL.
- [ ] **Step 3: Implement** the types and store above. Load tolerates a corrupt file (defaults, as now). Per-field tolerance: parse the file as `serde_json::Value` and take each known key that deserializes, so one bad field does not discard the rest.
- [ ] **Step 4: Run** `cargo test -p sdm-app settings`; expected: PASS.
- [ ] **Step 5: Commit** "Store every SwiftUI setting in the Rust settings file".

### Task 2: Apply settings to the engine and expose the commands

**Files:**
- Modify: `sdm-core/crates/sdm-app/src/app.rs` (`Dependencies`, `with_limits`, `restore`, `update_settings`, `binaries_status`), `sdm-core/crates/sdm-app/src/toolchain.rs`, `sdm-core/crates/sdm-app/src/simulation.rs` (if it builds `Dependencies`), `sdm-tauri/src-tauri/src/commands.rs`, `sdm-tauri/src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: Task 1's `Settings`, `SettingsStore`, `SharedSettings`.
- Produces: `Dependencies.settings: Option<SettingsStore>` (production loads it in `production_dependencies` and shares `store.shared()` with the closures of Task 3; `None` makes `App` load its own from `host.data_dir()`).
- Produces: `fn engine_settings(settings: &Settings) -> EngineSettings` (all five concurrency fields, `min_segment_size_bytes = mb * 1024 * 1024`, folder). `new_with_concurrency` keeps its override (the load harness) by storing `Option<Limits>` that takes precedence.
- Produces: `App::update_settings(&self, dto: SettingsDto) -> Result<(), AppError>` now applies engine settings and publishes; `App::restore` calls `engine.resume_all` after the engine restore when `auto_start_downloads_on_launch`; `App::check_binaries_now(&self) -> Result<(), AppError>` (`CheckReason` that SwiftUI's "Check now" uses; error when no binaries owner); `BinariesStatusDto.bundled = host.bundled_tools_dir().is_some()`, `downloading = matches!(tools_activity, Fetching(_))`.
- Produces: tauri command `check_binaries_now`, registered in `collect_commands!`; bindings regenerate (`cargo test -p sdm-tauri export_bindings` from `sdm-core/`, or a debug build).

- [ ] **Step 1: Write failing tests** in `sdm-app` (existing test files for `App` with `FakeHost`): `update_settings_changes_the_running_limit` (set max concurrent 1, add three items, tick, exactly one runs), `update_settings_with_no_folder_leaves_the_folder`, `settings_survive_a_restart` (new `App` on the same data dir reads them), `auto_resume_on_launch_resumes_enabled_stopped_items_only` (flag on: an enabled stopped item runs after `restore`; flag off: stays stopped; a disabled item stays stopped), `binaries_status_reports_bundled_from_the_host`.
- [ ] **Step 2: Run** `cargo test -p sdm-app`; expected: FAIL.
- [ ] **Step 3: Implement** the interfaces above. Replace the two uses of the settings folder getter and the `engine_settings(limits, folder)` calls.
- [ ] **Step 4: Run** `cargo test -p sdm-app && cargo test -p sdm-tauri`; expected: PASS (bindings regenerated, `bindings.ts` diff reviewed).
- [ ] **Step 5: Commit** "Apply the stored engine settings and add check-now".

### Task 3: Apply grabber, resolver and binaries settings live

**Files:**
- Modify: `sdm-core/crates/sdm-app/src/grabber.rs` (remove `GrabberDefaults` value constants that become settings: deep sniff, auto-clear, quality preferences; keep the probe-limit constants), `sdm-core/crates/sdm-app/src/toolchain.rs`, `sdm-core/crates/sdm-grabber/src/link_prober.rs`, `sdm-core/crates/sdm-app/src/app.rs` (line using `GrabberDefaults::quality_preferences`)

**Interfaces:**
- Consumes: `SharedSettings`.
- Produces: `GrabberController::new(..., settings: SharedSettings)` and `production(..., settings)`; `quality_preferences()` built from the settings (`QualityPreferences` from the booleans and height, an empty set restored to all as in SwiftUI); `prune` uses `auto_clear_interval.duration()` and does nothing for `Never`.
- Produces: `LinkProber::with_deep_sniff_source(self, source: impl Fn() -> bool + Send + Sync + 'static) -> Self`, which takes precedence over the `deep_sniff_enabled` field (field and `new` stay, other callers unaffected); the grabber installs a source reading the settings.
- Produces: in `production_dependencies`, the `YtDlpResolver` and `YtDlpWholesaleDownloader` cookie closures read `cookie_source`, the playlist closure reads `max_playlist_videos`, and `ManagedBinaries::new`'s channel closure reads `yt_dlp_channel`.

- [ ] **Step 1: Write failing tests**: in `sdm-grabber`, `a_deep_sniff_source_overrides_the_flag_per_probe` (source flips between two probes with a fake transport); in `sdm-app`, `quality_preferences_follow_the_settings` (height and codec toggles), `an_emptied_codec_set_falls_back_to_all`, `prune_uses_the_stored_interval` (advance the injected clock past 15 minutes with the interval set to fifteen minutes: row cleared; with `Never`: kept), `the_resolver_reads_the_stored_cookie_source_and_playlist_cap` (against the fake process runner, assert the arguments contain `--cookies-from-browser chrome` and the cap after an update), `the_binaries_channel_follows_the_setting`.
- [ ] **Step 2: Run** `cargo test -p sdm-grabber -p sdm-app`; expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p sdm-grabber -p sdm-app && cargo build -p sdm-tauri`; expected: PASS. Also `cargo build` for the workspace so the FFI crate still compiles.
- [ ] **Step 5: Commit** "Make grabber, resolver and tool-channel settings live".

### Task 4: Frontend types, bridges and the settings model

**Files:**
- Modify: `sdm-tauri/web/src/lib/bridge.ts`, `tauriBridge.ts`, `mockBridge.ts`, `mockBridge.test.ts`
- Create: `sdm-tauri/web/src/lib/settingsModel.ts`, `settingsModel.test.ts`

**Interfaces:**
- Consumes: regenerated `SettingsDto`, `BinariesStatusDto` in `bindings.ts`.
- Produces on `Bridge`: `checkBinariesNow(): Promise<void>`; `openSettings(): Promise<void>` (opens or focuses the settings window); `broadcastTheme(themeId: string | null): Promise<void>` (preview, `null` = revert to the stored one) and `onThemePreview(handler: (themeId: string | null) => void): () => void`. `MockBridge` implements all, keeps a full in-memory `SettingsDto` with the same validation as Rust (ranges), a settable `binaries` status, and records opens and broadcasts for tests.
- Produces in `settingsModel.ts`: `RESOLUTION_CHOICES = [1920,1440,1080,720,540,480]`, `snapResolution(h: number): number`, `orderThemes(themes: {id: string; name: string}[]): {id: string; name: string}[]` (Light, Dark, then the rest by name), `lastChecked(flags: boolean[]): boolean` helper `isLastChecked(group: boolean[], index: number): boolean`, `class SettingsDraft` (holds a copy of `SettingsDto`, `dirty`, `set(field, value)`, `toDto()` without `downloadFolder`), `AUTO_CLEAR_LABELS`, `cookieChoices(platform): string[]` (None, Chrome, Firefox, Edge, Brave, plus Safari on mac, in the SwiftUI `CookieSource` order: None, Safari, Chrome, Firefox, Edge, Brave).

- [ ] **Step 1: Write failing tests** (`settingsModel.test.ts`): `snapResolution(1000)` is 720, `snapResolution(5000)` 1920, `snapResolution(300)` 480, an on-ladder value is unchanged; `orderThemes` puts light and dark first then alphabetical; `isLastChecked` true only for the single checked box; `cookieChoices("windows")` has no Safari and starts with None, `cookieChoices("mac")` has Safari second; `toDto` never contains `downloadFolder`. Extend `mockBridge.test.ts`: an out-of-range `updateSettings` rejects and keeps the old value, a valid one round-trips.
- [ ] **Step 2: Run** `pnpm test` in `sdm-tauri/web`; expected: FAIL.
- [ ] **Step 3: Implement** the interfaces. In `TauriBridge`, `openSettings` and the theme broadcast go through two new Tauri commands (Task 6) and `@tauri-apps/api/event`.
- [ ] **Step 4: Run** `pnpm test && pnpm check`; expected: PASS.
- [ ] **Step 5: Commit** "Add the settings model and bridge methods".

### Task 5: The Settings UI

**Files:**
- Create: `sdm-tauri/web/src/components/settings/SettingsWindow.svelte`, `SettingsSection.svelte`, `SteppedNumberField.svelte`, `SettingsPopup.svelte`, `AppearanceTab.svelte`, `DownloadsTab.svelte`, `LinkgrabberTab.svelte`, `MediaSitesTab.svelte`, `ComponentsSection.svelte`, `NotificationsTab.svelte`, `sdm-tauri/web/e2e/settings.spec.ts`, `sdm-tauri/web/src/test/settings.test.ts`
- Modify: `sdm-tauri/web/src/main.ts`, `sdm-tauri/web/src/App.svelte`, `sdm-tauri/web/src/lib/icons.ts`

**Interfaces:**
- Consumes: Task 4's `SettingsDraft`, `Bridge` methods, `detectPlatform`, `Icon`.
- Produces: `SettingsWindow` (props: none; reads the bridge, a draft and `themeStore` data from context). `main.ts` mounts `SettingsWindow` instead of `App` when the Tauri window label is `settings` or the browser URL has `?window=settings`; the settings window gets its own `UiState`-free context (bridge only) and applies the stored theme on load.
- `SteppedNumberField` props: `label: string`, `value: number`, `min: number`, `max: number`, `onchange(value)`, `hint?: string` (the Media Sites field shows "10..200").
- Tab icons (Lucide, in `icons.ts`): `paintbrush`, `circle-arrow-down`, `link`, `clapperboard` (for `film.stack`), `bell`.

- [ ] **Step 1: Write failing component tests** (`settings.test.ts`, `@testing-library/svelte` against `MockBridge`): `shows the five tabs in order`; `the Downloads tab shows the five stepped fields with their defaults`; `OK sends one update and closes; Cancel sends none`; `the last checked codec checkbox is disabled`; `the Dock / Menu Bar section is absent on windows and linux and present on mac` (inject the platform); `the Components rows say (bundled) when status.bundled and a download status when not`; `an update the bridge rejects keeps the window open and shows the message`; `Enter presses OK and Escape presses Cancel`; `changing the update channel runs a check immediately and is not buffered`; `no control edits the download folder`.
- [ ] **Step 2: Run** `pnpm test`; expected: FAIL.
- [ ] **Step 3: Implement** the components to the layout in the reference screenshots: 560 wide, section = bold heading over a bordered rounded card (`--surface-secondary`, `--border`), three side-by-side cards on Media Sites, popups right-aligned with the widths from `SettingsView.swift` (theme and Dock 220, auto-clear 170, cookies 160, resolution 120, channel 120), OK as the accent default button. Components polling: `binariesStatus` every 5 s while mounted. The theme popup lists System, then `orderThemes` of the catalog (add `theme_list` command? no: reuse a small Rust command `list_themes() -> Vec<ThemeInfo { id, name }>` added in Task 6 and a `listThemes` bridge method; until then the mock returns a fixed list).
- [ ] **Step 4: Playwright**: `settings.spec.ts` opens `?window=settings&theme=deep-purple` at the reference width, screenshots each tab into `sdm-tauri/web/test-results/` for by-eye comparison with the references, and asserts tab switching and the disabled-last-box rule. Run `pnpm exec playwright test settings`; expected: PASS.
- [ ] **Step 5: Run** `pnpm test && pnpm check`; expected: PASS. Commit "Add the Settings window".

### Task 6: Open the window, theme everywhere, shell integration

**Files:**
- Modify: `sdm-tauri/src-tauri/src/lib.rs`, `commands.rs`, `sdm-tauri/src-tauri/capabilities/default.json`, `sdm-tauri/src-tauri/tauri.macos.conf.json` or code for the menu, `sdm-core/crates/sdm-app/src/app.rs` (`list_themes`), `sdm-tauri/web/src/components/Sidebar.svelte`, `Shell.svelte`, `sdm-tauri/web/src/lib/shortcuts.ts`, `shortcuts.test.ts`, `App.svelte`, `sdm-tauri/web/src/lib/tauriBridge.ts`

**Interfaces:**
- Produces: commands `open_settings` (creates the `settings` webview window at 560 wide, not resizable, not maximizable, titled "Settings", url `index.html?window=settings`; if it exists, show and focus it), `broadcast_theme(theme_id: Option<String>)` (emits event `theme-preview` to all windows), `list_themes() -> Vec<ThemeInfo { id: String, name: String }>`; `App::list_themes`.
- Produces: `ShortcutContext.openSettings(): void`; `handleKey` calls it for Mod+, (Cmd+, on mac, Ctrl+, elsewhere), subject to the modal and editable guards, on every tab. The settings window itself also handles Mod+, by focusing itself (no-op) and Escape/Enter per Task 5.
- Produces: a gear button at the bottom of the sidebar (`aria-label="Settings"`, Lucide `settings`), on every platform, calling `bridge.openSettings()`; on macOS a native app menu item "Settings…" (accelerator `CmdOrCtrl+,`) that calls the same open logic; no menu on Windows and Linux.
- Produces: theme application. `App.svelte` reads the stored `themeId` at startup (`getSettings`), resolves "system" with `matchMedia("(prefers-color-scheme: dark)")` to `light` or `dark` (and follows OS changes while "system"), falls back to system for an unknown id, and swaps the `<style id="theme">` content when a `theme-preview` event arrives (`null` = reload the stored theme). The settings window does the same. Capability file lists both windows.

- [ ] **Step 1: Write failing tests**: `shortcuts.test.ts`: `Mod+, opens settings on mac with Meta and on windows with Ctrl`, `is ignored behind a modal and inside a text field`, `does not fire for the wrong modifier`; `Sidebar` test: `the gear is present and opens settings through the bridge on every platform`; Rust: `list_themes_returns_the_bundled_themes_with_light_and_dark` in `sdm-app`; a `theme` frontend unit (`lib/themeSelection.test.ts` with `themeSelection.ts`): `system resolves to dark or light from the OS`, `an unknown id falls back to system`.
- [ ] **Step 2: Run** `pnpm test` and `cargo test -p sdm-app`; expected: FAIL.
- [ ] **Step 3: Implement** the interfaces. `TauriBridge.broadcastTheme` calls `broadcast_theme`; `onThemePreview` uses `listen("theme-preview")`. On Cancel and on window close the settings window broadcasts `null`; on OK it persists first, then broadcasts the new id.
- [ ] **Step 4: Run** `cargo test -p sdm-tauri` (regenerates bindings), `pnpm test`, `pnpm check`; expected: PASS. Run `pnpm tauri dev` (or the repo's run script) and confirm by hand: the gear, the shortcut and (macOS) the menu item open one window and a second use focuses it; picking a theme previews in the main window; Cancel reverts it. Report any step that could not be checked.
- [ ] **Step 5: Commit** "Open Settings from the gear, the shortcut and the macOS menu; theme the app".

### Task 7: Remove the temporary UI and finish

**Files:**
- Delete: `sdm-tauri/web/src/components/TemporaryTools.svelte`
- Modify: `sdm-tauri/web/src/components/LinkgrabberView.svelte`, `sdm-tauri/web/e2e/*.spec.ts` and `src/test/grabber.test.ts` (anything that referenced the folder field or tools line), `docs/tauri-roadmap.md`, `docs/superpowers/specs/2026-10-24-tauri-settings-design.md` (status banner), `CLAUDE.md` (Tauri UI paragraph)

- [ ] **Step 1: Write the failing test**: `grabber.test.ts` `the link grabber view has no download folder field and no tools line`.
- [ ] **Step 2: Run** `pnpm test`; expected: FAIL.
- [ ] **Step 3: Remove** `TemporaryTools` and its use; fix the tests and specs that referenced it; the `GrabberDefaults` doc comment about spec 4 is updated to say what remains constant.
- [ ] **Step 4: Full verification**: `cargo test` (workspace, from `sdm-core/`), `cargo clippy --workspace -- -D warnings`, `pnpm test`, `pnpm check`, `pnpm exec playwright test`, plus the cross-compile gates the foundation spec lists if they are scripted in `scripts/` (run them if present, report if skipped). Update the roadmap row 4 to "built on branch `tauri-ui`" with the unverified items (Windows VM visuals, a real tool download, side-by-side with SwiftUI), the status banner in the design, and the CLAUDE.md paragraph (specs 1 to 4 built).
- [ ] **Step 5: Commit** "Remove TemporaryTools and close spec 4".
