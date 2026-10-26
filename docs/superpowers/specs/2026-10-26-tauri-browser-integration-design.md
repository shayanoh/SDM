# Tauri UI, spec 6: Browser integration

Status: **APPROVED** by Shayan (2026-10-26); implemented inline on branch `tauri-ui`. Written with Shayan. Part of
[docs/tauri-roadmap.md](../../tauri-roadmap.md); builds on the [foundation](2026-10-14-tauri-foundation-design.md),
the [settings window](2026-10-24-tauri-settings-design.md) and
[system integration](2026-10-24-tauri-system-integration-design.md).

## 1. Intent

Two things Shayan wants from Chrome, and two bugs left over from spec 5.

1. **Download capture.** The Chrome extension (already built for the SwiftUI app, `SDMChromeExtension/`) hands
   Chrome's downloads to SDM. The Tauri app has no native host, no install flow and no hand-off, so on Windows,
   Linux and the Tauri macOS build the extension does nothing.
2. **Cookies for yt-dlp.** yt-dlp cannot read Chrome's cookies on Windows (Chrome 127+ App-Bound Encryption, the
   database is also locked while Chrome runs; yt-dlp issue 7271). On macOS the `--cookies-from-browser chrome`
   route makes the system ask for the user's password several times. **The extension can read the cookies itself**
   (`chrome.cookies`, decrypted, HttpOnly included), so the extension sends them to SDM, SDM keeps a `cookies.txt`
   current, and yt-dlp gets `--cookies <that file>` whenever the cookie source is Chrome. This replaces
   `--cookies-from-browser chrome` on every platform in the Tauri app.
3. **Two open bugs** (section 7): the tray popover is visible at startup, and every tool run on Windows flashes a
   console window.

## 2. Goals, non-goals, anti-goals

**Goals**

- The extension works with the Tauri app on Windows, Linux and macOS: downloads are captured, the SDM app
  receives them whether or not it is running.
- A setup dialog in the Tauri app with the Load-unpacked steps, equivalent to the SwiftUI
  `ChromeExtensionSetupView`, and a "connected / not connected / update needed" status.
- Chrome cookies reach yt-dlp on all three platforms with no password prompt, no decryption hack, and no Chrome
  restart.
- Only cookies for sites SDM can resolve leave Chrome, including those on related domains and subdomains.
- A Settings note, visible only when the cookie source is Chrome, saying this method needs the extension.
- Fix the two open bugs.

**Non-goals**

- The Chrome Web Store (**decided: load-unpacked only**), Firefox / Edge / Brave extensions, Safari.
- Any change to the SwiftUI app (it keeps `SDMNativeHost`) or to `sdm-ffi`.
- Reading Chrome's cookie database, decrypting it, or driving Chrome's debugging port.
- Cookie handling for the other cookie sources (they keep `--cookies-from-browser`).
- The "small ones" list at the end of the roadmap's spec 5 section.

**Anti-goals** (a failure even if it technically works)

- Cookies for sites outside SDM's list leaving Chrome, or sitting in a file SDM does not need.
- A silent fallback to `--cookies-from-browser chrome` when the extension is not connected: that is the path that
  fails (or prompts), and hiding it only makes the failure harder to find.
- A password prompt, an admin prompt, or a console window appearing at any point.

## 3. Constraints and fixed decisions

- Load-unpacked only. The extension ID stays `fnnpcbmjpapeohfmpiefbocloodhlgjf` (the `key` in `manifest.json`), and the
  native host name stays `com.shayanoh.sdm.chrome_extension`. The same extension serves both apps.
- No network and no real clock in tests; time is tick-driven or injected (project rule).
- The cookie file holds live logins: owner-only access, written atomically, never logged.
- Shared state with the SwiftUI app stays compatible: `~/Library/Application Support/SDM/ChromeExtensionStatus.json`
  (`{version, lastSeen}`) keeps its format so the same extension report works for either app.
- **Several Chrome profiles** with the extension: the last sync wins (Shayan's decision). One file, replaced by each
  snapshot; no merging, since that would mix accounts.
- Only one native-messaging manifest can exist per host name per user. If both apps are installed on a Mac, the
  last one to write its manifest wins. Accepted; the setup dialog rewrites the manifest every time it opens.

## 4. Design

### 4.1 Native host (Rust)

Rust code in a new `sdm-core` crate (`sdm-browser`) reusing `sdm-resolve` (the site list). **Plan-time change (my
call): there is no separate sidecar binary.** Chrome launches the manifest's `path` with `chrome-extension://<id>/` as
its first argument, so the Tauri binary itself is the host: `main` branches on that argument before Tauri starts. This
avoids target-triple sidecar bundling, and `$APPIMAGE` (or `current_exe`) is already a stable path, so the "copy the
host to the data dir" call in 4.3 is dropped. Risk to check by hand on Windows: stdio in a GUI-subsystem exe. It
replaces `SDMNativeHost/main.swift` for the Tauri app. Chrome starts it once per message (`sendNativeMessage`), so it is
short-lived and stateless. It speaks Chrome's native-messaging framing (4-byte little-endian length, JSON). The
incoming limit rises from the Swift host's 64 KB to a few MB (cookie snapshots); the reply stays under Chrome's 1 MB.

Messages from the extension (the existing two keep their shape):

| `type` | Payload | Host action | Reply |
|---|---|---|---|
| `extensionStatus` | `version` | write `ChromeExtensionStatus.json` | `{success}` |
| `download` | `url`, `referrer` | hand the URL to the app (4.2) | `{success}` |
| `siteList` | none | read the cookie domains from the compiled-in `SiteRegistry` | `{success, version, domains}` |
| `cookies` | `siteListVersion`, `cookies[]` | filter again with the Rust rule, write the cookie file | `{success, written, siteListVersion}` |

`siteListVersion` is a hash of the domain list. If it differs from the host's own, the host still writes the file
(filtered by its own list) and replies with the new list so the extension catches up on its next sync.

### 4.2 Hand-off of a download to the app

Constraints, not mechanism: it must start SDM if it is not running, hand the URL to the running instance if it is,
need no admin rights, show no console window, and end up doing exactly what the SwiftUI `com-shayanoh-sdm://download`
URL does. **That behavior (read from `SDM/SDMApp.swift`):** the main window comes forward on the Linkgrabber, and the
URL goes through the grabber's `ingest(urls:)`, i.e. into the Linkgrabber, not straight into the download list; the
referrer is not used. The Tauri app does the same (**my call**), including bringing the main window forward on a cold
start. **My guess at the mechanism**: the host
launches the SDM executable with `--add-url <url>`, and the existing `tauri-plugin-single-instance` callback
(`lib.rs`) receives the arguments in the running instance. If launching the bundle's executable directly is wrong
on macOS, use the platform's "open application with arguments" route. The builder decides after trying it.

### 4.3 Install flow

- **Native-messaging manifest** (name, path to the host binary, `type: stdio`, `allowed_origins` with the extension
  ID), written by the app each time the setup dialog opens and once at startup if missing or pointing at a stale path:
  - macOS: `~/Library/Application Support/Google/Chrome/NativeMessagingHosts/<name>.json`
  - Linux: `~/.config/google-chrome/NativeMessagingHosts/<name>.json`
  - Windows: a JSON file in the app's data folder plus the registry value
    `HKCU\Software\Google\Chrome\NativeMessagingHosts\<name>` pointing at it.
  - (These paths are from memory of Chrome's documented locations; the builder verifies them against current docs.)
- **Chrome variants** (**my call**): stable Google Chrome only. The dialog says other Chromium builds are not
  supported. On Linux that is the single `~/.config/google-chrome` directory.
- **Stable host path** (**my call**): the manifest holds an absolute path, and an AppImage mounts at a random path on
  every launch. On Windows and Linux the app therefore copies the host binary to `SDM/` under the data dir and points
  the manifest there, refreshing the copy when the app version changes. On macOS the host stays inside the signed
  app bundle (the builder checks that Chrome can launch it without a Gatekeeper prompt).
- **Extension folder.** The extension ships as an app resource. The setup dialog first copies it to a stable folder
  under the data dir (`SDM/ChromeExtension/`), because Chrome remembers the unpacked folder's path and a path inside
  an app bundle or install directory changes with updates. **My call.**
- **Setup dialog**, a Tauri window styled like Settings (theme roles, no literals): steps 1-4 as in SwiftUI (open
  `chrome://extensions`, enable Developer mode, Load unpacked, pick the folder). Windows and Linux file pickers do not
  accept a dragged folder reliably, so the dialog offers **Show folder** and **Copy path** instead of the drag tile
  (**my call**). The step images are reused. The dialog watches `ChromeExtensionStatus.json` and shows "You're all
  set" with the version when the bundled version has reported in, as the SwiftUI view does.
- **Opening Chrome's extensions page**: launch Chrome with `chrome://extensions/` (the executable's location per
  platform is the builder's to find). If Chrome is not installed the dialog says so.
- **Where it is reachable from**: a button in Settings (next to the cookie source) and the Linkgrabber/first-run hint
  the SwiftUI app shows, if any (builder checks `SDM/` for where `ChromeExtensionSetupView` is presented).
- **Version status**: connected (latest bundled version reported), outdated (an older version reported: tell the
  user to press Reload on `chrome://extensions`), not connected (no report, or none in the last 2 hours; the extension
  reports hourly).

### 4.4 Extension changes (version 1.1.0)

- Permissions: add `cookies`; add `host_permissions: ["<all_urls>"]` (needed to read cookies of arbitrary
  listed domains). Existing users get Chrome's permission prompt or a disabled extension until they accept; the setup
  dialog's "outdated" state covers telling them.
- **Site list**: on startup and on each sync the extension asks the host for `siteList` and stores the result in
  `chrome.storage.local` (domains + version). A copy of the list is **bundled in the extension** as a failover for the
  very first run and for when the host cannot answer. A build/test step regenerates the bundled copy from the
  registry and a Rust test fails if it is out of date. This means new sites in a later SDM release need **no
  extension update**.
- **Sync**: `chrome.cookies.onChanged` for a cookie whose domain matches the list sets a debounce with
  `chrome.alarms` (the service worker sleeps; a plain timer may die; the alarm minimum in current Chrome is about
  30 s, builder verifies). When it fires, the extension calls `chrome.cookies.getAll` for each listed domain,
  de-duplicates, and sends one `cookies` message with a **full snapshot** (not deltas). A snapshot is also sent at
  browser start and install. Incognito and partitioned (CHIPS) cookies are not sent (builder may revisit if a site
  needs them).
- `getAll` is only ever called per listed domain; `onChanged` events for unlisted domains are ignored and never
  stored or logged.
- Failures (host missing, SDM not installed) are logged and retried on the next change or the hourly alarm; no
  tight retry loops.

### 4.5 Cookie domains: matching that includes subdomains and related domains

The registry's `HostMatch` lists where videos are served, not where logins live. YouTube's sign-in cookies are on
`.google.com`, which is not in the YouTube pattern. So:

- Each `SitePattern` gets a separate `cookie_domains: Vec<String>` of **registrable domains written by hand**
  (YouTube: `youtube.com`, `youtu.be`, `google.com`, and whatever testing shows is needed, e.g. `googlevideo.com`).
  Pure data, fixture-tested, in the same style as the rest of the registry: tune the data, not the control flow.
- **Rule**: a cookie is included if its domain, with any leading dot removed and lowercased, equals a listed domain
  or ends with `.` plus a listed domain. So `.youtube.com`, `music.youtube.com` and a host-only
  `accounts.google.com` (a cookie with no leading dot) all match, and `notyoutube.com` does not.
- **Drift guard test**: every host in `SiteRegistry` (exact hosts and suffix hosts) is covered by that same
  pattern's `cookie_domains`. Adding a site without cookie domains fails the suite.
- **Filtered twice**: by the extension (so unrelated cookies never leave Chrome) and by the host with the same
  Rust function (the tested source of truth) before writing the file.
- **Per-site checks by hand**: the initial `cookie_domains` for the major sites (YouTube first, then Vimeo, Twitch,
  Reddit, TikTok and a couple of others) are verified by running a login-requiring download. The list is expected to
  grow after real use; that is why it is data.

### 4.6 Cookie file and yt-dlp

- Path: `<data_dir>/chrome-cookies.txt`, Netscape format as yt-dlp expects it (HttpOnly cookies with the
  `#HttpOnly_` prefix, subdomain flag from the leading dot, secure flag, expiry 0 for session cookies, tab
  separated; the builder checks yt-dlp's loader for edge cases). Written to a temp file in the same folder and
  renamed, owner-only on macOS and Linux (`0600`); on Windows the user profile's default access.
- `CookieSource::Chrome` in the Tauri app no longer produces `--cookies-from-browser chrome`: `yt_dlp_arguments`
  yields `--cookies <path>`. yt-dlp **rewrites the cookie file when it exits**, so racing with the host's writes or
  with another yt-dlp run is a real risk: SDM gives each yt-dlp invocation **its own temporary copy** and deletes it
  afterwards (**my call**; the builder confirms yt-dlp's behavior). The SwiftUI path through `sdm-ffi` keeps the old
  arguments.
- **Not connected**: if the file does not exist (the extension has never synced), resolves still run, without
  cookies, so public videos keep working (**my call**). It is never silent: the notice "Chrome cookies are
  unavailable: the SDM extension is not connected" is shown with the setup dialog as its action, and when yt-dlp
  fails with a sign-in / age-restriction error the failure text adds the same hint. No fallback to the browser's own
  database.
- **Temporary copies** (**my call**): whatever yt-dlp writes back into its copy is discarded (the next host write is
  the source of truth); copies are deleted after the run, and any left over from a crash are swept at app startup. They
  live in the app's per-user data folder, so they carry the same access as the rest of the user's data on Windows.
- **No forced refresh** (**my call**): Chrome's native messaging gives SDM no way to ask the extension for a sync
  without a long-lived port. Staleness is accepted; the hourly alarm and any cookie change refresh the file.
- **Stale**: the file is as of the last sync, which only runs while Chrome is open. Settings shows "Last synced <time
  ago>" (from the file's modified time) or "Extension not connected".

### 4.7 Settings note

In the YouTube tab's cookie-source row: when Chrome is selected, a note appears (hidden for other sources): "Chrome
cookies come from the SDM Chrome extension. Install it from [Set up the extension]." plus the sync status line from
4.6. Wording is a first suggestion. Theme roles only; this keeps the buffered OK/Cancel behavior of the Settings
window.

## 5. What is left to the builder

Mechanisms and file-level structure of the Rust crates and Svelte components; the hand-off mechanism (4.2); exact
registry paths and Chrome executable discovery per platform (verify against current docs); the alarm debounce
value; the Netscape writer details; where the setup dialog's entry points go; how the extension's bundled list and
its test are generated; Windows manifest registration code; test structure.

## 6. Testing

- Rust, fixture tables: cookie-domain matching (subdomain, leading dot, host-only, look-alike domain), the drift guard,
  the Netscape writer (HttpOnly, secure, session, domain flags), native-messaging framing and each message type with a
  fake stdin/stdout, `yt_dlp_arguments` for Chrome, the temporary-copy handling.
- The extension's filtering and debounce logic gets unit tests (the extension has no test setup today; adding one is
  in scope).
- Web: the setup dialog states and the Settings note (visible only for Chrome) as component tests.
- **By hand, owed**: Windows (YouTube with a signed-in Chrome, no console window), Fedora (manifest location,
  Load unpacked, sync), macOS (no password prompt; both apps installed), and the real extension update from 1.0.x.

## 7. The two bugs (first batch, before the feature)

They are independent of the feature, so they go first but are not a hard gate: if the popover investigation runs
long, the feature work can proceed in parallel (**my call**).

1. **Popover visible at startup.** The spec 5 fix builds the window at startup (`popover::init`, `.visible(false)`),
   and Shayan reports the popover is now visible when the app starts and stays until clicked and then clicked
   elsewhere. Reproduce on macOS (and Windows), find the actual cause (the root cause is not known yet; do not guess
   at a patch), fix it, and confirm that the first click still shows the popover. Acceptance: nothing visible at
   startup; the first click shows it; clicking elsewhere hides it. This also settles the "unverified fix" note in
   the roadmap.
2. **Console window on Windows.** Every tool run (yt-dlp, ffmpeg, ffprobe, qjs, the new host) must start with the
   `CREATE_NO_WINDOW` creation flag: `SystemProcessRunner` and every other place that starts a process, in
   `sdm-core` and `sdm-tauri`. Acceptance: no console window during a YouTube download or a mux on Windows.
