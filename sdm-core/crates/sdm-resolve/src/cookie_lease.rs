//! The cookie arguments for one yt-dlp run. A provider hands out a lease per
//! run; whatever the lease owns (a temporary copy of a cookie file) is
//! released when the run is over and the lease is dropped.

use std::sync::Arc;

pub struct CookieLease {
    arguments: Vec<String>,
    chrome_unavailable: bool,
    _guard: Option<Box<dyn std::any::Any + Send + Sync>>,
}

impl CookieLease {
    /// Arguments that own nothing, e.g. `--cookies-from-browser firefox`.
    pub fn plain(arguments: Vec<String>) -> Self {
        Self {
            arguments,
            chrome_unavailable: false,
            _guard: None,
        }
    }

    /// Arguments plus something to drop when the run ends.
    pub fn with_guard(arguments: Vec<String>, guard: Box<dyn std::any::Any + Send + Sync>) -> Self {
        Self {
            arguments,
            chrome_unavailable: false,
            _guard: Some(guard),
        }
    }

    /// The cookie source is Chrome but the extension has not written its
    /// cookie file: no arguments, and a sign-in failure should say why.
    pub fn chrome_unavailable() -> Self {
        Self {
            arguments: Vec::new(),
            chrome_unavailable: true,
            _guard: None,
        }
    }

    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    pub fn is_chrome_unavailable(&self) -> bool {
        self.chrome_unavailable
    }
}

/// Called once per yt-dlp run.
pub type CookieProvider = Arc<dyn Fn() -> CookieLease + Send + Sync>;
