//! `GrabberSession`: the FFI wrapper over `sdm_grabber::GrabberSession`.
//!
//! The session is pull-only: the app calls [`GrabberSession::snapshot`] after
//! each mutation, as `GrabberController` does today. Its settings
//! (`quality_preferences`, `ffmpeg_available`, the yt-dlp closures' values)
//! are pushed in through setters and read by the session's provider closures
//! from small stored copies, because those closures run while the session's
//! lock is held and must neither block nor call back into the session.
//!
//! Like the engine's, the session's state lock recovers from poisoning: a
//! panic raised while it is held is reported to that caller as
//! [`CoreError::Panicked`], and later calls (the polled `snapshot` included)
//! carry on over whatever the half-run update left, which is a valid state.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use sdm_core::{FormatChoice, LinkResolver, QualityPreferences, SourceUrl};
use sdm_grabber::{GrabberBudget, GrabberSnapshot, LinkProber, ReqwestProbeTransport};
use sdm_resolve::CookieSource;
use uuid::Uuid;

use crate::error::CoreError;
use crate::runtime::{guard, run};
use crate::shared::{GrabberSettings, Toolchain};

/// How to build a production [`GrabberSession`].
#[derive(Debug, Clone, uniffi::Record)]
pub struct GrabberConfig {
    pub budget: GrabberBudget,
    /// Whether stage 2 (the magic-byte sniff) runs after a link looks alive.
    pub deep_sniff_enabled: bool,
    /// Where `yt-dlp` is looked for after a Settings override. Empty means
    /// the default install locations.
    pub binary_search_paths: Vec<PathBuf>,
    pub quality_preferences: QualityPreferences,
    /// Whether `ffmpeg` is on disk, so muxed formats are offered.
    pub ffmpeg_available: bool,
    /// Initial yt-dlp cookie source; change it later with `set_cookie_source`.
    pub cookie_source: CookieSource,
    /// Initial cap on playlist videos resolved (Swift's default was 50).
    pub max_playlist_videos: u32,
    /// Initial extra arguments for every yt-dlp invocation.
    pub yt_dlp_extra_arguments: Vec<String>,
}

/// Refuses a budget the session would trap on.
pub fn validate_budget(budget: &GrabberBudget) -> Result<(), CoreError> {
    let at_least_one = |name: &str, value: usize| {
        if value >= 1 {
            Ok(())
        } else {
            Err(CoreError::invalid(name, "must be at least 1"))
        }
    };
    at_least_one(
        "globalMaxConcurrentProbes",
        budget.global_max_concurrent_probes,
    )?;
    at_least_one("maxConcurrentPerHost", budget.max_concurrent_per_host)?;
    at_least_one("maxConcurrentResolves", budget.max_concurrent_resolves)
}

/// The link grabber's session.
#[derive(uniffi::Object)]
pub struct GrabberSession {
    session: sdm_grabber::GrabberSession,
    tools: Toolchain,
    settings: Arc<GrabberSettings>,
}

impl GrabberSession {
    /// A session over an injected prober and resolver, for tests. `new`
    /// builds the production ones and calls this. `clock` stands in for
    /// `SystemTime::now` when a row enters the session.
    pub fn with_dependencies(
        prober: LinkProber,
        budget: GrabberBudget,
        resolver: Option<Arc<dyn LinkResolver>>,
        settings: Arc<GrabberSettings>,
        tools: Toolchain,
        clock: Option<Arc<dyn Fn() -> SystemTime + Send + Sync>>,
    ) -> Result<Arc<Self>, CoreError> {
        validate_budget(&budget)?;
        let session = guard(|| {
            let quality = Arc::clone(&settings);
            let ffmpeg = Arc::clone(&settings);
            let mut builder = sdm_grabber::GrabberSession::builder(prober)
                .budget(budget)
                .quality_preferences(move || quality.quality_preferences())
                .ffmpeg_available(move || ffmpeg.ffmpeg_available());
            if let Some(resolver) = resolver {
                builder = builder.resolver(resolver);
            }
            if let Some(clock) = clock {
                builder = builder.clock(move || clock());
            }
            builder.build()
        })?;
        Ok(Arc::new(Self {
            session,
            tools,
            settings,
        }))
    }

    /// Test hook: panics inside the guarded async path.
    #[doc(hidden)]
    pub async fn panic_for_testing(&self, message: String) -> Result<(), CoreError> {
        run(async move { panic!("{message}") }).await
    }

    /// Test hook: panics inside the guarded synchronous path.
    #[doc(hidden)]
    pub fn panic_synchronously_for_testing(&self, message: String) -> Result<(), CoreError> {
        guard(|| panic!("{message}"))
    }
}

#[uniffi::export]
impl GrabberSession {
    /// Builds a session with the production collaborators: the `reqwest`
    /// probe transport and the yt-dlp resolver.
    #[uniffi::constructor]
    pub fn new(config: GrabberConfig) -> Result<Arc<Self>, CoreError> {
        let tools = Toolchain::configured(
            config.binary_search_paths,
            config.cookie_source,
            config.max_playlist_videos,
            config.yt_dlp_extra_arguments,
        );
        let settings = Arc::new(GrabberSettings::new(
            config.quality_preferences,
            config.ffmpeg_available,
        ));
        let prober = LinkProber::new(
            Arc::new(ReqwestProbeTransport::new()),
            config.deep_sniff_enabled,
        );
        let resolver = tools.resolver();
        Self::with_dependencies(prober, config.budget, Some(resolver), settings, tools, None)
    }

    // MARK: Settings the session's closures read

    pub fn set_cookie_source(&self, source: CookieSource) -> Result<(), CoreError> {
        guard(|| self.tools.settings.set_cookie_source(source))
    }

    pub fn set_max_playlist_videos(&self, count: u32) -> Result<(), CoreError> {
        guard(|| self.tools.settings.set_max_playlist_videos(count as usize))
    }

    pub fn set_yt_dlp_extra_arguments(&self, arguments: Vec<String>) -> Result<(), CoreError> {
        guard(|| self.tools.settings.set_extra_arguments(arguments))
    }

    pub fn set_binary_override(
        &self,
        name: String,
        path: Option<PathBuf>,
    ) -> Result<(), CoreError> {
        guard(|| self.tools.set_binary_override(&name, path))?
    }

    /// Replaces the quality preferences the session reads when it picks a
    /// format for a newly resolved media row.
    pub fn set_quality_preferences(
        &self,
        preferences: QualityPreferences,
    ) -> Result<(), CoreError> {
        guard(|| self.settings.set_quality_preferences(preferences))
    }

    pub fn set_ffmpeg_available(&self, available: bool) -> Result<(), CoreError> {
        guard(|| self.settings.set_ffmpeg_available(available))
    }

    // MARK: Ingest

    /// Extracts links from pasted text, de-duplicates them against the
    /// session, then probes the new ones.
    pub async fn ingest_text(&self, text: String) -> Result<(), CoreError> {
        let session = self.session.clone();
        run(async move { session.ingest_text(&text).await }).await
    }

    pub async fn ingest_urls(&self, urls: Vec<SourceUrl>) -> Result<(), CoreError> {
        let session = self.session.clone();
        run(async move { session.ingest_urls(&urls).await }).await
    }

    pub async fn recheck_failed(&self) -> Result<(), CoreError> {
        let session = self.session.clone();
        run(async move { session.recheck_failed().await }).await
    }

    /// Re-probes / re-resolves exactly these rows.
    pub async fn recheck(&self, ids: HashSet<Uuid>) -> Result<(), CoreError> {
        let session = self.session.clone();
        run(async move { session.recheck(&ids).await }).await
    }

    // MARK: Snapshot and edits

    pub fn snapshot(&self) -> Result<GrabberSnapshot, CoreError> {
        guard(|| self.session.snapshot())
    }

    pub fn set_format_choice(&self, choice: FormatChoice, id: Uuid) -> Result<(), CoreError> {
        guard(|| self.session.set_format_choice(choice, id))
    }

    pub fn set_known_download_urls(&self, urls: HashSet<SourceUrl>) -> Result<(), CoreError> {
        guard(|| self.session.set_known_download_urls(urls))
    }

    pub fn remove_link(&self, id: Uuid) -> Result<(), CoreError> {
        guard(|| self.session.remove_link(id))
    }

    pub fn remove_package(&self, id: Uuid) -> Result<(), CoreError> {
        guard(|| self.session.remove_package(id))
    }

    pub fn clear(&self) -> Result<(), CoreError> {
        guard(|| self.session.clear())
    }

    /// Drops rows that entered the session more than `max_age` before `now`
    /// (the auto-clear setting). `now` comes from the caller because the
    /// session has no clock of its own here.
    pub fn prune_older_than(&self, max_age: Duration, now: SystemTime) -> Result<(), CoreError> {
        guard(|| self.session.prune_older_than(max_age, now))
    }

    pub fn move_link(&self, id: Uuid, package_id: Uuid) -> Result<(), CoreError> {
        guard(|| self.session.move_link(id, package_id))
    }

    pub fn rename_package(&self, id: Uuid, new_name: String) -> Result<(), CoreError> {
        guard(|| self.session.rename_package(id, &new_name))
    }

    pub fn merge_packages(&self, source_id: Uuid, destination_id: Uuid) -> Result<(), CoreError> {
        guard(|| self.session.merge_packages(source_id, destination_id))
    }

    pub fn split_package(&self, id: Uuid) -> Result<(), CoreError> {
        guard(|| self.session.split_package(id))
    }

    /// Stops in-flight probes and resolves; the header's "Cancel Checks".
    pub fn cancel_checks(&self) -> Result<(), CoreError> {
        guard(|| self.session.cancel_checks())
    }
}
