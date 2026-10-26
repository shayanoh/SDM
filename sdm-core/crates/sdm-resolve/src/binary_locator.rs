use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

type IsExecutable = Box<dyn Fn(&Path) -> bool + Send + Sync>;

#[derive(Default)]
struct State {
    overrides: HashMap<String, PathBuf>,
    /// `Some(None)` is a memoized miss.
    memo: HashMap<String, Option<PathBuf>>,
}

/// Finds `yt-dlp` / `ffmpeg`: a Settings override first, then a fixed list
/// of common install locations. Parent spec §4.5. A `.app` launched from
/// Finder has no shell `PATH`, so callers always pass the absolute path
/// this returns.
///
/// The Swift type is an actor with no suspension points, so every method is
/// a plain synchronous function over a mutex-guarded state.
pub struct BinaryLocator {
    search_paths: Vec<PathBuf>,
    is_executable: IsExecutable,
    state: Mutex<State>,
}

impl BinaryLocator {
    pub fn default_search_paths() -> Vec<PathBuf> {
        #[cfg_attr(windows, allow(unused_mut))]
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

    /// Default search paths and the real file-system executable check; use
    /// [`Self::with_search_paths`] / [`Self::with_is_executable`] to override.
    pub fn new() -> Self {
        Self {
            search_paths: Self::default_search_paths(),
            is_executable: Box::new(is_executable_file),
            state: Mutex::new(State::default()),
        }
    }

    pub fn with_search_paths(mut self, search_paths: Vec<PathBuf>) -> Self {
        self.search_paths = search_paths;
        self
    }

    /// Replaces the executable check. [`Self::locate`] calls it while holding
    /// the locator's lock, so the closure must not call back into this
    /// locator (that would deadlock).
    pub fn with_is_executable(
        mut self,
        is_executable: impl Fn(&Path) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.is_executable = Box::new(is_executable);
        self
    }

    /// Sets (or, with `None`, clears) the override for `name` and forgets
    /// that name's memoized result. Argument order follows the Swift
    /// `setOverride(_:for:)`.
    pub fn set_override(&self, path: Option<PathBuf>, name: &str) {
        let mut state = self.state.lock().unwrap();
        match path {
            Some(path) => state.overrides.insert(name.to_owned(), path),
            None => state.overrides.remove(name),
        };
        state.memo.remove(name);
    }

    pub fn invalidate(&self) {
        self.state.lock().unwrap().memo.clear();
    }

    /// The path of `name`, memoized (misses too). The executable check runs
    /// under the lock, so it must not call back into this locator.
    pub fn locate(&self, name: &str) -> Option<PathBuf> {
        let mut state = self.state.lock().unwrap();
        if let Some(cached) = state.memo.get(name) {
            return cached.clone();
        }
        let resolved = self.resolve(&state, name);
        state.memo.insert(name.to_owned(), resolved.clone());
        resolved
    }

    fn resolve(&self, state: &State, name: &str) -> Option<PathBuf> {
        if let Some(path) = state.overrides.get(name)
            && (self.is_executable)(path)
        {
            return Some(path.clone());
        }
        for directory in &self.search_paths {
            let candidate = directory.join(executable_name(name));
            if (self.is_executable)(&candidate) {
                return Some(candidate);
            }
        }
        None
    }
}

impl Default for BinaryLocator {
    fn default() -> Self {
        Self::new()
    }
}

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
