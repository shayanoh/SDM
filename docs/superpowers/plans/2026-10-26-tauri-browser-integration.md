# Browser integration (spec 6) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the two open Tauri bugs, then make the Chrome extension work with the Tauri app on Windows, Linux and macOS: capture downloads, and keep a site-filtered `cookies.txt` current for yt-dlp.

**Architecture:** A new library crate `sdm-browser` (native-messaging framing, message handling, cookie filter and Netscape writer, status file, host manifest) is called by the Tauri binary itself when Chrome launches it as the native host (no separate sidecar: Chrome passes `chrome-extension://<id>/` as the first argument, and `main` branches on it before Tauri starts). The extension asks the host for the site list, filters `chrome.cookies` by it and sends full snapshots. `sdm-resolve` gains per-site `cookie_domains` and a cookie-provider seam so `CookieSource::Chrome` yields `--cookies <temp copy>` in the Tauri app while `sdm-ffi` (SwiftUI) keeps `--cookies-from-browser`.

**Tech Stack:** Rust 2024 workspace (`sdm-core/`), Tauri v2 + tauri-specta, Svelte 5 + vitest, MV3 extension (plain JS, `node --test`).

**Spec:** [docs/superpowers/specs/2026-10-26-tauri-browser-integration-design.md](../specs/2026-10-26-tauri-browser-integration-design.md)

## Global Constraints

- Load-unpacked only; extension ID `fnnpcbmjpapeohfmpiefbocloodhlgjf`; host name `com.shayanoh.sdm.chrome_extension`.
- Stable Google Chrome only. Several profiles: last sync wins; one file, no merging.
- Cookie file: `<data_dir>/chrome-cookies.txt`, Netscape format, written atomically, `0600` on macOS/Linux, never logged. Each yt-dlp run gets its own temp copy, deleted after; stale copies swept at startup.
- Missing cookie file: resolves still run without cookies; never fall back to `--cookies-from-browser chrome`.
- Download hand-off = SwiftUI behavior: main window forward on the Linkgrabber, URL goes through the grabber ingest, referrer unused.
- No test touches the network or a real clock. Do not edit `legacy/SDMKitSwift` or the SwiftUI app. `sdm-ffi` behavior must not change.
- Format Rust with `cargo fmt`, TS/Svelte with `prettier -w`. New dependencies via `cargo add` / `pnpm add`, never by hand-editing manifests. Commits are allowed freely, fast-forward pushes only.
- No password prompt, admin prompt or console window at any point.

## Review Focus

- Cookie `domain` forms: `.youtube.com`, `music.youtube.com` (host-only), `YOUTUBE.com` (case), a look-alike `notyoutube.com`, and an empty domain; wrong inclusion leaks a login or drops one.
- Cookie values with tabs, newlines, `#` at the start of a domain or name, and unicode: a malformed `cookies.txt` line breaks yt-dlp's whole jar.
- Session cookies (no expiry), `__Host-`/`__Secure-` prefixed names, and huge snapshots (several thousand cookies): the writer must not truncate or reorder-corrupt.
- A native message that is 0 bytes, truncated, over the cap, or not JSON; the host must exit cleanly without writing the file.
- The cookie file replaced while yt-dlp runs (copy-per-run) and a crash leaving temp copies behind.
- Two quick `cookies` messages (two host processes writing at once): the file must always be a complete snapshot.

---

### Task 1: Popover visible at startup (bug)

**Files:**
- Modify: `sdm-tauri/src-tauri/src/lib.rs` (the `tauri_plugin_window_state` denylist), `sdm-tauri/src-tauri/src/popover.rs` if the cause is elsewhere
- Test: `sdm-tauri/src-tauri/src/lib.rs` `tests` module

**Interfaces:**
- Produces: `popover::LABEL` stays `"tray"`; no new API.

Working hypothesis (verify first, do not patch blindly): `tauri_plugin_window_state` is registered for every window except `settings`, so it restores the tray window's saved `visible: true` (or shows it) when the window is created at startup.

- [ ] **Step 1: Reproduce.** Run `cd sdm-tauri && pnpm tauri dev` on macOS; confirm the popover shows at startup and stays until a click and a click elsewhere. Look at the saved window-state file (`~/Library/Application Support/com.shayanoh.sdm.tauri/.window-state.json`) for a `tray` entry.
- [ ] **Step 2: Write the failing test** `window_state_skips_the_tray_and_settings_windows` in `lib.rs` tests: asserts the denylist constant (extract it as `const WINDOW_STATE_DENYLIST: [&str; 2]`) contains `popover::LABEL` and `settings_window::LABEL`. Run it; it fails to compile/pass.
- [ ] **Step 3:** Extract the constant, add the tray label, use it in `with_denylist`. If Step 1 shows another cause, fix that instead and replace the test with one that pins the real cause.
- [ ] **Step 4:** `cargo test -p sdm-tauri window_state` passes. Delete the stale `tray` entry from the developer's `.window-state.json` and re-run the app: nothing visible at startup; the first tray click shows the popover; a click elsewhere hides it. Record the result (macOS only; Windows owed by hand).
- [ ] **Step 5: Commit** `Fix the tray popover showing at startup`.

### Task 2: No console window on Windows (bug)

**Files:**
- Modify: `sdm-core/crates/sdm-resolve/src/system_process_runner.rs:193` (the single `Command::new` in `execute`)
- Test: `sdm-core/crates/sdm-resolve/tests/system_process_runner_tests.rs` (a unit test inside the module is fine for the constant)

**Interfaces:**
- Produces: `pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;` and `pub(crate) fn hide_console(command: &mut tokio::process::Command)` (no-op off Windows).

- [ ] **Step 1: Write the failing test** `create_no_window_flag_value` asserting `CREATE_NO_WINDOW == 0x0800_0000`.
- [ ] **Step 2:** Run `cargo test -p sdm-resolve create_no_window`; fails (not defined).
- [ ] **Step 3:** Implement both items; call `hide_console(&mut command)` in `execute` before `spawn`. On Windows use `tokio::process::Command::creation_flags` (check docs via Context7 for the current tokio API).
- [ ] **Step 4:** Grep `sdm-core/crates` and `sdm-tauri/src-tauri/src` for any other `Command::new` (known: `tray_icon.rs` runs `gsettings`, Linux only, no change). Run `cargo test -p sdm-resolve` and `cargo check --target x86_64-pc-windows-msvc -p sdm-resolve` if the target is installed.
- [ ] **Step 5: Commit** `Run tools without a console window on Windows`. (By-hand check on the Windows VM is owed: no console flash during a YouTube download or mux.)

### Task 3: Cookie domains in the site registry

**Files:**
- Modify: `sdm-core/crates/sdm-resolve/src/site_registry.rs`, `sdm-core/crates/sdm-resolve/src/lib.rs`
- Test: `sdm-core/crates/sdm-resolve/tests/site_registry_tests.rs`

**Interfaces:**
- Produces on `SitePattern`: field `pub cookie_domains: Vec<String>` and `pub fn with_cookie_domains(self, domains: &[&str]) -> Self`.
- Produces on `SiteRegistry`: `pub fn cookie_domains() -> Vec<String>` (all patterns' domains, lowercased, sorted, de-duplicated), `pub fn cookie_domain_matches(domain: &str) -> bool`, `pub fn cookie_domains_version() -> String` (FNV-1a 64-bit of the joined list, 16 lowercase hex digits).

- [ ] **Step 1: Write failing tests:** `cookie_domain_matches` for `.youtube.com`, `music.youtube.com`, `accounts.google.com`, `YOUTUBE.COM`, `.google.com` (all true), `notyoutube.com`, `youtube.com.evil.net`, `""`, `.com` (all false); `cookie_domains_are_sorted_and_unique`; `version_changes_when_the_list_changes` (compute the hash with a helper taking a slice) and a fixed expected value for a known input; **drift guard** `every_registry_host_is_covered_by_its_own_cookie_domains` (for each pattern, each `Exact(h)` and each `Suffix(s)` host `h` has a `cookie_domains` entry equal to it or a parent of it).
- [ ] **Step 2:** Run; fails.
- [ ] **Step 3:** Implement the matcher: lowercase, strip one leading dot, match when equal to an entry or ends with `.` + entry; reject empty. Add `cookie_domains` data to every pattern (registrable domains; YouTube: `youtube.com`, `youtu.be`, `google.com`, `googlevideo.com`; read each site's hosts in the file and add the parent registrable domain plus well-known login domains you are sure of). Mark the list as "grown by real use" in a comment.
- [ ] **Step 4:** `cargo test -p sdm-resolve site_registry` passes; `cargo clippy -p sdm-resolve`.
- [ ] **Step 5: Commit** `Add cookie domains to the site registry`.

### Task 4: `sdm-browser` crate: cookie model, filter, Netscape writer

**Files:**
- Create: `sdm-core/crates/sdm-browser/` (`Cargo.toml` via `cargo new --lib`, `src/lib.rs`, `src/cookies.rs`, `tests/cookies_tests.rs`)

**Interfaces:**
- Consumes: `sdm_resolve::SiteRegistry::cookie_domain_matches`.
- Produces (`sdm_browser::cookies`): 
  `pub struct ChromeCookie { pub domain: String, pub name: String, pub value: String, pub path: String, pub secure: bool, pub http_only: bool, pub expiration_date: Option<f64> }` (serde `camelCase`, as `chrome.cookies` returns it, `hostOnly` ignored);
  `pub fn filter_cookies(cookies: Vec<ChromeCookie>) -> Vec<ChromeCookie>` (drops non-matching and cookies with a tab, CR or LF in domain, name, value or path);
  `pub fn render_netscape(cookies: &[ChromeCookie]) -> String`;
  `pub fn write_cookie_file(path: &Path, contents: &str) -> std::io::Result<()>` (temp file in the same folder, `0600` on Unix, atomic rename).

- [ ] **Step 1: Write failing tests** (fixture tables): a header line `# Netscape HTTP Cookie File`; HttpOnly rows get the `#HttpOnly_` prefix; domain flag `TRUE` iff the domain starts with a dot; secure flag; expiry `0` for session cookies and the integer part for others; tab-separated seven fields; `filter_cookies` drops `notyoutube.com` and a cookie whose value contains a tab/newline; `write_cookie_file` leaves a complete file (second write replaces the first; no leftover temp files; mode `0600` on Unix).
- [ ] **Step 2:** `cargo test -p sdm-browser` fails.
- [ ] **Step 3:** Implement. Deps via `cargo add serde --features derive`, `cargo add serde_json`, `cargo add sdm-resolve --path ../sdm-resolve`, `cargo add --dev tempfile`.
- [ ] **Step 4:** Tests pass; `cargo clippy -p sdm-browser`.
- [ ] **Step 5: Commit** `Add the sdm-browser crate with the cookie filter and writer`.

### Task 5: Native-messaging framing and message handling

**Files:**
- Create: `sdm-core/crates/sdm-browser/src/framing.rs`, `src/host.rs`, `src/status.rs`, `tests/framing_tests.rs`, `tests/host_tests.rs`

**Interfaces:**
- Produces (`framing`): `pub const MAX_INCOMING: usize = 8 * 1024 * 1024;` `pub const MAX_OUTGOING: usize = 1024 * 1024;` `pub fn read_message(input: &mut impl Read) -> Result<Vec<u8>, FramingError>`; `pub fn write_message(output: &mut impl Write, payload: &[u8]) -> Result<(), FramingError>`; `FramingError::{Eof, ZeroLength, TooLarge, Io(String)}`.
- Produces (`status`): `pub struct ExtensionStatus { pub version: String, pub last_seen: String }` (RFC 3339 `lastSeen`, same file the Swift host writes); `pub fn write_status(dir: &Path, version: &str, now: SystemTime) -> io::Result<()>`; `pub fn read_status(dir: &Path) -> Option<ExtensionStatus>`.
- Produces (`host`): `pub struct HostEnvironment { pub data_dir: PathBuf, pub app_executable: PathBuf, pub now: Box<dyn Fn() -> SystemTime + Send + Sync>, pub launch: Box<dyn Fn(&Path, &[String]) -> io::Result<()> + Send + Sync> }`; `pub fn handle(env: &HostEnvironment, request: &[u8]) -> Result<Vec<u8>, HostError>` (returns the JSON reply: `{"success":true}` / `{"success":true,"version":…,"domains":[…]}` for `siteList` / `{"success":true,"written":n,"siteListVersion":…}` for `cookies`); `pub fn run_stdio(env: &HostEnvironment) -> i32` (one message in, one out, exit code).
  Message types: `extensionStatus {version}`, `download {url, referrer}` (validates `http(s)` URL, calls `launch(app_executable, ["--add-url", url])`), `siteList`, `cookies {siteListVersion, cookies}` (filters, renders, writes `<data_dir>/chrome-cookies.txt`; replies with the host's own version when it differs).

- [ ] **Step 1: Write failing tests:** framing round trip, zero length, over-cap, truncated body, little-endian length; `handle` per message type with a temp dir and a recording `launch` (download launches once with the right args; a `file://` URL is rejected, nothing launched); `cookies` writes the file and a second snapshot replaces it entirely; invalid JSON / unknown `type` returns `HostError` and writes nothing; `siteList` returns `SiteRegistry::cookie_domains()` and its version.
- [ ] **Step 2:** Run; fails.
- [ ] **Step 3:** Implement; reuse `write_cookie_file`. `run_stdio` must never print to stdout except the reply (stderr only for errors, no cookie values).
- [ ] **Step 4:** Tests pass.
- [ ] **Step 5: Commit** `Add native-messaging framing and the host message handlers`.

### Task 6: Host mode in the Tauri binary and the `--add-url` hand-off

**Files:**
- Modify: `sdm-tauri/src-tauri/src/main.rs`, `lib.rs` (single-instance callback and setup), `sdm-tauri/src-tauri/Cargo.toml` (`cargo add sdm-browser --path ../../sdm-core/crates/sdm-browser`)
- Create: `sdm-tauri/src-tauri/src/handoff.rs`
- Test: unit tests in `handoff.rs`

**Interfaces:**
- Produces: `pub fn is_native_host_launch(args: &[String]) -> bool` (true when any argument starts with `chrome-extension://`); `pub fn add_url_from_args(args: &[String]) -> Option<String>` (value after `--add-url`, only `http`/`https`); `pub fn deliver(app: &AppHandle, core: &Arc<App>, url: String)` (shows the main window, navigates to the Linkgrabber like the notification click does, ingests the URL through the grabber).
- Consumes: `sdm_browser::host::{run_stdio, HostEnvironment}`; `app::ingest` equivalent (find the controller method the clipboard watcher uses, `clipboard.rs`).

- [ ] **Step 1: Write failing tests** for `is_native_host_launch` and `add_url_from_args` (missing value, `file://`, extra args, Windows `--parent-window=123` argument alongside the origin).
- [ ] **Step 2:** Fails.
- [ ] **Step 3:** Implement. `main()` checks `is_native_host_launch` first and returns `std::process::exit(run_stdio(...))` without starting Tauri (`data_dir` as `TauriHost` computes it: factor that path logic into a function usable without an `AppHandle`, e.g. `dirs`-free via `tauri::path` is not available pre-run; use the same `data_dir` rule documented in `host.rs` through the `dirs`-equivalent already in the dependency tree, checked with Context7/cargo tree). In `lib.rs`, the single-instance callback and setup call `deliver` when `add_url_from_args` matches; on a cold start `deliver` runs after the controller exists.
- [ ] **Step 4:** Tests pass; with the app running, run `target/debug/sdm-tauri --add-url https://example.com/a.zip` and confirm the running instance shows the Linkgrabber with the URL (macOS by hand).
- [ ] **Step 5: Commit** `Run the native host from the app binary and hand downloads to the running app`.

### Task 7: Cookie provider seam in the resolver and the Chrome cookie lease

**Files:**
- Modify: `sdm-core/crates/sdm-resolve/src/yt_dlp_resolver.rs` (three call sites that extend args with the cookie source), `yt_dlp_wholesale_downloader.rs` (one), `lib.rs`
- Create: `sdm-core/crates/sdm-resolve/src/cookie_lease.rs`; `sdm-core/crates/sdm-browser/src/lease.rs`
- Modify: `sdm-core/crates/sdm-app/src/toolchain.rs`, `Cargo.toml` (`cargo add sdm-browser`)
- Test: `sdm-resolve/tests/yt_dlp_resolver_tests.rs`, `yt_dlp_wholesale_downloader_tests.rs`, `sdm-browser/tests/lease_tests.rs`, `sdm-app/tests/`

**Interfaces:**
- Produces (`sdm-resolve`): `pub struct CookieLease { pub arguments: Vec<String> /* guard kept private */ }` with `CookieLease::new(arguments: Vec<String>, guard: Box<dyn Send + Sync>)` and `CookieLease::plain(arguments)`; `pub type CookieProvider = Arc<dyn Fn() -> CookieLease + Send + Sync>`; `YtDlpResolver::with_cookie_provider(self, CookieProvider) -> Self` and the same on `YtDlpWholesaleDownloader` (when set it replaces `cookie_source().yt_dlp_arguments()`; the lease lives until the process run returns).
- Produces (`sdm-browser::lease`): `pub fn chrome_cookie_path(data_dir: &Path) -> PathBuf`; `pub enum CookieFileState { Missing, Present { modified: SystemTime } }`; `pub fn cookie_file_state(data_dir: &Path) -> CookieFileState`; `pub fn lease_for_chrome(data_dir: &Path) -> CookieLease` (copies the file to `<data_dir>/cookie-leases/<unique>.txt` with `0600` and returns `--cookies <copy>` with a guard that deletes it on drop; `CookieLease::plain(vec![])` when the file is missing); `pub fn sweep_stale_leases(data_dir: &Path)`.
- Consumes in `toolchain.rs`: provider = if `cookie_source_reader` says `Chrome` then `lease_for_chrome(&data_dir)` else `CookieLease::plain(source.yt_dlp_arguments())`.

- [ ] **Step 1: Write failing tests:** resolver with a provider returns its arguments and not `--cookies-from-browser`; the lease is dropped after the run (use a guard that flips an `AtomicBool`); wholesale likewise; `lease_for_chrome` with a present file returns `--cookies <path that exists>` and deleting the lease removes the copy, a missing file returns no arguments; `sweep_stale_leases` removes leftovers; toolchain-level test: settings `chrome` + missing file ⇒ no cookie arguments; `firefox` ⇒ `--cookies-from-browser firefox`.
- [ ] **Step 2:** Run; fails.
- [ ] **Step 3:** Implement; call `sweep_stale_leases` from `production_dependencies`. The sign-in failure text (resolver, near the "sign in … cookies" check) gains the hint "Chrome cookies come from the SDM extension; open Settings → Media Sites to set it up" only when the provider reported Chrome with a missing file (carry a flag on the lease, `CookieLease::chrome_missing`).
- [ ] **Step 4:** `cargo test -p sdm-resolve -p sdm-browser -p sdm-app` and `cargo test -p sdm-ffi` (unchanged behavior) pass.
- [ ] **Step 5: Commit** `Give yt-dlp the extension's cookie file when the cookie source is Chrome`.

### Task 8: Extension status, folder preparation and manifest registration (Rust side)

**Files:**
- Create: `sdm-core/crates/sdm-browser/src/install.rs`, `tests/install_tests.rs`; `sdm-tauri/src-tauri/src/chrome_setup.rs`
- Modify: `sdm-tauri/src-tauri/src/commands.rs`, `lib.rs` (register commands), `sdm-tauri/src-tauri/tauri.conf.json` (`bundle.resources` adds `"../../SDMChromeExtension": "chrome-extension"`), `sdm-core/crates/sdm-app/src/protocol.rs` (DTO)

**Interfaces:**
- Produces (`install`): `pub fn manifest_json(host_path: &Path) -> String` (name, description, path, `type: "stdio"`, `allowed_origins: ["chrome-extension://fnnpcbmjpapeohfmpiefbocloodhlgjf/"]`); `pub enum Platform { MacOs, Windows, Linux }`; `pub fn manifest_path(platform: Platform, home: &Path, data_dir: &Path) -> PathBuf` (macOS `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/<name>.json`, Linux `~/.config/google-chrome/NativeMessagingHosts/<name>.json`, Windows `<data_dir>/<name>.json`); `pub fn prepare_extension_folder(bundled: &Path, data_dir: &Path) -> io::Result<PathBuf>` (copies to `<data_dir>/ChromeExtension`, replacing a different version, leaving an identical one untouched); `pub fn bundled_version(folder: &Path) -> Option<String>`; `pub enum ExtensionState { NotConnected, Outdated, Connected }`; `pub fn extension_state(bundled: Option<&str>, status: Option<&ExtensionStatus>, now: SystemTime) -> ExtensionState` (not connected when no status or `last_seen` older than 2 h; outdated when versions differ).
- Produces (DTO in `sdm-app::protocol`): `ChromeExtensionDto { state: String /* "connected"|"outdated"|"notConnected" */, installed_version: Option<String>, bundled_version: Option<String>, cookies_synced_seconds_ago: Option<u64>, extension_folder: String, chrome_installed: bool }`.
- Produces (commands): `chrome_extension_status() -> ChromeExtensionDto`, `prepare_chrome_extension() -> String` (prepares folder, writes the manifest, on Windows sets `HKCU\Software\Google\Chrome\NativeMessagingHosts\<name>` via `winreg`, returns the folder), `open_chrome_extensions()` (launch Chrome with `chrome://extensions/`; per-platform executable discovery), `open_chrome_setup()` (shows the setup window, Task 10). Host path = `$APPIMAGE` if set, else `std::env::current_exe()`.
- At startup (`setup`), when the manifest is missing or its `path` differs, rewrite it (no dialog needed).

- [ ] **Step 1: Write failing tests:** `manifest_json` fields and origin; `manifest_path` per platform; `prepare_extension_folder` copies, is idempotent, replaces on a version change; `extension_state` table (none, fresh matching, fresh older version, matching but 3 h old).
- [ ] **Step 2:** Fails.
- [ ] **Step 3:** Implement; Chrome discovery: macOS `open -a "Google Chrome" --args …` style via `NSWorkspace`-free `open -b com.google.Chrome`; Windows registry `App Paths\chrome.exe`; Linux `google-chrome` / `google-chrome-stable` on `PATH`. Verify Chrome's native-messaging paths against current docs (Context7 / Chrome docs) and fix `manifest_path` if they differ.
- [ ] **Step 4:** `cargo test -p sdm-browser -p sdm-tauri` passes; `cargo check --target x86_64-pc-windows-msvc -p sdm-tauri` if the target is installed; regenerate `web/src/bindings.ts` by running the `export_bindings` test.
- [ ] **Step 5: Commit** `Add the Chrome extension install status and manifest registration`.

### Task 9: Extension 2.0.0

**Files:**
- Modify: `SDMChromeExtension/manifest.json` (version `2.0.0`, permissions add `cookies`, `host_permissions: ["<all_urls>"]`), `SDMChromeExtension/background.js`
- Create: `SDMChromeExtension/cookies.js` (pure functions), `SDMChromeExtension/cookie-domains.json`, `SDMChromeExtension/cookies.test.js` (`node --test`), `sdm-core/crates/sdm-browser/tests/bundled_domains_tests.rs`
- Modify: `todo.md`-style note only if needed

**Interfaces (`cookies.js`, ES module):** `export function domainMatches(domain, list)`; `export function cookiesToSend(cookies, list)` (same rule as Rust, also drops cookies with tab/CR/LF); `export const DEBOUNCE_MINUTES = 0.5`.
`background.js` flow: on install/startup/hourly alarm/debounce alarm → `sync()`: `sendNativeMessage({type:"siteList"})` and cache `{domains, version}` in `chrome.storage.local` (fall back to the cache, then to `cookie-domains.json`) → `chrome.cookies.getAll({domain})` per listed domain → de-duplicate by `domain|path|name` → `sendNativeMessage({type:"cookies", siteListVersion, cookies})`. `chrome.cookies.onChanged` sets the debounce alarm only when `domainMatches`; unlisted changes are ignored, not stored, not logged.

- [ ] **Step 1: Write failing tests** `node --test SDMChromeExtension/cookies.test.js`: the same domain table as Task 3; `cookiesToSend` drops look-alikes and newline values; and the Rust test `bundled_domains_match_the_registry` (reads `cookie-domains.json`, compares with `SiteRegistry::cookie_domains()`; with `UPDATE_COOKIE_DOMAINS=1` it rewrites the file).
- [ ] **Step 2:** Both fail.
- [ ] **Step 3:** Implement; generate `cookie-domains.json` with `UPDATE_COOKIE_DOMAINS=1 cargo test -p sdm-browser bundled_domains`. Check `chrome.alarms` minimum delay in current Chrome docs and set `DEBOUNCE_MINUTES` to a valid value.
- [ ] **Step 4:** `node --test` and the Rust test pass. Load the unpacked folder in Chrome (Playwright is not needed; by hand): confirm one `cookies` message arrives and `chrome-cookies.txt` appears with only listed domains.
- [ ] **Step 5: Commit** `Extension 2.0.0: send site-filtered cookies to SDM`.

### Task 10: Setup window and the Settings note

**Files:**
- Create: `sdm-tauri/src-tauri/src/chrome_setup_window.rs` (second window like `settings_window.rs`, label `chrome-setup`), `sdm-tauri/web/src/components/ChromeSetup.svelte`, `sdm-tauri/web/src/lib/chromeSetup.ts` (+ `.test.ts`), copy the step images from `Assets/` used by `Chrome-DeveloperMode` / `Chrome-Load` into `web/public/`
- Modify: `sdm-tauri/web/src/main.ts` (route `?window=chrome-setup`), the YouTube settings tab component under `web/src/components/settings/`, `web/src/lib/mockBridge.ts` (+ test), `web/src/bindings.ts` (regenerated)

**Interfaces:**
- `chromeSetup.ts`: `export function statusLine(dto: ChromeExtensionDto): string` ("Connected, extension 2.0.0, cookies synced 3 min ago" / "Update available: press Reload on chrome://extensions" / "Extension not connected") and `export function steps(platform): Step[]`.
- The setup window polls `chrome_extension_status` once per second while open, shows the four steps (Open Chrome Extensions, enable Developer mode, Load unpacked, then **Show folder** / **Copy path** instead of the drag tile) and the "You're all set" state; calls `prepare_chrome_extension` when it opens.
- Settings Media Sites tab: when the draft `cookieSource` is `chrome`, show the note "Chrome cookies come from the SDM Chrome extension." plus the `statusLine` and a **Set up the extension** button (`open_chrome_setup`); hidden for any other source. Theme roles only.

- [ ] **Step 1: Write failing vitest tests:** `statusLine` per state; the Settings component shows the note only when the draft source is `chrome` and the button invokes `open_chrome_setup` (component test style as the existing settings tests); mockBridge answers the new commands.
- [ ] **Step 2:** `cd sdm-tauri/web && pnpm test` fails.
- [ ] **Step 3:** Implement window, component, route, note; `prettier -w` the changed files; run `pnpm check`.
- [ ] **Step 4:** `pnpm test`, `pnpm check`, `cargo test -p sdm-tauri`, and a Playwright mock run (`pnpm e2e`) pass. By hand on macOS: open the dialog, follow the steps, see "You're all set".
- [ ] **Step 5: Commit** `Add the Chrome extension setup window and the cookie-source note`.

### Task 11: Docs and by-hand checklist

**Files:**
- Modify: `docs/tauri-roadmap.md` (spec 6 row, the "Open" bullets for Windows YouTube and the console window, the "unverified fix" note, a log entry), `CLAUDE.md` (Current state), `docs/superpowers/specs/2026-10-26-tauri-browser-integration-design.md` (status, and the two deviations: host is the app binary, no copied host)

- [ ] **Step 1:** Update the docs with what was built and what is owed by hand: Windows (YouTube with signed-in Chrome, no console window, popover at startup, `main` branching with a GUI-subsystem exe for stdio), Fedora (manifest path, Load unpacked, sync), macOS (no password prompt), 1.0.x → 2.0.0 extension update.
- [ ] **Step 2:** Run the whole suite: `cd sdm-core && cargo test --workspace`, `cargo clippy --workspace`, `cd ../sdm-tauri/web && pnpm test && pnpm check`, `node --test ../../SDMChromeExtension`.
- [ ] **Step 3: Commit** `Close spec 6: roadmap, spec status, CLAUDE.md`.
