//! The Tauri side of the Chrome extension's install flow: the status the
//! setup window shows, preparing the folder to load unpacked, registering the
//! native-messaging host with Chrome, and opening Chrome's extensions page.
//! Spec `2026-10-26-tauri-browser-integration-design.md` §4.3.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use sdm_app::AppError;
use sdm_app::chrome::chrome_extension_dto;
use sdm_app::protocol::ChromeExtensionDto;
use sdm_browser::install::{
    Platform, bundled_version, manifest_is_usable, manifest_path, prepare_extension_folder,
    write_manifest,
};
use tauri::{AppHandle, Manager};

fn failed(what: &str, error: impl std::fmt::Display) -> AppError {
    AppError::failed(format!("{what}: {error}"))
}

fn data_dir() -> Result<PathBuf, AppError> {
    crate::handoff::data_dir().ok_or_else(|| AppError::failed("could not find SDM's data folder"))
}

/// The extension folder shipped with the app: the bundle's resources, or the
/// source tree when running from a build.
fn bundled_extension_dir(app: &AppHandle) -> Option<PathBuf> {
    let shipped = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join("chrome-extension"));
    let source_tree = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SDMChromeExtension");
    [shipped, Some(source_tree)]
        .into_iter()
        .flatten()
        .find(|dir| dir.join("manifest.json").is_file())
}

/// The binary Chrome runs as the host: this app (the AppImage file itself
/// when there is one).
fn host_binary() -> Result<PathBuf, AppError> {
    let current = std::env::current_exe().map_err(|e| failed("could not find SDM's program", e))?;
    Ok(crate::handoff::app_executable(
        std::env::var_os("APPIMAGE"),
        current,
    ))
}

/// The folder Chrome's profile root hangs off, for `manifest_path`.
fn platform_dir(data_dir: &Path) -> PathBuf {
    let dir = if cfg!(target_os = "macos") {
        dirs::data_dir()
    } else {
        dirs::config_dir()
    };
    dir.unwrap_or_else(|| data_dir.to_path_buf())
}

/// Writes the native-messaging manifest and, on Windows, the registry value
/// that points Chrome at it.
fn register_host(data_dir: &Path) -> Result<(), AppError> {
    let manifest = manifest_path(Platform::current(), &platform_dir(data_dir), data_dir);
    write_manifest(&manifest, &host_binary()?)
        .map_err(|e| failed("could not write the host manifest", e))?;
    #[cfg(windows)]
    register_in_registry(&manifest)?;
    Ok(())
}

#[cfg(windows)]
fn register_in_registry(manifest: &Path) -> Result<(), AppError> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let path = format!(
        r"Software\Google\Chrome\NativeMessagingHosts\{}",
        sdm_browser::install::HOST_NAME
    );
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(path)
        .map_err(|e| failed("could not register the host with Chrome", e))?;
    key.set_value("", &manifest.to_string_lossy().to_string())
        .map_err(|e| failed("could not register the host with Chrome", e))
}

/// At startup: make sure Chrome can find the host, without overwriting a
/// working manifest (the SwiftUI app writes the same file on a Mac).
pub fn ensure_host_registered() {
    let Ok(data_dir) = data_dir() else {
        return;
    };
    let manifest = manifest_path(Platform::current(), &platform_dir(&data_dir), &data_dir);
    // Only when something exists to point at: a user who never set up the
    // extension gets no file in Chrome's folders.
    let wants_host = cfg!(windows) || manifest.exists();
    if wants_host
        && !manifest_is_usable(&manifest)
        && let Err(error) = register_host(&data_dir)
    {
        tracing::warn!("{error}");
    }
}

#[cfg(target_os = "macos")]
fn chrome_launcher() -> Option<(String, Vec<String>)> {
    let installed = ["/Applications/Google Chrome.app"]
        .iter()
        .map(PathBuf::from)
        .chain(dirs::home_dir().map(|h| h.join("Applications/Google Chrome.app")))
        .any(|path| path.exists());
    installed.then(|| {
        (
            "open".to_owned(),
            vec![
                "-a".into(),
                "Google Chrome".into(),
                "chrome://extensions/".into(),
            ],
        )
    })
}

#[cfg(windows)]
fn chrome_launcher() -> Option<(String, Vec<String>)> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let subkey = r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\chrome.exe";
    [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER]
        .into_iter()
        .find_map(|root| {
            RegKey::predef(root)
                .open_subkey(subkey)
                .ok()?
                .get_value::<String, _>("")
                .ok()
        })
        .filter(|path| Path::new(path).exists())
        .map(|path| (path, vec!["chrome://extensions/".to_owned()]))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn chrome_launcher() -> Option<(String, Vec<String>)> {
    let path = std::env::var_os("PATH")?;
    find_in_path(&["google-chrome-stable", "google-chrome"], &path).map(|program| {
        (
            program.to_string_lossy().into_owned(),
            vec!["chrome://extensions/".to_owned()],
        )
    })
}

/// The first of `names` found as a file in one of the `path` folders.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
fn find_in_path(names: &[&str], path: &std::ffi::OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .flat_map(|folder| names.iter().map(move |name| folder.join(name)))
        .find(|candidate| candidate.is_file())
}

pub fn chrome_installed() -> bool {
    chrome_launcher().is_some()
}

pub fn open_chrome_extensions() -> Result<(), AppError> {
    let (program, arguments) = chrome_launcher()
        .ok_or_else(|| AppError::failed("Google Chrome was not found on this computer"))?;
    std::process::Command::new(program)
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| failed("could not open Chrome", e))
}

pub fn status(app: &AppHandle) -> Result<ChromeExtensionDto, AppError> {
    let data_dir = data_dir()?;
    let bundled = bundled_extension_dir(app).and_then(|dir| bundled_version(&dir));
    Ok(chrome_extension_dto(
        &data_dir,
        bundled.as_deref(),
        chrome_installed(),
        SystemTime::now(),
    ))
}

/// Shortly after launch, opens the setup window if Chrome is installed and the
/// extension is missing or older than the shipped one, at most three times per
/// shipped version (`sdm_browser::prompt`).
pub async fn prompt_after_launch(app: AppHandle) {
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let (Ok(data_dir), Ok(dto)) = (data_dir(), status(&app)) else {
        return;
    };
    let (show, next) = sdm_browser::prompt::decide(
        &sdm_browser::prompt::load(&data_dir),
        dto.bundled_version.as_deref(),
        dto.installed_version.as_deref(),
        dto.chrome_installed,
    );
    if !show {
        return;
    }
    if let Err(error) = sdm_browser::prompt::save(&data_dir, &next) {
        tracing::warn!("could not remember the extension prompt: {error}");
    }
    if let Err(error) = crate::chrome_setup_window::open(&app) {
        tracing::warn!("{error}");
    }
}

/// Copies the extension to its stable folder and registers the host with
/// Chrome. Returns the folder to load unpacked.
pub fn prepare(app: &AppHandle) -> Result<String, AppError> {
    let data_dir = data_dir()?;
    let bundled = bundled_extension_dir(app)
        .ok_or_else(|| AppError::failed("the Chrome extension is missing from this build"))?;
    let folder = prepare_extension_folder(&bundled, &data_dir)
        .map_err(|e| failed("could not prepare the extension folder", e))?;
    register_host(&data_dir)?;
    Ok(folder.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_installed_chrome_name_wins() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("google-chrome"), "").unwrap();
        let path = std::env::join_paths([Path::new("/nonexistent"), dir.path()]).unwrap();
        assert_eq!(
            find_in_path(&["google-chrome-stable", "google-chrome"], &path),
            Some(dir.path().join("google-chrome"))
        );
        std::fs::write(dir.path().join("google-chrome-stable"), "").unwrap();
        assert_eq!(
            find_in_path(&["google-chrome-stable", "google-chrome"], &path),
            Some(dir.path().join("google-chrome-stable"))
        );
        assert_eq!(find_in_path(&["chromium"], &path), None);
    }

    #[test]
    fn the_source_tree_extension_is_found_when_nothing_is_bundled() {
        let source_tree =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SDMChromeExtension");
        assert!(source_tree.join("manifest.json").is_file());
    }
}
