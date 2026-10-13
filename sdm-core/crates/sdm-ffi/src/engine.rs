//! `DownloadEngine`: the FFI wrapper over `sdm_engine::DownloadEngine`.
//!
//! Every method returns `Result<_, CoreError>` and runs on the core runtime
//! (see `runtime`), so a Rust panic reaches Swift as a thrown
//! [`CoreError::Panicked`] and never aborts the process.
//!
//! # Concurrency and panics
//!
//! The engine's state sits behind one `std::sync::Mutex`, locked for each
//! synchronous stretch and never across an `.await`, so concurrent calls from
//! any number of Swift tasks (a `tick` in flight while the UI calls
//! `set_enabled`) interleave at await points and cannot deadlock. If a call
//! panics while the lock is held the mutex is poisoned; the engine's
//! `Inner::lock` recovers the guard (`unwrap_or_else(PoisonError::into_inner)`)
//! rather than propagating the poison. That is deliberate: the state is a set
//! of plain collections mutated in small steps, so what a panicking stretch
//! leaves behind is a state the next `tick` and `reconcile` repair, and the
//! alternative -- every later call panicking too -- would turn one bug into a
//! dead app.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use sdm_core::{DownloadPackage, LinkResolver, Priority, WholesaleDownloader};
use sdm_engine::{
    EngineSettings, EngineSnapshot, HttpTransport, JsonStateStore, Muxer, ReqwestTransport,
    RetryPolicy, StateStore, SystemTrash, Trash,
};
use sdm_resolve::CookieSource;
use uuid::Uuid;

use crate::error::CoreError;
use crate::runtime::{guard, run};
use crate::shared::Toolchain;

/// How to build a production [`DownloadEngine`].
#[derive(Debug, Clone, uniffi::Record)]
pub struct EngineConfig {
    /// The JSON state file (`state.json`). Created on first save.
    pub state_file: PathBuf,
    pub settings: EngineSettings,
    /// Where `yt-dlp` and `ffmpeg` are looked for, in order, after a Settings
    /// override. Empty means the default install locations (Homebrew,
    /// `/usr/local/bin`, `/usr/bin`, `~/.local/bin`).
    pub binary_search_paths: Vec<PathBuf>,
    /// Initial yt-dlp cookie source; change it later with `set_cookie_source`.
    pub cookie_source: CookieSource,
    /// Initial cap on playlist videos resolved (Swift's default was 50).
    pub max_playlist_videos: u32,
    /// Initial extra arguments for every yt-dlp invocation.
    pub yt_dlp_extra_arguments: Vec<String>,
}

/// The collaborators of an engine, for tests that substitute fakes. Not
/// exported through UniFFI.
pub struct EngineDependencies {
    pub transport: Arc<dyn HttpTransport>,
    pub state_store: Arc<dyn StateStore>,
    pub trash: Arc<dyn Trash>,
    pub resolver: Option<Arc<dyn LinkResolver>>,
    pub muxer: Option<Arc<dyn Muxer>>,
    pub wholesale_downloader: Option<Arc<dyn WholesaleDownloader>>,
    pub retry_policy: RetryPolicy,
}

/// The download engine. See the module documentation for the threading model.
#[derive(uniffi::Object)]
pub struct DownloadEngine {
    engine: Arc<sdm_engine::DownloadEngine>,
    tools: Toolchain,
}

/// Checks the preconditions `EngineSettings::validate` asserts, as errors.
pub fn validate_settings(settings: &EngineSettings) -> Result<(), CoreError> {
    let at_least_one = |name: &str, value: usize| {
        if value >= 1 {
            Ok(())
        } else {
            Err(CoreError::invalid(name, "must be at least 1"))
        }
    };
    at_least_one("maxConcurrent", settings.max_concurrent)?;
    at_least_one("segmentsPerItem", settings.segments_per_item)?;
    at_least_one("globalMaxConnections", settings.global_max_connections)?;
    at_least_one("maxConnectionsPerHost", settings.max_connections_per_host)?;
    at_least_one("persistDebounceTicks", settings.persist_debounce_ticks)?;
    at_least_one("fileCheckIntervalTicks", settings.file_check_interval_ticks)?;
    if settings.checkpoint_interval_bytes <= 0 {
        return Err(CoreError::invalid(
            "checkpointIntervalBytes",
            "must be positive",
        ));
    }
    if settings.min_segment_size_bytes <= 0 {
        return Err(CoreError::invalid(
            "minSegmentSizeBytes",
            "must be positive",
        ));
    }
    Ok(())
}

/// Refuses a package the engine would trap on: no name, or an item with no
/// component or no output filename (the invariants `DownloadItem::new`
/// asserts and a hand-built record can break).
pub fn validate_package(package: &DownloadPackage) -> Result<(), CoreError> {
    if package.name.is_empty() {
        return Err(CoreError::invalid("package", "the package name is empty"));
    }
    for item in &package.items {
        if item.components.is_empty() {
            return Err(CoreError::invalid(
                "package",
                "a download item needs at least one component",
            ));
        }
        if item.output_filename.is_empty() {
            return Err(CoreError::invalid(
                "package",
                "a download item's output filename is empty",
            ));
        }
    }
    Ok(())
}

impl DownloadEngine {
    /// An engine over injected collaborators, for tests. `new` builds the
    /// production ones and calls this.
    pub fn with_dependencies(
        settings: EngineSettings,
        dependencies: EngineDependencies,
        tools: Toolchain,
    ) -> Result<Arc<Self>, CoreError> {
        validate_settings(&settings)?;
        let EngineDependencies {
            transport,
            state_store,
            trash,
            resolver,
            muxer,
            wholesale_downloader,
            retry_policy,
        } = dependencies;
        let engine = guard(|| {
            sdm_engine::DownloadEngine::new(
                transport,
                state_store,
                settings,
                retry_policy,
                resolver,
                muxer,
                wholesale_downloader,
            )
            .with_trash(trash)
        })?;
        Ok(Arc::new(Self {
            engine: Arc::new(engine),
            tools,
        }))
    }

    /// A toolchain with no search paths set, for tests.
    pub fn default_tools() -> Toolchain {
        Toolchain::new(Vec::new())
    }

    /// Test hook: panics inside the engine wrapper's guarded path, exactly as
    /// any exported call would if the code under it panicked.
    #[doc(hidden)]
    pub async fn panic_for_testing(&self, message: String) -> Result<(), CoreError> {
        run(async move { panic!("{message}") }).await
    }

    /// Test hook: the synchronous counterpart of [`Self::panic_for_testing`].
    #[doc(hidden)]
    pub fn panic_synchronously_for_testing(&self, message: String) -> Result<(), CoreError> {
        guard(|| panic!("{message}"))
    }
}

#[uniffi::export]
impl DownloadEngine {
    /// Builds an engine with the production collaborators: the `reqwest`
    /// transport, a JSON state store at `config.state_file`, the system
    /// Trash, the yt-dlp resolver and wholesale downloader, and the ffmpeg
    /// muxer.
    #[uniffi::constructor]
    pub fn new(config: EngineConfig) -> Result<Arc<Self>, CoreError> {
        let tools = Toolchain::configured(
            config.binary_search_paths,
            config.cookie_source,
            config.max_playlist_videos,
            config.yt_dlp_extra_arguments,
        );
        let dependencies = EngineDependencies {
            transport: Arc::new(ReqwestTransport::new()),
            state_store: Arc::new(JsonStateStore::new(config.state_file)),
            trash: Arc::new(SystemTrash),
            resolver: Some(tools.resolver()),
            muxer: Some(tools.muxer()),
            wholesale_downloader: Some(tools.wholesale_downloader()),
            retry_policy: RetryPolicy::new(),
        };
        Self::with_dependencies(config.settings, dependencies, tools)
    }

    // MARK: Settings the yt-dlp closures read

    pub fn set_cookie_source(&self, source: CookieSource) -> Result<(), CoreError> {
        guard(|| self.tools.settings.set_cookie_source(source))
    }

    pub fn set_max_playlist_videos(&self, count: u32) -> Result<(), CoreError> {
        guard(|| self.tools.settings.set_max_playlist_videos(count as usize))
    }

    pub fn set_yt_dlp_extra_arguments(&self, arguments: Vec<String>) -> Result<(), CoreError> {
        guard(|| self.tools.settings.set_extra_arguments(arguments))
    }

    /// Points the binary `name` (`"yt-dlp"`, `"ffmpeg"`) at `path`, or clears
    /// the override with `None`.
    pub fn set_binary_override(
        &self,
        name: String,
        path: Option<PathBuf>,
    ) -> Result<(), CoreError> {
        guard(|| self.tools.set_binary_override(&name, path))?
    }

    // MARK: Lifecycle

    pub async fn add(&self, package: DownloadPackage) -> Result<(), CoreError> {
        validate_package(&package)?;
        let engine = Arc::clone(&self.engine);
        run(async move { engine.add(package).await }).await
    }

    pub async fn restore(&self) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.restore().await }).await
    }

    pub async fn update_settings(&self, settings: EngineSettings) -> Result<(), CoreError> {
        validate_settings(&settings)?;
        let engine = Arc::clone(&self.engine);
        run(async move { engine.update_settings(settings).await }).await
    }

    pub async fn set_enabled(&self, enabled: bool, item_id: Uuid) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.set_enabled(enabled, item_id).await }).await
    }

    pub async fn start_item(&self, item_id: Uuid) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.start_item(item_id).await }).await
    }

    pub async fn retry(&self, item_id: Uuid) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.retry(item_id).await }).await
    }

    pub async fn retry_mux(&self, item_id: Uuid) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.retry_mux(item_id).await }).await
    }

    pub async fn stop_item(&self, item_id: Uuid) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.stop_item(item_id).await }).await
    }

    pub async fn set_priority(
        &self,
        priority: Option<Priority>,
        item_id: Uuid,
    ) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.set_priority(priority, item_id).await }).await
    }

    pub async fn pause_all(&self) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.pause_all().await }).await
    }

    pub async fn resume_all(&self) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.resume_all().await }).await
    }

    pub async fn set_segment_count(&self, count: usize, item_id: Uuid) -> Result<(), CoreError> {
        if count < 1 {
            return Err(CoreError::invalid("count", "must be at least 1"));
        }
        let engine = Arc::clone(&self.engine);
        run(async move { engine.set_segment_count(count, item_id).await }).await
    }

    /// Stops everything and writes durable state. Idempotent.
    pub async fn shutdown(&self) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.shutdown().await }).await
    }

    // MARK: Heartbeat and snapshots

    /// Advances the engine one tick (`AppTiming.ticksPerSecond` per second).
    pub async fn tick(&self) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.tick().await }).await
    }

    pub async fn snapshot(&self) -> Result<EngineSnapshot, CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.snapshot().await }).await
    }

    /// The snapshot, or `None` when its `generation` equals `since_generation`
    /// (the caller already holds it). Pass `None` for the first call. The
    /// engine decides this under its lock without copying the snapshot, so an
    /// idle engine costs one comparison per tick instead of a serialised list.
    pub async fn snapshot_if_changed(
        &self,
        since_generation: Option<u64>,
    ) -> Result<Option<EngineSnapshot>, CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.snapshot_if_changed(since_generation).await }).await
    }

    /// Each requested item's full speed history; items with no sampler are
    /// absent from the result.
    pub async fn speed_histories(
        &self,
        item_ids: HashSet<Uuid>,
    ) -> Result<HashMap<Uuid, Vec<f64>>, CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.speed_histories(&item_ids).await }).await
    }

    // MARK: Organizing

    pub async fn reorder_items(
        &self,
        item_ids: Vec<Uuid>,
        package_id: Uuid,
    ) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.reorder_items(&item_ids, package_id).await }).await
    }

    pub async fn move_item(
        &self,
        item_id: Uuid,
        to_package: Uuid,
        at_index: Option<usize>,
    ) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.move_item(item_id, to_package, at_index).await }).await
    }

    pub async fn move_items_to_new_package(
        &self,
        item_ids: Vec<Uuid>,
        package_name: String,
    ) -> Result<(), CoreError> {
        if package_name.is_empty() {
            return Err(CoreError::invalid(
                "packageName",
                "the package name is empty",
            ));
        }
        let engine = Arc::clone(&self.engine);
        run(async move {
            engine
                .move_items_to_new_package(&item_ids, &package_name)
                .await
        })
        .await
    }

    pub async fn move_items_to_package(
        &self,
        item_ids: Vec<Uuid>,
        package_id: Uuid,
        at_index: Option<usize>,
    ) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move {
            engine
                .move_items_to_package(&item_ids, package_id, at_index)
                .await
        })
        .await
    }

    pub async fn reorder_packages(&self, package_ids: Vec<Uuid>) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.reorder_packages(&package_ids).await }).await
    }

    pub async fn remove_item(&self, item_id: Uuid, delete_file: bool) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.remove_item(item_id, delete_file).await }).await
    }

    pub async fn remove_package(
        &self,
        package_id: Uuid,
        delete_files: bool,
    ) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.remove_package(package_id, delete_files).await }).await
    }

    pub async fn reset_download(&self, item_id: Uuid) -> Result<(), CoreError> {
        let engine = Arc::clone(&self.engine);
        run(async move { engine.reset_download(item_id).await }).await
    }
}
