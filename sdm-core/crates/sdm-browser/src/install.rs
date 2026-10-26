//! Installing the Chrome extension's side of the link: the native-messaging
//! manifest Chrome reads, a stable copy of the extension folder to load
//! unpacked, and whether the extension has reported in.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Deserialize;

use crate::status::ExtensionStatus;

pub const HOST_NAME: &str = "com.shayanoh.sdm.chrome_extension";
/// Fixed by the `key` in the extension's `manifest.json`.
pub const EXTENSION_ID: &str = "fnnpcbmjpapeohfmpiefbocloodhlgjf";

/// The extension reports hourly; no report for this long means it is gone.
const CONNECTED_WINDOW: Duration = Duration::from_secs(2 * 60 * 60);
const EXTENSION_FOLDER: &str = "ChromeExtension";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Windows,
    Linux,
}

impl Platform {
    pub fn current() -> Platform {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }
}

/// The manifest Chrome reads to find the host binary.
pub fn manifest_json(host_path: &Path) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "name": HOST_NAME,
        "description": "Shayan's Download Manager native messaging host",
        "path": host_path,
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{EXTENSION_ID}/")],
    }))
    .expect("a JSON literal serializes")
}

/// Where the manifest goes. `platform_dir` is the folder Chrome's profile
/// root hangs off: `~/Library/Application Support` on macOS, the config
/// folder (`$XDG_CONFIG_HOME` or `~/.config`) on Linux; Windows ignores it
/// and keeps the file in SDM's `data_dir` (the registry points at it).
pub fn manifest_path(platform: Platform, platform_dir: &Path, data_dir: &Path) -> PathBuf {
    let file = format!("{HOST_NAME}.json");
    match platform {
        Platform::MacOs => platform_dir
            .join("Google/Chrome/NativeMessagingHosts")
            .join(file),
        Platform::Linux => platform_dir
            .join("google-chrome/NativeMessagingHosts")
            .join(file),
        Platform::Windows => data_dir.join(file),
    }
}

pub fn write_manifest(manifest: &Path, host_path: &Path) -> std::io::Result<()> {
    crate::cookies::write_cookie_file(manifest, &manifest_json(host_path))
}

/// Whether the manifest exists and the binary it names still does.
pub fn manifest_is_usable(manifest: &Path) -> bool {
    #[derive(Deserialize)]
    struct Manifest {
        path: PathBuf,
    }
    std::fs::read(manifest)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Manifest>(&bytes).ok())
        .is_some_and(|m| m.path.exists())
}

/// The `version` in an extension folder's `manifest.json`.
pub fn bundled_version(folder: &Path) -> Option<String> {
    #[derive(Deserialize)]
    struct Manifest {
        version: String,
    }
    let bytes = std::fs::read(folder.join("manifest.json")).ok()?;
    serde_json::from_slice::<Manifest>(&bytes)
        .ok()
        .map(|m| m.version)
}

/// Copies the bundled extension to `<data_dir>/ChromeExtension`, the folder
/// the user loads unpacked: a path inside an app bundle or install folder
/// changes with updates, Chrome remembers the path. An identical copy is left
/// untouched; a different one is replaced whole.
pub fn prepare_extension_folder(bundled: &Path, data_dir: &Path) -> std::io::Result<PathBuf> {
    if !bundled.join("manifest.json").is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "the bundled extension is missing",
        ));
    }
    let destination = data_dir.join(EXTENSION_FOLDER);
    if !same_tree(bundled, &destination) {
        if destination.exists() {
            std::fs::remove_dir_all(&destination)?;
        }
        copy_tree(bundled, &destination)?;
    }
    Ok(destination)
}

fn visible_entries(folder: &Path) -> Vec<std::fs::DirEntry> {
    let mut entries: Vec<_> = std::fs::read_dir(folder)
        .map(|read| read.flatten().collect())
        .unwrap_or_default();
    entries.retain(|e| !e.file_name().to_string_lossy().starts_with('.'));
    entries.sort_by_key(|e| e.file_name());
    entries
}

fn same_tree(a: &Path, b: &Path) -> bool {
    let (left, right) = (visible_entries(a), visible_entries(b));
    left.len() == right.len()
        && left.iter().zip(&right).all(|(x, y)| {
            if x.file_name() != y.file_name() {
                return false;
            }
            let (px, py) = (x.path(), y.path());
            match (px.is_dir(), py.is_dir()) {
                (true, true) => same_tree(&px, &py),
                (false, false) => std::fs::read(&px).ok() == std::fs::read(&py).ok(),
                _ => false,
            }
        })
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in visible_entries(from) {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionState {
    /// No report yet, or none for two hours.
    NotConnected,
    /// Reporting, but an older version than the bundled one.
    Outdated,
    Connected,
}

pub fn extension_state(
    bundled: Option<&str>,
    reported: Option<&ExtensionStatus>,
    now: SystemTime,
) -> ExtensionState {
    let Some(reported) = reported else {
        return ExtensionState::NotConnected;
    };
    let age = now
        .duration_since(reported.last_seen_at())
        .unwrap_or_default();
    if age > CONNECTED_WINDOW {
        return ExtensionState::NotConnected;
    }
    match bundled {
        Some(bundled) if bundled != reported.version => ExtensionState::Outdated,
        _ => ExtensionState::Connected,
    }
}
