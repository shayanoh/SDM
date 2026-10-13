//! Owns the batch of links being grabbed: extraction, connection-budgeted
//! probing, resolver-backed media resolution, and package clustering.
//! Port of `GrabberSession.swift`. Spec §7.5 / §6.
//!
//! # From an actor to a mutex
//!
//! Swift's `GrabberSession` is an `actor`. Here all of its state sits in
//! [`State`] behind one `std::sync::Mutex`, which is **never held across an
//! `.await`**. Every Swift stretch without a suspension is one lock scope, so
//! it stays atomic; after every `.await` the state is read again, because
//! (as in the actor) anything may have changed in between. A concurrent
//! `ingest` call started while another is still probing sees a consistent,
//! if interleaved, picture.
//!
//! The suspension points, and what the code after each assumes:
//!
//! * `probe_bounded`: `group.next()` — afterwards the link may have been
//!   removed (`links[id]` is re-read; a finished probe is merged back under
//!   its id regardless, as in Swift); the local pending list, per-host counts
//!   and active count are the call's own and are touched only there.
//! * `resolve_media`: `resolver.resolve` — afterwards the row may be gone
//!   (every write is `if let Some(row)`), and a playlist expansion replaces
//!   the origin row without checking that it still exists.
//! * `expand_playlist` / `resolve_media_bounded`: `group.next()` — afterwards
//!   rows may be gone; `cancel_requested` is re-read by every launch.
//! * `ingest_urls`: the awaits on the three helpers above, then `recluster`
//!   in a fresh scope. `recheck_failed` / `recheck` likewise.
//!
//! The session never cancels in-flight work: `cancel_checks` only sets
//! `cancel_requested`, which stops the probe and resolve loops from
//! launching *new* work. Probes and resolves already handed out finish and
//! are recorded. So, as in Swift, no cancellation token is involved (and
//! `LinkResolver::resolve` takes none). Dropping a caller's future drops the
//! call's `JoinSet`, which stops its children — the twin of a cancelled
//! Swift task group.
//!
//! Probe and resolve concurrency use Swift's own accounting (a pending list,
//! an active counter and per-host counts, all local to the call, decided in
//! one lock scope per launch), not a semaphore, so the order in which waiting
//! work launches is Swift's.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use sdm_core::{
    FormatChoice, LinkResolver, QualityPreferences, ResolveError, ResolvedMedia, ResolvedTarget,
    SourceUrl,
};
use sdm_resolve::FormatSelector;
use tokio::task::{JoinError, JoinSet};
use uuid::Uuid;

use crate::grabber_snapshot::GrabberSnapshot;
use crate::link_prober::LinkProber;
use crate::media_row::{MediaRow, MediaRowState};
use crate::package_clustering::{ClusterableLink, PackageCandidate, PackageClustering};
use crate::probed_link::{ProbedLink, Stage};
use crate::url_extractor::UrlExtractor;
use crate::url_text;
use crate::verdict::Verdict;
use crate::verdict_rules::VerdictRules;

/// Swift `GrabberSession.Budget`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrabberBudget {
    pub global_max_concurrent_probes: usize,
    pub max_concurrent_per_host: usize,
    /// How many yt-dlp `-J` extractions may run at once when expanding a
    /// playlist. Kept small and independent of the probe caps: each resolve
    /// spawns a Python process that runs a JS runtime and hits the network,
    /// so a big playlist at the probe concurrency (dozens) pins the CPU and
    /// every resolve times out. Parent spec §6.2.
    pub max_concurrent_resolves: usize,
}

impl GrabberBudget {
    /// # Panics
    ///
    /// When any limit is zero (Swift `precondition(... >= 1)`).
    pub fn new(
        global_max_concurrent_probes: usize,
        max_concurrent_per_host: usize,
        max_concurrent_resolves: usize,
    ) -> Self {
        assert!(global_max_concurrent_probes >= 1);
        assert!(max_concurrent_per_host >= 1);
        assert!(max_concurrent_resolves >= 1);
        Self {
            global_max_concurrent_probes,
            max_concurrent_per_host,
            max_concurrent_resolves,
        }
    }
}

impl Default for GrabberBudget {
    /// Swift defaults: 16 probes, 4 per host, 4 resolves.
    fn default() -> Self {
        Self::new(16, 4, 4)
    }
}

pub type QualityPreferencesProvider = Arc<dyn Fn() -> QualityPreferences + Send + Sync>;
pub type FfmpegAvailability = Arc<dyn Fn() -> bool + Send + Sync>;
/// Stands in for Swift's `Date()` when a row enters the session. Production
/// uses `SystemTime::now`; tests move their own.
pub type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

#[derive(Default)]
struct State {
    links: HashMap<Uuid, ProbedLink>,
    media_rows: HashMap<Uuid, MediaRow>,
    order: Vec<Uuid>,
    seen_urls: HashSet<SourceUrl>,
    known_download_urls: HashSet<SourceUrl>,
    packages: Vec<PackageCandidate>,
    manual_overrides: HashMap<Uuid, String>,
    /// Note text keyed by package name, reattached after every `recluster()`
    /// — the mechanism for a truncated playlist's "50 of 320 videos".
    package_notes: HashMap<String, String>,
    /// When each row entered the session, for the "auto-clear after N
    /// minutes" setting.
    added_at: HashMap<Uuid, SystemTime>,
    /// Set by `cancel_checks()`; the probe and resolve loops stop launching
    /// new work while it's true. Cleared at the start of every
    /// ingest/recheck.
    cancel_requested: bool,
}

struct Inner {
    prober: LinkProber,
    budget: GrabberBudget,
    resolver: Option<Arc<dyn LinkResolver>>,
    quality_preferences: QualityPreferencesProvider,
    ffmpeg_available: FfmpegAvailability,
    clock: Clock,
    state: Mutex<State>,
}

/// Cheap to clone; clones share one session.
#[derive(Clone)]
pub struct GrabberSession {
    inner: Arc<Inner>,
}

/// Builds a [`GrabberSession`] with Swift's initializer defaults: the default
/// [`GrabberBudget`], no resolver, [`QualityPreferences::default`], ffmpeg
/// available, and the system clock.
pub struct GrabberSessionBuilder {
    prober: LinkProber,
    budget: GrabberBudget,
    resolver: Option<Arc<dyn LinkResolver>>,
    quality_preferences: QualityPreferencesProvider,
    ffmpeg_available: FfmpegAvailability,
    clock: Clock,
}

impl GrabberSessionBuilder {
    pub fn budget(mut self, budget: GrabberBudget) -> Self {
        self.budget = budget;
        self
    }

    pub fn resolver(mut self, resolver: Arc<dyn LinkResolver>) -> Self {
        self.resolver = Some(resolver);
        self
    }

    pub fn quality_preferences(
        mut self,
        provider: impl Fn() -> QualityPreferences + Send + Sync + 'static,
    ) -> Self {
        self.quality_preferences = Arc::new(provider);
        self
    }

    pub fn ffmpeg_available(mut self, provider: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        self.ffmpeg_available = Arc::new(provider);
        self
    }

    pub fn clock(mut self, clock: impl Fn() -> SystemTime + Send + Sync + 'static) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    pub fn build(self) -> GrabberSession {
        GrabberSession::new(
            self.prober,
            self.budget,
            self.resolver,
            self.quality_preferences,
            self.ffmpeg_available,
            self.clock,
        )
    }
}

/// How a launch loop's children are tracked: Swift's `withTaskGroup`.
fn join_outcome<T>(joined: Result<T, JoinError>) -> Option<T> {
    match joined {
        Ok(value) => Some(value),
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        // Only a runtime shutting down cancels a child; nothing here aborts.
        Err(_) => None,
    }
}

impl GrabberSession {
    /// # Panics
    ///
    /// When a [`GrabberBudget`] limit is zero.
    pub fn new(
        prober: LinkProber,
        budget: GrabberBudget,
        resolver: Option<Arc<dyn LinkResolver>>,
        quality_preferences: QualityPreferencesProvider,
        ffmpeg_available: FfmpegAvailability,
        clock: Clock,
    ) -> Self {
        assert!(budget.global_max_concurrent_probes >= 1);
        assert!(budget.max_concurrent_per_host >= 1);
        assert!(budget.max_concurrent_resolves >= 1);
        Self {
            inner: Arc::new(Inner {
                prober,
                budget,
                resolver,
                quality_preferences,
                ffmpeg_available,
                clock,
                state: Mutex::new(State::default()),
            }),
        }
    }

    pub fn builder(prober: LinkProber) -> GrabberSessionBuilder {
        GrabberSessionBuilder {
            prober,
            budget: GrabberBudget::default(),
            resolver: None,
            quality_preferences: Arc::new(QualityPreferences::default),
            ffmpeg_available: Arc::new(|| true),
            clock: Arc::new(SystemTime::now),
        }
    }

    /// Extracts links from pasted or dropped text, dedupes against links
    /// already in this session, then probes the new ones under budget.
    pub async fn ingest_text(&self, text: &str) {
        self.ingest_urls(&UrlExtractor::extract_links(text)).await;
    }

    /// Links are identified by the URL string exactly as written
    /// ([`SourceUrl`] compares and hashes on it), never by a normalised form.
    pub async fn ingest_urls(&self, urls: &[SourceUrl]) {
        self.inner.ingest_urls(urls).await;
    }

    /// Sets a format on a media row (the picker override) and re-clusters —
    /// the derived filename changes. Parent spec §9.2.
    pub fn set_format_choice(&self, choice: FormatChoice, id: Uuid) {
        self.inner.set_format_choice(choice, id);
    }

    pub fn snapshot(&self) -> GrabberSnapshot {
        self.inner.state().snapshot()
    }

    /// Refreshes which grabbed links are already in the download list.
    /// Spec §7.5: badged as duplicates, never silently dropped.
    pub fn set_known_download_urls(&self, urls: HashSet<SourceUrl>) {
        let mut state = self.inner.state();
        for id in state.order.clone() {
            if let Some(link) = state.links.get_mut(&id) {
                link.is_duplicate = urls.contains(&link.original_url);
            } else if let Some(row) = state.media_rows.get_mut(&id) {
                row.is_duplicate = urls.contains(&row.source_url);
            }
        }
        state.known_download_urls = urls;
    }

    pub fn remove_link(&self, id: Uuid) {
        let mut state = self.inner.state();
        if !state.take_row(&id) {
            return;
        }
        state.order.retain(|other| *other != id);
        state.manual_overrides.remove(&id);
        state.added_at.remove(&id);
        state.recluster();
    }

    /// Removes every member link of one package. Identified by id — see
    /// [`GrabberSession::move_link`]'s doc comment for why.
    pub fn remove_package(&self, id: Uuid) {
        let mut state = self.inner.state();
        let Some(package) = state.packages.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        for link_id in &package.link_ids {
            state.take_row(link_id);
        }
        state
            .order
            .retain(|link_id| !package.link_ids.contains(link_id));
        for link_id in &package.link_ids {
            state.manual_overrides.remove(link_id);
            state.added_at.remove(link_id);
        }
        state.package_notes.remove(&package.name);
        state.recluster();
    }

    /// Removes every link in the session, clearing the list back to empty.
    pub fn clear(&self) {
        let mut state = self.inner.state();
        state.links.clear();
        state.media_rows.clear();
        state.order.clear();
        state.seen_urls.clear();
        state.manual_overrides.clear();
        state.package_notes.clear();
        state.added_at.clear();
        state.packages.clear();
    }

    /// Removes every row that entered the session more than `max_age` ago —
    /// the "auto-clear grabbed links after N minutes" setting. `now` comes
    /// from the caller (Swift defaults it to `Date()`); rows are stamped with
    /// the session's clock when they enter.
    pub fn prune_older_than(&self, max_age: Duration, now: SystemTime) {
        // A cutoff before the epoch's representable range expires nothing.
        let Some(cutoff) = now.checked_sub(max_age) else {
            return;
        };
        let mut state = self.inner.state();
        let expired: Vec<Uuid> = state
            .order
            .iter()
            .copied()
            .filter(|id| state.added_at.get(id).copied().unwrap_or(now) < cutoff)
            .collect();
        if expired.is_empty() {
            return;
        }
        let expired_set: HashSet<Uuid> = expired.iter().copied().collect();
        for id in &expired {
            state.take_row(id);
            state.manual_overrides.remove(id);
            state.added_at.remove(id);
        }
        state.order.retain(|id| !expired_set.contains(id));
        state.recluster();
    }

    /// Forces a link into a package, overriding automatic clustering. Spec
    /// §7.4: "All of it is fully overridable."
    ///
    /// Identified by id, not name: `packages` can (however briefly) hold two
    /// entries with the same name — see `recluster()`'s duplicate-name merge
    /// — and even when it can't, a name captured by the caller earlier is
    /// not guaranteed to still uniquely pick out the package the operator
    /// actually meant.
    pub fn move_link(&self, id: Uuid, package_id: Uuid) {
        let mut state = self.inner.state();
        if !(state.links.contains_key(&id) || state.media_rows.contains_key(&id)) {
            return;
        }
        let Some(name) = state
            .packages
            .iter()
            .find(|p| p.id == package_id)
            .map(|p| p.name.clone())
        else {
            return;
        };
        state.manual_overrides.insert(id, name);
        state.recluster();
    }

    /// Renames a package by moving every member link's manual override to the
    /// new name. Spec §7.4: "rename." Identified by id — see
    /// [`GrabberSession::move_link`]'s doc comment for why.
    pub fn rename_package(&self, id: Uuid, new_name: &str) {
        if new_name.is_empty() {
            return;
        }
        let mut state = self.inner.state();
        let Some(package) = state.packages.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        if package.name == new_name {
            return;
        }
        for link_id in &package.link_ids {
            state.manual_overrides.insert(*link_id, new_name.to_owned());
        }
        if let Some(note) = state.package_notes.remove(&package.name) {
            state.package_notes.insert(new_name.to_owned(), note);
        }
        state.recluster();
    }

    /// Combines one package's links into another's. Spec §7.4: "merge."
    /// Identified by id — see [`GrabberSession::move_link`]'s doc comment for
    /// why.
    pub fn merge_packages(&self, source_id: Uuid, destination_id: Uuid) {
        if source_id == destination_id {
            return;
        }
        let mut state = self.inner.state();
        let source = state.packages.iter().find(|p| p.id == source_id).cloned();
        let destination = state
            .packages
            .iter()
            .find(|p| p.id == destination_id)
            .cloned();
        let (Some(source), Some(destination)) = (source, destination) else {
            return;
        };
        for link_id in &source.link_ids {
            state
                .manual_overrides
                .insert(*link_id, destination.name.clone());
        }
        state.recluster();
    }

    /// Splits a package so each member link becomes its own single-link
    /// package, named after that link. Spec §7.4: "split." Identified by id
    /// — see [`GrabberSession::move_link`]'s doc comment for why.
    pub fn split_package(&self, id: Uuid) {
        let mut state = self.inner.state();
        let Some(package) = state.packages.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        for link_id in &package.link_ids {
            let name = if let Some(link) = state.links.get(link_id) {
                link.effective_filename()
            } else if let Some(row) = state.media_rows.get(link_id) {
                row.display_filename()
            } else {
                continue;
            };
            state.manual_overrides.insert(*link_id, name);
        }
        state.recluster();
    }

    /// Re-checks everything worth retrying: HTTP links whose probe failed
    /// outright (`.checkFailed`), and media rows that failed to resolve or
    /// are waiting on a now-maybe-installed binary. Faulty/offline links and
    /// `.unsupported` media rows are genuine verdicts and are left alone.
    pub async fn recheck_failed(&self) {
        self.inner.recheck_failed().await;
    }

    /// Re-probes / re-resolves exactly the given rows, whatever their
    /// current verdict — the per-item "Recheck" context-menu action. Unlike
    /// [`GrabberSession::recheck_failed`], an already-online link or resolved
    /// media row is retried too, because the operator asked for it
    /// explicitly.
    pub async fn recheck(&self, ids: &HashSet<Uuid>) {
        self.inner.recheck(ids).await;
    }

    /// Stops the in-flight probe and resolve loops from launching any
    /// further work. Rows already handed to a probe or a yt-dlp process
    /// finish and are recorded; rows still waiting are settled as check
    /// failures so they stay recheckable. The header's "Cancel Checks"
    /// button.
    pub fn cancel_checks(&self) {
        self.inner.state().cancel_requested = true;
    }
}

/// One probe waiting for a slot: the link id, its URL, and the host its
/// per-host count is keyed on.
struct PendingProbe {
    id: Uuid,
    url: SourceUrl,
    host: String,
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().expect("grabber session state poisoned")
    }

    async fn ingest_urls(self: &Arc<Self>, urls: &[SourceUrl]) {
        // Same gate as text extraction — a dropped bare-host URL is not a
        // download. `can_handle` is cheap and stateless; asking it before
        // taking the lock keeps resolver code out of the critical section.
        let grabbable: Vec<(SourceUrl, bool)> = urls
            .iter()
            .filter(|url| UrlExtractor::is_grabbable(url))
            .map(|url| {
                let handled = self
                    .resolver
                    .as_ref()
                    .is_some_and(|resolver| resolver.can_handle(url));
                (url.clone(), handled)
            })
            .collect();
        let now = (self.clock)();

        let (fresh_probes, fresh_media) = {
            let mut state = self.state();
            state.cancel_requested = false;
            let mut fresh_probes: Vec<Uuid> = Vec::new();
            let mut fresh_media: Vec<Uuid> = Vec::new();
            for (url, handled) in grabbable {
                if !state.seen_urls.insert(url.clone()) {
                    continue;
                }
                let id = Uuid::new_v4();
                state.added_at.insert(id, now);
                let is_duplicate = state.known_download_urls.contains(&url);
                if handled {
                    state.media_rows.insert(
                        id,
                        MediaRow::new(url).with_id(id).with_duplicate(is_duplicate),
                    );
                    state.order.push(id);
                    fresh_media.push(id);
                } else {
                    state.links.insert(
                        id,
                        ProbedLink::new(url)
                            .with_id(id)
                            .with_duplicate(is_duplicate),
                    );
                    state.order.push(id);
                    fresh_probes.push(id);
                }
            }
            (fresh_probes, fresh_media)
        };
        if fresh_probes.is_empty() && fresh_media.is_empty() {
            return;
        }
        if !fresh_probes.is_empty() {
            self.probe_bounded(&fresh_probes).await;
        }
        for id in fresh_media {
            self.resolve_media(id).await;
        }
        self.state().recluster();
    }

    // MARK: - Media resolution

    /// Resolves one media row through the injected resolver and runs
    /// auto-pick. A playlist URL fans out into one row per entry. Parent
    /// spec §6.2.
    async fn resolve_media(self: &Arc<Self>, id: Uuid) {
        let Some(resolver) = self.resolver.clone() else {
            return;
        };
        let Some(url) = self
            .state()
            .media_rows
            .get(&id)
            .map(|row| row.source_url.clone())
        else {
            return;
        };
        let target = match resolver.resolve(&url).await {
            Ok(target) => target,
            Err(error) => {
                if let Some(row) = self.state().media_rows.get_mut(&id) {
                    row.state = row_state_for(&error);
                }
                return;
            }
        };
        match target {
            ResolvedTarget::Single(media) => self.apply_resolved_media(media, id),
            ResolvedTarget::Playlist {
                title,
                entries,
                total_available,
            } => {
                self.expand_playlist(id, url, title, entries, total_available)
                    .await
            }
        }
    }

    fn apply_resolved_media(&self, media: ResolvedMedia, id: Uuid) {
        // The preference and ffmpeg closures run outside the lock; they hold
        // no session state.
        let preferences = (self.quality_preferences)();
        let choice = FormatSelector::pick(&media, &preferences);
        let chosen_state = choice.as_ref().map(|choice| self.state_for(choice));
        let mut state = self.state();
        let Some(row) = state.media_rows.get_mut(&id) else {
            return;
        };
        row.title = media.title.clone();
        row.media = Some(media);
        match (choice, chosen_state) {
            (Some(choice), Some(chosen_state)) => {
                row.choice = Some(choice);
                row.state = chosen_state;
            }
            _ => {
                row.choice = None;
                row.state = MediaRowState::Unselected;
            }
        }
    }

    /// `.needsFfmpeg` when the choice must be muxed and ffmpeg is missing,
    /// otherwise `.resolved`.
    fn state_for(&self, choice: &FormatChoice) -> MediaRowState {
        if choice.requires_mux() && !(self.ffmpeg_available)() {
            MediaRowState::NeedsFfmpeg
        } else {
            MediaRowState::Resolved
        }
    }

    fn set_format_choice(&self, choice: FormatChoice, id: Uuid) {
        let chosen_state = self.state_for(&choice);
        let mut state = self.state();
        let Some(row) = state.media_rows.get_mut(&id) else {
            return;
        };
        row.choice = Some(choice);
        row.state = chosen_state;
        state.recluster();
    }

    /// Fans a playlist out into one row per entry, clusters them into one
    /// named package, and lazily resolves each entry's formats. Parent spec
    /// §6.2.
    async fn expand_playlist(
        self: &Arc<Self>,
        origin_id: Uuid,
        origin_url: SourceUrl,
        title: String,
        entries: Vec<ResolvedMedia>,
        total_available: i64,
    ) {
        let now = (self.clock)();
        let entry_ids: Vec<Uuid> = {
            let mut state = self.state();
            // Replace the origin row with one row per entry, all in one
            // group. The playlist URL itself no longer has a row, so it must
            // leave the seen set — otherwise re-adding the same playlist
            // later is silently deduped and nothing happens.
            let group = Uuid::new_v4();
            state.order.retain(|id| *id != origin_id);
            state.media_rows.remove(&origin_id);
            state.seen_urls.remove(&origin_url);
            // The entries inherit the origin row's age, so a playlist added
            // and forgotten clears on the same schedule as anything else.
            let added_time = state.added_at.remove(&origin_id).unwrap_or(now);
            let mut entry_ids: Vec<Uuid> = Vec::new();
            for entry in &entries {
                let entry_id = Uuid::new_v4();
                // The entry's own page URL (any site), not a synthesized
                // YouTube watch URL. Parent spec §5.3.
                let Some(watch_url) = entry.source_url.clone() else {
                    continue;
                };
                if !state.seen_urls.insert(watch_url.clone()) {
                    continue;
                }
                state.added_at.insert(entry_id, added_time);
                let is_duplicate = state.known_download_urls.contains(&watch_url);
                state.media_rows.insert(
                    entry_id,
                    MediaRow::new(watch_url)
                        .with_id(entry_id)
                        .with_title(entry.title.clone())
                        .with_state(MediaRowState::Resolving)
                        .with_playlist_group(group)
                        .with_duplicate(is_duplicate),
                );
                state.order.push(entry_id);
                entry_ids.push(entry_id);
            }
            for entry_id in &entry_ids {
                state.manual_overrides.insert(*entry_id, title.clone());
            }
            if (entries.len() as i64) < total_available {
                state.package_notes.insert(
                    title.clone(),
                    format!("{} of {} videos", entries.len(), total_available),
                );
            } else {
                state.package_notes.remove(&title);
            }
            entry_ids
        };

        // Lazily resolve each entry, but only a few yt-dlp processes at once
        // — the probe concurrency (dozens) would pin the CPU and time every
        // extraction out. See `GrabberBudget::max_concurrent_resolves`.
        let mut pending: VecDeque<Uuid> = entry_ids.into();
        let mut active = 0usize;
        let mut children: JoinSet<(Uuid, Result<ResolvedTarget, ResolveError>)> = JoinSet::new();
        self.launch_entry_resolves(&mut pending, &mut active, &mut children);
        while let Some(joined) = children.join_next().await {
            let Some((entry_id, result)) = join_outcome(joined) else {
                break;
            };
            active -= 1;
            match result {
                Ok(ResolvedTarget::Single(media)) => self.apply_resolved_media(media, entry_id),
                Ok(ResolvedTarget::Playlist { .. }) => {
                    if let Some(row) = self.state().media_rows.get_mut(&entry_id) {
                        row.state = MediaRowState::Unsupported;
                    }
                }
                Err(error) => {
                    if let Some(row) = self.state().media_rows.get_mut(&entry_id) {
                        row.state = row_state_for(&error);
                    }
                }
            }
            self.launch_entry_resolves(&mut pending, &mut active, &mut children);
        }
    }

    /// `cancel_checks()` stops the expansion from launching any more yt-dlp
    /// extractions; entries that never started are settled as check failures
    /// so "Recheck All" / per-item Recheck picks them back up. Mirrors
    /// `resolve_media_bounded`.
    fn launch_entry_resolves(
        &self,
        pending: &mut VecDeque<Uuid>,
        active: &mut usize,
        children: &mut JoinSet<(Uuid, Result<ResolvedTarget, ResolveError>)>,
    ) {
        let mut launches: Vec<(Uuid, SourceUrl, Arc<dyn LinkResolver>)> = Vec::new();
        {
            let mut state = self.state();
            if state.cancel_requested {
                for id in pending.drain(..) {
                    if let Some(row) = state.media_rows.get_mut(&id) {
                        row.state = MediaRowState::Failed("Check cancelled".into());
                    }
                }
                return;
            }
            while *active < self.budget.max_concurrent_resolves {
                let Some(entry_id) = pending.pop_front() else {
                    break;
                };
                let Some(url) = state
                    .media_rows
                    .get(&entry_id)
                    .map(|row| row.source_url.clone())
                else {
                    continue;
                };
                let Some(resolver) = self.resolver.clone() else {
                    continue;
                };
                *active += 1;
                launches.push((entry_id, url, resolver));
            }
        }
        for (entry_id, url, resolver) in launches {
            children.spawn(async move { (entry_id, resolver.resolve(&url).await) });
        }
    }

    // MARK: - Rechecking

    async fn recheck_failed(self: &Arc<Self>) {
        let (failed_probes, failed_media) = {
            let mut state = self.state();
            state.cancel_requested = false;
            let failed_probes: Vec<Uuid> = state
                .order
                .iter()
                .copied()
                .filter(|id| {
                    state
                        .links
                        .get(id)
                        .is_some_and(|link| link.verdict == Some(Verdict::CheckFailed))
                })
                .collect();
            let failed_media: Vec<Uuid> = state
                .order
                .iter()
                .copied()
                .filter(|id| {
                    state.media_rows.get(id).is_some_and(|row| {
                        matches!(
                            row.state,
                            MediaRowState::Failed(_)
                                | MediaRowState::NeedsYtDlp
                                | MediaRowState::NeedsFfmpeg
                        )
                    })
                })
                .collect();
            (failed_probes, failed_media)
        };
        if failed_probes.is_empty() && failed_media.is_empty() {
            return;
        }
        if !failed_probes.is_empty() {
            self.probe_bounded(&failed_probes).await;
        }
        self.resolve_media_bounded(failed_media).await;
        self.state().recluster();
    }

    async fn recheck(self: &Arc<Self>, ids: &HashSet<Uuid>) {
        let (probes, media) = {
            let mut state = self.state();
            state.cancel_requested = false;
            let probes: Vec<Uuid> = state
                .order
                .iter()
                .copied()
                .filter(|id| ids.contains(id) && state.links.contains_key(id))
                .collect();
            let media: Vec<Uuid> = state
                .order
                .iter()
                .copied()
                .filter(|id| ids.contains(id) && state.media_rows.contains_key(id))
                .collect();
            (probes, media)
        };
        if probes.is_empty() && media.is_empty() {
            return;
        }
        if !probes.is_empty() {
            self.probe_bounded(&probes).await;
        }
        self.resolve_media_bounded(media).await;
        self.state().recluster();
    }

    /// Re-resolves the given media rows under the small resolve concurrency
    /// cap — a burst of yt-dlp processes trips YouTube's rate limit.
    async fn resolve_media_bounded(self: &Arc<Self>, ids: Vec<Uuid>) {
        let mut pending: VecDeque<Uuid> = {
            let mut state = self.state();
            let pending: VecDeque<Uuid> = ids
                .into_iter()
                .filter(|id| state.media_rows.contains_key(id))
                .collect();
            for id in &pending {
                if let Some(row) = state.media_rows.get_mut(id) {
                    row.state = MediaRowState::Resolving;
                }
            }
            pending
        };
        if pending.is_empty() {
            return;
        }
        let mut active = 0usize;
        let mut children: JoinSet<()> = JoinSet::new();
        self.launch_media_resolves(&mut pending, &mut active, &mut children);
        while let Some(joined) = children.join_next().await {
            if join_outcome(joined).is_none() {
                break;
            }
            active -= 1;
            self.launch_media_resolves(&mut pending, &mut active, &mut children);
        }
    }

    fn launch_media_resolves(
        self: &Arc<Self>,
        pending: &mut VecDeque<Uuid>,
        active: &mut usize,
        children: &mut JoinSet<()>,
    ) {
        let mut launches: Vec<Uuid> = Vec::new();
        {
            let mut state = self.state();
            if state.cancel_requested {
                for id in pending.drain(..) {
                    if let Some(row) = state.media_rows.get_mut(&id) {
                        row.state = MediaRowState::Failed("Check cancelled".into());
                    }
                }
                return;
            }
            while *active < self.budget.max_concurrent_resolves {
                let Some(id) = pending.pop_front() else {
                    break;
                };
                if !state.media_rows.contains_key(&id) {
                    continue;
                }
                *active += 1;
                launches.push(id);
            }
        }
        for id in launches {
            let session = Arc::clone(self);
            children.spawn(async move { session.resolve_media(id).await });
        }
    }

    // MARK: - Probing

    /// Launches probes for `ids` under the global and per-host caps. All
    /// mutable session state here is only touched inside a lock scope that
    /// contains no suspension, so a concurrent `ingest` call started while
    /// this one is still probing sees a consistent, if interleaved, picture —
    /// the same actor-reentrancy shape `DownloadEngine.reconcile` documents
    /// in Phase 1. The pending list, per-host counts and active count belong
    /// to this call alone.
    async fn probe_bounded(self: &Arc<Self>, ids: &[Uuid]) {
        let mut pending: Vec<PendingProbe> = {
            let state = self.state();
            ids.iter()
                .filter_map(|id| {
                    let url = state.links.get(id)?.original_url.clone();
                    let host = url_text::host(&url).unwrap_or_default();
                    Some(PendingProbe { id: *id, url, host })
                })
                .collect()
        };
        let mut host_counts: HashMap<String, i64> = HashMap::new();
        let mut active = 0usize;
        let mut children: JoinSet<(Uuid, ProbedLink)> = JoinSet::new();

        let launches = {
            let mut state = self.state();
            self.next_probes(&mut state, &mut pending, &mut host_counts, &mut active)
        };
        self.spawn_probes(launches, &mut children);

        while let Some(joined) = children.join_next().await {
            let Some((id, probed)) = join_outcome(joined) else {
                break;
            };
            active -= 1;
            let launches = {
                let mut state = self.state();
                if let Some(host) = state
                    .links
                    .get(&id)
                    .and_then(|link| url_text::host(&link.original_url))
                {
                    *host_counts.entry(host).or_insert(1) -= 1;
                }
                let mut finished = probed;
                finished.is_duplicate = state.known_download_urls.contains(&finished.original_url);
                finished.verdict = Some(VerdictRules::evaluate(&finished));
                state.links.insert(id, finished);
                self.next_probes(&mut state, &mut pending, &mut host_counts, &mut active)
            };
            self.spawn_probes(launches, &mut children);
        }

        // Anything `cancel_checks()` kept from ever launching is left with no
        // verdict and a spinning row — settle it as a check failure so
        // "Recheck All" / the per-item Recheck picks it back up.
        let mut state = self.state();
        if state.cancel_requested {
            for id in ids {
                if let Some(link) = state.links.get_mut(id)
                    && link.verdict.is_none()
                {
                    link.stage = Stage::Done;
                    link.verdict = Some(Verdict::CheckFailed);
                }
            }
        }
    }

    /// Picks the probes to start now: while under the global cap, the first
    /// waiting probe whose host is under the per-host cap (so a saturated
    /// host does not block the ones behind it). Marks each `Probing` in the
    /// same scope that chose it.
    fn next_probes(
        &self,
        state: &mut State,
        pending: &mut Vec<PendingProbe>,
        host_counts: &mut HashMap<String, i64>,
        active: &mut usize,
    ) -> Vec<(Uuid, SourceUrl)> {
        let mut launches = Vec::new();
        if state.cancel_requested {
            return launches;
        }
        while *active < self.budget.global_max_concurrent_probes {
            let per_host = self.budget.max_concurrent_per_host as i64;
            let Some(index) = pending
                .iter()
                .position(|entry| host_counts.get(&entry.host).copied().unwrap_or(0) < per_host)
            else {
                break;
            };
            let entry = pending.remove(index);
            *host_counts.entry(entry.host).or_insert(0) += 1;
            *active += 1;
            if let Some(link) = state.links.get_mut(&entry.id) {
                link.stage = Stage::Probing;
            }
            launches.push((entry.id, entry.url));
        }
        launches
    }

    fn spawn_probes(
        &self,
        launches: Vec<(Uuid, SourceUrl)>,
        children: &mut JoinSet<(Uuid, ProbedLink)>,
    ) {
        for (id, url) in launches {
            let prober = self.prober.clone();
            children.spawn(async move { (id, prober.probe_with_id(&url, id).await) });
        }
    }
}

impl State {
    fn snapshot(&self) -> GrabberSnapshot {
        let ordered_links: Vec<ProbedLink> = self
            .order
            .iter()
            .filter_map(|id| self.links.get(id).cloned())
            .collect();
        let ordered_media: Vec<MediaRow> = self
            .order
            .iter()
            .filter_map(|id| self.media_rows.get(id).cloned())
            .collect();
        let media_checked = ordered_media
            .iter()
            .filter(|row| row.state != MediaRowState::Resolving)
            .count();
        let checked_count = ordered_links
            .iter()
            .filter(|l| l.stage == Stage::Done)
            .count()
            + media_checked;
        let total_count = ordered_links.len() + ordered_media.len();
        GrabberSnapshot::new(
            ordered_links,
            ordered_media,
            self.packages.clone(),
            checked_count,
            total_count,
        )
    }

    /// Removes a link or media row and frees its URL for re-grabbing.
    /// Returns whether there was one.
    fn take_row(&mut self, id: &Uuid) -> bool {
        if let Some(link) = self.links.remove(id) {
            self.seen_urls.remove(&link.original_url);
            true
        } else if let Some(row) = self.media_rows.remove(id) {
            self.seen_urls.remove(&row.source_url);
            true
        } else {
            false
        }
    }

    fn recluster(&mut self) {
        let clusterable: Vec<ClusterableLink> = self
            .order
            .iter()
            .filter_map(|id| {
                if let Some(link) = self.links.get(id) {
                    let host = url_text::host(&link.final_url)
                        .or_else(|| url_text::host(&link.original_url))
                        .unwrap_or_default();
                    let directory_path = url_text::directory_path(&link.final_url);
                    return Some(ClusterableLink::new(
                        *id,
                        link.effective_filename(),
                        host,
                        directory_path,
                    ));
                }
                if let Some(row) = self.media_rows.get(id) {
                    return Some(ClusterableLink::new(
                        *id,
                        row.display_filename(),
                        url_text::host(&row.source_url).unwrap_or_else(|| "media".into()),
                        "/watch",
                    ));
                }
                None
            })
            .collect();
        let mut candidates = PackageClustering::cluster(&clusterable);

        // Swift walks the override dictionary in its own (unspecified)
        // order; rows are walked in session order here so the result is
        // deterministic.
        if !self.manual_overrides.is_empty() {
            for id in &self.order {
                let Some(name) = self.manual_overrides.get(id) else {
                    continue;
                };
                for candidate in &mut candidates {
                    candidate.link_ids.retain(|other| other != id);
                }
                if let Some(candidate) = candidates.iter_mut().find(|c| &c.name == name) {
                    candidate.link_ids.push(*id);
                } else {
                    candidates.push(PackageCandidate::new(name.clone(), vec![*id]));
                }
            }
            candidates.retain(|candidate| !candidate.link_ids.is_empty());
        }

        let packages: Vec<PackageCandidate> = Self::merging_duplicate_names(candidates)
            .into_iter()
            .map(|candidate| self.reconciling_identity(candidate))
            .map(|mut candidate| {
                candidate.note = self.package_notes.get(&candidate.name).cloned();
                candidate
            })
            .collect();
        self.packages = packages;
    }

    /// Clustering and manual overrides can each independently land two
    /// candidates on the same name — two ingested batches whose derived
    /// titles happen to coincide, or a rename/merge target that already
    /// matches another package. A name is the only thing that identifies a
    /// package to the operator, so two sharing one reads as a single,
    /// confusingly duplicated package rather than two — collapse them into
    /// one, keeping the earlier one's place. This is also the mechanism
    /// `rename_package`/`merge_packages` themselves rely on: both work by
    /// giving the moved links' override the target's name and letting this
    /// merge fold them back together.
    fn merging_duplicate_names(candidates: Vec<PackageCandidate>) -> Vec<PackageCandidate> {
        let mut merged: Vec<PackageCandidate> = Vec::new();
        for candidate in candidates {
            if let Some(existing) = merged.iter_mut().find(|m| m.name == candidate.name) {
                existing.link_ids.extend(candidate.link_ids);
                existing.is_archive = existing.is_archive || candidate.is_archive;
            } else {
                merged.push(candidate);
            }
        }
        merged
    }

    /// Gives a candidate the `id` of whichever *previous* package (from
    /// before this `recluster()` call) already had the same name, so a
    /// package's identity survives reclustering instead of becoming a new
    /// id — and hence a new package as far as any id-keyed UI state is
    /// concerned — every time a link is added, removed, or just reordered
    /// underneath an unchanged name. A name with no previous match keeps the
    /// fresh id `PackageClustering`/`PackageCandidate::new` already gave it.
    fn reconciling_identity(&self, mut candidate: PackageCandidate) -> PackageCandidate {
        if let Some(previous) = self.packages.iter().find(|p| p.name == candidate.name) {
            candidate.id = previous.id;
        }
        candidate
    }
}

/// The row state a failed resolve leaves behind.
fn row_state_for(error: &ResolveError) -> MediaRowState {
    match error {
        ResolveError::BinaryMissing => MediaRowState::NeedsYtDlp,
        ResolveError::Unsupported | ResolveError::DrmProtected => MediaRowState::Unsupported,
        ResolveError::AuthRequired => MediaRowState::Failed(
            "The site blocked the request (sign-in / anti-bot). Set \"Cookies from browser\" \
             in Settings → Media Sites, or update yt-dlp."
                .into(),
        ),
        ResolveError::PrivateVideo => MediaRowState::Failed("Private video.".into()),
        ResolveError::Unavailable => MediaRowState::Failed("Video unavailable.".into()),
        ResolveError::Timeout => MediaRowState::Failed("yt-dlp timed out.".into()),
        ResolveError::FormatGone => {
            MediaRowState::Failed("The requested format is no longer available.".into())
        }
        ResolveError::YtDlpFailed { stderr_tail } => {
            // Surface the real yt-dlp error verbatim — the UI puts the full
            // text in the row's tooltip. A couple of known-cryptic YouTube
            // messages get a short hint prepended.
            let cleaned = stderr_tail.trim();
            if cleaned.is_empty() {
                return MediaRowState::Failed("yt-dlp could not resolve this video.".into());
            }
            if cleaned
                .to_lowercase()
                .contains("the page needs to be reloaded")
            {
                return MediaRowState::Failed(format!(
                    "YouTube rejected the session — your browser cookies are likely stale or \
                     locked. Quit the browser and retry, try a different browser, or update \
                     yt-dlp. (macOS: Chrome needs App-Bound decryption; Safari needs Full \
                     Disk Access for SDM.)\n\n{cleaned}"
                ));
            }
            MediaRowState::Failed(cleaned.to_owned())
        }
        // Swift's `default`: any other error, such as a cancellation.
        ResolveError::Cancelled => MediaRowState::Failed(format!("Could not resolve: {error}")),
    }
}
