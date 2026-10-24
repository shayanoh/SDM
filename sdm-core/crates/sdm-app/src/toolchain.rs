//! The production collaborators: reqwest transport, JSON state store, system
//! Trash, yt-dlp resolver and wholesale downloader, ffmpeg muxer, and the
//! managed-binaries owner of `<data dir>/bin`.

use std::sync::Arc;

use std::sync::atomic::{AtomicBool, Ordering};

use sdm_core::{LinkResolver, WholesaleDownloader};
use sdm_engine::{FfmpegMuxer, JsonStateStore, Muxer, ReqwestTransport, RetryPolicy, SystemTrash};
use sdm_resolve::{
    BinaryLocator, CookieSource, ManagedBinaries, ProcessRunner, ReqwestBinaryFetcher,
    SystemProcessRunner, YtDlpChannel, YtDlpResolver, YtDlpWholesaleDownloader,
};

use crate::app::{App, Dependencies};
use crate::error::AppError;
use crate::grabber::GrabberController;
use crate::host::Host;

const DEFAULT_MAX_PLAYLIST_VIDEOS: usize = 50;

pub fn production_dependencies(host: &dyn Host) -> Dependencies {
    let data_dir = host.data_dir();
    let bin_directory = data_dir.join("bin");
    let runner: Arc<dyn ProcessRunner> = Arc::new(SystemProcessRunner::new());

    // The managed `bin/` first, then the platform defaults.
    let mut search_paths = vec![bin_directory.clone()];
    search_paths.extend(BinaryLocator::default_search_paths());
    let locator = Arc::new(BinaryLocator::new().with_search_paths(search_paths));

    let resolver: Arc<dyn LinkResolver> = Arc::new(YtDlpResolver::new(
        Arc::clone(&runner),
        Arc::clone(&locator),
        Arc::new(|| CookieSource::None),
        Arc::new(|| DEFAULT_MAX_PLAYLIST_VIDEOS),
        Arc::new(Vec::new),
    ));
    let wholesale: Arc<dyn WholesaleDownloader> = Arc::new(YtDlpWholesaleDownloader::new(
        Arc::clone(&runner),
        Arc::clone(&locator),
        Arc::new(|| CookieSource::None),
        Arc::new(Vec::new),
    ));
    let muxer: Arc<dyn Muxer> =
        Arc::new(FfmpegMuxer::new(Arc::clone(&runner), Arc::clone(&locator)));

    // macOS inflates the blobs its bundle ships into `bin/` (shared with the
    // SwiftUI app); Windows and Linux download the tools instead.
    let bundled_dir = host.bundled_tools_dir();
    let vendor_assets = move || match &bundled_dir {
        Some(dir) => sdm_resolve::vendor_assets(dir, sdm_resolve::host_vendor_arch()),
        None => Vec::new(),
    };
    let invalidate = Arc::clone(&locator);
    // Whether `ffmpeg` is on disk, which the grabber needs to offer muxed
    // formats; refreshed whenever the managed binaries change.
    let ffmpeg_name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let ffmpeg_path = bin_directory.join(ffmpeg_name);
    let ffmpeg_available = Arc::new(AtomicBool::new(is_executable_file(&ffmpeg_path)));
    let flag = Arc::clone(&ffmpeg_available);
    let binaries = ManagedBinaries::new(
        bin_directory,
        Arc::new(ReqwestBinaryFetcher::new()),
        runner,
        vendor_assets,
        || YtDlpChannel::Stable,
    )
    .with_on_binaries_changed(move || {
        let locator = Arc::clone(&invalidate);
        let flag = Arc::clone(&flag);
        let ffmpeg_path = ffmpeg_path.clone();
        Box::pin(async move {
            locator.invalidate();
            flag.store(is_executable_file(&ffmpeg_path), Ordering::SeqCst);
        })
    });

    let grabber = GrabberController::production(Some(Arc::clone(&resolver)), ffmpeg_available);

    Dependencies {
        transport: Arc::new(ReqwestTransport::new()),
        state_store: Arc::new(JsonStateStore::new(data_dir.join("state.json"))),
        trash: Arc::new(SystemTrash),
        resolver: Some(resolver),
        muxer: Some(muxer),
        wholesale_downloader: Some(wholesale),
        retry_policy: RetryPolicy::new(),
        binaries: Some(binaries),
        grabber: Some(grabber),
    }
}

impl App {
    pub fn production(host: Arc<dyn Host>) -> Result<Arc<App>, AppError> {
        let dependencies = production_dependencies(host.as_ref());
        App::new(host, dependencies)
    }
}

/// A file the current user may run (the check `ffmpeg` availability needs).
fn is_executable_file(path: &std::path::Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}
