# Tauri foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build spec 1 of the Tauri UI: the `sdm-app` controller crate, a Tauri v2 shell, a Svelte 5 frontend that shows a live download list, and the core changes that let all of it build for Windows and Linux from the Mac.

**Architecture:** `sdm-app` wraps the in-process `sdm-engine` and turns engine snapshots into small ordered patches that it pushes to the UI through one sink (a Tauri `Channel`). The Tauri shell is a thin command layer plus a 5 Hz heartbeat. The Svelte frontend holds a keyed store that applies patches and renders virtualized rows with canvas progress bars. Before any of that, the core crates are made portable (Unix-only I/O, process and path code moved behind small cfg-gated helpers) so `cargo xwin` and a Linux container can compile them.

**Tech Stack:** Rust 2024 (workspace in `sdm-core/`, toolchain `stable`), tokio, `specta` + `tauri-specta` 2.0.0-rc, Tauri v2, TypeScript, Vite, pnpm, Svelte 5, Vitest, Playwright (against a mock bridge), `cargo-xwin`, NSIS, Docker.

**Spec:** `docs/superpowers/specs/2026-10-14-tauri-foundation-design.md` (read it first). Roadmap: `docs/tauri-roadmap.md`.

## Global Constraints

Copied from the spec and the repo's conventions. Every task obeys these.

- `sdm-app` has **no clock and no Tauri dependency**. Time advances only through `App::tick()`. No test sleeps on a real clock or touches the network.
- Never hold a `std::sync::Mutex` guard across an `.await` (workspace lint `await_holding_lock = "deny"`; clippy enforces it).
- Format Rust with `cargo fmt`; format TypeScript/Svelte/JSON/Markdown with `prettier -w <file>` (user rule: always format every TypeScript file with prettier).
- Add packages only with the package manager (`cargo add`, `pnpm add`, `pnpm add -D`); never hand-edit `Cargo.toml` dependency lists, `package.json` dependencies, or lockfiles. (Editing `[workspace] members` and `[target.'cfg(unix)'.dependencies]` table _headers_ by hand is allowed because `cargo add` cannot create a workspace member; say so in the commit message.)
- Validate input in every function and handle the failure; no `unwrap()`/`expect()` on user-influenced values.
- The SwiftUI app, `sdm-ffi`, and the legacy tests must keep building and passing: run `cd sdm-core && cargo test --workspace --exclude sdm-tauri` after any change to a core crate.
- The data directory on macOS stays `~/Library/Application Support/SDM` (decision D5: `state.json` and sidecars are shared with the SwiftUI app). Windows: `%APPDATA%\SDM`. Linux: `~/.local/share/SDM`.
- Wire types use `String` ids (UUID lowercase hyphenated), `f64` for byte counts, `u32` for counts and `seq`. `specta` refuses `i64`/`u64` in TypeScript output by default, and a JS number holds integers exactly up to 2^53.
- Verify every library API against current docs (Context7) at the task that uses it; versions named here were current on 2026-10-14.
- Commit after each task. The project's `CLAUDE.md` grants blanket consent to commit and to fast-forward push; never force-push. Commit messages end with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`.
- Work on branch `tauri-ui`.

## Review Focus

The five inputs and conditions this spec implies but no task would exercise by default, most likely first. Each has a test in the task named in brackets.

1. **A URL with no usable filename** (`https://example.com/`, `https://example.com/%00`, a query-only path, a name that is `..` or a Windows reserved name like `CON.txt`) must produce a safe filename, never an empty one or a panic in the engine. [Task 9, Task 4]
2. **A patch dropped or reordered** (the window reloads, the channel closes, the sink fails once) must leave the UI recoverable: the next patch is a full one and sequence numbers never silently skip. [Task 10, Task 13]
3. **An item removed while its details are open, or ids sent by the UI that don't exist or aren't UUIDs**, must not panic or leak history entries. [Task 10]
4. **Sequence number wraparound** and a store that receives a non-full patch before any full one. [Task 13]
5. **A download folder that doesn't exist, isn't writable, or is empty** in settings must be rejected with a clear error and leave the old setting in force. [Task 9]

---

## File Structure

```
sdm-core/rust-toolchain.toml                      modify: add the Windows target
sdm-core/Cargo.toml                               modify: workspace member for the shell
sdm-core/crates/sdm-core/src/filename_rules.rs    new: pure Windows filename rules (all OSes)
sdm-core/crates/sdm-core/src/safe_path_component.rs   modify: apply the rules on Windows
sdm-core/crates/sdm-engine/src/platform_io.rs     new: positional write, fsync, open mode
sdm-core/crates/sdm-engine/src/sparse_file.rs     modify: use platform_io
sdm-core/crates/sdm-engine/src/resume_sidecar.rs  modify: use platform_io
sdm-core/crates/sdm-resolve/src/binary_locator.rs modify: portable executable check, per-OS dirs, .exe
sdm-core/crates/sdm-resolve/src/system_process_runner.rs  modify: portable PATH, exit status, terminate
sdm-core/crates/sdm-resolve/src/managed_binaries*.rs      modify: per-OS yt-dlp asset, mac-only codesign
sdm-core/crates/sdm-app/                          new crate
  Cargo.toml
  src/lib.rs        re-exports
  src/error.rs      AppError
  src/protocol.rs   wire DTOs (Patch, Structure, ItemFigures, ...)
  src/convert.rs    engine snapshot -> DTO
  src/differ.rs     last-sent state, produces deltas
  src/notify.rs     Notification, notifications_between
  src/host.rs       Host, PatchSink, FakeHost, RecordingSink
  src/url_input.rs  parse_download_url, derive_filename
  src/settings.rs   SettingsDto, SettingsStore
  src/theme.rs      theme_css
  src/toolchain.rs  production wiring (resolver, muxer, locator, ManagedBinaries)
  src/app.rs        App
  tests/            integration tests with FakeOrigin
sdm-tauri/
  package.json, pnpm-workspace.yaml      Tauri CLI at the project root
  src-tauri/        shell crate (Cargo.toml, tauri.conf.json, src/main.rs, src/lib.rs, src/host.rs, capabilities/)
  web/              Vite + Svelte 5 + TypeScript
    src/bindings.ts            generated, committed
    src/lib/bridge.ts          Bridge interface
    src/lib/tauriBridge.ts     real implementation
    src/lib/mockBridge.ts      browser implementation
    src/lib/store.svelte.ts    keyed store
    src/lib/windowing.ts       visible-range function
    src/lib/format.ts          byte/speed formatting
    src/components/*.svelte    ProgressCanvas, Sparkline, Row, List, AddUrl, Banner
    src/App.svelte, src/main.ts, src/theme.css
  docker/linux-build.Dockerfile
scripts/check-windows.sh, scripts/check-linux.sh, scripts/build-tauri.sh
test.sh                                           modify
docs/tauri-roadmap.md                             modify: status and measurements
```

## Deviations from the spec

Where writing this plan found a better or necessary change. Task 15 edits the spec to match; none of them changes a decision in the roadmap.

1. **All patches go through the sink.** Commands return `()`, not `Option<Patch>`. One ordered transport removes the ordering race between a command's response and the channel. (Spec 4.3, 4.5, 5.)
2. **`Patch.full: bool`** marks "drop everything and rebuild from this patch". (Spec 4.3.)
3. **`seq` is `u32`** and wraps; the frontend treats any non-consecutive value as a gap. (Spec 4.3.)
4. **A theme CSS generator is in scope** (`sdm-app::theme_css`), because `ThemeCatalog` already exists in Rust. (Spec 6 mentions it.)
5. **The `ffmpeg`/`ffprobe`/`qjs` download is Task 16**, last and off the skeleton's critical path (spec 7's added note).

---

### Task 1: Cross-target tooling and baseline

**Files:**

- Modify: `sdm-core/rust-toolchain.toml`
- Create: `scripts/check-windows.sh`, `scripts/check-linux.sh`, `sdm-tauri/docker/linux-build.Dockerfile`

**Interfaces:**

- Produces: `scripts/check-windows.sh` and `scripts/check-linux.sh` (exit 0 when the portable crates compile for that target; used by Task 5 and `test.sh`).

- [ ] **Step 1: Install the tools**

```bash
rustup target add x86_64-pc-windows-msvc
cargo install --locked cargo-xwin
brew install nsis llvm
```

`cargo-xwin` needs a clang; Homebrew's `llvm` provides it. If the Homebrew `llvm` is keg-only, `cargo xwin` finds `clang-cl` through `$(brew --prefix llvm)/bin` — add that to `PATH` in the scripts below. Confirm against the `cargo-xwin` README (Context7: resolve `cargo-xwin`) and the Tauri Windows installer doc (`/tauri-apps/tauri-docs`, "windows-installer") before continuing.

Expected: `cargo xwin --version` prints a version; `makensis -VERSION` prints one.

- [ ] **Step 2: Add the Windows target to the toolchain file**

`sdm-core/rust-toolchain.toml` becomes:

```toml
[toolchain]
channel = "stable"
targets = ["x86_64-apple-darwin", "aarch64-apple-darwin", "x86_64-pc-windows-msvc"]
components = ["rustfmt", "clippy"]
```

- [ ] **Step 3: Write the Windows check script**

`scripts/check-windows.sh`:

```bash
#!/bin/bash
# Type-checks the portable core crates for 64-bit Windows from macOS.
# Needs: rustup target x86_64-pc-windows-msvc, cargo-xwin, Homebrew llvm.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$(brew --prefix llvm 2>/dev/null)/bin:$PATH"
cd sdm-core
cargo xwin check --target x86_64-pc-windows-msvc \
  -p sdm-core -p sdm-resolve -p sdm-engine -p sdm-grabber "$@"
```

Run `chmod +x scripts/check-windows.sh`.

- [ ] **Step 4: Write the Linux Dockerfile and check script**

`sdm-tauri/docker/linux-build.Dockerfile`:

```dockerfile
# Ubuntu 22.04 keeps the glibc requirement low (see the Tauri AppImage/deb docs)
# and provides libwebkit2gtk-4.1-dev from its standard repositories.
FROM ubuntu:22.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential curl ca-certificates pkg-config git file \
    libwebkit2gtk-4.1-dev libssl-dev libgtk-3-dev libayatana-appindicator3-dev \
    librsvg2-dev libxdo-dev patchelf \
 && rm -rf /var/lib/apt/lists/*
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH="/root/.cargo/bin:${PATH}"
WORKDIR /work
```

`scripts/check-linux.sh`:

```bash
#!/bin/bash
# Type-checks the portable core crates (and, with --shell, the Tauri shell)
# for Linux inside a container. Skips with a message if Docker is not running.
set -euo pipefail
cd "$(dirname "$0")/.."
if ! docker info >/dev/null 2>&1; then
  echo "check-linux: Docker is not running; skipping the Linux check." >&2
  exit 0
fi
IMAGE=sdm-linux-build
docker build -q -t "$IMAGE" -f sdm-tauri/docker/linux-build.Dockerfile sdm-tauri/docker >/dev/null
PACKAGES="-p sdm-core -p sdm-resolve -p sdm-engine -p sdm-grabber"
if [[ "${1:-}" == "--shell" ]]; then PACKAGES="$PACKAGES -p sdm-app -p sdm-tauri"; fi
docker run --rm \
  -v "$PWD":/work \
  -v sdm-linux-target:/work/sdm-core/target-linux \
  -e CARGO_TARGET_DIR=/work/sdm-core/target-linux \
  "$IMAGE" bash -c "cd sdm-core && cargo check $PACKAGES"
```

Run `chmod +x scripts/check-linux.sh`. Add `sdm-core/target-linux/` to `.gitignore` if the volume mount leaves files there (check `git status`).

- [ ] **Step 5: Run both scripts and record the baseline**

Run: `scripts/check-windows.sh 2>&1 | tail -40` and `scripts/check-linux.sh 2>&1 | tail -40`

Expected today: the Windows run **fails** with errors in `sdm-engine` (`std::os::unix`, `libc`) and `sdm-resolve` (`std::os::unix`, `libc`, `pid_t`). The Linux run should pass (everything is Unix). If the Windows run fails earlier, inside a dependency's build script (a C dependency under `cargo-xwin`), stop and report it: it changes the plan (spec risk table).

Write the failing file list into the commit message body.

- [ ] **Step 6: Commit**

```bash
git add sdm-core/rust-toolchain.toml scripts/check-windows.sh scripts/check-linux.sh sdm-tauri/docker/linux-build.Dockerfile
git commit -m "Add Windows and Linux portability check scripts

Baseline: the Windows check fails in sdm-engine and sdm-resolve (Unix-only APIs); the Linux check passes.

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Platform I/O in `sdm-engine`

**Files:**

- Create: `sdm-core/crates/sdm-engine/src/platform_io.rs`
- Modify: `sdm-core/crates/sdm-engine/src/lib.rs`, `sparse_file.rs`, `resume_sidecar.rs`, `Cargo.toml`
- Test: the existing `sdm-core/crates/sdm-engine/tests/*` (they must still pass) and unit tests inside `platform_io.rs`

**Interfaces:**

- Produces (crate-private, in `crate::platform_io`):
  - `pub(crate) const EIO: i32`, `pub(crate) const EBADF: i32`
  - `pub(crate) fn write_at(file: &File, buf: &[u8], offset: u64) -> io::Result<usize>`
  - `pub(crate) fn fsync(file: &File) -> io::Result<()>`
  - `pub(crate) fn apply_mode(options: &mut OpenOptions, mode: u32)` (no-op on Windows)

- [ ] **Step 1: Write the failing test**

`platform_io.rs` starts with its tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn write_at_writes_at_the_requested_offset_without_moving_the_cursor_contract() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        apply_mode(&mut options, 0o644);
        let file = options.open(&path).unwrap();
        file.set_len(8).unwrap();

        assert_eq!(write_at(&file, b"abc", 2).unwrap(), 3);
        fsync(&file).unwrap();

        let mut contents = Vec::new();
        std::fs::File::open(&path).unwrap().read_to_end(&mut contents).unwrap();
        assert_eq!(contents, [0, 0, b'a', b'b', b'c', 0, 0, 0]);
    }

    #[test]
    fn error_codes_are_posix_numbers_on_every_os() {
        assert_eq!(EIO, 5);
        assert_eq!(EBADF, 9);
    }
}
```

Add `mod platform_io;` to `lib.rs` (private, alphabetical).

- [ ] **Step 2: Run to verify it fails**

Run: `cd sdm-core && cargo test -p sdm-engine platform_io`
Expected: compile error, functions not defined.

- [ ] **Step 3: Implement**

Above the tests in `platform_io.rs`:

```rust
//! The few file operations whose spelling differs between Unix and Windows,
//! kept in one place so the rest of the engine is portable.

use std::fs::{File, OpenOptions};
use std::io;

/// Reported when a write returns zero bytes or an `io::Error` carries no OS
/// code. The POSIX values on every OS: they are only ever shown to the user.
pub(crate) const EIO: i32 = 5;
/// Reported for a write after the file was closed.
pub(crate) const EBADF: i32 = 9;

/// One positional write (`pwrite(2)` / `WriteFile` with an offset). Returns the
/// number of bytes written; the caller loops over short writes.
pub(crate) fn write_at(file: &File, buf: &[u8], offset: u64) -> io::Result<usize> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.write_at(buf, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        file.seek_write(buf, offset)
    }
}

/// `fsync(2)` on Unix: deliberately not `File::sync_all`, which on macOS issues
/// the much slower `F_FULLFSYNC` (the Swift core called plain `fsync`).
/// On Windows `sync_all` is `FlushFileBuffers`.
pub(crate) fn fsync(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: `file` owns an open descriptor for the duration of the call.
        let result = unsafe { libc::fsync(file.as_raw_fd()) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(windows)]
    {
        file.sync_all()
    }
}

/// Sets the creation mode on Unix (the umask still applies); does nothing on
/// Windows, which has no such bits.
pub(crate) fn apply_mode(options: &mut OpenOptions, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(windows)]
    {
        let _ = (options, mode);
    }
}
```

- [ ] **Step 4: Switch `sparse_file.rs` to the helpers**

In `sparse_file.rs`:

1. Delete the imports `use std::os::fd::AsRawFd;` and `use std::os::unix::fs::{FileExt, OpenOptionsExt};`; add `use crate::platform_io::{self, EBADF, EIO};`.
2. Delete the `const EIO`, `const EBADF` definitions and the whole `pub(crate) fn fsync` (lines ~46-67); keep `fn errno_of` but make it use the imported `EIO`.
3. Re-export for the sidecar: `pub(crate) use crate::platform_io::fsync;` right below the imports (so `crate::sparse_file::fsync` keeps resolving).
4. In `SparseFile::open`, replace the `OpenOptions::new()...mode(0o644)...open(...)` chain with:

```rust
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        platform_io::apply_mode(&mut options, 0o644);
        let file = options.open(&incomplete_path).map_err(|error| {
            SparseFileError::CouldNotOpen {
                path: incomplete_path.to_string_lossy().into_owned(),
                errno: errno_of(&error),
            }
        })?;
```

5. In `write`, replace `file.write_at(&data[written..], offset as u64 + written as u64)` with `platform_io::write_at(file, &data[written..], offset as u64 + written as u64)`.

- [ ] **Step 5: Switch `resume_sidecar.rs`**

In `write_atomically`: delete `use std::os::unix::fs::OpenOptionsExt;`, and replace the `OpenOptions::new()...mode(0o666)...open(&temp_path)?` chain with:

```rust
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        crate::platform_io::apply_mode(&mut options, 0o666); // the umask reduces this to the usual 0644
        let mut file = options.open(&temp_path)?;
```

- [ ] **Step 6: Make `libc` a Unix-only dependency**

In `sdm-core/crates/sdm-engine/Cargo.toml`, remove `libc` from `[dependencies]` and add it under a target table, using the package manager:

Run: `cd sdm-core && cargo remove -p sdm-engine libc && cargo add -p sdm-engine libc --target 'cfg(unix)'`

Expected: `Cargo.toml` now has `[target.'cfg(unix)'.dependencies] libc = "..."`. If other engine code still uses `libc::` outside `cfg(unix)`, the Windows check (Task 5) finds it. The macOS `read_boot_session_id` is already gated.

- [ ] **Step 7: Run all engine tests**

Run: `cd sdm-core && cargo test -p sdm-engine 2>&1 | tail -15`
Expected: every test passes (the churn and durable-state tests exercise `SparseFile` and `write_atomically` heavily). Also `cargo clippy -p sdm-engine --all-targets` has no new warnings.

- [ ] **Step 8: Commit**

```bash
git add sdm-core/crates/sdm-engine
git commit -m "Move positional I/O, fsync and open modes behind platform_io

libc becomes a cfg(unix) dependency (added with cargo add --target).

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 3: `sdm-resolve` portability

**Files:**

- Modify: `sdm-core/crates/sdm-resolve/src/binary_locator.rs`, `system_process_runner.rs`, `managed_binaries.rs`, `managed_binaries_types.rs`, `Cargo.toml`
- Test: unit tests in each file (existing ones must pass) plus the new ones below

**Interfaces:**

- Produces:
  - `BinaryLocator::default_search_paths() -> Vec<PathBuf>` unchanged signature, per-OS content.
  - `pub(crate) fn executable_name(name: &str) -> String` in `binary_locator.rs`: `name` on Unix, `name.exe` on Windows (unless it already ends in `.exe`).
  - `SystemProcessRunner::child_environment_from(executable, inherited, home) -> HashMap<OsString, OsString>` unchanged signature.
  - `pub fn yt_dlp_asset_name() -> &'static str` in `managed_binaries_types.rs`.

- [ ] **Step 1: Write failing tests**

Append to `binary_locator.rs`'s existing test module (create a `#[cfg(test)] mod portability_tests` if there is none):

```rust
#[cfg(test)]
mod portability_tests {
    use super::*;

    #[test]
    fn executable_name_adds_exe_only_on_windows() {
        if cfg!(windows) {
            assert_eq!(executable_name("yt-dlp"), "yt-dlp.exe");
            assert_eq!(executable_name("ffmpeg.exe"), "ffmpeg.exe");
        } else {
            assert_eq!(executable_name("yt-dlp"), "yt-dlp");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_file_without_the_exec_bit_is_not_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool");
        std::fs::write(&path, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!is_executable_file(&path));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_executable_file(&path));
    }

    #[test]
    fn a_directory_is_never_executable() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_executable_file(dir.path()));
    }
}
```

Append to the tests of `system_process_runner.rs`:

```rust
#[cfg(test)]
mod portability_tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn child_path_keeps_the_executable_directory_first_and_has_no_duplicates() {
        let executable = Path::new(if cfg!(windows) { r"C:\tools\yt-dlp.exe" } else { "/opt/tools/yt-dlp" });
        let inherited_path = std::env::join_paths(["/usr/bin", "/usr/bin", "/extra"]).unwrap();
        let env = SystemProcessRunner::child_environment_from(
            executable,
            [(OsString::from("PATH"), inherited_path)],
            PathBuf::from(if cfg!(windows) { r"C:\Users\me" } else { "/home/me" }),
        );
        let path = env.get(std::ffi::OsStr::new("PATH")).expect("PATH is set");
        let dirs: Vec<PathBuf> = std::env::split_paths(path).collect();
        assert_eq!(dirs[0], executable.parent().unwrap());
        let mut seen = std::collections::HashSet::new();
        assert!(dirs.iter().all(|d| seen.insert(d.clone())), "duplicate PATH entries: {dirs:?}");
    }
}
```

Append to `managed_binaries_types.rs` tests:

```rust
#[cfg(test)]
mod asset_tests {
    use super::*;

    #[test]
    fn yt_dlp_asset_matches_the_current_platform() {
        let name = yt_dlp_asset_name();
        if cfg!(target_os = "macos") {
            assert_eq!(name, "yt-dlp_macos");
        } else if cfg!(windows) {
            assert_eq!(name, "yt-dlp.exe");
        } else if cfg!(target_arch = "aarch64") {
            assert_eq!(name, "yt-dlp_linux_aarch64");
        } else {
            assert_eq!(name, "yt-dlp_linux");
        }
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd sdm-core && cargo test -p sdm-resolve portability asset_tests 2>&1 | tail -20`
Expected: compile errors (`executable_name`, `yt_dlp_asset_name` not found).

- [ ] **Step 3: `binary_locator.rs`**

Replace the imports and platform parts:

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
```

(delete `CString` and `OsStrExt`). Replace `default_search_paths`:

```rust
    pub fn default_search_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        #[cfg(unix)]
        {
            paths.push(PathBuf::from("/opt/homebrew/bin"));
            paths.push(PathBuf::from("/usr/local/bin"));
            paths.push(PathBuf::from("/usr/bin"));
            paths.push(match std::env::home_dir() {
                Some(home) => home.join(".local/bin"),
                None => PathBuf::from("~/.local/bin"),
            });
        }
        // On Windows nothing is searched by default: the managed `bin/` and
        // the Settings overrides are the sources (tools are not installed in
        // fixed system locations there).
        paths
    }
```

In `locate`, change `let candidate = directory.join(name);` to `let candidate = directory.join(executable_name(name));`.

Add and replace the check:

```rust
/// The on-disk file name for the tool `name`: `name` on Unix, `name.exe` on
/// Windows (unless it already has the suffix).
pub(crate) fn executable_name(name: &str) -> String {
    if cfg!(windows) && !name.to_ascii_lowercase().ends_with(".exe") {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

/// The path is a file and the current user may execute it: the exec bits on
/// Unix (`access(2)` with `X_OK`, as `FileManager.isExecutableFile` does), the
/// `.exe`/`.bat`/`.cmd` suffix on Windows.
pub(crate) fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        // SAFETY: `c_path` is a valid NUL-terminated string that outlives the call.
        unsafe { libc::access(c_path.as_ptr(), libc::X_OK) == 0 }
    }
    #[cfg(windows)]
    {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "exe" | "bat" | "cmd"))
    }
}
```

(The old function returned true for a directory with `x` set; `is_file` makes a directory never executable, which the new test pins. `access(2)` already ran for files only in practice.)

- [ ] **Step 4: `system_process_runner.rs`**

1. Replace the two `use std::os::unix::...` lines at the top with nothing.
2. In `child_environment_from`, replace the tool-dir list and the PATH merge. The Unix-only directory block becomes:

```rust
        #[cfg(unix)]
        {
            for dir in [
                "/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin", "/usr/local/sbin",
                "/opt/local/bin", "/opt/local/sbin", "/usr/bin", "/bin", "/usr/sbin", "/sbin",
            ] {
                tool_dirs.push(PathBuf::from(dir));
            }
            tool_dirs.push(home.join(".local/bin"));
        }
        #[cfg(windows)]
        {
            let _ = &home;
        }
```

and the inherited-PATH parsing and the final join use `std::env::split_paths` / `std::env::join_paths`:

```rust
        let existing: Vec<PathBuf> = env
            .get(std::ffi::OsStr::new("PATH"))
            .map(|p| std::env::split_paths(p).filter(|d| !d.as_os_str().is_empty()).collect())
            .unwrap_or_default();
```

and where the merged vector is turned back into the `PATH` value, use `std::env::join_paths(merged).unwrap_or_default()` (an entry containing the separator makes `join_paths` fail; falling back to an empty `PATH` would break the child, so on `Err` keep the original inherited value instead: `match join_paths(&merged) { Ok(p) => p, Err(_) => original_path.clone() }` — read the lines that follow the part shown in the current file and keep its dedupe-by-`HashSet` logic.)

`home.join(".local/bin")` replaces the byte-level trailing-slash trimming (`PathBuf::join` handles a trailing separator). 3. Exit status: replace `status.map(|s| s.code().or_else(|| s.signal()).unwrap_or(-1))` with `status.map(exit_code)` and add:

```rust
/// The exit code, or on Unix the terminating signal number when there is no
/// code; `-1` when neither is known.
fn exit_code(status: ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.code().or_else(|| status.signal()).unwrap_or(-1)
    }
    #[cfg(not(unix))]
    {
        status.code().unwrap_or(-1)
    }
}
```

4. `ChildGuard::terminate`: keep the SIGTERM body under `#[cfg(unix)]` and add the Windows branch:

```rust
    fn terminate(&self) {
        #[cfg(unix)]
        if let Some(pid) = self.child.as_ref().and_then(Child::id) {
            // A pid that does not fit `pid_t` cannot be a real child: skip.
            let Ok(pid) = libc::pid_t::try_from(pid) else {
                return;
            };
            // SAFETY: `pid` is a child we have not yet reaped (`Child::id`
            // returns `None` once it has), so it cannot have been recycled.
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
        }
        // Windows has no SIGTERM: ask the OS to end the process now. The
        // bounded wait that follows (`drain_after_terminate`) is unchanged.
        // yt-dlp's `--continue` resumes from its fragment files after this.
        #[cfg(windows)]
        if let Some(child) = self.child.as_ref() {
            let _ = child.start_kill();
        }
    }
```

`Child::start_kill` is tokio's `Child` method (takes `&mut self` in some versions). If the compiler rejects `&self`, change `terminate` to take `&mut self` for the Windows branch and update its two callers (`execute` has `guard` as `mut`; `Drop::drop` has `&mut self`). Verify the signature in the tokio docs (Context7 `/tokio-rs/tokio`).

5. `libc` in `sdm-resolve/Cargo.toml`: `cargo remove -p sdm-resolve libc && cargo add -p sdm-resolve libc --target 'cfg(unix)'`.

- [ ] **Step 5: `managed_binaries*.rs`**

In `managed_binaries_types.rs` add:

```rust
/// The release asset name of yt-dlp for the platform this build runs on.
pub fn yt_dlp_asset_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "yt-dlp_macos"
    } else if cfg!(windows) {
        "yt-dlp.exe"
    } else if cfg!(target_arch = "aarch64") {
        "yt-dlp_linux_aarch64"
    } else {
        "yt-dlp_linux"
    }
}
```

and change the download URL builder (line ~39, `.../{tag}/yt-dlp_macos`) to interpolate `yt_dlp_asset_name()`; export the function from `lib.rs` with the other `managed_binaries_types` items.

In `managed_binaries.rs`:

- `yt_dlp_path`: `self.bin_directory.join(crate::binary_locator::executable_name("yt-dlp"))`.
- `verify(&bytes, &sums, "yt-dlp_macos")?` becomes `verify(&bytes, &sums, yt_dlp_asset_name())?`.
- `write_executable`: gate the permissions call:

```rust
        write_atomically(path, data)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
        }
        Ok(())
```

- Wrap the two `runner.run(Path::new("/usr/bin/xattr")...)` and `/usr/bin/codesign` calls in a `#[cfg(target_os = "macos")] { ... }` block.
- The test helper at the bottom (`use std::os::unix::fs::PermissionsExt;` near line 470) is already in a test module; add `#[cfg(unix)]` to the function that uses it.

- [ ] **Step 6: Run the tests**

Run: `cd sdm-core && cargo test -p sdm-resolve 2>&1 | tail -15`
Expected: all pass, including the three new groups. `cargo clippy -p sdm-resolve --all-targets` clean.

- [ ] **Step 7: Commit**

```bash
git add sdm-core/crates/sdm-resolve
git commit -m "Make sdm-resolve portable: executable check, PATH, exit status, yt-dlp asset

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Windows filename rules in `sdm-core`

**Files:**

- Create: `sdm-core/crates/sdm-core/src/filename_rules.rs`
- Modify: `sdm-core/crates/sdm-core/src/lib.rs`, `safe_path_component.rs`
- Test: inside `filename_rules.rs` (a fixture table; runs on every OS)

**Interfaces:**

- Produces: `pub fn windows_safe_name(name: &str) -> Option<String>` in `sdm_core::filename_rules`, re-exported at the crate root. Pure and OS-independent. `safe_path_component` calls it on Windows only.

- [ ] **Step 1: Write the failing fixture-table test**

`filename_rules.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// (input, expected). `None` means nothing usable is left.
    const CASES: &[(&str, Option<&str>)] = &[
        ("plain.bin", Some("plain.bin")),
        ("with spaces.bin", Some("with spaces.bin")),
        ("نام فایل.bin", Some("نام فایل.bin")),
        ("a<b>c:d\"e|f?g*h.txt", Some("a_b_c_d_e_f_g_h.txt")),
        ("tab\there.txt", Some("tab_here.txt")),
        ("trailing dot.", Some("trailing dot")),
        ("trailing space ", Some("trailing space")),
        ("trailing. . ", Some("trailing")),
        ("CON", Some("_CON")),
        ("con.txt", Some("_con.txt")),
        ("Nul.tar.gz", Some("_Nul.tar.gz")),
        ("COM1", Some("_COM1")),
        ("lpt9.log", Some("_lpt9.log")),
        ("COM0.txt", Some("COM0.txt")),
        ("console.txt", Some("console.txt")),
        ("folder\\evil.txt", Some("evil.txt")),
        ("a/b\\c.txt", Some("c.txt")),
        ("..", None),
        (".", None),
        ("", None),
        ("   ", None),
        ("...", None),
    ];

    #[test]
    fn fixture_table() {
        for (input, expected) in CASES {
            assert_eq!(
                windows_safe_name(input).as_deref(),
                *expected,
                "input: {input:?}"
            );
        }
    }

    #[test]
    fn long_names_are_truncated_keeping_the_extension() {
        let long = format!("{}.mkv", "a".repeat(400));
        let out = windows_safe_name(&long).unwrap();
        assert!(out.encode_utf16().count() <= 255, "{}", out.len());
        assert!(out.ends_with(".mkv"));
    }

    #[test]
    fn a_huge_extension_cannot_defeat_the_limit() {
        let long = format!("name.{}", "b".repeat(400));
        let out = windows_safe_name(&long).unwrap();
        assert!(out.encode_utf16().count() <= 255);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd sdm-core && cargo test -p sdm-core filename_rules`
Expected: compile error (module missing). Add `pub mod filename_rules; pub use filename_rules::windows_safe_name;` to `lib.rs` first, then expect "function not found".

- [ ] **Step 3: Implement**

```rust
//! Windows filename rules, as a pure function so every OS can test them.
//!
//! Windows rejects the characters `<>:"|?*` and control characters, treats `\`
//! as a separator, silently strips trailing dots and spaces, reserves the
//! device names `CON PRN AUX NUL COM1-9 LPT1-9` (with or without an extension),
//! and limits a path component to 255 UTF-16 units.

const MAX_UNITS: usize = 255;
const RESERVED: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];

fn is_reserved_stem(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    if RESERVED.contains(&upper.as_str()) {
        return true;
    }
    for prefix in ["COM", "LPT"] {
        if let Some(digit) = upper.strip_prefix(prefix)
            && digit.len() == 1
            && matches!(digit.as_bytes()[0], b'1'..=b'9')
        {
            return true;
        }
    }
    false
}

fn units(text: &str) -> usize {
    text.encode_utf16().count()
}

/// The last path component of `name` reduced to something Windows accepts, or
/// `None` when nothing usable is left. Splits on both `/` and `\`.
pub fn windows_safe_name(name: &str) -> Option<String> {
    let last = name
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("");
    let replaced: String = last
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let trimmed = replaced.trim_end_matches(['.', ' ']);
    if trimmed.is_empty() || trimmed.chars().all(|c| c == '.') {
        return None;
    }

    let (stem, extension) = match trimmed.find('.') {
        Some(0) | None => (trimmed, ""),
        Some(index) => trimmed.split_at(index),
    };
    // `extension` keeps its leading dot. Truncate the stem first, then the
    // extension if it alone is too long, so the total stays within the limit.
    let mut stem = stem.to_owned();
    let mut extension = extension.to_owned();
    while units(&stem) + units(&extension) > MAX_UNITS && units(&stem) > 1 {
        stem.pop();
    }
    while units(&stem) + units(&extension) > MAX_UNITS && !extension.is_empty() {
        extension.pop();
    }

    let mut out = format!("{stem}{extension}");
    if is_reserved_stem(&stem) {
        out.insert(0, '_');
    }
    Some(out)
}
```

- [ ] **Step 4: Run to verify the table passes**

Run: `cd sdm-core && cargo test -p sdm-core filename_rules`
Expected: 3 tests pass. If a case fails, fix the function, not the table, unless the table contradicts the rules above.

- [ ] **Step 5: Apply the rules on Windows only**

In `safe_path_component.rs`, at the top of `single_path_component`:

```rust
pub fn single_path_component(name: &str) -> Option<String> {
    #[cfg(windows)]
    {
        return crate::filename_rules::windows_safe_name(name);
    }
    #[cfg(not(windows))]
    {
        let trimmed = name.trim_end_matches('/');
        // ... the existing body, unchanged ...
    }
}
```

Keep the existing body inside the `not(windows)` block verbatim so macOS and Linux behavior is byte-identical (the existing "ordinary names are unchanged" tests pin it).

- [ ] **Step 6: Run the whole core crate and commit**

Run: `cd sdm-core && cargo test -p sdm-core -p sdm-engine 2>&1 | tail -8`
Expected: all pass.

```bash
git add sdm-core/crates/sdm-core
git commit -m "Add pure Windows filename rules and apply them on Windows

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Portability gates green and wired into `test.sh`

**Files:**

- Modify: `test.sh`

**Interfaces:**

- Consumes: Tasks 1–4.

- [ ] **Step 1: Run the Windows check**

Run: `scripts/check-windows.sh 2>&1 | tail -60`
Expected: **PASS** (`Finished`). If errors remain, they are Unix-only code this plan missed (the spec's survey was a grep, not a compile): fix each in the owning file the same way as Tasks 2–3 (cfg-gate, or add to `platform_io`), re-run, and list what you fixed in the commit message. Do not silence errors with blanket `#[cfg(unix)]` around logic the Windows build needs.

- [ ] **Step 2: Run the Linux check**

Run: `scripts/check-linux.sh 2>&1 | tail -20`
Expected: PASS (or the skip message when Docker is off; start Docker and re-run).

- [ ] **Step 3: Wire both into `test.sh`**

`test.sh` becomes:

```bash
#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
(cd sdm-core && cargo test --workspace --exclude sdm-tauri)
./scripts/check-windows.sh
./scripts/check-linux.sh
./scripts/build-rust-core.sh debug
(cd SDMKit && swift test)
```

(`--exclude sdm-tauri` is harmless before the shell exists: cargo ignores an unknown excluded name with a warning; if it errors, add the flag in Task 12 instead and leave this line as `--workspace` until then.)

- [ ] **Step 4: Run the whole script**

Run: `./test.sh 2>&1 | tail -25`
Expected: exits 0.

- [ ] **Step 5: Commit**

```bash
git add test.sh sdm-core
git commit -m "Gate the Windows and Linux compile checks in test.sh

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 6: `sdm-app` crate: errors, wire types and conversion

**Files:**

- Create: `sdm-core/crates/sdm-app/Cargo.toml`, `src/lib.rs`, `src/error.rs`, `src/protocol.rs`, `src/convert.rs`
- Test: unit tests in `convert.rs`

**Interfaces:**

- Produces (public): `AppError`, and in `protocol`: `Patch`, `Structure`, `StructurePackage`, `StructureItem`, `ItemStateDto`, `ItemFigures`, `RangeDto`, `PackageFigures`, `EngineStats`, `ItemHistory`, `SettingsDto`, `BinariesStatusDto`.
- Produces (crate-private, `convert`): `structure_package(&PackageSnapshot) -> StructurePackage`, `item_figures(&ItemSnapshot) -> ItemFigures`, `package_figures(&PackageSnapshot) -> PackageFigures`, `engine_stats(&EngineSnapshot) -> EngineStats`.

- [ ] **Step 1: Create the crate**

```bash
cd sdm-core
cargo new --lib crates/sdm-app
cargo add -p sdm-app sdm-core --path crates/sdm-core
cargo add -p sdm-app sdm-engine --path crates/sdm-engine
cargo add -p sdm-app sdm-resolve --path crates/sdm-resolve
cargo add -p sdm-app serde --features derive
cargo add -p sdm-app serde_json thiserror tracing url uuid percent-encoding async-trait
cargo add -p sdm-app tokio --features rt,sync,time,macros
cargo add -p sdm-app specta@=2.0.0-rc.25 --features derive
cargo add -p sdm-app --dev tempfile
cargo add -p sdm-app --dev tokio --features rt-multi-thread,macros,test-util
cargo add -p sdm-app --dev sdm-resolve --path crates/sdm-resolve --features testing
```

Then set `edition.workspace = true`, `version.workspace = true`, `publish.workspace = true` and `[lints] workspace = true` in the new `Cargo.toml` (this edits the package metadata, not the dependency list). Check the `specta` version and feature names against Context7 (`/specta-rs/specta`) first; the docs on 2026-10-14 pinned `specta@=2.0.0-rc.25`. Use whatever pinned pair `tauri-specta` requires: the two must agree, and Task 12 adds `tauri-specta`.

`lib.rs`:

```rust
//! The controller layer between the Rust download engine and a UI: turns
//! engine snapshots into small ordered patches, owns settings, and decides
//! when a notification is due. No clock and no UI framework in here.

pub mod error;
pub mod protocol;

mod convert;

pub use error::AppError;
```

- [ ] **Step 2: Write `error.rs`**

```rust
use serde::Serialize;
use specta::Type;

/// What a command reports to the UI. `kind` is stable; `message` is for people.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type, thiserror::Error)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AppError {
    #[error("invalid {argument}: {message}")]
    InvalidArgument { argument: String, message: String },
    #[error("{message}")]
    Failed { message: String },
    #[error("internal error: {message}")]
    Internal { message: String },
}

impl AppError {
    pub fn invalid(argument: &str, message: impl Into<String>) -> Self {
        AppError::InvalidArgument {
            argument: argument.to_owned(),
            message: message.into(),
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        AppError::Failed {
            message: message.into(),
        }
    }
}
```

- [ ] **Step 3: Write `protocol.rs`**

```rust
//! The wire types. Everything the UI receives or sends is defined here, so one
//! file shows the whole protocol. Ids are lowercase hyphenated UUID strings;
//! byte counts are `f64` (exact to 2^53, and `specta` rejects 64-bit integers).

use serde::{Deserialize, Serialize};
use specta::Type;

/// One ordered update. The receiver applies patches in `seq` order; a patch
/// with `full` set replaces all prior state.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    /// `+1` per emitted patch, wrapping. Any non-consecutive value is a gap.
    pub seq: u32,
    pub full: bool,
    /// Present only when the shape of the list (or a row's non-numeric content) changed.
    pub structure: Option<Structure>,
    pub items: Vec<ItemFigures>,
    pub packages: Vec<PackageFigures>,
    pub stats: Option<EngineStats>,
    /// Full speed history, only for items whose details panel is open.
    pub histories: Vec<ItemHistory>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Structure {
    pub packages: Vec<StructurePackage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StructurePackage {
    pub id: String,
    pub name: String,
    /// `Priority` as 0 (lowest) to 4 (highest).
    pub priority: u32,
    pub items: Vec<StructureItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StructureItem {
    pub id: String,
    pub url: String,
    pub filename: String,
    pub state: ItemStateDto,
    pub is_enabled: bool,
    pub is_resumable: Option<bool>,
    pub is_assembling: bool,
    pub file_missing: bool,
    pub metadata: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ItemStateDto {
    Stopped,
    Queued,
    Running,
    Completed,
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RangeDto {
    pub start: f64,
    pub end: f64,
}

/// The numbers that change while an item transfers.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ItemFigures {
    pub id: String,
    pub completed: Vec<RangeDto>,
    pub completed_bytes: f64,
    pub total_bytes: Option<f64>,
    pub active_segments: u32,
    pub configured_segments: u32,
    pub bytes_per_second: f64,
    /// The short trailing history every snapshot carries (64 samples).
    pub speed_history: Vec<f64>,
    pub retry_hold_seconds: Option<u32>,
    pub failed_attempt_count: Option<u32>,
    pub last_failure_reason: Option<String>,
    pub checkpoint_failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackageFigures {
    pub id: String,
    pub completed_bytes: f64,
    pub total_bytes: f64,
    pub bytes_per_second: f64,
    pub speed_history: Vec<f64>,
    pub eta_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EngineStats {
    pub bytes_per_second: f64,
    pub history: Vec<f64>,
    pub running_count: u32,
    pub queued_count: u32,
    pub overall_fraction: f64,
    pub ring_completed_bytes: f64,
    pub ring_total_bytes: f64,
    pub waiting_item_count: u32,
    pub waiting_package_count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ItemHistory {
    pub id: String,
    pub speed_history: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub download_folder: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BinariesStatusDto {
    pub yt_dlp_version: Option<String>,
    pub latest_known: Option<String>,
    pub ffmpeg_version: Option<String>,
    pub qjs_version: Option<String>,
    pub last_error: Option<String>,
}
```

- [ ] **Step 4: Write the failing conversion test**

`convert.rs` starts with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sdm_core::{ByteRange, ItemState, Priority, RangeSet, SourceUrl};
    use sdm_engine::{ItemSnapshot, PackageSnapshot};
    use uuid::Uuid;

    fn item(id: Uuid, state: ItemState) -> ItemSnapshot {
        let mut completed = RangeSet::new();
        completed.insert(ByteRange::new(0, 100));
        completed.insert(ByteRange::new(200, 250));
        ItemSnapshot::new(
            id,
            SourceUrl::new("https://example.com/a.bin").unwrap(),
            "a.bin",
            Some(1000),
            completed,
            state,
            true,
            Some(true),
            2,
            4,
            512.0,
            vec![1.0, 2.0],
        )
    }

    #[test]
    fn item_figures_carry_ranges_and_counts() {
        let id = Uuid::new_v4();
        let figures = item_figures(&item(id, ItemState::Running));
        assert_eq!(figures.id, id.to_string());
        assert_eq!(figures.completed_bytes, 150.0);
        assert_eq!(figures.total_bytes, Some(1000.0));
        assert_eq!(figures.completed.len(), 2);
        assert_eq!(figures.completed[1], RangeDto { start: 200.0, end: 250.0 });
        assert_eq!((figures.active_segments, figures.configured_segments), (2, 4));
        assert_eq!(figures.speed_history, vec![1.0, 2.0]);
    }

    #[test]
    fn a_failed_state_keeps_its_reason() {
        let id = Uuid::new_v4();
        let package = PackageSnapshot::new(
            Uuid::new_v4(),
            "Pack",
            Priority::High,
            vec![item(id, ItemState::Failed { reason: "disk full".into() })],
            0.0,
            1,
        );
        let structure = structure_package(&package);
        assert_eq!(structure.priority, 3);
        assert_eq!(
            structure.items[0].state,
            ItemStateDto::Failed { reason: "disk full".into() }
        );
    }

    #[test]
    fn package_figures_use_the_aggregate() {
        let package = PackageSnapshot::new(
            Uuid::new_v4(),
            "Pack",
            Priority::Normal,
            vec![item(Uuid::new_v4(), ItemState::Running)],
            0.0,
            1,
        );
        let figures = package_figures(&package);
        assert_eq!(figures.completed_bytes, 150.0);
        assert_eq!(figures.total_bytes, 1000.0);
        assert_eq!(figures.bytes_per_second, 512.0);
    }
}
```

- [ ] **Step 5: Run to verify it fails**

Run: `cd sdm-core && cargo test -p sdm-app 2>&1 | tail -15`
Expected: compile error, `convert` functions undefined.

- [ ] **Step 6: Implement `convert.rs`**

Above the tests:

```rust
//! Engine snapshot types -> wire types.

use sdm_core::ItemState;
use sdm_engine::{EngineSnapshot, ItemSnapshot, PackageSnapshot};

use crate::protocol::*;

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn state_dto(state: &ItemState) -> ItemStateDto {
    match state {
        ItemState::Stopped => ItemStateDto::Stopped,
        ItemState::Queued => ItemStateDto::Queued,
        ItemState::Running => ItemStateDto::Running,
        ItemState::Completed => ItemStateDto::Completed,
        ItemState::Failed { reason } => ItemStateDto::Failed {
            reason: reason.clone(),
        },
    }
}

pub(crate) fn structure_package(package: &PackageSnapshot) -> StructurePackage {
    StructurePackage {
        id: package.id.to_string(),
        name: package.name.clone(),
        priority: package.priority as u32,
        items: package
            .items
            .iter()
            .map(|item| StructureItem {
                id: item.id.to_string(),
                url: item.url.as_str().to_owned(),
                filename: item.filename.clone(),
                state: state_dto(&item.state),
                is_enabled: item.is_enabled,
                is_resumable: item.is_resumable,
                is_assembling: item.is_assembling,
                file_missing: item.file_missing,
                metadata: item.metadata.clone(),
            })
            .collect(),
    }
}

pub(crate) fn item_figures(item: &ItemSnapshot) -> ItemFigures {
    ItemFigures {
        id: item.id.to_string(),
        completed: item
            .completed
            .ranges()
            .iter()
            .map(|range| RangeDto {
                start: range.start as f64,
                end: range.end as f64,
            })
            .collect(),
        completed_bytes: item.completed.total_bytes() as f64,
        total_bytes: item.total_bytes.map(|bytes| bytes as f64),
        active_segments: count(item.active_segments),
        configured_segments: count(item.configured_segments),
        bytes_per_second: item.bytes_per_second,
        speed_history: item.speed_history.clone(),
        retry_hold_seconds: item.retry_hold_seconds.map(count),
        failed_attempt_count: item.failed_attempt_count.map(count),
        last_failure_reason: item.last_failure_reason.clone(),
        checkpoint_failure: item.checkpoint_failure.clone(),
    }
}

pub(crate) fn package_figures(package: &PackageSnapshot) -> PackageFigures {
    let aggregate = &package.aggregate;
    PackageFigures {
        id: package.id.to_string(),
        completed_bytes: aggregate.completed_bytes as f64,
        total_bytes: aggregate.total_bytes as f64,
        bytes_per_second: aggregate.bytes_per_second,
        speed_history: aggregate.speed_history.clone(),
        eta_seconds: aggregate.eta,
    }
}

pub(crate) fn engine_stats(snapshot: &EngineSnapshot) -> EngineStats {
    let figures = &snapshot.figures;
    EngineStats {
        bytes_per_second: snapshot.global_bytes_per_second,
        history: snapshot.global_history.clone(),
        running_count: count(figures.running_count),
        queued_count: count(figures.queued_count),
        overall_fraction: figures.overall_fraction,
        ring_completed_bytes: figures.ring_completed_bytes as f64,
        ring_total_bytes: figures.ring_total_bytes as f64,
        waiting_item_count: count(figures.waiting_item_count),
        waiting_package_count: count(figures.waiting_package_count),
    }
}
```

`Priority as u32` requires `Priority` to be `Copy` with explicit discriminants (it has `Lowest = 0 … Highest = 4`). If `Priority` is not `Copy`, use `package.priority.clone() as u32` is invalid on non-Copy enums; check `priority.rs` and match explicitly instead.

- [ ] **Step 7: Run to verify it passes**

Run: `cd sdm-core && cargo test -p sdm-app 2>&1 | tail -10 && cargo clippy -p sdm-app --all-targets 2>&1 | tail -5`
Expected: 3 tests pass, no clippy warnings.

- [ ] **Step 8: Commit**

```bash
git add sdm-core/Cargo.lock sdm-core/crates/sdm-app
git commit -m "Add the sdm-app crate with the wire protocol and snapshot conversion

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 7: The differ

**Files:**

- Create: `sdm-core/crates/sdm-app/src/differ.rs`
- Modify: `src/lib.rs` (add `mod differ;`)
- Test: unit tests in `differ.rs`

**Interfaces:**

- Consumes: Task 6's `convert` and `protocol`.
- Produces (crate-private):

```rust
pub(crate) struct Delta {
    pub structure: Option<Structure>,
    pub items: Vec<ItemFigures>,
    pub packages: Vec<PackageFigures>,
    pub stats: Option<EngineStats>,
}
#[derive(Default)] pub(crate) struct Differ { .. }
impl Differ {
    pub(crate) fn reset(&mut self);
    pub(crate) fn diff(&mut self, snapshot: &EngineSnapshot) -> Option<Delta>;   // None when nothing changed
    pub(crate) fn full(&mut self, snapshot: &EngineSnapshot) -> Delta;           // everything, always
}
```

- [ ] **Step 1: Write the failing tests**

`differ.rs` starts with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sdm_core::{ByteRange, ItemState, Priority, RangeSet, SourceUrl};
    use sdm_engine::{ItemSnapshot, PackageSnapshot};
    use uuid::Uuid;

    struct Fixture {
        package_id: Uuid,
        item_ids: Vec<Uuid>,
    }

    fn item(id: Uuid, state: ItemState, done: i64, speed: f64) -> ItemSnapshot {
        let mut completed = RangeSet::new();
        if done > 0 {
            completed.insert(ByteRange::new(0, done));
        }
        ItemSnapshot::new(
            id,
            SourceUrl::new("https://example.com/f").unwrap(),
            "f.bin",
            Some(1000),
            completed,
            state,
            true,
            Some(true),
            1,
            4,
            speed,
            vec![speed],
        )
    }

    fn snapshot(fixture: &Fixture, revision: u64, generation: u64, items: Vec<ItemSnapshot>) -> EngineSnapshot {
        let package = PackageSnapshot::new(
            fixture.package_id,
            "Pack",
            Priority::Normal,
            items,
            0.0,
            revision,
        );
        EngineSnapshot::new(vec![package], 0.0, vec![], generation)
    }

    fn fixture(count: usize) -> Fixture {
        Fixture {
            package_id: Uuid::new_v4(),
            item_ids: (0..count).map(|_| Uuid::new_v4()).collect(),
        }
    }

    #[test]
    fn the_first_diff_sends_everything() {
        let f = fixture(2);
        let snap = snapshot(&f, 1, 1, vec![
            item(f.item_ids[0], ItemState::Running, 10, 5.0),
            item(f.item_ids[1], ItemState::Queued, 0, 0.0),
        ]);
        let mut differ = Differ::default();
        let delta = differ.diff(&snap).expect("first diff is never empty");
        let structure = delta.structure.expect("structure sent first time");
        assert_eq!(structure.packages[0].items.len(), 2);
        assert_eq!(delta.items.len(), 2);
        assert_eq!(delta.packages.len(), 1);
        assert!(delta.stats.is_some());
    }

    #[test]
    fn an_identical_snapshot_produces_no_delta() {
        let f = fixture(1);
        let snap = snapshot(&f, 1, 1, vec![item(f.item_ids[0], ItemState::Running, 10, 5.0)]);
        let mut differ = Differ::default();
        differ.diff(&snap);
        assert!(differ.diff(&snap).is_none());
    }

    #[test]
    fn a_progress_change_sends_figures_only() {
        let f = fixture(2);
        let mut differ = Differ::default();
        differ.diff(&snapshot(&f, 1, 1, vec![
            item(f.item_ids[0], ItemState::Running, 10, 5.0),
            item(f.item_ids[1], ItemState::Running, 10, 5.0),
        ]));
        let delta = differ
            .diff(&snapshot(&f, 2, 2, vec![
                item(f.item_ids[0], ItemState::Running, 50, 5.0),
                item(f.item_ids[1], ItemState::Running, 10, 5.0),
            ]))
            .expect("progress changed");
        assert!(delta.structure.is_none(), "shape did not change");
        assert_eq!(delta.items.len(), 1, "only the changed item is sent");
        assert_eq!(delta.items[0].id, f.item_ids[0].to_string());
        assert_eq!(delta.items[0].completed_bytes, 50.0);
    }

    #[test]
    fn a_state_change_sends_the_structure() {
        let f = fixture(1);
        let mut differ = Differ::default();
        differ.diff(&snapshot(&f, 1, 1, vec![item(f.item_ids[0], ItemState::Running, 10, 5.0)]));
        let delta = differ
            .diff(&snapshot(&f, 2, 2, vec![item(f.item_ids[0], ItemState::Completed, 1000, 0.0)]))
            .unwrap();
        let structure = delta.structure.expect("state is structural");
        assert_eq!(structure.packages[0].items[0].state, ItemStateDto::Completed);
    }

    #[test]
    fn an_unchanged_revision_is_skipped_without_comparing_items() {
        // Same revision but (deliberately) different figures: the differ trusts
        // the engine's revision, exactly as the SwiftUI app does.
        let f = fixture(1);
        let mut differ = Differ::default();
        differ.diff(&snapshot(&f, 7, 1, vec![item(f.item_ids[0], ItemState::Running, 10, 5.0)]));
        assert!(differ
            .diff(&snapshot(&f, 7, 2, vec![item(f.item_ids[0], ItemState::Running, 99, 5.0)]))
            .is_none());
    }

    #[test]
    fn a_removed_item_is_dropped_from_the_structure() {
        let f = fixture(2);
        let mut differ = Differ::default();
        differ.diff(&snapshot(&f, 1, 1, vec![
            item(f.item_ids[0], ItemState::Running, 10, 5.0),
            item(f.item_ids[1], ItemState::Running, 10, 5.0),
        ]));
        let delta = differ
            .diff(&snapshot(&f, 2, 2, vec![item(f.item_ids[0], ItemState::Running, 10, 5.0)]))
            .unwrap();
        assert_eq!(delta.structure.unwrap().packages[0].items.len(), 1);
        // The removed item's figures are forgotten: re-adding it later sends them again.
        let again = differ
            .diff(&snapshot(&f, 3, 3, vec![
                item(f.item_ids[0], ItemState::Running, 10, 5.0),
                item(f.item_ids[1], ItemState::Running, 10, 5.0),
            ]))
            .unwrap();
        assert!(again.items.iter().any(|i| i.id == f.item_ids[1].to_string()));
    }

    #[test]
    fn a_removed_package_is_dropped() {
        let f = fixture(1);
        let mut differ = Differ::default();
        differ.diff(&snapshot(&f, 1, 1, vec![item(f.item_ids[0], ItemState::Running, 10, 5.0)]));
        let empty = EngineSnapshot::new(vec![], 0.0, vec![], 2);
        let delta = differ.diff(&empty).expect("package removal is a change");
        assert!(delta.structure.unwrap().packages.is_empty());
    }

    #[test]
    fn full_always_returns_everything_even_for_an_empty_engine() {
        let mut differ = Differ::default();
        let delta = differ.full(&EngineSnapshot::new(vec![], 0.0, vec![], 1));
        assert!(delta.structure.is_some());
        assert!(delta.stats.is_some());
    }

    #[test]
    fn full_resets_what_was_sent() {
        let f = fixture(1);
        let snap = snapshot(&f, 1, 1, vec![item(f.item_ids[0], ItemState::Running, 10, 5.0)]);
        let mut differ = Differ::default();
        differ.diff(&snap);
        let delta = differ.full(&snap);
        assert_eq!(delta.items.len(), 1);
        assert!(differ.diff(&snap).is_none(), "after full the snapshot counts as sent");
    }

    #[test]
    fn reordering_packages_sends_the_structure() {
        let a = fixture(1);
        let b = fixture(1);
        let pa = PackageSnapshot::new(a.package_id, "A", Priority::Normal,
            vec![item(a.item_ids[0], ItemState::Running, 1, 1.0)], 0.0, 1);
        let pb = PackageSnapshot::new(b.package_id, "B", Priority::Normal,
            vec![item(b.item_ids[0], ItemState::Running, 1, 1.0)], 0.0, 1);
        let mut differ = Differ::default();
        differ.diff(&EngineSnapshot::new(vec![pa.clone(), pb.clone()], 0.0, vec![], 1));
        let delta = differ
            .diff(&EngineSnapshot::new(vec![pb, pa], 0.0, vec![], 2))
            .expect("order changed");
        let order: Vec<String> = delta.structure.unwrap().packages.iter().map(|p| p.name.clone()).collect();
        assert_eq!(order, ["B", "A"]);
    }
}
```

Add `mod differ;` to `lib.rs`.

- [ ] **Step 2: Run to verify it fails**

Run: `cd sdm-core && cargo test -p sdm-app differ 2>&1 | tail -10`
Expected: compile error, `Differ`/`Delta` undefined.

- [ ] **Step 3: Implement**

Above the tests:

```rust
//! Turns successive engine snapshots into deltas.
//!
//! It remembers what it last sent. A package whose `revision` matches the one
//! last sent is skipped without looking at its items (the engine bumps a
//! package's revision whenever anything inside it changed), which keeps an
//! idle or mostly idle list cheap.

use std::collections::{HashMap, HashSet};

use sdm_engine::EngineSnapshot;
use uuid::Uuid;

use crate::convert;
use crate::protocol::*;

pub(crate) struct Delta {
    pub structure: Option<Structure>,
    pub items: Vec<ItemFigures>,
    pub packages: Vec<PackageFigures>,
    pub stats: Option<EngineStats>,
}

#[derive(Default)]
pub(crate) struct Differ {
    structure: Vec<StructurePackage>,
    revisions: HashMap<Uuid, u64>,
    items: HashMap<String, ItemFigures>,
    packages: HashMap<String, PackageFigures>,
    stats: Option<EngineStats>,
}

impl Differ {
    /// Forgets everything sent, so the next diff is a full one.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    /// The change since the last call, or `None` when nothing changed.
    pub(crate) fn diff(&mut self, snapshot: &EngineSnapshot) -> Option<Delta> {
        self.diff_inner(snapshot, false)
    }

    /// Everything, regardless of what was sent before; afterwards the snapshot
    /// counts as sent.
    pub(crate) fn full(&mut self, snapshot: &EngineSnapshot) -> Delta {
        self.reset();
        self.diff_inner(snapshot, true)
            .expect("a forced diff always produces a delta")
    }

    fn diff_inner(&mut self, snapshot: &EngineSnapshot, force: bool) -> Option<Delta> {
        let old_order: Vec<String> = self.structure.iter().map(|p| p.id.clone()).collect();
        let mut old_by_id: HashMap<String, StructurePackage> =
            std::mem::take(&mut self.structure)
                .into_iter()
                .map(|package| (package.id.clone(), package))
                .collect();

        let mut new_structure = Vec::with_capacity(snapshot.packages.len());
        let mut items_out = Vec::new();
        let mut packages_out = Vec::new();
        let mut structure_changed = force;

        for package in &snapshot.packages {
            let package_id = package.id.to_string();
            let old = old_by_id.remove(&package_id);
            let revision_unchanged =
                !force && self.revisions.get(&package.id) == Some(&package.revision);

            let entry = match (revision_unchanged, old) {
                (true, Some(old)) => old,
                (_, old) => {
                    let fresh = convert::structure_package(package);
                    if old.as_ref() != Some(&fresh) {
                        structure_changed = true;
                    }
                    for item in &package.items {
                        let figures = convert::item_figures(item);
                        if self.items.get(&figures.id) != Some(&figures) {
                            self.items.insert(figures.id.clone(), figures.clone());
                            items_out.push(figures);
                        }
                    }
                    let figures = convert::package_figures(package);
                    if self.packages.get(&figures.id) != Some(&figures) {
                        self.packages.insert(figures.id.clone(), figures.clone());
                        packages_out.push(figures);
                    }
                    self.revisions.insert(package.id, package.revision);
                    fresh
                }
            };
            new_structure.push(entry);
        }

        if !old_by_id.is_empty() {
            structure_changed = true;
            for (id, removed) in &old_by_id {
                self.packages.remove(id);
                for item in &removed.items {
                    self.items.remove(&item.id);
                }
            }
            let live: HashSet<Uuid> = snapshot.packages.iter().map(|p| p.id).collect();
            self.revisions.retain(|id, _| live.contains(id));
        }
        let new_order: Vec<&String> = new_structure.iter().map(|p| &p.id).collect();
        if old_order.iter().ne(new_order.iter().copied()) {
            structure_changed = true;
        }
        if structure_changed {
            // Drop figures of items that left a package that stayed.
            let live_items: HashSet<&str> = new_structure
                .iter()
                .flat_map(|p| p.items.iter().map(|i| i.id.as_str()))
                .collect();
            self.items.retain(|id, _| live_items.contains(id.as_str()));
        }

        let stats = convert::engine_stats(snapshot);
        let stats_changed = force || self.stats.as_ref() != Some(&stats);
        if stats_changed {
            self.stats = Some(stats.clone());
        }

        let structure = structure_changed.then(|| Structure {
            packages: new_structure.clone(),
        });
        self.structure = new_structure;

        if structure.is_none() && items_out.is_empty() && packages_out.is_empty() && !stats_changed
        {
            return None;
        }
        Some(Delta {
            structure,
            items: items_out,
            packages: packages_out,
            stats: stats_changed.then_some(stats),
        })
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd sdm-core && cargo test -p sdm-app differ 2>&1 | tail -15 && cargo clippy -p sdm-app --all-targets 2>&1 | tail -5`
Expected: all 9 differ tests pass; no warnings. If `a_progress_change_sends_figures_only` fails because `PackageAggregate` makes `EngineStats`/package figures differ each call, check the assertion that `stats` may be present; the test only asserts `items`/`structure`, so a changed `stats` is allowed.

- [ ] **Step 5: Commit**

```bash
git add sdm-core/crates/sdm-app
git commit -m "Add the snapshot differ

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Host, sink and notifications

**Files:**

- Create: `sdm-core/crates/sdm-app/src/host.rs`, `src/notify.rs`
- Modify: `src/lib.rs`
- Test: unit tests in `notify.rs`

**Interfaces:**

- Produces (public):

```rust
pub trait Host: Send + Sync {
    fn data_dir(&self) -> PathBuf;
    fn default_download_dir(&self) -> PathBuf;
    fn reveal_in_file_manager(&self, path: &Path) -> Result<(), HostError>;
    fn notify(&self, notification: Notification) -> Result<(), HostError>;
}
pub struct HostError(pub String);
pub trait PatchSink: Send + Sync { fn send(&self, patch: Patch) -> Result<(), SinkClosed>; }
pub struct SinkClosed;
pub enum Notification { Finished { filename: String }, Failed { filename: String, reason: String } }
pub struct FakeHost { .. }        // records notifications; configurable dirs
pub struct RecordingSink { .. }   // records patches; `fail_next()` makes the next send fail
pub(crate) fn notifications_between(old: &EngineSnapshot, new: &EngineSnapshot) -> Vec<Notification>
```

- [ ] **Step 1: Write `host.rs`**

```rust
//! What `sdm-app` needs from its surroundings. The Tauri shell implements
//! these; tests use the fakes at the bottom.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::notify::Notification;
use crate::protocol::Patch;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct HostError(pub String);

pub trait Host: Send + Sync {
    /// Holds `state.json`, resume sidecars, `settings.json` and `bin/`.
    fn data_dir(&self) -> PathBuf;
    fn default_download_dir(&self) -> PathBuf;
    fn reveal_in_file_manager(&self, path: &Path) -> Result<(), HostError>;
    fn notify(&self, notification: Notification) -> Result<(), HostError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkClosed;

/// Where patches go. One ordered transport: `send` is called with a lock held
/// so patches reach the sink in `seq` order, and must not block.
pub trait PatchSink: Send + Sync {
    fn send(&self, patch: Patch) -> Result<(), SinkClosed>;
}

/// A [`Host`] for tests: fixed directories, records notifications.
pub struct FakeHost {
    data_dir: PathBuf,
    download_dir: PathBuf,
    notifications: Mutex<Vec<Notification>>,
}

impl FakeHost {
    pub fn new(data_dir: PathBuf, download_dir: PathBuf) -> Self {
        Self {
            data_dir,
            download_dir,
            notifications: Mutex::new(Vec::new()),
        }
    }

    pub fn notifications(&self) -> Vec<Notification> {
        self.notifications.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Host for FakeHost {
    fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }
    fn default_download_dir(&self) -> PathBuf {
        self.download_dir.clone()
    }
    fn reveal_in_file_manager(&self, _path: &Path) -> Result<(), HostError> {
        Ok(())
    }
    fn notify(&self, notification: Notification) -> Result<(), HostError> {
        self.notifications
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(notification);
        Ok(())
    }
}

/// A [`PatchSink`] for tests: keeps every patch; can be told to fail once.
#[derive(Default)]
pub struct RecordingSink {
    patches: Mutex<Vec<Patch>>,
    fail_next: Mutex<bool>,
}

impl RecordingSink {
    pub fn patches(&self) -> Vec<Patch> {
        self.patches.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn take(&self) -> Vec<Patch> {
        std::mem::take(&mut *self.patches.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn fail_next(&self) {
        *self.fail_next.lock().unwrap_or_else(|e| e.into_inner()) = true;
    }
}

impl PatchSink for RecordingSink {
    fn send(&self, patch: Patch) -> Result<(), SinkClosed> {
        let mut fail = self.fail_next.lock().unwrap_or_else(|e| e.into_inner());
        if std::mem::take(&mut *fail) {
            return Err(SinkClosed);
        }
        self.patches.lock().unwrap_or_else(|e| e.into_inner()).push(patch);
        Ok(())
    }
}
```

- [ ] **Step 2: Write the failing notification tests in `notify.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sdm_core::{ItemState, Priority, RangeSet, SourceUrl};
    use sdm_engine::{ItemSnapshot, PackageSnapshot};
    use uuid::Uuid;

    fn item(id: Uuid, name: &str, state: ItemState) -> ItemSnapshot {
        ItemSnapshot::new(id, SourceUrl::new("https://e.com/x").unwrap(), name, Some(10),
            RangeSet::new(), state, true, Some(true), 0, 1, 0.0, vec![])
    }

    fn package(id: Uuid, revision: u64, items: Vec<ItemSnapshot>) -> PackageSnapshot {
        PackageSnapshot::new(id, "P", Priority::Normal, items, 0.0, revision)
    }

    fn snapshot(generation: u64, packages: Vec<PackageSnapshot>) -> EngineSnapshot {
        EngineSnapshot::new(packages, 0.0, vec![], generation)
    }

    #[test]
    fn a_transition_to_completed_notifies_once() {
        let (p, i) = (Uuid::new_v4(), Uuid::new_v4());
        let old = snapshot(1, vec![package(p, 1, vec![item(i, "a.bin", ItemState::Running)])]);
        let new = snapshot(2, vec![package(p, 2, vec![item(i, "a.bin", ItemState::Completed)])]);
        assert_eq!(
            notifications_between(&old, &new),
            vec![Notification::Finished { filename: "a.bin".into() }]
        );
        // The same transition seen again (old == new) is not a transition.
        assert!(notifications_between(&new, &new).is_empty());
    }

    #[test]
    fn a_failure_carries_its_reason() {
        let (p, i) = (Uuid::new_v4(), Uuid::new_v4());
        let old = snapshot(1, vec![package(p, 1, vec![item(i, "a.bin", ItemState::Running)])]);
        let new = snapshot(2, vec![package(p, 2, vec![item(i, "a.bin",
            ItemState::Failed { reason: "404".into() })])]);
        assert_eq!(
            notifications_between(&old, &new),
            vec![Notification::Failed { filename: "a.bin".into(), reason: "404".into() }]
        );
    }

    #[test]
    fn an_unchanged_package_is_ignored_even_if_it_holds_a_completed_item() {
        let (p, i) = (Uuid::new_v4(), Uuid::new_v4());
        let old = snapshot(1, vec![package(p, 5, vec![item(i, "a.bin", ItemState::Completed)])]);
        let new = snapshot(2, vec![package(p, 5, vec![item(i, "a.bin", ItemState::Completed)])]);
        assert!(notifications_between(&old, &new).is_empty());
    }

    #[test]
    fn moving_a_finished_item_between_packages_does_not_read_as_finished() {
        let (p1, p2, i) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let old = snapshot(1, vec![
            package(p1, 1, vec![item(i, "a.bin", ItemState::Completed)]),
            package(p2, 1, vec![]),
        ]);
        let new = snapshot(2, vec![
            package(p1, 2, vec![]),
            package(p2, 2, vec![item(i, "a.bin", ItemState::Completed)]),
        ]);
        assert!(notifications_between(&old, &new).is_empty());
    }

    #[test]
    fn a_newly_added_item_already_in_a_terminal_state_notifies() {
        // Mirrors the SwiftUI app: an id missing from the old snapshot compares
        // unequal to any state.
        let (p, i) = (Uuid::new_v4(), Uuid::new_v4());
        let old = snapshot(1, vec![package(p, 1, vec![])]);
        let new = snapshot(2, vec![package(p, 2, vec![item(i, "a.bin", ItemState::Completed)])]);
        assert_eq!(notifications_between(&old, &new).len(), 1);
    }
}
```

- [ ] **Step 3: Run to verify it fails**

Add `pub mod host; pub mod notify;` to `lib.rs`, then run `cd sdm-core && cargo test -p sdm-app notify 2>&1 | tail -10`.
Expected: compile error in `notify.rs` (`Notification`, `notifications_between` undefined).

- [ ] **Step 4: Implement `notify.rs`**

Above the tests:

```rust
//! Which transitions deserve a notification. A port of the SwiftUI app's
//! `EngineController.notifyChanges`.

use std::collections::HashMap;

use sdm_core::ItemState;
use sdm_engine::EngineSnapshot;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notification {
    Finished { filename: String },
    Failed { filename: String, reason: String },
}

/// Notifications due for the step from `old` to `new`.
///
/// Only packages whose `revision` changed can hold a transition. Both sides are
/// collected (an item moved between packages changed its old package and its
/// new one), so the item's previous state is always found and a move never
/// reads as "finished".
pub(crate) fn notifications_between(old: &EngineSnapshot, new: &EngineSnapshot) -> Vec<Notification> {
    if old.generation == new.generation {
        return Vec::new();
    }
    let old_by_id: HashMap<Uuid, u64> = old.packages.iter().map(|p| (p.id, p.revision)).collect();
    let new_by_id: HashMap<Uuid, u64> = new.packages.iter().map(|p| (p.id, p.revision)).collect();

    let changed_new: Vec<_> = new
        .packages
        .iter()
        .filter(|p| old_by_id.get(&p.id) != Some(&p.revision))
        .collect();
    if changed_new.is_empty() {
        return Vec::new();
    }
    let old_states: HashMap<Uuid, &ItemState> = old
        .packages
        .iter()
        .filter(|p| new_by_id.get(&p.id) != Some(&p.revision))
        .flat_map(|p| p.items.iter().map(|i| (i.id, &i.state)))
        .collect();

    let mut out = Vec::new();
    for item in changed_new.iter().flat_map(|p| p.items.iter()) {
        if old_states.get(&item.id).copied() == Some(&item.state) {
            continue;
        }
        match &item.state {
            ItemState::Completed => out.push(Notification::Finished {
                filename: item.filename.clone(),
            }),
            ItemState::Failed { reason } => out.push(Notification::Failed {
                filename: item.filename.clone(),
                reason: reason.clone(),
            }),
            _ => {}
        }
    }
    out
}
```

- [ ] **Step 5: Run to verify, then commit**

Run: `cd sdm-core && cargo test -p sdm-app 2>&1 | tail -8 && cargo clippy -p sdm-app --all-targets 2>&1 | tail -5`
Expected: all pass. Check `an_unchanged_package...`: same revision on both sides → `changed_new` empty. Check the move test: `p1` and `p2` both changed on both sides, so `old_states` has `i`'s old state `Completed`, equal to the new → no notification.

```bash
git add sdm-core/crates/sdm-app
git commit -m "Add Host, PatchSink, fakes and notification detection

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 9: URL input and settings

**Files:**

- Create: `sdm-core/crates/sdm-app/src/url_input.rs`, `src/settings.rs`, `src/theme.rs`
- Modify: `src/lib.rs`
- Test: unit tests in each file

**Interfaces:**

- Produces (crate-private unless noted):
  - `parse_download_url(input: &str) -> Result<SourceUrl, AppError>`: trims, requires `http`/`https` and a host.
  - `derive_filename(url: &SourceUrl) -> String`: a safe, never-empty file name.
  - `pub struct SettingsStore`: `SettingsStore::load(path: PathBuf, default_folder: PathBuf) -> SettingsStore`, `get(&self) -> SettingsDto`, `update(&mut self, dto: SettingsDto) -> Result<(), AppError>` (validates, writes JSON atomically only on success), `download_folder(&self) -> PathBuf`.
  - `pub fn theme_css(theme: &sdm_core::Theme) -> String`.

- [ ] **Step 1: Write failing URL tests**

`url_input.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_and_https_and_trims() {
        assert!(parse_download_url("  https://example.com/a.zip \n").is_ok());
        assert!(parse_download_url("http://example.com/a.zip").is_ok());
    }

    #[test]
    fn rejects_everything_else_with_a_clear_error() {
        for bad in ["", "   ", "not a url", "ftp://example.com/a", "file:///etc/passwd",
                    "javascript:alert(1)", "https://", "magnet:?xt=urn:btih:abc"] {
            let error = parse_download_url(bad).expect_err(bad);
            assert!(matches!(error, AppError::InvalidArgument { .. }), "{bad}: {error:?}");
        }
    }

    #[test]
    fn filenames_come_from_the_last_path_segment_decoded() {
        let cases = [
            ("https://example.com/files/report%20final.pdf", "report final.pdf"),
            ("https://example.com/a/b/c.tar.gz?token=1#frag", "c.tar.gz"),
            ("https://example.com/%E6%97%A5%E6%9C%AC%E8%AA%9E.txt", "日本語.txt"),
        ];
        for (url, expected) in cases {
            let url = parse_download_url(url).unwrap();
            assert_eq!(derive_filename(&url), expected);
        }
    }

    #[test]
    fn unusable_names_fall_back_and_never_escape() {
        for url in ["https://example.com/", "https://example.com", "https://example.com/?q=1",
                    "https://example.com/%2e%2e", "https://example.com/a/..", "https://example.com/%00"] {
            let name = derive_filename(&parse_download_url(url).unwrap());
            assert!(!name.is_empty(), "{url}");
            assert!(!name.contains('/') && !name.contains('\\') && !name.contains('\0'), "{url}: {name:?}");
            assert_ne!(name, ".");
            assert_ne!(name, "..");
        }
    }

    #[test]
    fn a_windows_reserved_name_is_made_safe_on_every_os() {
        let url = parse_download_url("https://example.com/CON.txt").unwrap();
        let name = derive_filename(&url);
        assert_eq!(name, "_CON.txt");
    }
}
```

- [ ] **Step 2: Run to verify it fails, then implement `url_input.rs`**

Add `mod url_input;` to `lib.rs`. Run `cargo test -p sdm-app url_input` — expected compile error. Implementation above the tests:

```rust
//! Turning what a person typed into a download the engine can take.

use percent_encoding::percent_decode_str;
use sdm_core::{SourceUrl, safe_path_component, windows_safe_name};

use crate::error::AppError;

/// Trims `input` and accepts only `http`/`https` URLs that have a host.
pub(crate) fn parse_download_url(input: &str) -> Result<SourceUrl, AppError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid("url", "the address is empty"));
    }
    let url = SourceUrl::new(trimmed)
        .map_err(|_| AppError::invalid("url", "that is not a valid web address"))?;
    let parsed = url
        .as_url()
        .ok_or_else(|| AppError::invalid("url", "that is not a valid web address"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::invalid(
            "url",
            "only http and https addresses can be downloaded",
        ));
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err(AppError::invalid("url", "the address has no host"));
    }
    Ok(url)
}

/// A file name for the download: the last non-empty path segment, percent
/// decoded, reduced to a single safe path component; `download` when none is
/// usable. Windows' reserved-name rule is applied on every OS so a download
/// added on a Mac can be moved to a Windows disk.
pub(crate) fn derive_filename(url: &SourceUrl) -> String {
    let segment = url
        .as_url()
        .and_then(|parsed| {
            parsed
                .path_segments()
                .and_then(|mut segments| segments.rfind(|s| !s.is_empty()))
        })
        .unwrap_or("");
    let decoded = percent_decode_str(segment).decode_utf8_lossy().into_owned();
    let component = safe_path_component(&decoded);
    // `windows_safe_name` is what Windows will apply anyway; running it here
    // too keeps names stable across machines (and handles `CON.txt`).
    windows_safe_name(&component).unwrap_or_else(|| "download".to_owned())
}
```

Run again: `cargo test -p sdm-app url_input` → 5 tests pass. If `https://example.com/%2e%2e` yields `..` (decoded) and `safe_path_component` returns the fallback `download`, the test passes; if not, fix in `derive_filename`.

- [ ] **Step 3: Write failing settings tests**

`settings.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &tempfile::TempDir) -> SettingsStore {
        SettingsStore::load(dir.path().join("settings.json"), dir.path().join("Downloads"))
    }

    #[test]
    fn a_missing_file_gives_the_default_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        assert_eq!(store.download_folder(), dir.path().join("Downloads"));
    }

    #[test]
    fn a_valid_update_is_persisted_and_reloaded() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("elsewhere");
        let mut s = store(&dir);
        s.update(SettingsDto { download_folder: target.to_string_lossy().into_owned() }).unwrap();
        assert!(target.is_dir(), "the folder is created");
        let reloaded = store(&dir);
        assert_eq!(reloaded.download_folder(), target);
    }

    #[test]
    fn invalid_updates_are_rejected_and_change_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(&dir);
        let file = dir.path().join("a-file");
        std::fs::write(&file, b"x").unwrap();
        for bad in ["", "   ", "relative/path", file.to_str().unwrap()] {
            let error = s.update(SettingsDto { download_folder: bad.to_owned() }).expect_err(bad);
            assert!(matches!(error, AppError::InvalidArgument { .. }), "{bad}: {error:?}");
        }
        assert_eq!(s.download_folder(), dir.path().join("Downloads"), "old setting still in force");
    }

    #[test]
    fn a_corrupt_file_falls_back_to_the_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), b"{not json").unwrap();
        assert_eq!(store(&dir).download_folder(), dir.path().join("Downloads"));
    }
}
```

- [ ] **Step 4: Implement `settings.rs`**

Add `pub mod settings;` to `lib.rs` (public: `SettingsStore` is used by `App`; `SettingsDto` lives in `protocol`). Above the tests:

```rust
//! The app's own settings, in `settings.json` inside the data directory. The
//! SwiftUI app keeps its settings in `UserDefaults`; the two apps share only
//! the download state.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::protocol::SettingsDto;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    download_folder: PathBuf,
}

pub struct SettingsStore {
    path: PathBuf,
    current: Stored,
}

impl SettingsStore {
    /// Reads `path`; a missing or unreadable file gives the defaults.
    pub fn load(path: PathBuf, default_folder: PathBuf) -> Self {
        let current = std::fs::read(&path)
            .ok()
            .and_then(|data| serde_json::from_slice::<Stored>(&data).ok())
            .filter(|stored| stored.download_folder.is_absolute())
            .unwrap_or(Stored {
                download_folder: default_folder,
            });
        Self { path, current }
    }

    pub fn download_folder(&self) -> PathBuf {
        self.current.download_folder.clone()
    }

    pub fn get(&self) -> SettingsDto {
        SettingsDto {
            download_folder: self.current.download_folder.to_string_lossy().into_owned(),
        }
    }

    /// Validates, applies and persists. On any error nothing changes.
    pub fn update(&mut self, dto: SettingsDto) -> Result<(), AppError> {
        let folder = validate_folder(&dto.download_folder)?;
        let next = Stored {
            download_folder: folder,
        };
        self.write(&next)?;
        self.current = next;
        Ok(())
    }

    fn write(&self, stored: &Stored) -> Result<(), AppError> {
        let data = serde_json::to_vec_pretty(stored)
            .map_err(|e| AppError::failed(format!("could not encode the settings: {e}")))?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::failed(format!("could not create {}: {e}", parent.display())))?;
        }
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, data)
            .and_then(|()| std::fs::rename(&temporary, &self.path))
            .map_err(|e| AppError::failed(format!("could not save the settings: {e}")))
    }
}

/// The folder must be a non-empty absolute path that exists (or can be
/// created) and accepts a new file.
fn validate_folder(text: &str) -> Result<PathBuf, AppError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid("downloadFolder", "the folder is empty"));
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(AppError::invalid("downloadFolder", "the folder must be an absolute path"));
    }
    std::fs::create_dir_all(&path)
        .map_err(|e| AppError::invalid("downloadFolder", format!("cannot use that folder: {e}")))?;
    if !path.is_dir() {
        return Err(AppError::invalid("downloadFolder", "that path is not a folder"));
    }
    probe_writable(&path)?;
    Ok(path)
}

fn probe_writable(folder: &Path) -> Result<(), AppError> {
    let probe = folder.join(format!(".sdm-write-test-{}", std::process::id()));
    std::fs::write(&probe, b"")
        .map_err(|e| AppError::invalid("downloadFolder", format!("that folder is not writable: {e}")))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}
```

Run `cargo test -p sdm-app settings` → 4 tests pass. (`create_dir_all` on a path that is an existing _file_ fails with an error, so the "is a file" case is rejected there.)

- [ ] **Step 5: Theme CSS test and implementation**

`theme.rs`:

```rust
//! The bundled themes as CSS custom properties, so the frontend never holds a
//! color literal: every color in a component is `var(--role)`.

use sdm_core::Theme;

/// A color value that is safe inside a declaration: `#rgb`, `#rgba`, `#rrggbb`,
/// `#rrggbbaa`, or an `rgb(...)`/`rgba(...)` of digits, commas, dots, percent
/// signs and spaces. Anything else becomes `transparent`, so a bad theme file
/// can never close the declaration or add another one.
fn css_color(value: &str) -> String {
    let v = value.trim();
    let hex = v.strip_prefix('#').is_some_and(|h| {
        matches!(h.len(), 3 | 4 | 6 | 8) && h.chars().all(|c| c.is_ascii_hexdigit())
    });
    let function = ["rgb(", "rgba("].iter().any(|p| v.starts_with(p))
        && v.ends_with(')')
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '(' | ')' | ',' | '.' | '%' | ' '));
    if hex || function { v.to_owned() } else { "transparent".to_owned() }
}

/// `:root { --surface-primary: #...; ... }` for `theme`, plus `color-scheme`.
pub fn theme_css(theme: &Theme) -> String {
    let roles: [(&str, &str); 15] = [
        ("surface-primary", &theme.surface_primary),
        ("surface-secondary", &theme.surface_secondary),
        ("surface-tertiary", &theme.surface_tertiary),
        ("text-primary", &theme.text_primary),
        ("text-secondary", &theme.text_secondary),
        ("text-tertiary", &theme.text_tertiary),
        ("accent", &theme.accent),
        ("border", &theme.border),
        ("status-online", &theme.status_online),
        ("status-faulty", &theme.status_faulty),
        ("status-offline", &theme.status_offline),
        ("status-failed", &theme.status_failed),
        ("progress-fill", &theme.progress_fill),
        ("completed-segment-fill", &theme.completed_segment_fill),
        ("active-head-tint", &theme.active_head_tint),
    ];
    let extra: [(&str, &str); 4] = [
        ("graph-stroke", &theme.graph_stroke),
        ("graph-average-stroke", &theme.graph_average_stroke),
        ("sidebar-background", &theme.sidebar_background),
        ("selection-tint", &theme.selection_tint),
    ];
    let mut css = String::from(":root {\n");
    for (name, value) in roles.iter().chain(extra.iter()) {
        css.push_str(&format!("  --{name}: {};\n", css_color(value)));
    }
    css.push_str(&format!(
        "  color-scheme: {};\n}}\n",
        if theme.is_dark { "dark" } else { "light" }
    ));
    css
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdm_core::ThemeCatalog;

    #[test]
    fn every_bundled_theme_produces_all_roles() {
        for theme in ThemeCatalog::built_in_themes() {
            let css = theme_css(&theme);
            for role in ["surface-primary", "text-primary", "accent", "progress-fill",
                         "completed-segment-fill", "selection-tint"] {
                assert!(css.contains(&format!("--{role}: #")), "{}: missing {role}", theme.id);
            }
            assert!(css.contains("color-scheme:"));
        }
    }

    #[test]
    fn a_hostile_value_cannot_close_the_declaration() {
        let mut theme = ThemeCatalog::built_in_themes().remove(0);
        theme.accent = "#fff; } body { display:none".into();
        let css = theme_css(&theme);
        assert_eq!(css.matches('}').count(), 1, "{css}");
        assert!(!css.contains("display"), "{css}");
        assert!(css.contains("--accent: transparent;"), "{css}");
    }
}
```

The array is sized 15 but lists 15 entries; the `extra` array has 4. (`graph_*`, `sidebar_background`, `selection_tint` are not used by the skeleton but are emitted for later specs.) Check `ThemeCatalog` and `Theme` are re-exported at `sdm_core`'s root; if not, import from their modules.

Add `pub mod theme; pub use theme::theme_css;` to `lib.rs`. Run `cargo test -p sdm-app theme` → pass.

- [ ] **Step 6: Commit**

```bash
cd sdm-core && cargo test -p sdm-app 2>&1 | tail -6 && cargo clippy -p sdm-app --all-targets 2>&1 | tail -4 && cd ..
git add sdm-core/crates/sdm-app
git commit -m "Add URL input, settings store and theme CSS generation

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 10: `App`: publish, tick and commands

**Files:**

- Create: `sdm-core/crates/sdm-app/src/app.rs`, `sdm-core/crates/sdm-app/tests/app_tests.rs`, `tests/support/mod.rs`
- Modify: `src/lib.rs`
- Test: `tests/app_tests.rs`

**Interfaces:**

- Consumes: Tasks 6–9.
- Produces (public):

```rust
pub struct Dependencies {
    pub transport: Arc<dyn HttpTransport>,
    pub state_store: Arc<dyn StateStore>,
    pub trash: Arc<dyn Trash>,
    pub resolver: Option<Arc<dyn LinkResolver>>,
    pub muxer: Option<Arc<dyn Muxer>>,
    pub wholesale_downloader: Option<Arc<dyn WholesaleDownloader>>,
    pub retry_policy: RetryPolicy,
    pub binaries: Option<sdm_resolve::ManagedBinaries>,
}
impl App {
    pub fn new(host: Arc<dyn Host>, dependencies: Dependencies) -> Result<Arc<App>, AppError>;
    pub async fn restore(&self) -> Result<(), AppError>;
    pub async fn attach(&self, sink: Arc<dyn PatchSink>) -> Result<(), AppError>;
    pub async fn tick(&self) -> Result<(), AppError>;
    pub async fn add_url(&self, url: &str, package_name: Option<&str>) -> Result<(), AppError>;
    pub async fn start_item(&self, id: &str) -> Result<(), AppError>;
    pub async fn stop_item(&self, id: &str) -> Result<(), AppError>;
    pub async fn retry_item(&self, id: &str) -> Result<(), AppError>;
    pub async fn remove_item(&self, id: &str, delete_file: bool) -> Result<(), AppError>;
    pub async fn pause_all(&self) -> Result<(), AppError>;
    pub async fn resume_all(&self) -> Result<(), AppError>;
    pub async fn set_list_visible(&self, visible: bool) -> Result<(), AppError>;
    pub async fn set_detail_items(&self, owner: &str, ids: &[String]) -> Result<(), AppError>;
    pub async fn full_state(&self) -> Result<(), AppError>;
    pub fn get_settings(&self) -> SettingsDto;
    pub async fn update_settings(&self, dto: SettingsDto) -> Result<(), AppError>;
    pub fn binaries_status(&self) -> BinariesStatusDto;
    pub fn theme_css(&self, theme_id: Option<&str>) -> Result<String, AppError>;
    pub async fn guarded<T, F, Fut>(self: &Arc<Self>, work: F) -> Result<T, AppError>;
}
```

- [ ] **Step 1: Write the test support**

`tests/support/mod.rs`:

```rust
use std::sync::Arc;

use sdm_app::host::{FakeHost, RecordingSink};
use sdm_app::{App, Dependencies};
use sdm_engine::{FakeOrigin, InMemoryStateStore, RetryPolicy, SystemTrash};

pub struct Harness {
    pub app: Arc<App>,
    pub sink: Arc<RecordingSink>,
    pub host: Arc<FakeHost>,
    pub downloads: tempfile::TempDir,
    pub data: tempfile::TempDir,
}

pub fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i.wrapping_mul(31).wrapping_add(7) % 251) as u8).collect()
}

pub async fn harness(origin: FakeOrigin) -> Harness {
    let downloads = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let host = Arc::new(FakeHost::new(data.path().to_path_buf(), downloads.path().to_path_buf()));
    let dependencies = Dependencies {
        transport: Arc::new(origin),
        state_store: Arc::new(InMemoryStateStore::new()),
        trash: Arc::new(SystemTrash),
        resolver: None,
        muxer: None,
        wholesale_downloader: None,
        retry_policy: RetryPolicy::new(),
        binaries: None,
    };
    let app = App::new(host.clone(), dependencies).expect("app");
    let sink = Arc::new(RecordingSink::default());
    app.attach(sink.clone()).await.expect("attach");
    Harness { app, sink, host, downloads, data }
}

/// Ticks until `condition` holds or `max_ticks` passed. There is no clock:
/// each iteration is one engine tick plus a yield to let download tasks run.
pub async fn tick_until(app: &App, max_ticks: usize, mut condition: impl FnMut() -> bool) -> bool {
    for _ in 0..max_ticks {
        app.tick().await.unwrap();
        if condition() {
            return true;
        }
        tokio::task::yield_now().await;
    }
    false
}
```

`SystemTrash` removes files to the real Trash; in these tests, `remove_item(.., delete_file=false)` is used where files matter, and the downloads live in a tempdir anyway. (If `InMemoryStateStore::new()` has a different constructor, use the one `sdm-engine/tests/support/engine_support.rs` uses.)

- [ ] **Step 2: Write the failing tests**

`tests/app_tests.rs`:

```rust
mod support;

use sdm_app::protocol::{ItemStateDto, Patch};
use sdm_app::AppError;
use sdm_engine::FakeOrigin;
use support::*;

fn last_structure_states(patches: &[Patch]) -> Vec<ItemStateDto> {
    patches
        .iter()
        .rev()
        .find_map(|p| p.structure.as_ref())
        .map(|s| s.packages.iter().flat_map(|p| p.items.iter().map(|i| i.state.clone())).collect())
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn attaching_sends_one_full_patch() {
    let h = harness(FakeOrigin::new(payload(10))).await;
    let patches = h.sink.take();
    assert_eq!(patches.len(), 1);
    assert!(patches[0].full);
    assert!(patches[0].structure.is_some());
    assert!(patches[0].stats.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_idle_engine_emits_nothing() {
    let h = harness(FakeOrigin::new(payload(10))).await;
    h.sink.take();
    for _ in 0..10 {
        h.app.tick().await.unwrap();
    }
    assert!(h.sink.take().is_empty(), "an idle tick must not produce a patch");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn adding_a_url_downloads_it_and_reports_completion() {
    let body = payload(5000);
    let h = harness(FakeOrigin::new(body.clone())).await;
    h.sink.take();
    h.app.add_url("https://example.com/files/data.bin", None).await.unwrap();

    let done = tick_until(&h.app, 400, || {
        last_structure_states(&h.sink.patches()).iter().any(|s| *s == ItemStateDto::Completed)
    })
    .await;
    assert!(done, "the item never completed");

    let file = h.downloads.path().join("data.bin").join("data.bin");
    let on_disk = std::fs::read(&file)
        .or_else(|_| std::fs::read(h.downloads.path().join("data.bin")))
        .expect("the downloaded file exists in the package folder");
    assert_eq!(on_disk, body);
    assert!(h.host.notifications().iter().any(|n| matches!(n,
        sdm_app::notify::Notification::Finished { filename } if filename == "data.bin")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn seq_increases_by_exactly_one_per_patch() {
    let h = harness(FakeOrigin::new(payload(3000))).await;
    h.app.add_url("https://example.com/a.bin", None).await.unwrap();
    tick_until(&h.app, 400, || {
        last_structure_states(&h.sink.patches()).iter().any(|s| *s == ItemStateDto::Completed)
    })
    .await;
    let seqs: Vec<u32> = h.sink.patches().iter().map(|p| p.seq).collect();
    for pair in seqs.windows(2) {
        assert_eq!(pair[1], pair[0].wrapping_add(1), "{seqs:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn full_state_equals_the_patches_applied_so_far() {
    let h = harness(FakeOrigin::new(payload(4000))).await;
    h.app.add_url("https://example.com/a.bin", None).await.unwrap();
    for _ in 0..5 {
        h.app.tick().await.unwrap();
    }
    h.sink.take();
    h.app.full_state().await.unwrap();
    let full = h.sink.take().remove(0);
    assert!(full.full);
    let structure = full.structure.expect("structure in a full patch");
    assert_eq!(structure.packages.len(), 1);
    assert_eq!(full.items.len(), structure.packages[0].items.len());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_send_makes_the_next_patch_full() {
    let h = harness(FakeOrigin::new(payload(2000))).await;
    h.sink.take();
    h.sink.fail_next();
    h.app.add_url("https://example.com/a.bin", None).await.unwrap(); // this patch is lost
    // The sink was dropped on failure; the shell re-attaches (a reloaded window).
    h.app.attach(h.sink.clone()).await.unwrap();
    let first = h.sink.take().remove(0);
    assert!(first.full, "after a lost patch the next one must be full");
    assert!(first.structure.unwrap().packages.iter().any(|p| !p.items.is_empty()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hidden_list_emits_nothing_and_showing_it_sends_a_full_patch() {
    let h = harness(FakeOrigin::new(payload(2000))).await;
    h.sink.take();
    h.app.set_list_visible(false).await.unwrap();
    h.app.add_url("https://example.com/a.bin", None).await.unwrap();
    for _ in 0..5 {
        h.app.tick().await.unwrap();
    }
    assert!(h.sink.take().is_empty(), "no patches while the list is hidden");
    h.app.set_list_visible(true).await.unwrap();
    let patches = h.sink.take();
    assert_eq!(patches.len(), 1);
    assert!(patches[0].full);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn detail_histories_are_sent_only_for_reported_items_and_ignore_bad_ids() {
    let h = harness(FakeOrigin::new(payload(200_000))).await;
    h.app.add_url("https://example.com/big.bin", None).await.unwrap();
    for _ in 0..3 {
        h.app.tick().await.unwrap();
    }
    let id = h.sink.patches().iter().rev().find_map(|p| p.structure.as_ref())
        .map(|s| s.packages[0].items[0].id.clone()).expect("an item exists");

    let error = h.app.set_detail_items("w1", &["not-a-uuid".to_owned()]).await.expect_err("bad id");
    assert!(matches!(error, AppError::InvalidArgument { .. }));

    h.app.set_detail_items("w1", &[id.clone()]).await.unwrap();
    h.sink.take();
    h.app.tick().await.unwrap();
    let with_history: Vec<_> = h.sink.take().into_iter().flat_map(|p| p.histories).collect();
    assert!(with_history.iter().all(|history| history.id == id));

    h.app.set_detail_items("w1", &[]).await.unwrap();
    h.sink.take();
    h.app.tick().await.unwrap();
    assert!(h.sink.take().iter().all(|p| p.histories.is_empty()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn commands_reject_bad_ids_and_urls_without_panicking() {
    let h = harness(FakeOrigin::new(payload(10))).await;
    for bad in ["", "zzz", "00000000-0000-0000-0000-000000000000"] {
        // A well-formed but unknown id is not an error: the engine ignores it.
        let result = h.app.stop_item(bad).await;
        if bad == "00000000-0000-0000-0000-000000000000" {
            assert!(result.is_ok());
        } else {
            assert!(matches!(result, Err(AppError::InvalidArgument { .. })), "{bad}");
        }
    }
    for url in ["", "nope", "ftp://x/y"] {
        assert!(matches!(h.app.add_url(url, None).await, Err(AppError::InvalidArgument { .. })), "{url}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn settings_updates_apply_to_the_engine_and_bad_ones_are_rejected() {
    let h = harness(FakeOrigin::new(payload(1500))).await;
    let new_folder = h.data.path().join("new-downloads");
    h.app
        .update_settings(sdm_app::protocol::SettingsDto { download_folder: new_folder.to_string_lossy().into_owned() })
        .await
        .unwrap();
    assert_eq!(h.app.get_settings().download_folder, new_folder.to_string_lossy());

    let before = h.app.get_settings();
    let bad = h.app.update_settings(sdm_app::protocol::SettingsDto { download_folder: "".into() }).await;
    assert!(bad.is_err());
    assert_eq!(h.app.get_settings(), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_panic_inside_guarded_work_becomes_an_internal_error() {
    let h = harness(FakeOrigin::new(payload(10))).await;
    let result: Result<(), AppError> = h.app.guarded(|_| async { panic!("boom") }).await;
    assert!(matches!(result, Err(AppError::Internal { message }) if message.contains("boom")));
}
```

- [ ] **Step 3: Run to verify they fail**

Add `pub mod app; pub use app::{App, Dependencies};` to `lib.rs` (and `pub mod host` etc. already). Run `cd sdm-core && cargo test -p sdm-app --test app_tests 2>&1 | tail -15`.
Expected: compile errors (`App` undefined).

- [ ] **Step 4: Implement `app.rs`**

```rust
//! The controller: owns the engine, turns snapshots into patches, and runs
//! user actions. Port of `EngineController` minus everything view-specific.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};

use sdm_core::{
    AppTiming, DownloadItem, DownloadPackage, LinkResolver, ThemeCatalog, WholesaleDownloader,
};
use sdm_engine::{
    DownloadEngine, EngineSettings, EngineSnapshot, HttpTransport, Muxer, RetryPolicy,
    StateStore, Trash,
};
use sdm_resolve::ManagedBinaries;
use uuid::Uuid;

use crate::differ::{Delta, Differ};
use crate::error::AppError;
use crate::host::{Host, PatchSink};
use crate::notify::notifications_between;
use crate::protocol::*;
use crate::settings::SettingsStore;
use crate::theme::theme_css;
use crate::url_input::{derive_filename, parse_download_url};

/// The engine's collaborators. `production()` (in `toolchain.rs`) builds the
/// real ones; tests pass fakes.
pub struct Dependencies {
    pub transport: Arc<dyn HttpTransport>,
    pub state_store: Arc<dyn StateStore>,
    pub trash: Arc<dyn Trash>,
    pub resolver: Option<Arc<dyn LinkResolver>>,
    pub muxer: Option<Arc<dyn Muxer>>,
    pub wholesale_downloader: Option<Arc<dyn WholesaleDownloader>>,
    pub retry_policy: RetryPolicy,
    pub binaries: Option<ManagedBinaries>,
}

/// Engine defaults the SwiftUI app uses (`EngineSettingsStore`): 3 concurrent
/// downloads, 8 segments, 32 connections, 8 per host, 10 MB minimum segment.
fn engine_settings(folder: std::path::PathBuf) -> EngineSettings {
    EngineSettings::new(3, 8, 32, folder)
}

struct PublishState {
    seq: u32,
    differ: Differ,
    last_generation: Option<u64>,
    previous: Option<Arc<EngineSnapshot>>,
    visible: bool,
    need_full: bool,
    detail_items: HashMap<String, HashSet<Uuid>>,
    sink: Option<Arc<dyn PatchSink>>,
    last_tick_error: Option<String>,
    ticks: u64,
}

pub struct App {
    engine: Arc<DownloadEngine>,
    host: Arc<dyn Host>,
    settings: Mutex<SettingsStore>,
    binaries: Option<ManagedBinaries>,
    state: Mutex<PublishState>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn parse_id(text: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(text.trim()).map_err(|_| AppError::invalid("id", "that is not a valid item id"))
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a panic with a non-text payload".to_owned()
    }
}

impl App {
    pub fn new(host: Arc<dyn Host>, dependencies: Dependencies) -> Result<Arc<Self>, AppError> {
        let data_dir = host.data_dir();
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| AppError::failed(format!("cannot create {}: {e}", data_dir.display())))?;
        let settings = SettingsStore::load(
            data_dir.join("settings.json"),
            host.default_download_dir(),
        );
        let engine = DownloadEngine::new(
            dependencies.transport,
            dependencies.state_store,
            engine_settings(settings.download_folder()),
            dependencies.retry_policy,
            dependencies.resolver,
            dependencies.muxer,
            dependencies.wholesale_downloader,
        )
        .with_trash(dependencies.trash);
        Ok(Arc::new(Self {
            engine: Arc::new(engine),
            host,
            settings: Mutex::new(settings),
            binaries: dependencies.binaries,
            state: Mutex::new(PublishState {
                seq: 0,
                differ: Differ::default(),
                last_generation: None,
                previous: None,
                visible: true,
                need_full: true,
                detail_items: HashMap::new(),
                sink: None,
                last_tick_error: None,
                ticks: 0,
            }),
        }))
    }

    // MARK: Lifecycle

    /// Loads durable state. Call once, before the first `tick`.
    pub async fn restore(&self) -> Result<(), AppError> {
        self.engine.restore().await;
        self.publish(false).await
    }

    /// Starts sending patches to `sink` (replacing any previous one) and sends
    /// a full patch first.
    pub async fn attach(&self, sink: Arc<dyn PatchSink>) -> Result<(), AppError> {
        {
            let mut state = lock(&self.state);
            state.sink = Some(sink);
            state.need_full = true;
        }
        self.publish(true).await
    }

    /// One heartbeat: advances the engine, then publishes. The shell calls this
    /// `AppTiming::TICKS_PER_SECOND` times a second; tests call it directly.
    pub async fn tick(&self) -> Result<(), AppError> {
        self.engine.tick().await;
        let run_binaries_tick = {
            let mut state = lock(&self.state);
            state.ticks += 1;
            state.ticks % AppTiming::TICKS_PER_SECOND as u64 == 0
        };
        if run_binaries_tick && let Some(binaries) = &self.binaries {
            binaries.tick().await;
        }
        self.publish(false).await
    }

    /// `tick`, with a failure logged once per distinct message instead of on
    /// every tick (the heartbeat must keep running).
    pub async fn heartbeat(&self) {
        let result = self.tick().await;
        let mut state = lock(&self.state);
        match result {
            Ok(()) => state.last_tick_error = None,
            Err(error) => {
                let message = error.to_string();
                if state.last_tick_error.as_deref() != Some(message.as_str()) {
                    tracing::error!("heartbeat tick failed: {message}");
                    state.last_tick_error = Some(message);
                }
            }
        }
    }

    /// Runs `work` on its own task so a panic becomes `AppError::Internal`
    /// instead of killing the caller (the Tauri command task would hang).
    pub async fn guarded<T, F, Fut>(self: &Arc<Self>, work: F) -> Result<T, AppError>
    where
        F: FnOnce(Arc<App>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, AppError>> + Send + 'static,
        T: Send + 'static,
    {
        let app = Arc::clone(self);
        match tokio::spawn(async move { work(app).await }).await {
            Ok(result) => result,
            Err(error) if error.is_panic() => Err(AppError::Internal {
                message: panic_message(error.into_panic()),
            }),
            Err(_) => Err(AppError::Internal {
                message: "the task was cancelled".to_owned(),
            }),
        }
    }

    // MARK: Publishing

    /// Fetches the snapshot if it changed, handles notifications, builds a
    /// patch and sends it. `force_full` is an explicit request for a full patch.
    async fn publish(&self, force_full: bool) -> Result<(), AppError> {
        let (since, histories_wanted) = {
            let state = lock(&self.state);
            let since = if force_full || state.need_full { None } else { state.last_generation };
            let ids: HashSet<Uuid> = state.detail_items.values().flatten().copied().collect();
            (since, if state.visible || force_full { ids } else { HashSet::new() })
        };
        // No lock is held across these awaits.
        let changed = self.engine.snapshot_if_changed(since).await;
        let histories = if histories_wanted.is_empty() {
            HashMap::new()
        } else {
            self.engine.speed_histories(&histories_wanted).await
        };

        let mut due = Vec::new();
        {
            let mut state = lock(&self.state);
            let mut delta: Option<Delta> = None;
            let mut full = false;
            let wants_full = force_full || state.need_full;
            let emit_allowed = state.sink.is_some() && (state.visible || force_full);

            if let Some(snapshot) = changed {
                let stale = state.last_generation.is_some_and(|g| snapshot.generation < g);
                if !stale {
                    if let Some(previous) = &state.previous {
                        due = notifications_between(previous, &snapshot);
                    }
                    state.last_generation = Some(snapshot.generation);
                    if emit_allowed {
                        if wants_full {
                            delta = Some(state.differ.full(&snapshot));
                            full = true;
                        } else {
                            delta = state.differ.diff(&snapshot);
                        }
                    } else {
                        // Not sending: whatever the differ knew is now stale.
                        state.need_full = true;
                    }
                    state.previous = Some(Arc::new(snapshot));
                }
            }

            if emit_allowed {
                let history_items: Vec<ItemHistory> = histories
                    .into_iter()
                    .map(|(id, speed_history)| ItemHistory { id: id.to_string(), speed_history })
                    .collect();
                if delta.is_some() || !history_items.is_empty() {
                    let delta = delta.unwrap_or(Delta {
                        structure: None,
                        items: Vec::new(),
                        packages: Vec::new(),
                        stats: None,
                    });
                    state.seq = state.seq.wrapping_add(1);
                    let patch = Patch {
                        seq: state.seq,
                        full,
                        structure: delta.structure,
                        items: delta.items,
                        packages: delta.packages,
                        stats: delta.stats,
                        histories: history_items,
                    };
                    let sent = match &state.sink {
                        Some(sink) => sink.send(patch).is_ok(),
                        None => false,
                    };
                    if sent {
                        if full {
                            state.need_full = false;
                        }
                    } else {
                        // The receiver may have missed it: forget the sink and
                        // make the next patch (after a re-attach) a full one.
                        state.sink = None;
                        state.need_full = true;
                        state.differ.reset();
                    }
                }
            }
        }
        for notification in due {
            if let Err(error) = self.host.notify(notification) {
                tracing::warn!("could not show a notification: {error}");
            }
        }
        Ok(())
    }

    // MARK: Commands

    pub async fn add_url(&self, url: &str, package_name: Option<&str>) -> Result<(), AppError> {
        let source = parse_download_url(url)?;
        let filename = derive_filename(&source);
        let name = match package_name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(name) => sdm_core::safe_path_component(name),
            None => filename.clone(),
        };
        let package = DownloadPackage::new(name).with_items(vec![DownloadItem::http(source, filename)]);
        self.engine.add(package).await;
        self.publish(false).await
    }

    pub async fn start_item(&self, id: &str) -> Result<(), AppError> {
        let id = parse_id(id)?;
        self.engine.start_item(id).await;
        self.publish(false).await
    }

    pub async fn stop_item(&self, id: &str) -> Result<(), AppError> {
        let id = parse_id(id)?;
        self.engine.stop_item(id).await;
        self.publish(false).await
    }

    pub async fn retry_item(&self, id: &str) -> Result<(), AppError> {
        let id = parse_id(id)?;
        self.engine.retry(id).await;
        self.publish(false).await
    }

    pub async fn remove_item(&self, id: &str, delete_file: bool) -> Result<(), AppError> {
        let id = parse_id(id)?;
        self.engine.remove_item(id, delete_file).await;
        {
            // An open details panel for a removed item must not keep fetching.
            let mut state = lock(&self.state);
            for ids in state.detail_items.values_mut() {
                ids.remove(&id);
            }
        }
        self.publish(false).await
    }

    pub async fn pause_all(&self) -> Result<(), AppError> {
        self.engine.pause_all().await;
        self.publish(false).await
    }

    pub async fn resume_all(&self) -> Result<(), AppError> {
        self.engine.resume_all().await;
        self.publish(false).await
    }

    pub async fn set_list_visible(&self, visible: bool) -> Result<(), AppError> {
        let became_visible = {
            let mut state = lock(&self.state);
            let was = state.visible;
            state.visible = visible;
            if visible && !was {
                state.need_full = true;
            }
            visible && !was
        };
        if became_visible {
            self.publish(true).await?;
        }
        Ok(())
    }

    /// Reports which items have a details panel open for `owner` (a window or
    /// panel id). Replaces that owner's previous set. Any id that is not a
    /// UUID rejects the whole call and changes nothing.
    pub async fn set_detail_items(&self, owner: &str, ids: &[String]) -> Result<(), AppError> {
        let parsed: HashSet<Uuid> = ids.iter().map(|id| parse_id(id)).collect::<Result<_, _>>()?;
        {
            let mut state = lock(&self.state);
            if parsed.is_empty() {
                state.detail_items.remove(owner);
            } else {
                state.detail_items.insert(owner.to_owned(), parsed);
            }
        }
        self.publish(false).await
    }

    /// An explicit resync: sends a full patch whatever was sent before.
    pub async fn full_state(&self) -> Result<(), AppError> {
        self.publish(true).await
    }

    // MARK: Settings and misc

    pub fn get_settings(&self) -> SettingsDto {
        lock(&self.settings).get()
    }

    pub async fn update_settings(&self, dto: SettingsDto) -> Result<(), AppError> {
        let new_settings = {
            let mut store = lock(&self.settings);
            store.update(dto)?;
            engine_settings(store.download_folder())
        };
        self.engine.update_settings(new_settings).await;
        self.publish(false).await
    }

    pub fn binaries_status(&self) -> BinariesStatusDto {
        match &self.binaries {
            Some(binaries) => {
                let status = binaries.status();
                BinariesStatusDto {
                    yt_dlp_version: status.yt_dlp_version,
                    latest_known: status.latest_known,
                    ffmpeg_version: status.ffmpeg_version,
                    qjs_version: status.qjs_version,
                    last_error: status.last_error,
                }
            }
            None => BinariesStatusDto {
                yt_dlp_version: None,
                latest_known: None,
                ffmpeg_version: None,
                qjs_version: None,
                last_error: None,
            },
        }
    }

    /// The CSS for `theme_id` (the default theme, `dark`, when `None`).
    pub fn theme_css(&self, theme_id: Option<&str>) -> Result<String, AppError> {
        let wanted = theme_id.unwrap_or("dark");
        ThemeCatalog::built_in_themes()
            .into_iter()
            .find(|theme| theme.id == wanted)
            .map(|theme| theme_css(&theme))
            .ok_or_else(|| AppError::invalid("themeId", format!("no theme named {wanted}")))
    }

    /// Stops everything and writes durable state. Call when the app quits.
    pub async fn shutdown(&self) {
        self.engine.shutdown().await;
    }

    /// The managed-binaries provisioning step for launch: inflate bundled
    /// blobs (macOS) and check for a newer yt-dlp. Meant for a background task.
    pub async fn provision_binaries(&self) {
        if let Some(binaries) = &self.binaries {
            binaries.provision_bundled_if_needed().await;
            let _ = binaries.check_now(sdm_resolve::CheckReason::Launch).await;
        }
    }
}
```

Compile it and fix what the compiler flags; expected adjustments:

- When `since` is `None` the engine always returns `Some(snapshot)`, so a full request always reaches the `if let Some(snapshot)` branch.
- `ManagedBinaries` is `Clone` (the FFI wrapper clones it); `binaries.tick()` etc. take `&self`. If they need `&mut`, wrap in `Arc`.
- `EngineSnapshot` must be `Send + Sync` for `Arc<EngineSnapshot>`; it is (plain data).
- `DownloadPackage::new` panics on an empty name; `name` is never empty here (`derive_filename` never returns empty, and `package_name` is filtered), but `safe_path_component` also returns a non-empty fallback.
- Use of `let ... && let` chains requires edition 2024, which the workspace uses.

- [ ] **Step 5: Run the tests**

Run: `cd sdm-core && cargo test -p sdm-app 2>&1 | tail -30`
Expected: every test in `app_tests.rs` passes. Likely first failures and fixes:

- `adding_a_url_downloads_it...`: the file path assertion. The engine stores a package's files under `<download folder>/<package name>/<filename>` (see `engine_downloads_all_items_into_a_package_folder`: `dir/"Batch"/"a.bin"`), so with package name = filename the path is `data.bin/data.bin`; the test already tries both.
- `an_idle_engine_emits_nothing`: if a patch appears every tick, the cause is `stats` changing (for example a history vector growing). Print the patch; if it is the engine's `global_history`, check whether the engine bumps `generation` on every tick when idle (it must not: `snapshot_if_changed` returns `None` for an unchanged generation). If `snapshot_if_changed(since)` returns `Some` for an idle engine, investigate in `engine/snapshot.rs` before changing this test: the SwiftUI app relies on the same behavior.
- `a_failed_send...`: after `fail_next`, the `add_url` publish fails to send and drops the sink; `attach` again sets `need_full`, so the first patch is full.

- [ ] **Step 6: Lint and commit**

Run: `cd sdm-core && cargo clippy -p sdm-app --all-targets 2>&1 | tail -10 && cargo fmt -p sdm-app`
Expected: clean (the `await_holding_lock` lint passes because no guard is held across an `.await`).

```bash
git add sdm-core/crates/sdm-app
git commit -m "Add App: patch publishing, heartbeat and commands

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 11: Production wiring

**Files:**

- Create: `sdm-core/crates/sdm-app/src/toolchain.rs`
- Modify: `src/lib.rs`, `src/app.rs` (add `App::production`)
- Test: `sdm-core/crates/sdm-app/tests/production_tests.rs`

**Interfaces:**

- Produces: `pub fn production_dependencies(host: &dyn Host) -> Dependencies` and `impl App { pub fn production(host: Arc<dyn Host>) -> Result<Arc<App>, AppError> }`.

- [ ] **Step 1: Write the failing test**

`tests/production_tests.rs`:

```rust
use std::sync::Arc;

use sdm_app::host::FakeHost;
use sdm_app::App;

#[tokio::test(flavor = "multi_thread")]
async fn the_production_app_builds_with_a_managed_bin_directory_and_no_network() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let host = Arc::new(FakeHost::new(data.path().to_path_buf(), downloads.path().to_path_buf()));
    let app = App::production(host).expect("builds");
    // No yt-dlp has been provisioned yet: the status is empty, not an error.
    let status = app.binaries_status();
    assert_eq!(status.yt_dlp_version, None);
    assert!(data.path().join("bin").parent().unwrap().exists());
}
```

(`provision_binaries()` is not called here: it would reach the network. It is exercised manually in Task 15.)

- [ ] **Step 2: Run to verify it fails, then implement `toolchain.rs`**

Run `cargo test -p sdm-app --test production_tests` → compile error (`App::production` missing). Implementation, copying the wiring from `sdm-ffi/src/shared.rs`'s `Toolchain` with fixed defaults (`CookieSource::None`, 50 playlist videos, no extra arguments; spec 4 makes them settings):

```rust
//! The production collaborators: reqwest transport, JSON state store, system
//! Trash, yt-dlp resolver and wholesale downloader, ffmpeg muxer, and the
//! managed-binaries owner of `<data dir>/bin`.

use std::sync::Arc;

use sdm_core::{LinkResolver, WholesaleDownloader};
use sdm_engine::{
    FfmpegMuxer, JsonStateStore, Muxer, ReqwestTransport, RetryPolicy, SystemTrash,
};
use sdm_resolve::{
    BinaryLocator, CookieSource, ManagedBinaries, ProcessRunner, ReqwestBinaryFetcher,
    SystemProcessRunner, YtDlpChannel, YtDlpResolver, YtDlpWholesaleDownloader,
};

use crate::app::{App, Dependencies};
use crate::error::AppError;
use crate::host::Host;

const DEFAULT_MAX_PLAYLIST_VIDEOS: usize = 50;

pub fn production_dependencies(host: &dyn Host) -> Dependencies {
    let data_dir = host.data_dir();
    let bin_directory = data_dir.join("bin");
    let runner: Arc<dyn ProcessRunner> = Arc::new(SystemProcessRunner::new());

    // The managed `bin/` first, then the platform defaults.
    let mut search_paths = vec![bin_directory.clone()];
    search_paths.extend(BinaryLocator::default_search_paths());
    let locator = Arc::new(BinaryLocator::new().with_search_paths(search_paths));

    let resolver: Arc<dyn LinkResolver> = Arc::new(YtDlpResolver::new(
        Arc::clone(&runner),
        Arc::clone(&locator),
        Arc::new(|| CookieSource::None),
        Arc::new(|| DEFAULT_MAX_PLAYLIST_VIDEOS),
        Arc::new(Vec::new),
    ));
    let wholesale: Arc<dyn WholesaleDownloader> = Arc::new(YtDlpWholesaleDownloader::new(
        Arc::clone(&runner),
        Arc::clone(&locator),
        Arc::new(|| CookieSource::None),
        Arc::new(Vec::new),
    ));
    let muxer: Arc<dyn Muxer> = Arc::new(FfmpegMuxer::new(Arc::clone(&runner), Arc::clone(&locator)));

    // No bundled blobs here: on macOS the SwiftUI app's bundle holds them and
    // shares this `bin/` directory; Windows and Linux download (Task 16).
    let invalidate = Arc::clone(&locator);
    let binaries = ManagedBinaries::new(
        bin_directory,
        Arc::new(ReqwestBinaryFetcher::new()),
        runner,
        Vec::new,
        || YtDlpChannel::Stable,
    )
    .with_on_binaries_changed(move || {
        let locator = Arc::clone(&invalidate);
        Box::pin(async move { locator.invalidate() })
    });

    Dependencies {
        transport: Arc::new(ReqwestTransport::new()),
        state_store: Arc::new(JsonStateStore::new(data_dir.join("state.json"))),
        trash: Arc::new(SystemTrash),
        resolver: Some(resolver),
        muxer: Some(muxer),
        wholesale_downloader: Some(wholesale),
        retry_policy: RetryPolicy::new(),
        binaries: Some(binaries),
    }
}

impl App {
    pub fn production(host: Arc<dyn Host>) -> Result<Arc<App>, AppError> {
        let dependencies = production_dependencies(host.as_ref());
        App::new(host, dependencies)
    }
}
```

Check against the real signatures while compiling: `YtDlpResolver::new` takes `CookieSourceFn`, `MaxPlaylistVideosFn`, `ExtraArgumentsFn` closures wrapped in `Arc` (as `sdm-ffi/src/shared.rs` does); `Arc::new(Vec::new)` is `Arc<fn() -> Vec<String>>` and coerces to the `Arc<dyn Fn() -> Vec<String> + Send + Sync>` type only when the target type is known; if the compiler rejects it, write `Arc::new(|| Vec::<String>::new())`. `BinaryLocator::invalidate(&self)` exists (used by `sdm-ffi`). `with_on_binaries_changed` expects `Fn() -> BinariesChangedFuture` where the future is `Pin<Box<dyn Future<Output = ()> + Send>>`.

Add `mod toolchain; pub use toolchain::production_dependencies;` to `lib.rs`.

- [ ] **Step 3: Run, lint, commit**

Run: `cd sdm-core && cargo test -p sdm-app 2>&1 | tail -6 && cargo clippy -p sdm-app --all-targets 2>&1 | tail -4`
Expected: all pass.

```bash
git add sdm-core/crates/sdm-app
git commit -m "Wire the production engine dependencies and managed binaries

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 12: The Tauri shell

**Files:**

- Create: `sdm-tauri/package.json`, `sdm-tauri/pnpm-workspace.yaml`, `sdm-tauri/src-tauri/Cargo.toml`, `build.rs`, `tauri.conf.json`, `capabilities/default.json`, `icons/`, `src/main.rs`, `src/lib.rs`, `src/host.rs`, `src/commands.rs`
- Modify: `sdm-core/Cargo.toml` (workspace member), `test.sh`

**Interfaces:**

- Consumes: the `sdm-app` public API from Task 10–11.
- Produces: the registered commands the frontend calls (names are the Rust function names, camelCased by tauri-specta in TypeScript): `attach(on_patch: Channel<Patch>)`, `add_url(url, package_name)`, `start_item(id)`, `stop_item(id)`, `retry_item(id)`, `remove_item(id, delete_file)`, `pause_all()`, `resume_all()`, `set_list_visible(visible)`, `set_detail_items(owner, ids)`, `full_state()`, `get_settings()`, `update_settings(settings)`, `binaries_status()`, `theme_css(theme_id)`; plus the generated `sdm-tauri/web/src/bindings.ts`.

- [ ] **Step 1: Re-verify the Tauri and specta APIs**

Use Context7 for `/tauri-apps/tauri-docs` (project structure with a `src-tauri` folder, `tauri.conf.json` v2 keys, capabilities, `tauri::ipc::Channel`) and `/specta-rs/tauri-specta` (`Builder::new().commands(collect_commands![..])`, `export`, `invoke_handler`, `mount_events`). Compare with the code below; **trust the docs over this plan** where they differ and note the difference in the commit message.

- [ ] **Step 2: Scaffold**

```bash
mkdir -p sdm-tauri/src-tauri/src sdm-tauri/src-tauri/capabilities sdm-tauri/web
cd sdm-tauri
pnpm init
pnpm add -D @tauri-apps/cli
printf 'packages:\n  - web\n' > pnpm-workspace.yaml
```

Add to `sdm-tauri/package.json` `"scripts": { "tauri": "tauri" }` (a script entry, not a dependency). Then create the shell crate:

```bash
cd ../sdm-core
cargo new --lib ../sdm-tauri/src-tauri --name sdm-tauri
cargo add -p sdm-tauri tauri --features tray-icon
cargo add -p sdm-tauri tauri-plugin-opener
cargo add -p sdm-tauri tauri-specta@=2.0.0-rc.25 --features derive,typescript
cargo add -p sdm-tauri specta@=2.0.0-rc.25
cargo add -p sdm-tauri specta-typescript@0.0.12
cargo add -p sdm-tauri sdm-app --path crates/sdm-app
cargo add -p sdm-tauri sdm-core --path crates/sdm-core
cargo add -p sdm-tauri serde --features derive
cargo add -p sdm-tauri tokio --features time,rt
cargo add -p sdm-tauri tracing tracing-subscriber
cargo add -p sdm-tauri --build tauri-build
```

(Drop `--features tray-icon` if `cargo add` shows the feature does not exist in the current Tauri; the tray belongs to spec 5.)

Edit `sdm-core/Cargo.toml`: `members = ["crates/*", "../sdm-tauri/src-tauri"]` (hand edit of a workspace member list; `cargo add` cannot do this). In `sdm-tauri/src-tauri/Cargo.toml` set `edition.workspace = true`, `version.workspace = true`, `publish.workspace = true`, `[lints] workspace = true`, and `[lib] name = "sdm_tauri_lib"` with `crate-type = ["staticlib", "cdylib", "rlib"]` per the Tauri template.

`tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "SDM",
  "version": "0.1.0",
  "identifier": "com.shayanoh.sdm.tauri",
  "build": {
    "beforeDevCommand": "pnpm --dir web dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "pnpm --dir web build",
    "frontendDist": "../web/dist"
  },
  "app": {
    "windows": [
      {
        "title": "SDM",
        "width": 1000,
        "height": 640,
        "minWidth": 640,
        "minHeight": 360
      }
    ],
    "security": { "csp": null }
  },
  "bundle": {
    "active": true,
    "targets": ["nsis", "app", "deb", "appimage"],
    "icon": ["icons/icon.png"]
  }
}
```

`capabilities/default.json` (core permissions only):

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Core window permissions",
  "windows": ["main"],
  "permissions": ["core:default"]
}
```

Create a placeholder `icons/icon.png` with `pnpm tauri icon <a 1024x1024 png>` (any plain image; real art comes later). The window must be labelled `main`: add `"label": "main"` to the window entry.

- [ ] **Step 3: Write `host.rs`**

```rust
//! The Tauri implementation of `sdm_app::Host`.

use std::path::{Path, PathBuf};

use sdm_app::host::{Host, HostError};
use sdm_app::notify::Notification;
use tauri::{AppHandle, Manager};

pub struct TauriHost {
    data_dir: PathBuf,
    download_dir: PathBuf,
    app: AppHandle,
}

impl TauriHost {
    pub fn new(app: &AppHandle) -> Result<Self, String> {
        let paths = app.path();
        // `data_dir()` is ~/Library/Application Support on macOS (shared with
        // the SwiftUI app, decision D5), %APPDATA% on Windows and
        // $XDG_DATA_HOME or ~/.local/share on Linux.
        let data_dir = paths.data_dir().map_err(|e| e.to_string())?.join("SDM");
        let download_dir = paths
            .download_dir()
            .map_err(|e| e.to_string())?
            .join("SDM");
        Ok(Self { data_dir, download_dir, app: app.clone() })
    }
}

impl Host for TauriHost {
    fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }

    fn default_download_dir(&self) -> PathBuf {
        self.download_dir.clone()
    }

    fn reveal_in_file_manager(&self, path: &Path) -> Result<(), HostError> {
        tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| HostError(e.to_string()))
    }

    fn notify(&self, notification: Notification) -> Result<(), HostError> {
        // Spec 5 turns this into an OS notification.
        let _ = &self.app;
        match notification {
            Notification::Finished { filename } => tracing::info!("finished: {filename}"),
            Notification::Failed { filename, reason } => tracing::info!("failed: {filename}: {reason}"),
        }
        Ok(())
    }
}
```

The macOS download folder default `~/Downloads/SDM` is a placeholder consistent with the roadmap; the SwiftUI app's own default is separate.

- [ ] **Step 4: Write `commands.rs`**

```rust
//! Thin command wrappers over `sdm_app::App`. Each runs inside
//! `App::guarded` so a panic becomes an error the window can show.

use std::sync::Arc;

use sdm_app::host::{PatchSink, SinkClosed};
use sdm_app::protocol::{BinariesStatusDto, Patch, SettingsDto};
use sdm_app::{App, AppError};
use tauri::State;
use tauri::ipc::Channel;

pub struct ChannelSink(pub Channel<Patch>);

impl PatchSink for ChannelSink {
    fn send(&self, patch: Patch) -> Result<(), SinkClosed> {
        self.0.send(patch).map_err(|_| SinkClosed)
    }
}

type Shared<'a> = State<'a, Arc<App>>;

#[tauri::command]
#[specta::specta]
pub async fn attach(app: Shared<'_>, on_patch: Channel<Patch>) -> Result<(), AppError> {
    let sink: Arc<dyn PatchSink> = Arc::new(ChannelSink(on_patch));
    app.guarded(move |app| async move { app.attach(sink).await }).await
}

#[tauri::command]
#[specta::specta]
pub async fn add_url(app: Shared<'_>, url: String, package_name: Option<String>) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.add_url(&url, package_name.as_deref()).await }).await
}

#[tauri::command]
#[specta::specta]
pub async fn start_item(app: Shared<'_>, id: String) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.start_item(&id).await }).await
}

#[tauri::command]
#[specta::specta]
pub async fn stop_item(app: Shared<'_>, id: String) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.stop_item(&id).await }).await
}

#[tauri::command]
#[specta::specta]
pub async fn retry_item(app: Shared<'_>, id: String) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.retry_item(&id).await }).await
}

#[tauri::command]
#[specta::specta]
pub async fn remove_item(app: Shared<'_>, id: String, delete_file: bool) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.remove_item(&id, delete_file).await }).await
}

#[tauri::command]
#[specta::specta]
pub async fn pause_all(app: Shared<'_>) -> Result<(), AppError> {
    app.guarded(|app| async move { app.pause_all().await }).await
}

#[tauri::command]
#[specta::specta]
pub async fn resume_all(app: Shared<'_>) -> Result<(), AppError> {
    app.guarded(|app| async move { app.resume_all().await }).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_list_visible(app: Shared<'_>, visible: bool) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.set_list_visible(visible).await }).await
}

#[tauri::command]
#[specta::specta]
pub async fn set_detail_items(app: Shared<'_>, owner: String, ids: Vec<String>) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.set_detail_items(&owner, &ids).await }).await
}

#[tauri::command]
#[specta::specta]
pub async fn full_state(app: Shared<'_>) -> Result<(), AppError> {
    app.guarded(|app| async move { app.full_state().await }).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_settings(app: Shared<'_>) -> Result<SettingsDto, AppError> {
    Ok(app.get_settings())
}

#[tauri::command]
#[specta::specta]
pub async fn update_settings(app: Shared<'_>, settings: SettingsDto) -> Result<(), AppError> {
    app.guarded(move |app| async move { app.update_settings(settings).await }).await
}

#[tauri::command]
#[specta::specta]
pub async fn binaries_status(app: Shared<'_>) -> Result<BinariesStatusDto, AppError> {
    Ok(app.binaries_status())
}

#[tauri::command]
#[specta::specta]
pub async fn theme_css(app: Shared<'_>, theme_id: Option<String>) -> Result<String, AppError> {
    app.theme_css(theme_id.as_deref())
}
```

If `State<'_, Arc<App>>` cannot call `guarded(self: &Arc<Self>)` through auto-deref, use `let app: Arc<App> = Arc::clone(&app);` first. `SettingsDto` needs `specta::Type` and `Deserialize`; it has both (Task 6). `Patch` is `Serialize + Type`; `tauri::ipc::Channel<T>` needs `T: Serialize` (and specta needs `Type` for the generated `TAURI_CHANNEL<Patch>`).

- [ ] **Step 5: Write `lib.rs`**

```rust
mod commands;
mod host;

use std::sync::Arc;
use std::time::Duration;

use commands::*;
use sdm_app::App;
use sdm_core::AppTiming;
use tauri::Manager;
use tauri_specta::{Builder, collect_commands};

fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        attach, add_url, start_item, stop_item, retry_item, remove_item, pause_all, resume_all,
        set_list_visible, set_detail_items, full_state, get_settings, update_settings,
        binaries_status, theme_css,
    ])
}

/// Writes `web/src/bindings.ts`. Run by a debug build at startup and by
/// `cargo test -p sdm-tauri export_bindings`.
#[cfg(debug_assertions)]
fn export_bindings(builder: &Builder<tauri::Wry>) {
    builder
        .export(specta_typescript::Typescript::default(), "../web/src/bindings.ts")
        .expect("failed to export the TypeScript bindings");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();
    let builder = specta_builder();
    #[cfg(debug_assertions)]
    export_bindings(&builder);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |tauri_app| {
            builder.mount_events(tauri_app);
            let host = Arc::new(host::TauriHost::new(tauri_app.handle())?);
            let app = App::production(host)?;
            tauri_app.manage(Arc::clone(&app));

            // Restore, then provision binaries in the background, then beat.
            let heartbeat_app = Arc::clone(&app);
            tauri::async_runtime::spawn(async move {
                if let Err(error) = heartbeat_app.restore().await {
                    tracing::error!("could not restore saved downloads: {error}");
                }
                let provisioner = Arc::clone(&heartbeat_app);
                tauri::async_runtime::spawn(async move { provisioner.provision_binaries().await });

                let period = Duration::from_millis(1000 / AppTiming::TICKS_PER_SECOND as u64);
                let mut interval = tokio::time::interval(period);
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    interval.tick().await;
                    // A panic in a tick must not end the heartbeat.
                    let app = Arc::clone(&heartbeat_app);
                    let _ = app.guarded(|app| async move { app.heartbeat().await; Ok(()) }).await;
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the SDM application");
}

#[cfg(all(test, debug_assertions))]
mod tests {
    #[test]
    fn export_bindings() {
        super::export_bindings(&super::specta_builder());
    }
}
```

`main.rs`:

```rust
// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    sdm_tauri_lib::run()
}
```

`build.rs`: `fn main() { tauri_build::build() }`.

- [ ] **Step 6: Export the bindings and check**

The shell needs a `web/dist` to compile (`generate_context!` checks `frontendDist`). Create it: `mkdir -p sdm-tauri/web/dist && printf '<!doctype html><title>SDM</title>' > sdm-tauri/web/dist/index.html` (git-ignored; Task 13 replaces it with the real build).

Run: `cd sdm-core && cargo test -p sdm-tauri export_bindings 2>&1 | tail -15 && ls -la ../sdm-tauri/web/src/bindings.ts`
Expected: the test passes and `bindings.ts` exists. Open it: it must contain `commands`, the `Patch` type with `seq`, `full`, `structure`, a `TAURI_CHANNEL`-style channel type for `attach`, and `AppError`. If specta rejects a type (for example `u64`), fix the DTO in `sdm-app`, not the exporter.

Then `cargo clippy -p sdm-tauri 2>&1 | tail -10`.

- [ ] **Step 7: First launch**

Run: `cd sdm-tauri && pnpm tauri dev` in a terminal the user can see **only if** a window is acceptable; otherwise confirm with `cargo build -p sdm-tauri` and stop. Expected for the dev run: the Tauri window opens with the placeholder page, the terminal logs no panics, and `~/Library/Application Support/SDM` still contains the SwiftUI app's `state.json` untouched (the app read it through `restore()`; it only rewrites it after a change).

- [ ] **Step 8: Update `test.sh` and commit**

In `test.sh` the first line becomes `(cd sdm-core && cargo test --workspace --exclude sdm-tauri)` if it was not already. Add `.gitignore` entries: `sdm-tauri/web/dist/`, `sdm-tauri/node_modules/`, `sdm-tauri/src-tauri/gen/`.

```bash
git add sdm-core/Cargo.toml sdm-core/Cargo.lock sdm-tauri test.sh .gitignore
git commit -m "Add the Tauri shell: commands, heartbeat, host and generated bindings

Workspace member added by hand (cargo add cannot add a member).

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 13: The frontend

**Files:** everything under `sdm-tauri/web/` (see File Structure).

**Interfaces:**

- Consumes: `sdm-tauri/web/src/bindings.ts` (Task 12).
- Produces: a running UI against the real core (`pnpm tauri dev`) and against `MockBridge` (`pnpm --dir web dev` in a browser).

- [ ] **Step 1: Scaffold and re-verify Svelte 5 APIs**

```bash
cd sdm-tauri/web
pnpm create vite@latest . --template svelte-ts   # accept the prompts; keep it in this folder
pnpm install
pnpm add @tauri-apps/api
pnpm add -D vitest jsdom @testing-library/svelte @playwright/test prettier prettier-plugin-svelte
```

Set Vite's dev server to `port: 1420, strictPort: true` in `vite.config.ts` (matches `tauri.conf.json`). Check Svelte 5 runes usage in `.svelte.ts` files and the Vitest + Svelte setup in Context7 (`/websites/svelte_dev`, `/vitest-dev/vitest`) before writing the store; the code below uses `$state` in a class in a `.svelte.ts` file and `SvelteMap` from `svelte/reactivity`.

- [ ] **Step 2: The Bridge interface**

`src/lib/bridge.ts`:

```ts
import type {
  Patch,
  SettingsDto,
  BinariesStatusDto,
  AppError,
} from "../bindings";

/** Every call the UI makes into the backend. Two implementations exist. */
export interface Bridge {
  /** Starts the patch stream; the first patch is always full. */
  attach(onPatch: (patch: Patch) => void): Promise<void>;
  fullState(): Promise<void>;
  addUrl(url: string, packageName?: string): Promise<void>;
  startItem(id: string): Promise<void>;
  stopItem(id: string): Promise<void>;
  retryItem(id: string): Promise<void>;
  removeItem(id: string, deleteFile: boolean): Promise<void>;
  pauseAll(): Promise<void>;
  resumeAll(): Promise<void>;
  setListVisible(visible: boolean): Promise<void>;
  setDetailItems(owner: string, ids: string[]): Promise<void>;
  getSettings(): Promise<SettingsDto>;
  updateSettings(settings: SettingsDto): Promise<void>;
  binariesStatus(): Promise<BinariesStatusDto>;
  themeCss(themeId?: string): Promise<string>;
}

/** What a rejected bridge call throws. */
export class BridgeError extends Error {
  constructor(public readonly error: AppError) {
    super("message" in error ? error.message : error.kind);
  }
}
```

- [ ] **Step 3: Write the failing store tests**

`src/lib/store.test.ts`:

```ts
import { describe, expect, it, vi } from "vitest";
import { DownloadStore } from "./store.svelte";
import type { Patch } from "../bindings";

const item = (id: string, over: Partial<any> = {}) => ({
  id,
  url: "https://e.com/f",
  filename: "f.bin",
  state: { kind: "running" },
  isEnabled: true,
  isResumable: true,
  isAssembling: false,
  fileMissing: false,
  metadata: null,
  ...over,
});
const figures = (id: string, over: Partial<any> = {}) => ({
  id,
  completed: [],
  completedBytes: 0,
  totalBytes: 100,
  activeSegments: 1,
  configuredSegments: 4,
  bytesPerSecond: 0,
  speedHistory: [],
  retryHoldSeconds: null,
  failedAttemptCount: null,
  lastFailureReason: null,
  checkpointFailure: null,
  ...over,
});
const stats = () => ({
  bytesPerSecond: 0,
  history: [],
  runningCount: 0,
  queuedCount: 0,
  overallFraction: 0,
  ringCompletedBytes: 0,
  ringTotalBytes: 0,
  waitingItemCount: 0,
  waitingPackageCount: 0,
});
const patch = (seq: number, over: Partial<Patch> = {}): Patch => ({
  seq,
  full: false,
  structure: null,
  items: [],
  packages: [],
  stats: null,
  histories: [],
  ...over,
});
const fullPatch = (seq: number, ids: string[]): Patch =>
  patch(seq, {
    full: true,
    structure: {
      packages: [
        { id: "p1", name: "P", priority: 2, items: ids.map((id) => item(id)) },
      ],
    },
    items: ids.map((id) => figures(id)),
    packages: [
      {
        id: "p1",
        completedBytes: 0,
        totalBytes: 100,
        bytesPerSecond: 0,
        speedHistory: [],
        etaSeconds: 0,
      },
    ],
    stats: stats(),
  });

describe("DownloadStore", () => {
  it("builds rows from a full patch", () => {
    const store = new DownloadStore(() => {});
    store.apply(fullPatch(5, ["a", "b"]));
    expect(store.order).toEqual([{ id: "p1", itemIds: ["a", "b"] }]);
    expect(store.items.get("a")?.filename).toBe("f.bin");
    expect(store.items.get("b")?.totalBytes).toBe(100);
  });

  it("applies figures to existing models in place", () => {
    const store = new DownloadStore(() => {});
    store.apply(fullPatch(1, ["a"]));
    const model = store.items.get("a")!;
    store.apply(
      patch(2, {
        items: [figures("a", { completedBytes: 40, bytesPerSecond: 7 })],
      }),
    );
    expect(store.items.get("a")).toBe(model); // same object: rows keep their identity
    expect(model.completedBytes).toBe(40);
    expect(model.bytesPerSecond).toBe(7);
  });

  it("drops items missing from a new structure", () => {
    const store = new DownloadStore(() => {});
    store.apply(fullPatch(1, ["a", "b"]));
    store.apply(
      patch(2, {
        structure: {
          packages: [{ id: "p1", name: "P", priority: 2, items: [item("a")] }],
        },
      }),
    );
    expect(store.items.has("b")).toBe(false);
    expect(store.order[0].itemIds).toEqual(["a"]);
  });

  it("asks for a resync on a gap and ignores patches until a full one arrives", () => {
    const resync = vi.fn();
    const store = new DownloadStore(resync);
    store.apply(fullPatch(1, ["a"]));
    store.apply(patch(3, { items: [figures("a", { completedBytes: 99 })] })); // seq 2 missing
    expect(resync).toHaveBeenCalledTimes(1);
    expect(store.items.get("a")?.completedBytes).toBe(0);
    store.apply(patch(4)); // still waiting for the full patch: no second request
    expect(resync).toHaveBeenCalledTimes(1);
    store.apply(fullPatch(9, ["a", "c"]));
    expect(store.items.has("c")).toBe(true);
    store.apply(patch(10, { items: [figures("c", { completedBytes: 5 })] }));
    expect(store.items.get("c")?.completedBytes).toBe(5);
  });

  it("wraps the sequence number at 2^32", () => {
    const resync = vi.fn();
    const store = new DownloadStore(resync);
    store.apply(fullPatch(4294967295, ["a"]));
    store.apply(patch(0, { items: [figures("a", { completedBytes: 1 })] }));
    expect(resync).not.toHaveBeenCalled();
    expect(store.items.get("a")?.completedBytes).toBe(1);
  });

  it("requests a resync when a non-full patch arrives before any full one", () => {
    const resync = vi.fn();
    const store = new DownloadStore(resync);
    store.apply(patch(7));
    expect(resync).toHaveBeenCalledTimes(1);
  });

  it("a full patch clears everything first", () => {
    const store = new DownloadStore(() => {});
    store.apply(fullPatch(1, ["a", "b"]));
    store.apply(fullPatch(2, ["c"]));
    expect([...store.items.keys()]).toEqual(["c"]);
  });

  it("keeps histories only for the ids in a patch", () => {
    const store = new DownloadStore(() => {});
    store.apply(fullPatch(1, ["a"]));
    store.apply(
      patch(2, { histories: [{ id: "a", speedHistory: [1, 2, 3] }] }),
    );
    expect(store.items.get("a")?.detailHistory).toEqual([1, 2, 3]);
  });
});
```

Run: `pnpm --dir sdm-tauri/web vitest run` → fails (`store.svelte` missing). Configure Vitest in `vite.config.ts` (`test: { environment: 'jsdom' }`) and add a `"test": "vitest run"` script.

- [ ] **Step 4: Implement the store**

`src/lib/store.svelte.ts`:

```ts
import { SvelteMap } from "svelte/reactivity";
import type {
  EngineStats,
  ItemFigures,
  ItemStateDto,
  Patch,
  PackageFigures,
  StructureItem,
} from "../bindings";

/** One row's data. Fields are `$state`, so a row re-renders only for what it reads. */
export class ItemModel {
  id: string;
  url = $state("");
  filename = $state("");
  state = $state.raw<ItemStateDto>({ kind: "queued" });
  isEnabled = $state(true);
  isResumable = $state<boolean | null>(null);
  isAssembling = $state(false);
  fileMissing = $state(false);
  metadata = $state<string | null>(null);

  completed = $state.raw<{ start: number; end: number }[]>([]);
  completedBytes = $state(0);
  totalBytes = $state<number | null>(null);
  activeSegments = $state(0);
  configuredSegments = $state(0);
  bytesPerSecond = $state(0);
  speedHistory = $state.raw<number[]>([]);
  retryHoldSeconds = $state<number | null>(null);
  failedAttemptCount = $state<number | null>(null);
  lastFailureReason = $state<string | null>(null);
  checkpointFailure = $state<string | null>(null);
  detailHistory = $state.raw<number[] | null>(null);

  constructor(id: string) {
    this.id = id;
  }

  applyStructure(s: StructureItem) {
    this.url = s.url;
    this.filename = s.filename;
    this.state = s.state;
    this.isEnabled = s.isEnabled;
    this.isResumable = s.isResumable;
    this.isAssembling = s.isAssembling;
    this.fileMissing = s.fileMissing;
    this.metadata = s.metadata;
  }

  applyFigures(f: ItemFigures) {
    this.completed = f.completed;
    this.completedBytes = f.completedBytes;
    this.totalBytes = f.totalBytes;
    this.activeSegments = f.activeSegments;
    this.configuredSegments = f.configuredSegments;
    this.bytesPerSecond = f.bytesPerSecond;
    this.speedHistory = f.speedHistory;
    this.retryHoldSeconds = f.retryHoldSeconds;
    this.failedAttemptCount = f.failedAttemptCount;
    this.lastFailureReason = f.lastFailureReason;
    this.checkpointFailure = f.checkpointFailure;
  }
}

export class PackageModel {
  id: string;
  name = $state("");
  priority = $state(2);
  completedBytes = $state(0);
  totalBytes = $state(0);
  bytesPerSecond = $state(0);
  speedHistory = $state.raw<number[]>([]);
  etaSeconds = $state(0);

  constructor(id: string) {
    this.id = id;
  }

  applyFigures(f: PackageFigures) {
    this.completedBytes = f.completedBytes;
    this.totalBytes = f.totalBytes;
    this.bytesPerSecond = f.bytesPerSecond;
    this.speedHistory = f.speedHistory;
    this.etaSeconds = f.etaSeconds;
  }
}

export interface PackageOrder {
  id: string;
  itemIds: string[];
}

const next = (seq: number) => (seq + 1) >>> 0;

/** The UI's copy of the download list, kept current by applying patches. */
export class DownloadStore {
  items = new SvelteMap<string, ItemModel>();
  packages = new SvelteMap<string, PackageModel>();
  order = $state.raw<PackageOrder[]>([]);
  stats = $state.raw<EngineStats | null>(null);

  #expected: number | null = null;
  #resyncRequested = false;
  #resync: () => void;

  constructor(resync: () => void) {
    this.#resync = resync;
  }

  apply(patch: Patch) {
    if (patch.full) {
      this.items.clear();
      this.packages.clear();
      this.order = [];
      this.#resyncRequested = false;
    } else if (this.#expected === null || patch.seq !== this.#expected) {
      // A gap (or no full patch yet): ignore until a full patch arrives.
      if (!this.#resyncRequested) {
        this.#resyncRequested = true;
        this.#resync();
      }
      return;
    }
    this.#expected = next(patch.seq);

    if (patch.structure) this.#applyStructure(patch.structure);
    for (const f of patch.items) this.items.get(f.id)?.applyFigures(f);
    for (const f of patch.packages) this.packages.get(f.id)?.applyFigures(f);
    if (patch.stats) this.stats = patch.stats;
    for (const h of patch.histories) {
      const model = this.items.get(h.id);
      if (model) model.detailHistory = h.speedHistory;
    }
  }

  #applyStructure(structure: Patch["structure"] & object) {
    const liveItems = new Set<string>();
    const livePackages = new Set<string>();
    const order: PackageOrder[] = [];
    for (const pkg of structure.packages) {
      livePackages.add(pkg.id);
      let pm = this.packages.get(pkg.id);
      if (!pm) this.packages.set(pkg.id, (pm = new PackageModel(pkg.id)));
      pm.name = pkg.name;
      pm.priority = pkg.priority;
      const itemIds: string[] = [];
      for (const s of pkg.items) {
        liveItems.add(s.id);
        itemIds.push(s.id);
        let im = this.items.get(s.id);
        if (!im) this.items.set(s.id, (im = new ItemModel(s.id)));
        im.applyStructure(s);
      }
      order.push({ id: pkg.id, itemIds });
    }
    for (const id of [...this.items.keys()])
      if (!liveItems.has(id)) this.items.delete(id);
    for (const id of [...this.packages.keys()])
      if (!livePackages.has(id)) this.packages.delete(id);
    this.order = order;
  }
}
```

Run: `pnpm --dir sdm-tauri/web test` → all 8 pass. (`$state` in a `.svelte.ts` class compiles under Vitest only with the Svelte Vite plugin active; the template's `vite.config.ts` has it. If runes are not compiled in tests, add `svelte({ compilerOptions: {} })` as the first plugin and make sure the file is named `*.svelte.ts`.)

- [ ] **Step 5: Windowing function, with tests**

`src/lib/windowing.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { visibleRange } from "./windowing";

describe("visibleRange", () => {
  it("covers the viewport plus overscan", () => {
    expect(
      visibleRange({
        scrollTop: 0,
        viewport: 100,
        rowHeight: 20,
        count: 1000,
        overscan: 2,
      }),
    ).toEqual({ start: 0, end: 7 });
    expect(
      visibleRange({
        scrollTop: 400,
        viewport: 100,
        rowHeight: 20,
        count: 1000,
        overscan: 2,
      }),
    ).toEqual({ start: 18, end: 27 });
  });
  it("clamps to the list", () => {
    expect(
      visibleRange({
        scrollTop: 9999,
        viewport: 100,
        rowHeight: 20,
        count: 10,
        overscan: 2,
      }),
    ).toEqual({ start: 8, end: 10 });
  });
  it("handles empty lists and bad input", () => {
    expect(
      visibleRange({
        scrollTop: 0,
        viewport: 100,
        rowHeight: 20,
        count: 0,
        overscan: 2,
      }),
    ).toEqual({ start: 0, end: 0 });
    expect(
      visibleRange({
        scrollTop: -50,
        viewport: 100,
        rowHeight: 20,
        count: 5,
        overscan: 0,
      }),
    ).toEqual({ start: 0, end: 5 });
    expect(
      visibleRange({
        scrollTop: 0,
        viewport: 100,
        rowHeight: 0,
        count: 5,
        overscan: 0,
      }),
    ).toEqual({ start: 0, end: 0 });
    expect(
      visibleRange({
        scrollTop: NaN,
        viewport: 100,
        rowHeight: 20,
        count: 5,
        overscan: 0,
      }),
    ).toEqual({ start: 0, end: 5 });
  });
});
```

`src/lib/windowing.ts`:

```ts
export interface WindowInput {
  scrollTop: number;
  viewport: number;
  rowHeight: number;
  count: number;
  overscan: number;
}

/** The half-open range of rows to render. Fixed row height. */
export function visibleRange({
  scrollTop,
  viewport,
  rowHeight,
  count,
  overscan,
}: WindowInput) {
  if (!(rowHeight > 0) || count <= 0) return { start: 0, end: 0 };
  const top = Number.isFinite(scrollTop) ? Math.max(0, scrollTop) : 0;
  const height = Number.isFinite(viewport) ? Math.max(0, viewport) : 0;
  const first = Math.floor(top / rowHeight);
  const last = Math.ceil((top + height) / rowHeight);
  const pad = Math.max(0, overscan);
  return {
    start: Math.max(0, Math.min(count, first - pad)),
    end: Math.max(0, Math.min(count, last + pad)),
  };
}
```

Run the tests; fix expected values by hand-computing only if the maths in the test is wrong (first case: rows 0..5 visible + 2 overscan → end 7).

- [ ] **Step 6: Formatting helpers with tests**

`src/lib/format.ts` exports `formatBytes(n: number): string` (`1536` → `"1.5 KB"`, `0` → `"0 B"`, negative or NaN → `"—"`), `formatSpeed(n)` (`"1.5 MB/s"`, `0` → `"—"`), `formatEta(seconds)` (`0`/NaN → `""`, `75` → `"1m 15s"`, `3700` → `"1h 1m"`). Write `format.test.ts` first covering exactly those examples, run it red, implement, run it green.

- [ ] **Step 7: Bridges**

`src/lib/tauriBridge.ts` wraps the generated `commands`. Open `src/bindings.ts` and match its exact shape: tauri-specta commands that return `Result<T, E>` resolve to `{ status: 'ok'; data: T } | { status: 'error'; error: E }` and the channel argument type is generated (`TAURI_CHANNEL<Patch>`). Write one helper:

```ts
import { Channel } from "@tauri-apps/api/core";
import { commands, type Patch } from "../bindings";
import { BridgeError, type Bridge } from "./bridge";

type Result<T, E> = { status: "ok"; data: T } | { status: "error"; error: E };

async function unwrap<T>(call: Promise<Result<T, any>>): Promise<T> {
  const result = await call;
  if (result.status === "error") throw new BridgeError(result.error);
  return result.data;
}

export class TauriBridge implements Bridge {
  attach(onPatch: (patch: Patch) => void) {
    const channel = new Channel<Patch>();
    channel.onmessage = onPatch;
    return unwrap(commands.attach(channel));
  }
  fullState() {
    return unwrap(commands.fullState());
  }
  addUrl(url: string, packageName?: string) {
    return unwrap(commands.addUrl(url, packageName ?? null));
  }
  startItem(id: string) {
    return unwrap(commands.startItem(id));
  }
  stopItem(id: string) {
    return unwrap(commands.stopItem(id));
  }
  retryItem(id: string) {
    return unwrap(commands.retryItem(id));
  }
  removeItem(id: string, deleteFile: boolean) {
    return unwrap(commands.removeItem(id, deleteFile));
  }
  pauseAll() {
    return unwrap(commands.pauseAll());
  }
  resumeAll() {
    return unwrap(commands.resumeAll());
  }
  setListVisible(visible: boolean) {
    return unwrap(commands.setListVisible(visible));
  }
  setDetailItems(owner: string, ids: string[]) {
    return unwrap(commands.setDetailItems(owner, ids));
  }
  getSettings() {
    return unwrap(commands.getSettings());
  }
  updateSettings(settings: import("../bindings").SettingsDto) {
    return unwrap(commands.updateSettings(settings));
  }
  binariesStatus() {
    return unwrap(commands.binariesStatus());
  }
  themeCss(themeId?: string) {
    return unwrap(commands.themeCss(themeId ?? null));
  }
}
```

Adjust method names and argument shapes to the generated file.

`src/lib/mockBridge.ts` implements `Bridge` in memory: it keeps a small model (a list of fake items), and on `attach` immediately emits a full patch and then, with `setInterval(…, 200)`, emits progress patches for items in a `running` state (advancing `completed` ranges and `bytesPerSecond`), completing them at 100%; `addUrl` rejects invalid URLs with `BridgeError({ kind: 'invalidArgument', argument: 'url', message: '…' })`, mirrors the real validation (http/https only), and creates an item named after the last URL segment. `themeCss` returns a fixed dark `:root{…}` block with every variable the components use. Add `src/lib/mockBridge.test.ts` asserting: attach emits a full patch first; seq increments by one; `addUrl('nope')` rejects with `invalidArgument`; a running item eventually completes (use `vi.useFakeTimers()` and `vi.advanceTimersByTime`).

Choose the bridge in `main.ts`: `'__TAURI_INTERNALS__' in window ? new TauriBridge() : new MockBridge()`.

- [ ] **Step 8: Components**

Write these Svelte 5 components (runes, no stores API), each ≤ 120 lines, colors only as `var(--…)`, no literals:

- `ProgressCanvas.svelte` — props `{ completed, total }`; a `<canvas>` sized to its box with `devicePixelRatio`; draws the track (`--surface-tertiary`), each completed range as a rect filled with `--completed-segment-fill`; redraws in an `$effect` that reads `completed`, `total` and the box width (via `bind:clientWidth`). Ranges are in bytes; map `x = range.start / total * width`. If `total` is null/0 draw nothing but the track. Guard against `NaN`.
- `Sparkline.svelte` — props `{ samples: number[] }`; draws a polyline with `--graph-stroke`; normalizes to the max sample (min 1); empty draws nothing.
- `ItemRow.svelte` — props `{ item: ItemModel, selected }`; shows filename, `formatBytes(completedBytes) / formatBytes(totalBytes)`, `formatSpeed`, a state label (`Running`, `Queued`, `Paused`, `Done`, `Failed: reason`), the `ProgressCanvas` and `Sparkline`, and buttons: pause/resume (`stopItem`/`startItem` depending on state), retry (when failed), remove. Buttons call the bridge from a context. Fixed height 56 px (the windowing constant).
- `PackageHeader.svelte` — props `{ pkg: PackageModel }`: name, summed bytes, speed, ETA. Fixed height 56 px.
- `DownloadList.svelte` — flattens `store.order` into rows (`{kind:'package', id}` then its `{kind:'item', id}`), computes `visibleRange` from a scroll container (`bind:clientHeight`, `onscroll`), renders `count*rowHeight` tall inner div and absolutely positions the visible slice. A row component reads only its own model.
- `AddUrl.svelte` — an input and an Add button; Enter submits; calls `bridge.addUrl`; clears on success; on `BridgeError` shows the message in `Banner`.
- `Banner.svelte` — dismissible error line (`--status-failed`).
- `App.svelte` — header (title, total speed from `store.stats`, Pause All / Resume All), `AddUrl`, `DownloadList`, binaries status line from `bridge.binariesStatus()` polled every 5 s (the only polling in the UI; it is not the download stream), a download-folder field bound to `getSettings`/`updateSettings`. On mount: `const css = await bridge.themeCss(); inject a <style id="theme">`; `await bridge.attach(store.apply.bind(store))`; `document.addEventListener('visibilitychange', …)` calls `setListVisible(!document.hidden)`.

Provide the bridge and store through Svelte context (`setContext('bridge', …)`), created once in `main.ts`.

`src/theme.css` holds only layout and typography using `var(--…)`; define the font stack `system-ui` and row dimensions as CSS variables. No colors.

- [ ] **Step 9: Visual check against the mock with Playwright**

Run the dev server (`pnpm --dir sdm-tauri/web dev`), then with the Playwright MCP: navigate to `http://localhost:1420`, wait for rows, take a screenshot, check: header shows a speed, at least one row has a progress bar partly filled, the add field rejects `nope` with a banner, adding `https://example.com/new.bin` adds a row. Save one screenshot to `docs/tauri-skeleton-mock.png` for the roadmap. Also add `e2e/mock.spec.ts` with a Playwright test covering the same three assertions (`pnpm exec playwright test`), configured with `webServer: { command: 'pnpm dev', port: 1420 }`.

- [ ] **Step 10: Format, run everything, commit**

```bash
cd sdm-tauri/web && pnpm exec prettier -w src e2e vite.config.ts && pnpm test && pnpm exec playwright test && pnpm build
```

Expected: unit tests pass, the e2e passes, `pnpm build` produces `dist/`. Then in a terminal the user can see: `cd ../ && pnpm tauri dev`, add a real small file URL (`https://proof.ovh.net/files/1Mb.dat`), and confirm the row completes and the file appears in `~/Downloads/SDM/`.

```bash
git add sdm-tauri docs/tauri-skeleton-mock.png
git commit -m "Add the Svelte frontend: store, windowed list, canvas bars and mock bridge

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 14: Simulation flag and the performance gate

**Files:**

- Create: `sdm-core/crates/sdm-app/src/simulation.rs`
- Modify: `sdm-core/crates/sdm-app/src/lib.rs`, `sdm-tauri/src-tauri/src/lib.rs`
- Create: `docs/tauri-performance.md`

**Interfaces:**

- Produces: `pub fn simulated_dependencies(host: &dyn Host, items: usize, active: usize) -> Dependencies` (a `FakeOrigin`-backed engine pre-loaded with `items` items, of which `active` stream slowly) and a debug-only `--simulate <items> <active>` launch flag.

- [ ] **Step 1: Write the failing test**

`simulation.rs` ends with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{FakeHost, RecordingSink};
    use std::sync::Arc;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_simulation_loads_the_requested_items_with_the_requested_active_count() {
        let data = tempfile::tempdir().unwrap();
        let downloads = tempfile::tempdir().unwrap();
        let host = Arc::new(FakeHost::new(data.path().into(), downloads.path().into()));
        let app = crate::App::new(host.clone(), simulated_dependencies(host.as_ref(), 40, 5)).unwrap();
        populate(&app, 40, 5).await.unwrap();
        let sink = Arc::new(RecordingSink::default());
        app.attach(sink.clone()).await.unwrap();
        for _ in 0..3 {
            app.tick().await.unwrap();
        }
        let patches = sink.patches();
        let structure = patches[0].structure.as_ref().unwrap();
        let total: usize = structure.packages.iter().map(|p| p.items.len()).sum();
        assert_eq!(total, 40);
        let running = structure
            .packages
            .iter()
            .flat_map(|p| &p.items)
            .filter(|i| matches!(i.state, crate::protocol::ItemStateDto::Running))
            .count();
        assert!(running <= 5, "never more than the active count: {running}");
    }

    #[test]
    fn bad_arguments_are_rejected() {
        assert!(parse_simulate_args(&["--simulate".into(), "0".into(), "0".into()]).is_err());
        assert!(parse_simulate_args(&["--simulate".into(), "10".into(), "20".into()]).is_err());
        assert!(parse_simulate_args(&["--simulate".into(), "x".into(), "1".into()]).is_err());
        assert_eq!(parse_simulate_args(&["--simulate".into(), "500".into(), "50".into()]).unwrap(), Some((500, 50)));
        assert_eq!(parse_simulate_args(&[]).unwrap(), None);
    }
}
```

- [ ] **Step 2: Implement**

Design: a `FakeOrigin` whose responses are throttled so active items stay active for minutes. `FakeOrigin` serves its whole payload for any URL instantly; to hold 50 items active, wrap it in a transport that yields the body in small chunks with a `tokio::time::sleep`-free pacing: a stream that emits 16 KiB per poll of a shared `tokio::time::interval(200 ms)` is real time in a _debug-only manual harness_, which the rule "no test sleeps on a real clock" allows because this code never runs in tests that assert timing (the unit test above uses only item counts). Implement `SlowOrigin { payload_len: usize, chunk: usize }: HttpTransport` returning a `RangeResponse` whose body is `futures::stream::unfold` over an `interval`. Check `HttpTransport`, `RangeRequest`, `RangeResponse`, `BodyStream` in `sdm-engine/src/http_transport.rs` and copy how `FakeOrigin` builds a `RangeResponse` (status 206 with `total_size`, `accepts_ranges: true`).

`populate(app, items, active)` calls `add_url` with `https://sim.invalid/file-N.bin` for each item as separate packages (500 packages stresses the list the most; use 10 packages of 50 items instead via `app.engine`-level `add` — expose a crate-private `App::add_package(DownloadPackage)` used by `add_url` and `populate`), then pauses all but `active` by setting `max_concurrent` to `active` through `update_settings`-like internal call: simplest is to construct the engine with `EngineSettings::new(active, 4, active*4, folder)` inside `simulated_dependencies`' app (add `App::new_with_settings`). Keep `populate` and `parse_simulate_args(&[String]) -> Result<Option<(usize, usize)>, AppError>` (`Some` only when `--simulate` is present; requires `1 <= active <= items <= 5000`).

In the shell `run()`: `#[cfg(debug_assertions)]` read `std::env::args()`; if `--simulate` parses, build `App::new(host, simulated_dependencies(...))` and `populate` instead of `App::production`.

Run `cargo test -p sdm-app simulation` until green.

- [ ] **Step 3: Measure on the Mac**

Run the app with `cd sdm-tauri && pnpm tauri dev -- -- --simulate 500 50` (the doubled `--` passes args to the app binary; confirm with the Tauri CLI docs). Measure with `ps`:

```bash
# idle: after the app is up with --simulate 500 0 ... (use active=1 and pause all via the UI for the idle case)
top -pid $(pgrep -f sdm-tauri | head -1) -l 20 -stats pid,cpu,mem | tail -5
```

Record for both loads: CPU of the main process plus `WebKit.WebContent` helper processes (sum them: `ps -A -o %cpu,rss,comm | grep -iE 'sdm|WebContent'`), and peak RSS. Run each for 60 s, report the median of the CPU samples.

- [ ] **Step 4: Write `docs/tauri-performance.md`**

Template:

```markdown
# Tauri skeleton performance (spec 1 gate)

Date, machine (Intel Mac model, macOS version), build (debug or release; **use `--release` for the gate**: `pnpm tauri build --debug false` or `cargo build --release`), load (500 items, 50 active, fake origin, 1.6 MB/s each).

| Platform / webview                          | Load              | CPU (% of one core, median of 60 s) | Peak RSS (MB) |
| ------------------------------------------- | ----------------- | ----------------------------------- | ------------- |
| macOS / WKWebView                           | idle (500 paused) | …                                   | …             |
| macOS / WKWebView                           | 50 active         | …                                   | …             |
| Windows x64 VM / WebView2                   | idle              | …                                   | …             |
| Windows x64 VM / WebView2                   | 50 active         | …                                   | …             |
| SwiftUI app, same engine load (if drivable) | 50 active         | …                                   | …             |

Thresholds (spec §10): idle < 1 % of a core; 50 active < 15 % of a core and < 400 MB; ≤ 2× the SwiftUI CPU where measured.
Result: PASS / FAIL per row. If any row fails: record the profile and the first optimization tried (coarsened ranges, spec §4.3) before spec 2 starts.
```

Fill it with real numbers; do not write PASS without them. The Windows rows are filled in Task 15.

- [ ] **Step 5: Commit**

```bash
git add sdm-core/crates/sdm-app sdm-tauri docs/tauri-performance.md
git commit -m "Add the --simulate load harness and the macOS performance measurements

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 15: Cross-builds, the Windows run, and closing spec 1

**Files:**

- Create: `scripts/build-tauri.sh`
- Modify: `docs/tauri-roadmap.md`, `docs/tauri-performance.md`, `docs/superpowers/specs/2026-10-14-tauri-foundation-design.md`, `CLAUDE.md` (one short pointer paragraph)

**Interfaces:** none new.

- [ ] **Step 1: The build script**

`scripts/build-tauri.sh`:

```bash
#!/bin/bash
# Builds the Tauri app: ./scripts/build-tauri.sh mac | windows | linux
set -euo pipefail
cd "$(dirname "$0")/../sdm-tauri"
export PATH="$HOME/.cargo/bin:$(brew --prefix llvm 2>/dev/null)/bin:$PATH"
pnpm --dir web install --frozen-lockfile
case "${1:-}" in
  mac)
    pnpm tauri build
    ;;
  windows)
    pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc
    echo "Installer: $(ls ../sdm-core/target/x86_64-pc-windows-msvc/release/bundle/nsis/*.exe)"
    ;;
  linux)
    docker build -q -t sdm-linux-build -f docker/linux-build.Dockerfile docker >/dev/null
    docker run --rm -v "$PWD/..":/work -w /work/sdm-tauri sdm-linux-build bash -c \
      "curl -fsSL https://get.pnpm.io/install.sh | sh - >/dev/null && export PATH=/root/.local/share/pnpm:\$PATH && pnpm install --frozen-lockfile && pnpm --dir web install --frozen-lockfile && pnpm tauri build --bundles deb"
    ;;
  *)
    echo "usage: $0 mac|windows|linux" >&2
    exit 2
    ;;
esac
```

`chmod +x scripts/build-tauri.sh`. The Linux branch is **optional** per the spec (a compile check is the requirement; a `.deb` bundle is a bonus): if it fails for a reason unrelated to this code (container networking, pnpm bootstrap), fall back to `scripts/check-linux.sh --shell` and record that in the roadmap.

- [ ] **Step 2: Build all three**

Run, one at a time:
`scripts/build-tauri.sh mac` — expect a `.app` under `sdm-core/target/release/bundle/macos/` (the workspace's `target/`).
`scripts/build-tauri.sh windows` — expect `…/nsis/SDM_0.1.0_x64-setup.exe`. A first-run cross-compile downloads the Windows SDK; it is slow once.
`scripts/check-linux.sh --shell` — expect PASS (the shell compiles against `libwebkit2gtk-4.1-dev`).

If the Windows build fails inside a C dependency, report the exact error; do not work around it silently.

- [ ] **Step 3: Run it in the Windows VM (manual, the user)**

Ask the user to copy the installer into the VM, install it, launch SDM, and check: (1) the window opens; (2) adding `https://proof.ovh.net/files/1Mb.dat` downloads it and the file appears in the Windows Downloads folder under `SDM`; (3) pausing and resuming a larger file (`10Mb.dat`) works; (4) closing and reopening the app restores the list from `%APPDATA%\SDM\state.json`; (5) `--simulate 500 50` is launched via `"C:\Program Files\SDM\SDM.exe" --simulate 500 50` **only if a release build accepts the flag**: the flag is debug-only, so for the Windows measurement cross-compile a debug-assertions build with `pnpm tauri build --debug --runner cargo-xwin --target x86_64-pc-windows-msvc` and say so in the performance doc. Record CPU and peak memory in Task Manager (Details tab, add "Memory (private working set)"; use the process tree of `SDM.exe` and `msedgewebview2.exe`) into `docs/tauri-performance.md`.

Do not mark spec 1 done until the user confirms these five results.

- [ ] **Step 4: Verify the SwiftUI app is unaffected**

Run: `./test.sh 2>&1 | tail -20` and `./build.sh --no-install 2>&1 | tail -10`.
Expected: both succeed (the Rust workspace tests including `sdm-app`, the Windows and Linux checks, the Swift tests, and the Xcode build).

- [ ] **Step 5: Update the spec to match what was built**

In `docs/superpowers/specs/2026-10-14-tauri-foundation-design.md` apply the five deviations listed at the top of this plan: in 4.3 add `full` to `Patch` and change `seq` to `u32`; in 4.3/4.5/5 replace "commands return `Result<Option<Patch>, AppError>`" with "commands return `Result<(), AppError>`; every patch goes through the attached sink"; keep section numbering. Format with `prettier -w`.

- [ ] **Step 6: Update the roadmap**

In `docs/tauri-roadmap.md`: set spec 1's status to `done`, link `docs/tauri-performance.md`, add a Log line with the date and what was verified, and add a "Spec 2 next" note listing the facts a later session needs (the `sdm-app` public API is in `crates/sdm-app/src/app.rs`; protocol in `protocol.rs`; the frontend store in `web/src/lib/store.svelte.ts`; how to run: `pnpm tauri dev`, `scripts/build-tauri.sh windows`).

Add to the root `CLAUDE.md` a two-sentence pointer under "Current state": the Tauri UI exists in `sdm-tauri/`, tracked in `docs/tauri-roadmap.md`.

- [ ] **Step 7: Commit and report**

```bash
git add -A scripts docs CLAUDE.md
git commit -m "Close Tauri spec 1: cross-build script, measurements, roadmap

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 16: Per-platform `ffmpeg`, `ffprobe` and `qjs` downloads

This task is **not on the skeleton's critical path** and may be moved to spec 2 if Task 15 is blocked. It implements decision T7.

**Files:**

- Create: `sdm-core/crates/sdm-resolve/src/platform_tools.rs`
- Modify: `sdm-core/crates/sdm-resolve/src/managed_binaries.rs`, `managed_binaries_types.rs`, `lib.rs`, `Cargo.toml`
- Test: `sdm-core/crates/sdm-resolve/src/platform_tools.rs` (unit) and an integration test using the existing fake fetcher

**Interfaces:**

- Produces: `pub struct PlatformTool { pub name: &'static str, pub url: &'static str, pub sha256: &'static str, pub archive: ArchiveKind, pub member: &'static str }`, `pub enum ArchiveKind { None, Zip, TarXz }`, `pub fn platform_tools() -> Vec<PlatformTool>` (empty on macOS), and `ManagedBinaries::provision_platform_tools()`.

- [ ] **Step 1: Research and pin the sources**

For Windows x64 and Linux x64/aarch64 choose a stable, versioned, checksum-published source for each of `ffmpeg`, `ffprobe`, `qjs` (QuickJS-ng): for example the BtbN `FFmpeg-Builds` release archives (`ffmpeg-n<version>-latest-win64-gpl-<version>.zip`, `…-linux64-gpl-….tar.xz`) and the `quickjs-ng/quickjs` GitHub release assets. For each, record the exact tag/version, URL, archive layout (the path of the executable inside), and SHA-256 (from the published checksum file, or compute it from a download you inspected). Put the findings in a table at the top of `platform_tools.rs` as a doc comment, with the date, and **never** pin a `latest` URL: a floating URL cannot be verified. Re-check licenses (FFmpeg GPL builds: the app downloads them at run time, it does not redistribute them, which is the reason for T7) and confirm with the user before pinning a source they would not expect.

- [ ] **Step 2: Tests for extraction and verification, first**

Write failing tests that build tiny in-memory archives and assert: a zip containing `bin/ffmpeg.exe` extracts exactly that member; a `.tar.xz` containing `ffmpeg-x/bin/ffmpeg` extracts it; a hash mismatch fails before anything is written; a member path containing `..` or an absolute path is refused (zip-slip); an archive with no matching member reports an error naming the member. Use the `tempfile` crate and the existing fake `BinaryFetching`.

- [ ] **Step 3: Implement**

Add the archive crates with the package manager (`cargo add -p sdm-resolve zip --no-default-features --features deflate`, `cargo add -p sdm-resolve tar xz2` or `lzma-rust2` if a pure-Rust xz decoder is preferred for cross-compiling to Windows; prefer pure Rust to avoid a C build under `cargo-xwin`; verify current crate versions and features in Context7). Implement `PlatformTool` extraction with a hard size cap (reject archives that expand beyond 512 MB), verify SHA-256 **before** extracting, write via `write_atomically`, mark executable on Unix, and record versions in `BinariesManifest` so `ManagedBinariesStatus` shows them. Call `provision_platform_tools()` from `provision_binaries()` when `platform_tools()` is non-empty and the manifest says the tool is missing or outdated.

- [ ] **Step 4: Verify, lint, commit**

Run: `cd sdm-core && cargo test -p sdm-resolve && cargo clippy -p sdm-resolve --all-targets && ../scripts/check-windows.sh && ../scripts/check-linux.sh`
Expected: all pass. Manual: on the Windows VM, add a YouTube URL once spec 3 exists, or run `provision_binaries` and check `%APPDATA%\SDM\bin\ffmpeg.exe` exists.

```bash
git add sdm-core
git commit -m "Download ffmpeg, ffprobe and qjs on Windows and Linux with checksum verification

Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

## Self-review (run 2026-10-14)

**Spec coverage.** §3 architecture → Tasks 6–12. §4.1–4.8 → Tasks 6–11 (4.4 visibility and histories in Task 10; 4.6 notifications in Task 8; 4.7 settings in Task 9; 4.8 tick-failure logging in Task 10's `heartbeat`). §5 shell → Task 12 (panic boundary via `guarded`; Host impl; specta bindings committed). §6 frontend → Task 13 (Bridge, mock, store, windowing, canvas, CSS variables from `theme_css`). §7 portability → Tasks 2–5 (+4 for filename rules, +3 for the yt-dlp asset), T7 → Task 16. §8 tooling → Tasks 1, 5, 15. §9 error handling → Tasks 6, 10, 12, 13. §10 tests and criteria → Tasks 5, 10, 13, 14, 15; criterion 3 (performance gate) → Task 14 + 15; criterion 5 (SwiftUI unaffected) → Task 15 step 4; criterion 6 → Task 15. No gaps found.

**Placeholder scan.** Searched for TBD/TODO/"similar to": none. Two places deliberately defer detail to a compile-time or docs check and say so: Task 3 step 4 (the remaining lines of `child_environment_from` after the PATH merge, which the implementer reads from the file) and Task 13 steps 7–8 (the exact shape of the generated `bindings.ts`, and component bodies described by behavior and constraints rather than full source). Component markup is the part of this plan least pinned by code; the store, windowing, format and bridge logic that the components depend on are fully specified and tested.

**Type consistency.** `Patch` fields (`seq`, `full`, `structure`, `items`, `packages`, `stats`, `histories`) match in Tasks 6, 7, 10, 13. `Differ::{reset, diff, full}` and `Delta` match between Tasks 7 and 10. `Host` and `PatchSink` signatures match between Tasks 8, 10, 12. `App` method names match between Task 10's interface block, its tests, and Task 12's commands. `SettingsDto.download_folder` (Rust) ↔ `downloadFolder` (TS, via `rename_all = "camelCase"`) ↔ `validate_folder` argument name `downloadFolder`.

**Review Focus coverage.** (1) URL names: Task 9 tests (`unusable_names_fall_back_and_never_escape`, `a_windows_reserved_name...`) and Task 4. (2) Lost/reordered patches: Task 10 (`a_failed_send_makes_the_next_patch_full`, `seq_increases_by_exactly_one_per_patch`) and Task 13 (gap test). (3) Removed item with open details / bad ids: Task 10 (`detail_histories_...ignore_bad_ids`, `commands_reject_bad_ids...`; `remove_item` prunes `detail_items`). (4) Wraparound and non-full-first patch: Task 13 tests. (5) Bad download folder: Task 9 `invalid_updates_are_rejected_and_change_nothing` and Task 10 `settings_updates_apply...`.

## Execution

Task order matters through Task 12; Tasks 13 and 14 need 12; Task 15 needs everything; Task 16 is independent of 6–15 once Tasks 2–3 are done and can run in parallel with them if desired.
