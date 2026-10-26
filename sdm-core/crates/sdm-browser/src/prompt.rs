//! When SDM opens the extension setup window by itself: shortly after launch,
//! if Chrome is installed and the extension is missing or older than the one
//! shipped, at most three times per shipped version (the SwiftUI app's rule).

use std::path::Path;

use serde::{Deserialize, Serialize};

pub const MAX_PROMPTS: u32 = 3;
const FILE_NAME: &str = "chrome-setup-prompt.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PromptState {
    /// The shipped version the count below is for.
    pub last_version: Option<String>,
    pub shown_count: u32,
}

/// Whether to open the setup window now, and the state to remember. Nothing is
/// counted unless a prompt is shown.
pub fn decide(
    state: &PromptState,
    bundled: Option<&str>,
    installed: Option<&str>,
    chrome_installed: bool,
) -> (bool, PromptState) {
    let Some(bundled) = bundled else {
        return (false, state.clone());
    };
    if !chrome_installed || installed == Some(bundled) {
        return (false, state.clone());
    }
    let mut next = state.clone();
    if next.last_version.as_deref() != Some(bundled) {
        next.last_version = Some(bundled.to_owned());
        next.shown_count = 0;
    }
    if next.shown_count >= MAX_PROMPTS {
        return (false, next);
    }
    next.shown_count += 1;
    (true, next)
}

pub fn load(data_dir: &Path) -> PromptState {
    std::fs::read(data_dir.join(FILE_NAME))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(data_dir: &Path, state: &PromptState) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(state).map_err(std::io::Error::other)?;
    crate::cookies::write_cookie_file(&data_dir.join(FILE_NAME), &json)
}
