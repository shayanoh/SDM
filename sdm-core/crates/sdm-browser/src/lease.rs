//! Giving yt-dlp the extension's cookie file. yt-dlp rewrites the cookie file
//! it is given when it exits, and the host may be replacing the real file at
//! the same moment, so every run gets its own private copy that is deleted
//! afterwards.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use sdm_resolve::CookieLease;

use crate::cookies::cookie_file_path;

const LEASE_FOLDER: &str = "cookie-leases";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookieFileState {
    /// The extension has never synced (or the file was removed).
    Missing,
    Present {
        modified: SystemTime,
    },
}

pub fn cookie_file_state(data_dir: &Path) -> CookieFileState {
    match std::fs::metadata(cookie_file_path(data_dir)).and_then(|m| m.modified()) {
        Ok(modified) => CookieFileState::Present { modified },
        Err(_) => CookieFileState::Missing,
    }
}

/// Deletes its file when the run that leased it ends.
struct LeasedCopy(PathBuf);

impl Drop for LeasedCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// `--cookies <private copy>` for one run, or an "unavailable" lease when the
/// extension's file does not exist or cannot be copied.
pub fn lease_for_chrome(data_dir: &Path) -> CookieLease {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let folder = data_dir.join(LEASE_FOLDER);
    let copy = folder.join(format!(
        "{}-{}.txt",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    match copy_private(&cookie_file_path(data_dir), &folder, &copy) {
        Ok(()) => CookieLease::with_guard(
            vec!["--cookies".to_owned(), copy.to_string_lossy().into_owned()],
            Box::new(LeasedCopy(copy)),
        ),
        Err(_) => CookieLease::chrome_unavailable(),
    }
}

fn copy_private(source: &Path, folder: &Path, copy: &Path) -> std::io::Result<()> {
    let mut input = std::fs::File::open(source)?;
    std::fs::create_dir_all(folder)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = options.open(copy)?;
    std::io::copy(&mut input, &mut output)?;
    Ok(())
}

/// Removes copies a crash left behind. Called once at startup, before any
/// run can hold one.
pub fn sweep_stale_leases(data_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(data_dir.join(LEASE_FOLDER)) else {
        return;
    };
    for entry in entries.flatten() {
        let _ = std::fs::remove_file(entry.path());
    }
}
