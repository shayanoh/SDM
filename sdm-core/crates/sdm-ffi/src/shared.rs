//! State shared by the engine, the grabber and the managed-binaries object:
//! the settings their yt-dlp closures read, the binary locators, and the
//! subprocess tooling built from them.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use sdm_core::{LinkResolver, QualityPreferences, WholesaleDownloader};
use sdm_engine::{FfmpegMuxer, Muxer};
use sdm_resolve::{
    BinaryLocator, CookieSource, ProcessRunner, SystemProcessRunner, YtDlpResolver,
    YtDlpWholesaleDownloader,
};

use crate::error::CoreError;

/// Locks `mutex`, recovering the value if a panic poisoned it.
///
/// Every mutex in this crate guards plain data that is replaced whole (a
/// setting, a queue of strings), so the value a panicking thread left behind
/// is still a valid one. Propagating the poison would make one panic disable
/// the setting for the rest of the process.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The values the resolver and wholesale-downloader closures read.
///
/// Swift supplied these as closures (`cookieSource`, `maxPlaylistVideos`,
/// `extraArguments`) evaluated on every use. Here the app pushes a new value
/// whenever a setting changes, and the closures read the stored copy. They
/// run inside the grabber's lock, so they only ever read a small value and
/// never call back into the session.
pub(crate) struct ResolverSettings {
    cookie_source: Mutex<CookieSource>,
    max_playlist_videos: AtomicUsize,
    extra_arguments: Mutex<Vec<String>>,
}

impl ResolverSettings {
    /// Swift's defaults: no cookies, 50 playlist videos, no extra arguments.
    pub(crate) fn new() -> Self {
        Self {
            cookie_source: Mutex::new(CookieSource::None),
            max_playlist_videos: AtomicUsize::new(50),
            extra_arguments: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn cookie_source(&self) -> CookieSource {
        *lock(&self.cookie_source)
    }

    pub(crate) fn set_cookie_source(&self, source: CookieSource) {
        *lock(&self.cookie_source) = source;
    }

    pub(crate) fn max_playlist_videos(&self) -> usize {
        self.max_playlist_videos.load(Ordering::Relaxed)
    }

    pub(crate) fn set_max_playlist_videos(&self, count: usize) {
        self.max_playlist_videos.store(count, Ordering::Relaxed);
    }

    pub(crate) fn extra_arguments(&self) -> Vec<String> {
        lock(&self.extra_arguments).clone()
    }

    pub(crate) fn set_extra_arguments(&self, arguments: Vec<String>) {
        *lock(&self.extra_arguments) = arguments;
    }
}

/// The grabber's quality preferences and ffmpeg flag, stored here so the
/// session's provider closures are a plain read.
pub struct GrabberSettings {
    quality_preferences: Mutex<QualityPreferences>,
    ffmpeg_available: AtomicBool,
}

impl GrabberSettings {
    pub fn new(preferences: QualityPreferences, ffmpeg_available: bool) -> Self {
        Self {
            quality_preferences: Mutex::new(preferences),
            ffmpeg_available: AtomicBool::new(ffmpeg_available),
        }
    }

    pub fn quality_preferences(&self) -> QualityPreferences {
        lock(&self.quality_preferences).clone()
    }

    pub fn set_quality_preferences(&self, preferences: QualityPreferences) {
        *lock(&self.quality_preferences) = preferences;
    }

    pub fn ffmpeg_available(&self) -> bool {
        self.ffmpeg_available.load(Ordering::Relaxed)
    }

    pub fn set_ffmpeg_available(&self, available: bool) {
        self.ffmpeg_available.store(available, Ordering::Relaxed);
    }
}

/// Every locator created through [`new_locator`], so a change to the managed
/// `bin/` directory can reach all of them. Swift shared one locator between
/// the grabber, the engine and `ManagedBinaries`; here each object builds its
/// own and `ManagedBinaries` invalidates them all.
static LOCATORS: Mutex<Vec<Weak<BinaryLocator>>> = Mutex::new(Vec::new());

/// A locator over `search_paths` (the default install locations when empty),
/// registered for [`invalidate_all_locators`].
pub(crate) fn new_locator(search_paths: Vec<PathBuf>) -> Arc<BinaryLocator> {
    let locator = if search_paths.is_empty() {
        BinaryLocator::new()
    } else {
        BinaryLocator::new().with_search_paths(search_paths)
    };
    let locator = Arc::new(locator);
    let mut registry = lock(&LOCATORS);
    registry.retain(|weak| weak.strong_count() > 0);
    registry.push(Arc::downgrade(&locator));
    locator
}

/// Forgets every locator's memoized lookups (including misses), as Swift's
/// `onBinariesChanged: { await locator.invalidate() }` did.
pub(crate) fn invalidate_all_locators() {
    let live: Vec<Arc<BinaryLocator>> = lock(&LOCATORS).iter().filter_map(Weak::upgrade).collect();
    for locator in live {
        locator.invalidate();
    }
}

/// The yt-dlp / ffmpeg tooling one engine or grabber owns.
pub struct Toolchain {
    pub(crate) settings: Arc<ResolverSettings>,
    pub(crate) locator: Arc<BinaryLocator>,
    runner: Arc<dyn ProcessRunner>,
}

impl Toolchain {
    pub fn new(search_paths: Vec<PathBuf>) -> Self {
        Self {
            settings: Arc::new(ResolverSettings::new()),
            locator: new_locator(search_paths),
            runner: Arc::new(SystemProcessRunner::new()),
        }
    }

    pub(crate) fn resolver(&self) -> Arc<dyn LinkResolver> {
        let cookies = Arc::clone(&self.settings);
        let playlist = Arc::clone(&self.settings);
        let extra = Arc::clone(&self.settings);
        Arc::new(YtDlpResolver::new(
            Arc::clone(&self.runner),
            Arc::clone(&self.locator),
            Arc::new(move || cookies.cookie_source()),
            Arc::new(move || playlist.max_playlist_videos()),
            Arc::new(move || extra.extra_arguments()),
        ))
    }

    pub(crate) fn wholesale_downloader(&self) -> Arc<dyn WholesaleDownloader> {
        let cookies = Arc::clone(&self.settings);
        let extra = Arc::clone(&self.settings);
        Arc::new(YtDlpWholesaleDownloader::new(
            Arc::clone(&self.runner),
            Arc::clone(&self.locator),
            Arc::new(move || cookies.cookie_source()),
            Arc::new(move || extra.extra_arguments()),
        ))
    }

    pub(crate) fn muxer(&self) -> Arc<dyn Muxer> {
        Arc::new(FfmpegMuxer::new(
            Arc::clone(&self.runner),
            Arc::clone(&self.locator),
        ))
    }

    /// Sets (or with `None` clears) the Settings override for the binary
    /// `name` (`"yt-dlp"`, `"ffmpeg"`).
    pub(crate) fn set_binary_override(
        &self,
        name: &str,
        path: Option<PathBuf>,
    ) -> Result<(), CoreError> {
        if name.is_empty() {
            return Err(CoreError::invalid("name", "the binary name is empty"));
        }
        self.locator.set_override(path, name);
        Ok(())
    }
}
