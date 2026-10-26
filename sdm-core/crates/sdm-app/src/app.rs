//! The controller: owns the engine, turns snapshots into patches, and runs
//! user actions. Port of `EngineController` minus everything view-specific.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};

use sdm_core::{
    AppTiming, DownloadItem, DownloadPackage, LinkResolver, ThemeCatalog, WholesaleDownloader,
};
use sdm_engine::{
    DownloadEngine, EngineSettings, EngineSnapshot, HttpTransport, Muxer, RetryPolicy, StateStore,
    Trash,
};
use sdm_grabber::{
    GrabberSnapshot, MediaFormatMenu, MediaHandoff, MediaRow, MediaRowState, PackageCandidate,
    ProbedLink, Verdict,
};
use sdm_resolve::{CheckReason, ManagedBinaries, ToolsActivity};
use uuid::Uuid;

use crate::autoadd::ready_packages;
use crate::clipboard::ClipboardWatcher;
use crate::differ::{Delta, Differ};
use crate::error::AppError;
use crate::grabber::GrabberController;
use crate::host::{Host, PatchSink};
use crate::notify::{Notification, notifications_between};
use crate::protocol::*;
use crate::settings::SettingsStore;
use crate::theme::theme_css;
use crate::tray::TrayStatus;
use crate::url_input::{derive_filename, parse_download_url};

/// The engine's collaborators. `production()` (in `toolchain.rs`) builds the
/// real ones; tests pass fakes.
pub struct Dependencies {
    pub transport: Arc<dyn HttpTransport>,
    pub state_store: Arc<dyn StateStore>,
    pub trash: Arc<dyn Trash>,
    pub resolver: Option<Arc<dyn LinkResolver>>,
    pub muxer: Option<Arc<dyn Muxer>>,
    pub wholesale_downloader: Option<Arc<dyn WholesaleDownloader>>,
    pub retry_policy: RetryPolicy,
    pub binaries: Option<ManagedBinaries>,
    /// The link grabber; `None` leaves every `grabber_*` action an error.
    pub grabber: Option<Arc<GrabberController>>,
    /// The loaded settings, when the caller shares them with other
    /// components (production does); `None` makes the app load its own.
    pub settings: Option<SettingsStore>,
}

/// How many downloads, segments and connections the engine may use at once.
#[derive(Debug, Clone, Copy)]
struct Limits {
    concurrent: usize,
    segments: usize,
    connections: usize,
}

/// Engine limits fixed by the load harness (`new_with_concurrency`), taking
/// precedence over the stored concurrency, segment and connection settings.
fn engine_settings(
    settings: &SettingsDto,
    folder: std::path::PathBuf,
    fixed: Option<Limits>,
) -> EngineSettings {
    let (concurrent, segments, connections) = match fixed {
        Some(limits) => (limits.concurrent, limits.segments, limits.connections),
        None => (
            settings.max_concurrent as usize,
            settings.segments_per_item as usize,
            settings.global_max_connections as usize,
        ),
    };
    EngineSettings::new(concurrent, segments, connections, folder)
        .with_max_connections_per_host(settings.max_connections_per_host as usize)
        .with_min_segment_size_bytes(i64::from(settings.min_segment_size_mb) * 1024 * 1024)
}

struct PublishState {
    seq: u32,
    differ: Differ,
    last_generation: Option<u64>,
    previous: Option<Arc<EngineSnapshot>>,
    visible: bool,
    need_full: bool,
    detail_items: HashMap<String, HashSet<Uuid>>,
    sink: Option<Arc<dyn PatchSink>>,
    last_tick_error: Option<String>,
    ticks: u64,
}

pub struct App {
    engine: Arc<DownloadEngine>,
    host: Arc<dyn Host>,
    settings: Mutex<SettingsStore>,
    binaries: Option<ManagedBinaries>,
    grabber: Option<Arc<GrabberController>>,
    fixed_limits: Option<Limits>,
    /// Row ids already sent to the downloads by auto-add.
    auto_added: Mutex<HashSet<Uuid>>,
    clipboard: Mutex<ClipboardWatcher>,
    state: Mutex<PublishState>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn parse_id(text: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(text.trim()).map_err(|_| AppError::invalid("id", "that is not a valid item id"))
}

/// Parses a batch of ids, rejecting the whole call on the first bad one and
/// dropping duplicates (first occurrence wins, order kept).
fn parse_ids(ids: &[String]) -> Result<Vec<Uuid>, AppError> {
    let mut seen = HashSet::new();
    let mut parsed = Vec::with_capacity(ids.len());
    for id in ids {
        let id = parse_id(id)?;
        if seen.insert(id) {
            parsed.push(id);
        }
    }
    Ok(parsed)
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a panic with a non-text payload".to_owned()
    }
}

impl App {
    pub fn new(host: Arc<dyn Host>, dependencies: Dependencies) -> Result<Arc<Self>, AppError> {
        Self::with_limits(host, dependencies, None)
    }

    /// [`Self::new`] with `max_concurrent` simultaneous downloads (and four
    /// segments each), for the load harness. Must be at least 1.
    pub fn new_with_concurrency(
        host: Arc<dyn Host>,
        dependencies: Dependencies,
        max_concurrent: usize,
    ) -> Result<Arc<Self>, AppError> {
        if max_concurrent == 0 {
            return Err(AppError::invalid(
                "maxConcurrent",
                "at least one download must be allowed",
            ));
        }
        let limits = Limits {
            concurrent: max_concurrent,
            segments: 4,
            connections: max_concurrent.saturating_mul(4),
        };
        Self::with_limits(host, dependencies, Some(limits))
    }

    fn with_limits(
        host: Arc<dyn Host>,
        dependencies: Dependencies,
        fixed_limits: Option<Limits>,
    ) -> Result<Arc<Self>, AppError> {
        let data_dir = host.data_dir();
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| AppError::failed(format!("cannot create {}: {e}", data_dir.display())))?;
        let settings = dependencies.settings.unwrap_or_else(|| {
            SettingsStore::load(data_dir.join("settings.json"), host.default_download_dir())
        });
        let engine = DownloadEngine::new(
            dependencies.transport,
            dependencies.state_store,
            engine_settings(&settings.get(), settings.download_folder(), fixed_limits),
            dependencies.retry_policy,
            dependencies.resolver,
            dependencies.muxer,
            dependencies.wholesale_downloader,
        )
        .with_trash(dependencies.trash);
        Ok(Arc::new(Self {
            engine: Arc::new(engine),
            host,
            settings: Mutex::new(settings),
            binaries: dependencies.binaries,
            grabber: dependencies.grabber,
            fixed_limits,
            auto_added: Mutex::new(HashSet::new()),
            clipboard: Mutex::new(ClipboardWatcher::new()),
            state: Mutex::new(PublishState {
                seq: 0,
                differ: Differ::default(),
                last_generation: None,
                previous: None,
                visible: true,
                need_full: true,
                detail_items: HashMap::new(),
                sink: None,
                last_tick_error: None,
                ticks: 0,
            }),
        }))
    }

    // MARK: Lifecycle

    /// Loads durable state. Call once, before the first `tick`.
    pub async fn restore(&self) -> Result<(), AppError> {
        self.engine.restore().await;
        // A window may have attached while the saved downloads were loading,
        // leaving an empty snapshot as the baseline. What `restore` brought in
        // happened in earlier runs, so the next snapshot starts afresh and
        // announces nothing.
        lock(&self.state).previous = None;
        if lock(&self.settings).get().auto_start_downloads_on_launch {
            self.engine.resume_all().await;
        }
        self.publish(false).await
    }

    /// Starts sending patches to `sink` (replacing any previous one) and sends
    /// a full patch first.
    pub async fn attach(&self, sink: Arc<dyn PatchSink>) -> Result<(), AppError> {
        {
            let mut state = lock(&self.state);
            state.sink = Some(sink);
            state.need_full = true;
        }
        self.publish(true).await
    }

    /// One heartbeat: advances the engine, then publishes. The shell calls this
    /// `AppTiming::TICKS_PER_SECOND` times a second; tests call it directly.
    pub async fn tick(&self) -> Result<(), AppError> {
        self.engine.tick().await;
        let (run_binaries_tick, run_prune) = {
            let mut state = lock(&self.state);
            state.ticks += 1;
            let per_second = AppTiming::TICKS_PER_SECOND as u64;
            (
                state.ticks.is_multiple_of(per_second),
                state.ticks.is_multiple_of(per_second * 60),
            )
        };
        if run_prune && let Some(grabber) = &self.grabber {
            grabber.prune();
        }
        if run_binaries_tick && let Some(binaries) = &self.binaries {
            binaries.tick().await;
        }
        self.publish(false).await?;
        self.auto_add_ready().await
    }

    /// With auto-add-and-start on, sends every fully ready grabbed package to
    /// the downloads and starts it, once per package. A failure is logged and
    /// leaves the rest of the tick alone.
    pub async fn auto_add_ready(&self) -> Result<(), AppError> {
        let Some(grabber) = &self.grabber else {
            return Ok(());
        };
        if !lock(&self.settings).get().auto_add_and_start_on_grab {
            return Ok(());
        }
        let ready = {
            let mut added = lock(&self.auto_added);
            let ready = ready_packages(&grabber.session.snapshot(), &added);
            for (_, rows) in &ready {
                added.extend(rows.iter().copied());
            }
            ready
        };
        for (package, _) in ready {
            let target = HandoffTarget::Package {
                id: package.to_string(),
            };
            if let Err(error) = self.grabber_add_to_downloads(target, true).await {
                tracing::warn!("could not auto-add a grabbed package: {error}");
            }
        }
        Ok(())
    }

    /// `tick`, with a failure logged once per distinct message instead of on
    /// every tick (the heartbeat must keep running).
    pub async fn heartbeat(&self) {
        let result = self.tick().await;
        let mut state = lock(&self.state);
        match result {
            Ok(()) => state.last_tick_error = None,
            Err(error) => {
                let message = error.to_string();
                if state.last_tick_error.as_deref() != Some(message.as_str()) {
                    tracing::error!("heartbeat tick failed: {message}");
                    state.last_tick_error = Some(message);
                }
            }
        }
    }

    /// Runs `work` on its own task so a panic becomes `AppError::Internal`
    /// instead of killing the caller (the Tauri command task would hang).
    pub async fn guarded<T, F, Fut>(self: &Arc<Self>, work: F) -> Result<T, AppError>
    where
        F: FnOnce(Arc<App>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<T, AppError>> + Send + 'static,
        T: Send + 'static,
    {
        let app = Arc::clone(self);
        match tokio::spawn(async move { work(app).await }).await {
            Ok(result) => result,
            Err(error) if error.is_panic() => Err(AppError::Internal {
                message: panic_message(error.into_panic()),
            }),
            Err(_) => Err(AppError::Internal {
                message: "the task was cancelled".to_owned(),
            }),
        }
    }

    // MARK: Publishing

    /// Fetches the snapshot if it changed, handles notifications, builds a
    /// patch and sends it. `force_full` is an explicit request for a full patch.
    async fn publish(&self, force_full: bool) -> Result<(), AppError> {
        let folder = lock(&self.settings).download_folder();
        let (since, histories_wanted) = {
            let mut state = lock(&self.state);
            if state.differ.set_download_folder(folder) {
                // Package folders changed with no engine change to trigger a
                // new snapshot, so ask for one.
                state.need_full = true;
            }
            let since = if force_full || state.need_full {
                None
            } else {
                state.last_generation
            };
            let ids: HashSet<Uuid> = state.detail_items.values().flatten().copied().collect();
            (
                since,
                if state.visible || force_full {
                    ids
                } else {
                    HashSet::new()
                },
            )
        };
        // No lock is held across these awaits.
        let changed = self.engine.snapshot_if_changed(since).await;
        let histories = if histories_wanted.is_empty() {
            HashMap::new()
        } else {
            self.engine.speed_histories(&histories_wanted).await
        };

        let tools = self.tools_notice();
        let mut known_urls = None;
        let mut due = Vec::new();
        {
            let mut state = lock(&self.state);
            let mut delta: Option<Delta> = None;
            let mut full = false;
            let wants_full = force_full || state.need_full;
            let emit_allowed = state.sink.is_some() && (state.visible || force_full);

            if let Some(snapshot) = changed {
                let stale = state
                    .last_generation
                    .is_some_and(|g| snapshot.generation < g);
                if !stale {
                    if let Some(previous) = &state.previous {
                        due = notifications_between(previous, &snapshot);
                    }
                    state.last_generation = Some(snapshot.generation);
                    if self.grabber.is_some() {
                        known_urls = Some(
                            snapshot
                                .packages
                                .iter()
                                .flat_map(|package| package.items.iter())
                                .map(|item| item.url.clone())
                                .collect::<HashSet<_>>(),
                        );
                    }
                    if emit_allowed {
                        if wants_full {
                            delta = Some(state.differ.full(&snapshot));
                            full = true;
                        } else {
                            delta = state.differ.diff(&snapshot);
                        }
                    } else {
                        // Not sending: whatever the differ knew is now stale.
                        state.need_full = true;
                    }
                    state.previous = Some(Arc::new(snapshot));
                }
            }

            let grabber_view = match &self.grabber {
                Some(grabber) if emit_allowed => grabber.changed_view(tools, wants_full),
                Some(grabber) => {
                    grabber.invalidate();
                    None
                }
                None => None,
            };
            if emit_allowed {
                let history_items: Vec<ItemHistory> = histories
                    .into_iter()
                    .map(|(id, speed_history)| ItemHistory {
                        id: id.to_string(),
                        speed_history,
                    })
                    .collect();
                if delta.is_some() || !history_items.is_empty() || grabber_view.is_some() {
                    let delta = delta.unwrap_or(Delta {
                        structure: None,
                        items: Vec::new(),
                        packages: Vec::new(),
                        stats: None,
                    });
                    state.seq = state.seq.wrapping_add(1);
                    let patch = Patch {
                        seq: state.seq,
                        full,
                        structure: delta.structure,
                        items: delta.items,
                        packages: delta.packages,
                        stats: delta.stats,
                        histories: history_items,
                        grabber: grabber_view,
                    };
                    let sent = match &state.sink {
                        Some(sink) => sink.send(patch).is_ok(),
                        None => false,
                    };
                    if sent {
                        if full {
                            state.need_full = false;
                        }
                    } else {
                        // The receiver may have missed it: forget the sink and
                        // make the next patch (after a re-attach) a full one.
                        state.sink = None;
                        state.need_full = true;
                        state.differ.reset();
                    }
                }
            }
        }
        if let (Some(grabber), Some(urls)) = (&self.grabber, known_urls) {
            grabber.set_known_download_urls(urls);
        }
        if let Some(grabber) = &self.grabber {
            let count = grabber.take_fresh_rows();
            if count > 0 {
                due.push(Notification::LinksGrabbed { count });
            }
        }
        let settings = lock(&self.settings).get();
        for notification in due.into_iter().filter(|n| n.enabled(&settings)) {
            if let Err(error) = self.host.notify(notification) {
                tracing::warn!("could not show a notification: {error}");
            }
        }
        Ok(())
    }

    // MARK: Commands

    pub async fn add_url(&self, url: &str, package_name: Option<&str>) -> Result<(), AppError> {
        let source = parse_download_url(url)?;
        let filename = derive_filename(&source);
        let name = match package_name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(name) => sdm_core::safe_path_component(name),
            None => filename.clone(),
        };
        let package =
            DownloadPackage::new(name).with_items(vec![DownloadItem::http(source, filename)]);
        self.add_package(package).await
    }

    /// Hands a ready-made package to the engine and publishes the change.
    pub(crate) async fn add_package(&self, package: DownloadPackage) -> Result<(), AppError> {
        self.engine.add(package).await;
        self.publish(false).await
    }

    pub async fn start_items(&self, ids: &[String]) -> Result<(), AppError> {
        let ids = parse_ids(ids)?;
        for id in &ids {
            self.engine.start_item(*id).await;
        }
        self.publish_if_any(&ids).await
    }

    pub async fn stop_items(&self, ids: &[String]) -> Result<(), AppError> {
        let ids = parse_ids(ids)?;
        for id in &ids {
            self.engine.stop_item(*id).await;
        }
        self.publish_if_any(&ids).await
    }

    pub async fn retry_items(&self, ids: &[String]) -> Result<(), AppError> {
        let ids = parse_ids(ids)?;
        for id in &ids {
            self.engine.retry(*id).await;
        }
        self.publish_if_any(&ids).await
    }

    pub async fn retry_mux_items(&self, ids: &[String]) -> Result<(), AppError> {
        let ids = parse_ids(ids)?;
        for id in &ids {
            self.engine.retry_mux(*id).await;
        }
        self.publish_if_any(&ids).await
    }

    /// Discards the progress of each item. A running item is skipped: a worker
    /// is writing to its file, so the engine only resets it as a backstop.
    pub async fn reset_items(&self, ids: &[String]) -> Result<(), AppError> {
        let ids = parse_ids(ids)?;
        if ids.is_empty() {
            return Ok(());
        }
        let running: HashSet<Uuid> = self
            .engine
            .snapshot()
            .await
            .packages
            .iter()
            .flat_map(|package| package.items.iter())
            .filter(|item| item.state == sdm_core::ItemState::Running)
            .map(|item| item.id)
            .collect();
        for id in ids.iter().filter(|id| !running.contains(id)) {
            self.engine.reset_download(*id).await;
        }
        self.publish(false).await
    }

    pub async fn set_items_enabled(&self, ids: &[String], enabled: bool) -> Result<(), AppError> {
        let ids = parse_ids(ids)?;
        for id in &ids {
            self.engine.set_enabled(enabled, *id).await;
        }
        self.publish_if_any(&ids).await
    }

    pub async fn remove_items(&self, ids: &[String], delete_files: bool) -> Result<(), AppError> {
        let ids = parse_ids(ids)?;
        for id in &ids {
            self.engine.remove_item(*id, delete_files).await;
        }
        {
            // An open details panel for a removed item must not keep fetching.
            let mut state = lock(&self.state);
            for open in state.detail_items.values_mut() {
                for id in &ids {
                    open.remove(id);
                }
            }
        }
        self.publish_if_any(&ids).await
    }

    pub async fn remove_package(&self, id: &str, delete_files: bool) -> Result<(), AppError> {
        let id = parse_id(id)?;
        self.engine.remove_package(id, delete_files).await;
        {
            // A details panel may be open for any of the package's items; the
            // next snapshot no longer has them, so stop asking for them now.
            let live: HashSet<Uuid> = self
                .engine
                .snapshot()
                .await
                .packages
                .iter()
                .flat_map(|package| package.items.iter().map(|item| item.id))
                .collect();
            let mut state = lock(&self.state);
            for open in state.detail_items.values_mut() {
                open.retain(|item| live.contains(item));
            }
        }
        self.publish(false).await
    }

    /// Moves items into `package_id`, as one contiguous run at `at_index`
    /// (against the list before the move) or at the end. The engine refuses
    /// the whole batch if any item is running or missing.
    pub async fn move_items(
        &self,
        ids: &[String],
        package_id: &str,
        at_index: Option<u32>,
    ) -> Result<(), AppError> {
        let ids = parse_ids(ids)?;
        let package = parse_id(package_id)?;
        if ids.is_empty() {
            return Ok(());
        }
        self.engine
            .move_items_to_package(&ids, package, at_index.map(|index| index as usize))
            .await;
        self.publish(false).await
    }

    pub async fn move_items_to_new_package(
        &self,
        ids: &[String],
        name: &str,
    ) -> Result<(), AppError> {
        let ids = parse_ids(ids)?;
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::invalid("name", "the package name is empty"));
        }
        if ids.is_empty() {
            return Ok(());
        }
        self.engine.move_items_to_new_package(&ids, name).await;
        self.publish(false).await
    }

    /// Orders `ids` inside `package_id`; items not named keep their relative
    /// order after the named ones.
    pub async fn reorder_items(&self, ids: &[String], package_id: &str) -> Result<(), AppError> {
        let ids = parse_ids(ids)?;
        let package = parse_id(package_id)?;
        self.engine.reorder_items(&ids, package).await;
        self.publish(false).await
    }

    pub async fn reorder_packages(&self, ids: &[String]) -> Result<(), AppError> {
        let ids = parse_ids(ids)?;
        self.engine.reorder_packages(&ids).await;
        self.publish(false).await
    }

    /// The files a deletion would trash, from the current snapshot and disk.
    pub async fn describe_deletion(
        &self,
        target: &DeletionTarget,
    ) -> Result<DeletionInfo, AppError> {
        enum Wanted {
            Items(HashSet<Uuid>),
            Package(Uuid),
        }
        let wanted = match target {
            DeletionTarget::Items { ids } => Wanted::Items(parse_ids(ids)?.into_iter().collect()),
            DeletionTarget::Package { id } => Wanted::Package(parse_id(id)?),
        };
        let snapshot = self.engine.snapshot().await;
        let download_folder = lock(&self.settings).download_folder();

        let mut total = 0.0;
        let mut names: Vec<String> = Vec::new();
        let mut found_package = false;
        for package in &snapshot.packages {
            let chosen: Vec<(String, Vec<String>)> = package
                .items
                .iter()
                .filter(|item| match &wanted {
                    Wanted::Items(ids) => ids.contains(&item.id),
                    Wanted::Package(id) => *id == package.id,
                })
                .map(|item| (item.filename.clone(), item.part_filenames.clone()))
                .collect();
            found_package |= matches!(&wanted, Wanted::Package(id) if *id == package.id);
            if chosen.is_empty() {
                continue;
            }
            let (bytes, found) =
                crate::deletion::describe(&download_folder.join(&package.name), &chosen);
            total += bytes;
            names.extend(found);
        }
        names.sort();
        if matches!(wanted, Wanted::Package(_)) && !found_package {
            return Ok(DeletionInfo {
                title: "Delete Package From Disk?".to_owned(),
                total_bytes: 0.0,
                names: Vec::new(),
            });
        }
        let title = if names.is_empty() {
            "Remove From List?"
        } else {
            "Remove From List And Delete Files?"
        };
        Ok(DeletionInfo {
            title: title.to_owned(),
            total_bytes: total,
            names,
        })
    }

    /// Opens a finished item's file with the OS default application. Nothing
    /// happens for an unfinished item or one whose file has gone missing.
    pub async fn open_item(&self, id: &str) -> Result<(), AppError> {
        let id = parse_id(id)?;
        let Some((folder, item)) = self.locate_item(id).await else {
            return Ok(());
        };
        if item.state != sdm_core::ItemState::Completed || item.file_missing {
            return Ok(());
        }
        self.host
            .open_path(&folder.join(&item.filename))
            .map_err(|error| AppError::failed(error.0))
    }

    /// Shows an item in the file manager: its finished file selected when it
    /// exists, otherwise its package folder opened.
    pub async fn reveal_item(&self, id: &str) -> Result<(), AppError> {
        let id = parse_id(id)?;
        let Some((folder, item)) = self.locate_item(id).await else {
            return Ok(());
        };
        let file = folder.join(&item.filename);
        let finished = item.state == sdm_core::ItemState::Completed && !item.file_missing;
        let shown = if finished && file.exists() {
            self.host.reveal_in_file_manager(&file)
        } else {
            self.host.open_path(&folder)
        };
        shown.map_err(|error| AppError::failed(error.0))
    }

    /// Opens a package's folder in the file manager.
    pub async fn reveal_package(&self, id: &str) -> Result<(), AppError> {
        let id = parse_id(id)?;
        let download_folder = lock(&self.settings).download_folder();
        let snapshot = self.engine.snapshot().await;
        let Some(package) = snapshot.packages.iter().find(|package| package.id == id) else {
            return Ok(());
        };
        self.host
            .open_path(&download_folder.join(&package.name))
            .map_err(|error| AppError::failed(error.0))
    }

    /// An item's package folder and its snapshot, if it exists.
    async fn locate_item(
        &self,
        id: Uuid,
    ) -> Option<(std::path::PathBuf, sdm_engine::ItemSnapshot)> {
        let download_folder = lock(&self.settings).download_folder();
        let snapshot = self.engine.snapshot().await;
        snapshot.packages.iter().find_map(|package| {
            package
                .items
                .iter()
                .find(|item| item.id == id)
                .map(|item| (download_folder.join(&package.name), item.clone()))
        })
    }

    /// Publishes once after a batch; an empty batch changed nothing.
    async fn publish_if_any(&self, ids: &[Uuid]) -> Result<(), AppError> {
        if ids.is_empty() {
            return Ok(());
        }
        self.publish(false).await
    }

    pub async fn pause_all(&self) -> Result<(), AppError> {
        self.engine.pause_all().await;
        self.publish(false).await
    }

    pub async fn resume_all(&self) -> Result<(), AppError> {
        self.engine.resume_all().await;
        self.publish(false).await
    }

    pub async fn set_list_visible(&self, visible: bool) -> Result<(), AppError> {
        let became_visible = {
            let mut state = lock(&self.state);
            let was = state.visible;
            state.visible = visible;
            if visible && !was {
                state.need_full = true;
            }
            visible && !was
        };
        if became_visible {
            self.publish(true).await?;
        }
        Ok(())
    }

    /// Reports which items have a details panel open for `owner` (a window or
    /// panel id). Replaces that owner's previous set. Any id that is not a
    /// UUID rejects the whole call and changes nothing.
    pub async fn set_detail_items(&self, owner: &str, ids: &[String]) -> Result<(), AppError> {
        let parsed: HashSet<Uuid> = ids
            .iter()
            .map(|id| parse_id(id))
            .collect::<Result<_, _>>()?;
        {
            let mut state = lock(&self.state);
            if parsed.is_empty() {
                state.detail_items.remove(owner);
            } else {
                state.detail_items.insert(owner.to_owned(), parsed);
            }
        }
        self.publish(false).await
    }

    /// An explicit resync: sends a full patch whatever was sent before.
    pub async fn full_state(&self) -> Result<(), AppError> {
        self.publish(true).await
    }

    // MARK: Settings and misc

    pub fn get_settings(&self) -> SettingsDto {
        lock(&self.settings).get()
    }

    pub async fn update_settings(&self, dto: SettingsDto) -> Result<(), AppError> {
        let new_settings = {
            let mut store = lock(&self.settings);
            store.update(dto)?;
            engine_settings(&store.get(), store.download_folder(), self.fixed_limits)
        };
        self.engine.update_settings(new_settings).await;
        self.publish(false).await
    }

    pub fn binaries_status(&self) -> BinariesStatusDto {
        match &self.binaries {
            Some(binaries) => {
                let status = binaries.status();
                BinariesStatusDto {
                    downloading: matches!(binaries.tools_activity(), ToolsActivity::Fetching(_)),
                    yt_dlp_version: status.yt_dlp_version,
                    latest_known: status.latest_known,
                    ffmpeg_version: status.ffmpeg_version,
                    qjs_version: status.qjs_version,
                    last_error: status.last_error,
                }
            }
            None => BinariesStatusDto {
                downloading: false,
                yt_dlp_version: None,
                latest_known: None,
                ffmpeg_version: None,
                qjs_version: None,
                last_error: None,
            },
        }
    }

    /// Looks for a newer yt-dlp now ("Check now"). Waits for the check.
    pub async fn check_binaries_now(&self) -> Result<(), AppError> {
        let binaries = self
            .binaries
            .as_ref()
            .ok_or_else(|| AppError::failed("the managed tools are not available"))?;
        binaries.check_now(CheckReason::Manual).await;
        Ok(())
    }

    /// The bundled themes for the picker, by name.
    pub fn list_themes(&self) -> Vec<ThemeInfo> {
        ThemeCatalog::built_in_themes()
            .into_iter()
            .map(|theme| ThemeInfo {
                id: theme.id,
                name: theme.name,
            })
            .collect()
    }

    /// The CSS for `theme_id` (the default theme, `dark`, when `None`).
    pub fn theme_css(&self, theme_id: Option<&str>) -> Result<String, AppError> {
        let wanted = theme_id.unwrap_or("dark");
        ThemeCatalog::built_in_themes()
            .into_iter()
            .find(|theme| theme.id == wanted)
            .map(|theme| theme_css(&theme))
            .ok_or_else(|| AppError::invalid("themeId", format!("no theme named {wanted}")))
    }

    /// Stops everything and writes durable state. Call when the app quits.
    pub async fn shutdown(&self) {
        self.engine.shutdown().await;
    }

    // MARK: Link grabber

    fn grabber(&self) -> Result<&Arc<GrabberController>, AppError> {
        self.grabber
            .as_ref()
            .ok_or_else(|| AppError::failed("the link grabber is not available"))
    }

    /// The line under the grabber header about downloading `ffmpeg`/`qjs`.
    fn tools_notice(&self) -> Option<ToolsNotice> {
        match self.binaries.as_ref()?.tools_activity() {
            ToolsActivity::Idle => None,
            ToolsActivity::Fetching(name) => Some(ToolsNotice {
                text: format!("Fetching {name}…"),
                is_error: false,
            }),
            ToolsActivity::Failed(message) => Some(ToolsNotice {
                text: format!("Could not {message}"),
                is_error: true,
            }),
        }
    }

    /// Extracts the links in `text`, probes them, and publishes as they land.
    pub async fn grabber_ingest_text(&self, text: &str) -> Result<(), AppError> {
        let grabber = self.grabber()?;
        if let Some(binaries) = self.binaries.clone() {
            // A new batch is a reason to look for missing tools now, not at
            // the next timer; never waited for.
            tokio::spawn(async move {
                binaries
                    .check_now(sdm_resolve::CheckReason::ResolveNeeded)
                    .await;
            });
        }
        grabber.session.ingest_text(text).await;
        self.publish(false).await
    }

    pub async fn grabber_remove_links(&self, ids: &[String]) -> Result<(), AppError> {
        let grabber = self.grabber()?;
        let ids = parse_ids(ids)?;
        for id in &ids {
            grabber.session.remove_link(*id);
        }
        self.publish(false).await
    }

    pub async fn grabber_remove_package(&self, id: &str) -> Result<(), AppError> {
        let grabber = self.grabber()?;
        grabber.session.remove_package(parse_id(id)?);
        self.publish(false).await
    }

    pub async fn grabber_clear(&self) -> Result<(), AppError> {
        let grabber = self.grabber()?;
        grabber.session.clear();
        grabber.set_held_back(None);
        self.publish(false).await
    }

    pub async fn grabber_move_link(&self, id: &str, package_id: &str) -> Result<(), AppError> {
        let grabber = self.grabber()?;
        grabber
            .session
            .move_link(parse_id(id)?, parse_id(package_id)?);
        self.publish(false).await
    }

    pub async fn grabber_rename_package(&self, id: &str, name: &str) -> Result<(), AppError> {
        let grabber = self.grabber()?;
        let id = parse_id(id)?;
        if name.trim().is_empty() {
            return Err(AppError::invalid("name", "the name is empty"));
        }
        grabber.session.rename_package(id, name.trim());
        self.publish(false).await
    }

    pub async fn grabber_merge_packages(
        &self,
        source: &str,
        destination: &str,
    ) -> Result<(), AppError> {
        let grabber = self.grabber()?;
        grabber
            .session
            .merge_packages(parse_id(source)?, parse_id(destination)?);
        self.publish(false).await
    }

    pub async fn grabber_split_package(&self, id: &str) -> Result<(), AppError> {
        let grabber = self.grabber()?;
        grabber.session.split_package(parse_id(id)?);
        self.publish(false).await
    }

    /// Re-probes exactly these rows, whatever their verdict.
    pub async fn grabber_recheck(&self, ids: &[String]) -> Result<(), AppError> {
        let grabber = self.grabber()?;
        let ids: HashSet<Uuid> = parse_ids(ids)?.into_iter().collect();
        if ids.is_empty() {
            return Ok(());
        }
        grabber.session.recheck(&ids).await;
        self.publish(false).await
    }

    pub async fn grabber_recheck_failed(&self) -> Result<(), AppError> {
        let grabber = self.grabber()?;
        grabber.session.recheck_failed().await;
        self.publish(false).await
    }

    pub async fn grabber_cancel_checks(&self) -> Result<(), AppError> {
        let grabber = self.grabber()?;
        grabber.session.cancel_checks();
        self.publish(false).await
    }

    /// The address "Copy URL" copies: a link's address or a media row's page.
    pub fn grabber_copy_url(&self, id: &str) -> Result<String, AppError> {
        let id = parse_id(id)?;
        let snapshot = self.grabber()?.session.snapshot();
        let url = match snapshot.links.iter().find(|l| l.id == id) {
            Some(link) => link.final_url.as_str().to_owned(),
            None => snapshot
                .media_rows
                .iter()
                .find(|r| r.id == id)
                .map(|r| r.source_url.as_str().to_owned())
                .ok_or_else(|| AppError::invalid("id", "that link is no longer in the grabber"))?,
        };
        // The caller is about to put this on the clipboard: do not grab it back.
        lock(&self.clipboard).ignore_own_write(&url);
        Ok(url)
    }

    /// What the tray shows now, from the last published snapshot. Takes no
    /// engine await, so the shell can call it on every tick.
    pub fn tray_status(&self) -> TrayStatus {
        let links_waiting = self.grabber.as_ref().map_or(0, |grabber| {
            u32::try_from(grabber.session.row_ids().len()).unwrap_or(u32::MAX)
        });
        match &lock(&self.state).previous {
            Some(snapshot) => TrayStatus::from_snapshot(snapshot, links_waiting),
            None => TrayStatus::from_snapshot(
                &EngineSnapshot::new(Vec::new(), 0.0, Vec::new(), 0),
                links_waiting,
            ),
        }
    }

    /// Whether the clipboard should be read at all. The shell checks this
    /// before every read so a switched-off watcher never touches the clipboard.
    pub fn clipboard_watching_enabled(&self) -> bool {
        lock(&self.settings).get().clipboard_watching_enabled
    }

    /// The shell's clipboard poll: `text` is what the clipboard holds now
    /// (`None` when empty or not text). Links in new text go to the grabber.
    pub async fn clipboard_changed(&self, text: Option<String>) -> Result<(), AppError> {
        if !self.clipboard_watching_enabled() {
            return Ok(());
        }
        let fresh = {
            let mut watcher = lock(&self.clipboard);
            watcher.changed(text.as_deref()).map(str::to_owned)
        };
        match fresh {
            Some(text) if self.grabber.is_some() => self.grabber_ingest_text(&text).await,
            _ => Ok(()),
        }
    }

    /// Applies one of the row's picker options. A stale option id (the row
    /// was re-resolved, or is not pickable) changes nothing and says so.
    pub async fn grabber_set_format(&self, id: &str, option_id: &str) -> Result<(), AppError> {
        let grabber = self.grabber()?;
        let id = parse_id(id)?;
        let snapshot = grabber.session.snapshot();
        let row = snapshot
            .media_rows
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| AppError::invalid("id", "that video is no longer in the grabber"))?;
        let media = row
            .media
            .as_ref()
            .ok_or_else(|| AppError::invalid("optionId", "this video has no formats yet"))?;
        let option = MediaFormatMenu::options(media, &grabber.quality_preferences())
            .into_iter()
            .find(|option| option.id == option_id)
            .ok_or_else(|| AppError::invalid("optionId", "that format is no longer offered"))?;
        grabber.session.set_format_choice(option.choice, id);
        self.publish(false).await
    }

    /// Hands grabber rows to the engine as packages, then removes the rows
    /// that went. Package and "all" targets send every link and every
    /// resolved media row (the rest are held back); a row selection sends
    /// only addable rows (online or faulty links, resolved media).
    pub async fn grabber_add_to_downloads(
        &self,
        target: HandoffTarget,
        start: bool,
    ) -> Result<(), AppError> {
        let grabber = self.grabber()?;
        let snapshot = grabber.session.snapshot();
        let mut held_back_total: Option<usize> = None;
        let mut groups: Vec<HandoffGroup> = Vec::new();
        match &target {
            HandoffTarget::Rows { ids } => {
                let wanted: HashSet<Uuid> = parse_ids(ids)?.into_iter().collect();
                groups = selection_groups(&snapshot, &wanted);
            }
            HandoffTarget::Package { id } => {
                let id = parse_id(id)?;
                let package = snapshot
                    .packages
                    .iter()
                    .find(|p| p.id == id)
                    .ok_or_else(|| AppError::invalid("id", "that package is no longer there"))?;
                let (group, held) = package_group(&snapshot, package);
                held_back_total = Some(held);
                groups.extend(group);
            }
            HandoffTarget::All => {
                let mut held = 0;
                for package in &snapshot.packages {
                    let (group, held_here) = package_group(&snapshot, package);
                    held += held_here;
                    groups.extend(group);
                }
                held_back_total = Some(held);
            }
        }
        if let Some(held) = held_back_total {
            grabber.set_held_back((held > 0).then_some(held as u32));
        }
        // Every group is resolved above, before anything is removed:
        // removing a row reclusters the packages.
        for group in groups {
            let items: Vec<DownloadItem> = group
                .items
                .into_iter()
                .map(|mut item| {
                    item.state = if start {
                        sdm_core::ItemState::Queued
                    } else {
                        sdm_core::ItemState::Stopped
                    };
                    item.is_enabled = true;
                    item
                })
                .collect();
            let mut package = DownloadPackage::new(group.name).with_items(items);
            package.note = group.note;
            self.add_package(package).await?;
            for id in group.handoff_ids {
                grabber.session.remove_link(id);
            }
        }
        self.publish(false).await
    }

    /// The managed-binaries provisioning step for launch: download any missing
    /// tool and check for a newer yt-dlp. Meant for a background task.
    pub async fn provision_binaries(&self) {
        if let Some(binaries) = &self.binaries {
            let _ = binaries.check_now(sdm_resolve::CheckReason::Launch).await;
        }
    }
}

/// One engine package's worth of grabber rows.
struct HandoffGroup {
    name: String,
    note: Option<String>,
    items: Vec<DownloadItem>,
    /// The rows that become downloads and leave the grabber.
    handoff_ids: Vec<Uuid>,
}

/// A whole grabber package. The second value counts media rows held back for
/// want of a format. `None` when nothing in it can be handed off.
fn package_group(
    snapshot: &GrabberSnapshot,
    package: &PackageCandidate,
) -> (Option<HandoffGroup>, usize) {
    let ids: HashSet<Uuid> = package.link_ids.iter().copied().collect();
    let links: Vec<ProbedLink> = snapshot
        .links
        .iter()
        .filter(|l| ids.contains(&l.id))
        .cloned()
        .collect();
    let media: Vec<MediaRow> = snapshot
        .media_rows
        .iter()
        .filter(|r| ids.contains(&r.id))
        .cloned()
        .collect();
    let (items, held_back) = MediaHandoff::build(&links, &media);
    if items.is_empty() {
        return (None, held_back);
    }
    let handoff_ids = links
        .iter()
        .map(|l| l.id)
        .chain(
            media
                .iter()
                .filter(|r| r.state == MediaRowState::Resolved)
                .map(|r| r.id),
        )
        .collect();
    let group = HandoffGroup {
        name: group_name(&package.name),
        note: package.note.clone(),
        items,
        handoff_ids,
    };
    (Some(group), held_back)
}

/// A row is addable when an HTTP link is online or faulty, or a media row is resolved.
fn is_addable(snapshot: &GrabberSnapshot, id: Uuid) -> bool {
    if let Some(link) = snapshot.links.iter().find(|l| l.id == id) {
        return matches!(link.verdict, Some(Verdict::Online | Verdict::Faulty { .. }));
    }
    snapshot
        .media_rows
        .iter()
        .find(|r| r.id == id)
        .is_some_and(|r| r.state == MediaRowState::Resolved)
}

/// One group per package the addable selected rows belong to; rows in no
/// package each go under their own filename.
fn selection_groups(snapshot: &GrabberSnapshot, wanted: &HashSet<Uuid>) -> Vec<HandoffGroup> {
    struct Pending {
        note: Option<String>,
        links: Vec<ProbedLink>,
        media: Vec<MediaRow>,
    }
    let mut order: Vec<String> = Vec::new();
    let mut by_name: HashMap<String, Pending> = HashMap::new();
    // Selection order is the session's row order, so the outcome is stable.
    let row_ids = snapshot
        .links
        .iter()
        .map(|l| l.id)
        .chain(snapshot.media_rows.iter().map(|r| r.id))
        .filter(|id| wanted.contains(id) && is_addable(snapshot, *id));
    for id in row_ids {
        let package = snapshot.packages.iter().find(|p| p.link_ids.contains(&id));
        let link = snapshot.links.iter().find(|l| l.id == id);
        let media = snapshot.media_rows.iter().find(|r| r.id == id);
        let name = match package {
            Some(package) => package.name.clone(),
            None => link
                .map(|l| l.effective_filename())
                .or_else(|| media.map(|r| r.display_filename()))
                .unwrap_or_else(|| "Download".to_owned()),
        };
        let pending = by_name.entry(name.clone()).or_insert_with(|| {
            order.push(name.clone());
            Pending {
                note: package.and_then(|p| p.note.clone()),
                links: Vec::new(),
                media: Vec::new(),
            }
        });
        if let Some(link) = link {
            pending.links.push(link.clone());
        } else if let Some(media) = media {
            pending.media.push(media.clone());
        }
    }
    order
        .into_iter()
        .filter_map(|name| {
            let pending = by_name.remove(&name)?;
            let (items, _) = MediaHandoff::build(&pending.links, &pending.media);
            if items.is_empty() {
                return None;
            }
            Some(HandoffGroup {
                name: group_name(&name),
                note: pending.note,
                items,
                handoff_ids: pending
                    .links
                    .iter()
                    .map(|l| l.id)
                    .chain(pending.media.iter().map(|r| r.id))
                    .collect(),
            })
        })
        .collect()
}

/// A package name the engine accepts (it refuses an empty one).
fn group_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "Download".to_owned()
    } else {
        trimmed.to_owned()
    }
}
