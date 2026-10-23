//! The link grabber's controller: owns the native grabber session and decides
//! when its view has changed. The `App` methods that act on it are in `app.rs`.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use sdm_core::{LinkResolver, QualityPreferences, SourceUrl};
use sdm_grabber::{
    Clock, GrabberBudget, GrabberSession, LinkProber, ProbeTransport, ReqwestProbeTransport,
};

use crate::grabber_view::build_view;
use crate::protocol::{GrabberView, ToolsNotice};

/// The grabber settings until Settings exist (spec 4): the SwiftUI defaults.
/// Spec 4 replaces these constants with stored settings.
pub struct GrabberDefaults;

impl GrabberDefaults {
    pub const DEEP_SNIFF: bool = true;
    /// How long an ungrabbed row stays before it is cleared automatically.
    pub const AUTO_CLEAR_AFTER: Duration = Duration::from_secs(30 * 60);
    pub const MAX_CONCURRENT_PROBES: usize = 32;
    pub const MAX_CONCURRENT_PER_HOST: usize = 8;
    pub const MAX_CONCURRENT_RESOLVES: usize = 4;

    pub fn quality_preferences() -> QualityPreferences {
        QualityPreferences::default()
    }

    pub fn budget() -> GrabberBudget {
        GrabberBudget::new(
            Self::MAX_CONCURRENT_PROBES,
            Self::MAX_CONCURRENT_PER_HOST,
            Self::MAX_CONCURRENT_RESOLVES,
        )
    }
}

pub struct GrabberController {
    pub(crate) session: GrabberSession,
    clock: Clock,
    held_back: Mutex<Option<u32>>,
    last_sent: Mutex<Option<GrabberView>>,
    known_urls: Mutex<HashSet<SourceUrl>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl GrabberController {
    /// A controller over an injected transport (tests) with an optional clock.
    pub fn new(
        transport: Arc<dyn ProbeTransport>,
        resolver: Option<Arc<dyn LinkResolver>>,
        ffmpeg_available: Arc<AtomicBool>,
        clock: Option<Clock>,
    ) -> Arc<Self> {
        let clock: Clock = clock.unwrap_or_else(|| Arc::new(SystemTime::now));
        let prober = LinkProber::new(transport, GrabberDefaults::DEEP_SNIFF);
        let mut builder = GrabberSession::builder(prober)
            .budget(GrabberDefaults::budget())
            .quality_preferences(GrabberDefaults::quality_preferences)
            .ffmpeg_available(move || ffmpeg_available.load(Ordering::SeqCst))
            .clock({
                let clock = Arc::clone(&clock);
                move || clock()
            });
        if let Some(resolver) = resolver {
            builder = builder.resolver(resolver);
        }
        Arc::new(Self {
            session: builder.build(),
            clock,
            held_back: Mutex::new(None),
            last_sent: Mutex::new(None),
            known_urls: Mutex::new(HashSet::new()),
        })
    }

    /// The real HTTP prober.
    pub fn production(
        resolver: Option<Arc<dyn LinkResolver>>,
        ffmpeg_available: Arc<AtomicBool>,
    ) -> Arc<Self> {
        Self::new(
            Arc::new(ReqwestProbeTransport::new()),
            resolver,
            ffmpeg_available,
            None,
        )
    }

    pub(crate) fn view(&self, tools: Option<ToolsNotice>) -> GrabberView {
        build_view(
            &self.session.snapshot(),
            &GrabberDefaults::quality_preferences(),
            *lock(&self.held_back),
            tools,
        )
    }

    /// The view if it differs from the last one sent (or `force`), remembering it.
    pub(crate) fn changed_view(
        &self,
        tools: Option<ToolsNotice>,
        force: bool,
    ) -> Option<GrabberView> {
        let view = self.view(tools);
        let mut last = lock(&self.last_sent);
        if !force && last.as_ref() == Some(&view) {
            return None;
        }
        *last = Some(view.clone());
        Some(view)
    }

    /// Forgets what was sent, so the next `changed_view` reports the view.
    pub(crate) fn invalidate(&self) {
        *lock(&self.last_sent) = None;
    }

    pub(crate) fn set_held_back(&self, count: Option<u32>) {
        *lock(&self.held_back) = count;
    }

    /// Marks rows whose address is already a download; no-op when unchanged.
    pub(crate) fn set_known_download_urls(&self, urls: HashSet<SourceUrl>) {
        let mut known = lock(&self.known_urls);
        if *known != urls {
            *known = urls.clone();
            self.session.set_known_download_urls(urls);
        }
    }

    /// Drops rows older than the auto-clear age.
    pub(crate) fn prune(&self) {
        self.session
            .prune_older_than(GrabberDefaults::AUTO_CLEAR_AFTER, (self.clock)());
    }
}
