//! The production collaborators: reqwest transport, JSON state store, system
//! Trash, yt-dlp resolver and wholesale downloader, ffmpeg muxer, and the
//! managed-binaries owner of `<data dir>/bin`.

use std::sync::Arc;

use sdm_core::{LinkResolver, WholesaleDownloader};
use sdm_engine::{FfmpegMuxer, JsonStateStore, Muxer, ReqwestTransport, RetryPolicy, SystemTrash};
use sdm_resolve::{
    BinaryLocator, CookieSource, ManagedBinaries, ProcessRunner, ReqwestBinaryFetcher,
    SystemProcessRunner, YtDlpChannel, YtDlpResolver, YtDlpWholesaleDownloader,
};

use crate::app::{App, Dependencies};
use crate::error::AppError;
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

    // No bundled blobs here: on macOS the SwiftUI app's bundle holds them and
    // shares this `bin/` directory; Windows and Linux download (Task 16).
    let invalidate = Arc::clone(&locator);
    let binaries = ManagedBinaries::new(
        bin_directory,
        Arc::new(ReqwestBinaryFetcher::new()),
        runner,
        Vec::new,
        || YtDlpChannel::Stable,
    )
    .with_on_binaries_changed(move || {
        let locator = Arc::clone(&invalidate);
        Box::pin(async move { locator.invalidate() })
    });

    Dependencies {
        transport: Arc::new(ReqwestTransport::new()),
        state_store: Arc::new(JsonStateStore::new(data_dir.join("state.json"))),
        trash: Arc::new(SystemTrash),
        resolver: Some(resolver),
        muxer: Some(muxer),
        wholesale_downloader: Some(wholesale),
        retry_policy: RetryPolicy::new(),
        binaries: Some(binaries),
    }
}

impl App {
    pub fn production(host: Arc<dyn Host>) -> Result<Arc<App>, AppError> {
        let dependencies = production_dependencies(host.as_ref());
        App::new(host, dependencies)
    }
}
