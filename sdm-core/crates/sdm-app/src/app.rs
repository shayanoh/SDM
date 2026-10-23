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

/// Engine defaults the SwiftUI app uses (`EngineSettingsStore`): 3 concurrent
/// downloads, 8 segments, 32 connections, 8 per host, 10 MB minimum segment.
fn engine_settings(folder: std::path::PathBuf) -> EngineSettings {
    EngineSettings::new(3, 8, 32, folder)
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
        let data_dir = host.data_dir();
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| AppError::failed(format!("cannot create {}: {e}", data_dir.display())))?;
        let settings =
            SettingsStore::load(data_dir.join("settings.json"), host.default_download_dir());
        let engine = DownloadEngine::new(
            dependencies.transport,
            dependencies.state_store,
            engine_settings(settings.download_folder()),
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
        let (since, histories_wanted) = {
            let state = lock(&self.state);
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
        self.engine.add(package).await;
        self.publish(false).await
    }

    pub async fn start_item(&self, id: &str) -> Result<(), AppError> {
        let id = parse_id(id)?;
        self.engine.start_item(id).await;
        self.publish(false).await
    }

    pub async fn stop_item(&self, id: &str) -> Result<(), AppError> {
        let id = parse_id(id)?;
        self.engine.stop_item(id).await;
        self.publish(false).await
    }

    pub async fn retry_item(&self, id: &str) -> Result<(), AppError> {
        let id = parse_id(id)?;
        self.engine.retry(id).await;
        self.publish(false).await
    }

    pub async fn remove_item(&self, id: &str, delete_file: bool) -> Result<(), AppError> {
        let id = parse_id(id)?;
        self.engine.remove_item(id, delete_file).await;
        {
            // An open details panel for a removed item must not keep fetching.
            let mut state = lock(&self.state);
            for ids in state.detail_items.values_mut() {
                ids.remove(&id);
            }
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
            engine_settings(store.download_folder())
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
