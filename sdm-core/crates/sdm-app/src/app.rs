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
use sdm_resolve::ManagedBinaries;
use uuid::Uuid;

use crate::differ::{Delta, Differ};
use crate::error::AppError;
use crate::host::{Host, PatchSink};
use crate::notify::notifications_between;
use crate::protocol::*;
use crate::settings::SettingsStore;
use crate::theme::theme_css;
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
}

/// How many downloads, segments and connections the engine may use at once.
#[derive(Debug, Clone, Copy)]
struct Limits {
    concurrent: usize,
    segments: usize,
    connections: usize,
}

/// Engine defaults the SwiftUI app uses (`EngineSettingsStore`): 3 concurrent
/// downloads, 8 segments, 32 connections, 8 per host, 10 MB minimum segment.
const DEFAULT_LIMITS: Limits = Limits {
    concurrent: 3,
    segments: 8,
    connections: 32,
};

fn engine_settings(limits: Limits, folder: std::path::PathBuf) -> EngineSettings {
    EngineSettings::new(
        limits.concurrent,
        limits.segments,
        limits.connections,
        folder,
    )
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
    limits: Limits,
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
        Self::with_limits(host, dependencies, DEFAULT_LIMITS)
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
        Self::with_limits(host, dependencies, limits)
    }

    fn with_limits(
        host: Arc<dyn Host>,
        dependencies: Dependencies,
        limits: Limits,
    ) -> Result<Arc<Self>, AppError> {
        let data_dir = host.data_dir();
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| AppError::failed(format!("cannot create {}: {e}", data_dir.display())))?;
        let settings =
            SettingsStore::load(data_dir.join("settings.json"), host.default_download_dir());
        let engine = DownloadEngine::new(
            dependencies.transport,
            dependencies.state_store,
            engine_settings(limits, settings.download_folder()),
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
            limits,
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
        let run_binaries_tick = {
            let mut state = lock(&self.state);
            state.ticks += 1;
            state
                .ticks
                .is_multiple_of(AppTiming::TICKS_PER_SECOND as u64)
        };
        if run_binaries_tick && let Some(binaries) = &self.binaries {
            binaries.tick().await;
        }
        self.publish(false).await
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

            if emit_allowed {
                let history_items: Vec<ItemHistory> = histories
                    .into_iter()
                    .map(|(id, speed_history)| ItemHistory {
                        id: id.to_string(),
                        speed_history,
                    })
                    .collect();
                if delta.is_some() || !history_items.is_empty() {
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
        for notification in due {
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
            engine_settings(self.limits, store.download_folder())
        };
        self.engine.update_settings(new_settings).await;
        self.publish(false).await
    }

    pub fn binaries_status(&self) -> BinariesStatusDto {
        match &self.binaries {
            Some(binaries) => {
                let status = binaries.status();
                BinariesStatusDto {
                    yt_dlp_version: status.yt_dlp_version,
                    latest_known: status.latest_known,
                    ffmpeg_version: status.ffmpeg_version,
                    qjs_version: status.qjs_version,
                    last_error: status.last_error,
                }
            }
            None => BinariesStatusDto {
                yt_dlp_version: None,
                latest_known: None,
                ffmpeg_version: None,
                qjs_version: None,
                last_error: None,
            },
        }
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

    /// The managed-binaries provisioning step for launch: inflate bundled
    /// blobs (macOS) and check for a newer yt-dlp. Meant for a background task.
    pub async fn provision_binaries(&self) {
        if let Some(binaries) = &self.binaries {
            binaries.provision_bundled_if_needed().await;
            let _ = binaries.check_now(sdm_resolve::CheckReason::Launch).await;
        }
    }
}
