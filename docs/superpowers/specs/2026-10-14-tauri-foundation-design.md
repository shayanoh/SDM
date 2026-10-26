# Tauri foundation — design (spec 1 of 6)

Status: APPROVED (2026-10-14).
Roadmap: [docs/tauri-roadmap.md](../../tauri-roadmap.md) — read it first for where this spec sits.

## 1. Purpose

SDM's core is Rust (`sdm-core/`), and the SwiftUI app runs on it through
UniFFI. This spec lays the foundation for a second UI, built with Tauri v2,
that runs on Windows (first), Linux and macOS.

It delivers an architecture plus a **walking skeleton**: a Tauri window that
adds a URL, downloads it through the real Rust core, and shows live progress.
The skeleton must run on the Mac, in a Windows VM (cross-compiled from the
Mac), and compile for Linux. The later specs (main window, link grabber,
settings, system integration, browser extension) build on what this one
proves.

### Decisions already made (from brainstorming, 2026-10-14)

| #   | Decision                                                                                                                                                                        |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T1  | Tauri v2, Rust in-process. No sidecar, no UniFFI on this path.                                                                                                                  |
| T2  | UI fidelity: **same layout and features, Mac-styled** on every OS. Liquid Glass is approximated with translucent blur (Mica/Acrylic on Windows). Window chrome stays OS-native. |
| T3  | The SwiftUI app **stays the macOS app**. The Tauri app on macOS is a dev/test host and an optional alternative. Both UIs sit on the same core.                                  |
| T4  | The Touch Bar is **dropped** for Tauri. Not spiked, not ported.                                                                                                                 |
| T5  | The browser-extension native host is a **later spec** (spec 6). Clipboard link grabbing is not deferred.                                                                        |
| T6  | App-level controller logic lives in a new Rust crate, `sdm-app`, not in the frontend.                                                                                           |
| T7  | Windows and Linux fetch `ffmpeg`, `ffprobe` and `qjs` on first use (download + SHA-256 verify), like `yt-dlp`. macOS keeps its bundled LZFSE blobs.                             |

## 2. Non-goals

The skeleton does not build: the real main window (details panel, graphs,
drag and drop, toolbar), the link grabber and format picker, the Settings
tabs, themes beyond a CSS-variable skeleton, the tray, notifications, the
clipboard watcher, activation modes, launch at login, or the browser
extension. It does not change the SwiftUI app or `sdm-ffi`.

## 3. Architecture

```
sdm-engine / sdm-resolve / sdm-grabber   (existing, in-process)
              │
           sdm-app          new crate: controller layer, no Tauri dependency
              │   Patch stream (Channel)         ▲ commands
              ▼                                  │
        sdm-tauri shell  ── implements Host ──────┘
              │
         Frontend (TypeScript, Svelte 5): keyed store + views
```

- `sdm-app` depends directly on the engine, resolve and grabber crates, not on
  `sdm-ffi`. It is the Rust counterpart of `EngineController`,
  `GrabberController` and the settings stores.
- The Tauri shell is thin: commands forward to `sdm-app`, a timer calls
  `tick()`, and a `Host` implementation provides OS services.
- The frontend renders patches. It holds no business logic: no diffing of
  snapshots, no notification rules, no scheduling.

### 3.1 Repository layout

```
sdm-core/crates/sdm-app/        new, library crate
sdm-tauri/
  src-tauri/                    Tauri shell crate; member of the sdm-core workspace
  web/                          Vite + Svelte 5 + TypeScript, pnpm
```

`sdm-tauri/src-tauri` joins the `sdm-core` workspace by path, so it shares
`Cargo.lock`, lints and the toolchain pin. The workspace lint
`await_holding_lock = "deny"` applies to it.

## 4. `sdm-app`

### 4.1 Responsibilities

- Owns the `DownloadEngine`, the grabber session (used from spec 3), the
  settings store, and the heartbeat state.
- `tick()` advances the engine one tick and returns an optional `Patch`.
  **There is no clock in `sdm-app`.** The shell calls `tick()` at
  `AppTiming::TICKS_PER_SECOND`; tests call it directly. This is the same
  rule as the engine.
- Translates user actions into engine calls and returns a `Patch` when the
  action changed state, so the UI updates without waiting for the next tick.
- Tracks the UI's reported visibility and open details panels (see 4.4).
- Decides when a notification is due (see 4.6) and hands it to the `Host`.

### 4.2 `Host` trait

OS services come in through a trait so `sdm-app` stays testable and Tauri-free.
Spec 1 needs only:

```rust
pub trait Host: Send + Sync {
    fn data_dir(&self) -> PathBuf;              // state.json, sidecars, bin/
    fn default_download_dir(&self) -> PathBuf;
    fn reveal_in_file_manager(&self, path: &Path) -> Result<(), HostError>;
    fn notify(&self, notification: Notification) -> Result<(), HostError>;
}
```

Clipboard change detection, launch at login and open-with are added in spec 5.
Tests use a `FakeHost` that records calls.

`data_dir()` returns the same location the SwiftUI app uses on macOS
(`~/Library/Application Support/SDM`), so a user's `state.json` and sidecars
work from either app (decision D5 of the Rust migration). On Windows it is
`%APPDATA%\SDM` and on Linux `~/.local/share/SDM`.

### 4.3 The patch protocol

The engine already exposes what the diff needs: `snapshot_if_changed` with a
`generation` counter, a per-package `revision`, and cheap equality when nothing
changed. `sdm-app` keeps the last snapshot it sent and builds a patch against it.

```rust
pub struct Patch {
    pub seq: u32,                          // +1 per emitted patch, wrapping at 2^32
    pub full: bool,                        // drop all prior state and rebuild from this patch
    pub structure: Option<Structure>,      // present only when list shape changed
    pub items: Vec<ItemFigures>,           // only items whose figures changed
    pub packages: Vec<PackageFigures>,     // only packages whose aggregate changed
    pub stats: Option<EngineStats>,        // present only when it changed
    pub histories: Vec<ItemHistory>,       // full speed history, detail-open items only
}

pub struct Structure {
    pub packages: Vec<StructurePackage>,   // ordered; each lists its item ids in order
}
```

- **Structure vs figures.** `Structure` is the ordered ids plus the fields that
  change the shape of the list or a row's content (name, filename, state,
  `is_enabled`, priority). `ItemFigures` are the per-tick numbers: completed
  ranges, total bytes, speed, segment counts, ETA inputs, retry hold, a short
  speed history. This mirrors the `structuralPackages` / live-model split in
  the SwiftUI app.
- **Skipping unchanged packages.** A package whose `revision` equals the last
  sent one is skipped entirely, with no per-item comparison. This is the O(1)
  check the SwiftUI app relies on.
- **Idle emits nothing.** If `generation` has not advanced and there are no
  detail histories to refresh, `tick()` returns `None`.
- **Completed ranges.** `completed` is sent as the `RangeSet`'s interval list.
  The frontend's canvas draws segments from it. If a profile shows this to be
  too large for 500 active items, sending a coarsened range list is the first
  optimization (the success-criteria gate in section 10 measures it).
- **Resync.** `full_state()` sends a `Patch` with `full` set, `seq` set to the
  next counter value, and `structure` plus every item and package populated,
  through the attached sink. The frontend calls it whenever it sees a `seq`
  gap (a non-consecutive value, including wraparound) or a non-full patch
  before any full one; the first patch after `attach` is always full.
- **Ordering.** Patches are produced under one lock scope in `sdm-app` and
  assigned `seq` there, so user-action patches and tick patches cannot
  interleave out of order. The engine call itself (an `await`) happens outside
  any lock: take the lock, build the diff and assign `seq` only after the
  snapshot is in hand.

### 4.4 Visibility and detail histories

Direct ports of `setListVisible` and `setDetailItems`:

- `set_list_visible(bool)`: while hidden, `tick()` still advances the engine and
  evaluates notifications but builds no patches. On becoming visible, the next
  patch is a full one.
- `set_detail_items(owner, ids)`: the frontend reports which items have a
  details panel open. For those, `tick()` fetches the engine's full speed
  history (`speed_histories`) and includes it as `ItemHistory`.

Spec 1's skeleton has no details panel, but the commands and the patch field
exist so spec 2 does not change the protocol.

### 4.5 Commands

The commands the skeleton uses, each returning `Result<(), AppError>`; every
patch goes through the attached sink, so one ordered transport carries both
command results and ticks (no ordering race between a response and the
channel):

`add_url(url, package_name?)`, `start`, `stop`, `set_enabled`, `retry`,
`remove(ids)`, `set_priority`, `get_settings`, `update_settings`,
`full_state`, `set_list_visible`, `set_detail_items`, and
`binaries_status` (the managed-binaries provisioning state and progress).

`add_url` validates the URL (scheme, host) before calling the engine. All
commands validate their input and map every failure to a typed `AppError`
derived from `CoreError` with a stable `kind` and a user-facing `message`.

### 4.6 Notifications

A port of `notifyChanges`: compare the previous and new snapshot, look only at
packages whose `revision` changed, collect both sides so a moved item never
reads as "finished", and emit `Notification::Finished` or `Notification::Failed`
on a transition to `Completed` or `Failed`. The result goes to `Host::notify`.
Spec 1's shell implements `notify` as a log line; spec 5 makes it a real OS
notification.

### 4.7 Settings

`sdm-app` persists app settings as a JSON file in `data_dir()`. The skeleton has
one setting (download folder). The store validates values (path must exist or be
creatable and writable) and applies them to the engine through
`update_settings`. The settings file format is defined here and extended by
spec 4. Existing SwiftUI preferences are not read: the SwiftUI app stores its
settings in `UserDefaults`, which sits outside the Rust core's reach, so each
app has its own settings and shares only the download state (D5).

### 4.8 Tick failures

A failed tick is logged with the same throttling as `logTickFailure`: log the
first occurrence, suppress repeats of the same message, and log again when the
message changes or after a success. The heartbeat keeps running.

## 5. Tauri shell (`sdm-tauri/src-tauri`)

- **Commands** are thin `#[tauri::command] #[specta::specta]` wrappers over
  `sdm-app`. Async commands return `Result<_, AppError>`.
- **Patches stream** to the frontend through a `tauri::ipc::Channel<Patch>`
  that the frontend registers once in an `attach` command. Commands do not
  return patches: every patch, including the result of a user action, arrives
  on that channel and is applied by the frontend in `seq` order.
- **Heartbeat:** a task spawned in `setup` with `tauri::async_runtime::spawn`
  calls `tick()` once per `1 / TICKS_PER_SECOND` seconds and sends any patch on
  the channel. If no channel is attached yet, the patch is dropped (the frontend
  resyncs on attach).
- **State:** `sdm-app::App` is managed by Tauri state. Internally it holds one
  mutex, with one lock scope per stretch that has no `await`.
- **Panic boundary:** command bodies and the heartbeat run inside
  `catch_unwind`-equivalent handling (tokio task `JoinError`) so a panic becomes
  an `AppError` of kind `internal` and the window keeps responding.
- **`Host` implementation:** paths through Tauri's path resolver, with the
  macOS data directory forced to `~/Library/Application Support/SDM` for D5
  compatibility. `reveal_in_file_manager` uses the platform's opener.
- **Type generation:** `tauri-specta` v2 with `specta` and `specta-typescript`
  generates `sdm-tauri/web/src/bindings.ts` in debug builds. The context7 docs
  currently pin `tauri-specta` and `specta` to `=2.0.0-rc.25`, a release
  candidate. Pin the exact versions in `Cargo.toml` (added with
  `cargo add`, never by hand-editing) and commit the generated `bindings.ts`
  so a build never depends on the generator running. If tauri-specta proves
  unworkable, the fallback is `specta` plus `specta-typescript` exporting the
  types alone, with hand-written `invoke` wrappers; the protocol does not
  change.
- **Capabilities:** the Tauri v2 permission set is minimal (core commands only)
  in this spec. Spec 5 widens it for tray, notification and clipboard plugins.

## 6. Frontend (`sdm-tauri/web`)

- **Stack:** TypeScript, Vite, pnpm, Svelte 5. Installed with `pnpm add`, never
  by editing `package.json`. Format with `prettier -w`.
- **Bridge:** all backend access goes through one interface.

  ```ts
  interface Bridge {
    attach(onPatch: (p: Patch) => void): Promise<void>;
    fullState(): Promise<void>;
    addUrl(url: string): Promise<void>;
    // ...one method per command in 4.5
  }
  ```

  Two implementations: `TauriBridge` (generated bindings) and `MockBridge`
  (scripted patches in a plain browser). The mock exists so the UI can be
  developed and visually checked with Playwright, because Tauri's WebDriver does
  not support WKWebView on macOS.

- **Store:** a `class` with `$state` fields and a `SvelteMap<Uuid, ItemModel>`,
  where each `ItemModel` is a class with `$state` fields. Applying a patch
  assigns only the fields that arrived, so a row reads only its own model and
  re-renders only when its own numbers change. This is the web counterpart of
  the per-item `@Observable` live models. A `seq` gap triggers `fullState()`.
- **List:** the visible rows are flattened (package header rows and item rows)
  into one array, each with a fixed height, and an in-house windowing
  component renders only what is in view. No virtual-list dependency; the
  logic is a small function over scroll offset and row height.
- **Canvas:** the segmented progress bar and sparkline are `<canvas>`
  components drawing from an item's `completed` ranges and speed history. They
  redraw only when their inputs change.
- **Theme:** every color is a CSS variable (`--role-name`). A small generator
  turns the theme roles from `sdm-core` into a `:root` block. No color literals
  in components.
- **Window styling:** translucent background with `backdrop-filter` where the
  webview supports it; Mica/Acrylic on Windows through Tauri's window effects
  (details settled in spec 2).

## 7. Core portability (Windows and Linux)

The core uses Unix-only APIs directly and does not compile for Windows today.
The changes, grouped by crate:

| Where                                         | Today                                                                             | Change                                                                                                                                                                                                  |
| --------------------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `sdm-resolve/system_process_runner.rs`        | `libc::kill(SIGTERM)`, `pid_t` for graceful cancel                                | Unix keeps SIGTERM with the bounded wait. Windows terminates the child after the same bounded wait. Wholesale resume relies on yt-dlp's `--continue`, so a hard stop stays resumable; verify on the VM. |
| `sdm-engine/sparse_file.rs`                   | `as_raw_fd`, `libc::fsync`, errno constants                                       | `File::sync_all`. The macOS `F_FULLFSYNC` path stays behind `cfg(target_os = "macos")`. Replace the errno constants with `io::ErrorKind` matching or gate them to Unix.                                 |
| `sdm-resolve/binary_locator.rs`               | `libc::access(X_OK)`, `/usr/bin`, `/usr/local/bin`                                | Exec-bit check on Unix, `.exe` suffix check on Windows. Search `PATH` plus the managed `bin/`.                                                                                                          |
| `sdm-resolve/managed_binaries.rs`             | `PermissionsExt`, `/usr/bin/xattr`, `/usr/bin/codesign`, Application Support path | Quarantine and codesign handling behind `cfg(target_os = "macos")`. `chmod` behind `cfg(unix)`. Data directory comes from `Host`.                                                                       |
| `sdm-resolve` managed binaries                | Bundled LZFSE blobs hold macOS binaries                                           | On Windows and Linux, download `ffmpeg`, `ffprobe` and `qjs` from pinned URLs with a SHA-256 manifest, through the same mechanism `yt-dlp` uses (T7). `lzfse_rust` is pure Rust and stays for macOS.    |
| `Cargo.toml` of `sdm-engine`, `sdm-resolve`   | `libc` unconditional                                                              | Move `libc` to `[target.'cfg(unix)'.dependencies]`.                                                                                                                                                     |
| `sdm-engine/sparse_file.rs` (also)            | `std::os::unix::fs::FileExt` (`write_at`) and `OpenOptionsExt::mode`              | A small `platform_io` module with `write_all_at` (Unix `write_at`, Windows `seek_write`); `mode(0o644)` stays behind `cfg(unix)`.                                                                       |
| `sdm-engine/resume_sidecar.rs`                | `OpenOptionsExt::mode(0o666)` in `write_atomically`                               | `mode` behind `cfg(unix)`.                                                                                                                                                                              |
| `sdm-resolve/system_process_runner.rs` (also) | `OsStrExt`/`OsStringExt` byte handling to build `PATH`, `ExitStatusExt::signal`   | Build `PATH` with `std::env::split_paths` / `join_paths`; report a signal exit only on Unix. The Unix tool directories (`/opt/homebrew/bin`, …) are skipped on Windows.                                 |
| `sdm-resolve/managed_binaries*.rs`            | The yt-dlp asset name `yt-dlp_macos` is hard-coded in the URL and checksum lookup | Per-platform asset name: `yt-dlp_macos`, `yt-dlp.exe`, `yt-dlp_linux`, `yt-dlp_linux_aarch64`.                                                                                                          |

The `ffmpeg`/`ffprobe`/`qjs` download (T7) needs archive extraction (the
common Windows and Linux builds ship as `.zip` and `.tar.xz`), so it is the last
task of the plan and is not on the skeleton's critical path: the skeleton
downloads plain HTTP files without any managed binary.

Already portable: `trash`, `reqwest` with rustls and `system-proxy`, and the
existing `cfg(target_os = "macos")` blocks in `resume_sidecar.rs` and
`organize.rs`.

**Windows filename rules.** Audit `safe_join` and the filename sanitizer
against: reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`..`COM9`,
`LPT1`..`LPT9`, with or without an extension), trailing dots and spaces,
the characters `<>:"|?*`, and paths longer than 260 characters (use the
extended-length `\\?\` form where needed). Add fixture-table tests, tuned as
data rather than control flow, per the project convention. These tests run on
every OS because the sanitizer is pure.

**Linux.** Compiles in a container (section 8). No portability changes beyond
the table above are expected.

## 8. Build, cross-compile and test tooling

- **macOS:** `pnpm tauri dev` and `pnpm tauri build` run natively.
- **Windows from the Mac:** `cargo-xwin` plus NSIS (`brew install nsis`).
  The command is
  `pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc`,
  producing an NSIS installer under
  `target/x86_64-pc-windows-msvc/release/bundle/nsis/`. The development Mac is
  Intel, so the Windows VM runs x64 natively and no ARM build is needed. MSI is
  not produced (it requires Windows).
- **Linux from the Mac:** a build inside a Docker container (OrbStack or Docker
  Desktop) based on Ubuntu 22.04, so the glibc requirement stays low and
  `libwebkit2gtk-4.1-dev` is available. Spec 1 requires a compile check
  (`cargo check`); a full bundle build is optional.
- **`test.sh`** gains two steps: `cargo xwin check` for the Windows target on
  the portable crates and the shell, and the Linux container `cargo check`. The
  Linux step is skipped with a clear message if Docker is not running.
- **Windows signing** is out of scope here. Unsigned installers are acceptable
  for the VM; signing is decided before the first distributed release.
- **Scripts:** `scripts/build-tauri.sh` wraps the three build targets so the
  commands live in one place.

## 9. Error handling

- Commands validate inputs and return a typed `AppError` (`kind`, `message`).
  The frontend shows it as a dismissible banner.
- A failed tick is logged with throttling (4.8); the heartbeat continues.
- A panic is contained at the shell boundary and reported as `internal`.
- A `seq` gap makes the frontend call `full_state()` rather than render
  stale state. After an error that might have lost a patch (a failed
  `Channel::send`), `sdm-app` marks the next patch as full.
- Provisioning failures (a binary download or hash mismatch) surface through
  `binaries_status` and do not block adding non-media downloads.

## 10. Testing and success criteria

### Tests

- **`sdm-app` (Rust):** tick-driven tests against the fake origin and a
  `FakeHost`. Cases: a patch contains only changed items; an unchanged package
  is skipped; an idle engine returns `None`; a structural change emits
  `structure`; `seq` is strictly increasing across user-action and tick
  patches; `full_state` equals applying every prior patch; hidden-list
  suppression and the full patch on re-show; detail histories appear only for
  reported items; notifications fire once on a transition and a moved item
  does not read as finished; input validation rejects bad URLs and settings.
  No test touches the network or sleeps on a real clock.
- **Portability:** the Windows filename fixture table; the process runner's
  Windows termination path is covered by a `cfg(windows)` test that runs on the
  VM.
- **Frontend:** Vitest for the store (patch application, `seq` gap resync,
  field-level updates) and the windowing function. Playwright against the
  `MockBridge` for visual checks.
- **The SwiftUI app's tests and the Rust workspace's existing tests keep
  passing.**

### Success criteria

1. The skeleton downloads a real file end to end on the Mac and in the Windows
   VM, from an installer cross-compiled on the Mac.
2. The Linux container build compiles.
3. **Performance gate.** A load of 500 items with 50 actively downloading runs
   in the skeleton on WKWebView and on WebView2. The load is real engine work
   against the in-process fake origin (the engine's existing `FakeOrigin`),
   started by a debug-only `--simulate <items> <active>` launch flag on the
   Tauri shell, so no scripted snapshots stand in for the engine. Record CPU
   and peak memory of the whole process tree. For comparison, run the same
   fake-origin load through `sdm-bench`'s engine in the SwiftUI app if it can be
   driven that way; if it cannot, the comparison is dropped and only the
   absolute thresholds apply. _Proposed thresholds, to adjust on review:_
   idle (all 500 paused) uses under 1% of one core; the 50-active load uses
   under 15% of one core and under 400 MB; where a SwiftUI comparison exists,
   no more than twice its CPU. A miss changes the plan before spec 2 starts:
   first the coarsened-ranges optimization (4.3), then a different frontend
   approach.
4. The frontend never polls, and an idle engine sends no patches (proved by the
   `sdm-app` test).
5. The SwiftUI app builds and its tests pass unchanged.
6. `docs/tauri-roadmap.md` records the measurements and marks spec 1 done.

## 11. Risks

| Risk                                                 | Mitigation                                                                                                                                                                     |
| ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| WebKitGTK performance and GPU quirks on Linux        | Not exercised until a Linux machine is available. The performance gate covers WKWebView and WebView2 only. Linux behavior is recorded as unverified in the roadmap.            |
| `tauri-specta` is a release candidate                | Version pinned, bindings committed, fallback to `specta` types plus hand-written wrappers (section 5).                                                                         |
| `cargo-xwin` cross-compile of C dependencies         | Run it first, before any UI work: the first plan task is a Windows `cargo check` of the portable crates so a problem shows up early.                                           |
| Windows hard-stop of yt-dlp loses wholesale progress | Test on the VM; `--continue` should resume from fragments. If not, Windows wholesale downloads are marked non-resumable (the engine already supports `is_resumable == false`). |
| Patch volume with many active items                  | Sections 4.3 and 10; coarsened ranges are the planned first optimization.                                                                                                      |
