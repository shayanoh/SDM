use serde::{Deserialize, Serialize};

/// Which browser's cookie jar yt-dlp should borrow for age-restricted,
/// private, or members-only videos. Parent spec §8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CookieSource {
    None,
    Safari,
    Chrome,
    Firefox,
    Edge,
    Brave,
}

impl CookieSource {
    /// Every case, in declaration order (Swift `CaseIterable`).
    pub const ALL: [CookieSource; 6] = [
        CookieSource::None,
        CookieSource::Safari,
        CookieSource::Chrome,
        CookieSource::Firefox,
        CookieSource::Edge,
        CookieSource::Brave,
    ];

    /// The Swift `rawValue`.
    pub fn raw_value(&self) -> &'static str {
        match self {
            CookieSource::None => "none",
            CookieSource::Safari => "safari",
            CookieSource::Chrome => "chrome",
            CookieSource::Firefox => "firefox",
            CookieSource::Edge => "edge",
            CookieSource::Brave => "brave",
        }
    }

    pub fn yt_dlp_arguments(&self) -> Vec<String> {
        match self {
            CookieSource::None => Vec::new(),
            _ => vec!["--cookies-from-browser".into(), self.raw_value().into()],
        }
    }
}
