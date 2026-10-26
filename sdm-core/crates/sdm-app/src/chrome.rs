//! What the setup window and the Settings note show about the Chrome
//! extension: whether it has reported in, which version, and how old the
//! cookie file is.

use std::path::Path;
use std::time::SystemTime;

use sdm_browser::install::{ExtensionState, extension_state};
use sdm_browser::lease::{CookieFileState, cookie_file_state};
use sdm_browser::status::read_status;

use crate::protocol::ChromeExtensionDto;

/// `bundled_version` is the version of the extension folder shipped with the
/// app; `data_dir` holds the status file, the cookie file and the prepared
/// extension folder.
pub fn chrome_extension_dto(
    data_dir: &Path,
    bundled_version: Option<&str>,
    chrome_installed: bool,
    now: SystemTime,
) -> ChromeExtensionDto {
    let reported = read_status(data_dir);
    let state = match extension_state(bundled_version, reported.as_ref(), now) {
        ExtensionState::NotConnected => "notConnected",
        ExtensionState::Outdated => "outdated",
        ExtensionState::Connected => "connected",
    };
    let cookies_synced_seconds_ago = match cookie_file_state(data_dir) {
        CookieFileState::Present { modified } => {
            Some(now.duration_since(modified).map_or(0, |elapsed| {
                u32::try_from(elapsed.as_secs()).unwrap_or(u32::MAX)
            }))
        }
        CookieFileState::Missing => None,
    };
    ChromeExtensionDto {
        state: state.to_owned(),
        installed_version: reported.map(|r| r.version),
        bundled_version: bundled_version.map(str::to_owned),
        cookies_synced_seconds_ago,
        extension_folder: data_dir
            .join("ChromeExtension")
            .to_string_lossy()
            .into_owned(),
        chrome_installed,
    }
}
