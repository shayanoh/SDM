//! The production collaborators: reqwest transport, JSON state store, system
//! Trash, yt-dlp resolver and wholesale downloader, ffmpeg muxer, and the
//! managed-binaries owner of `<data dir>/bin`.

use std::sync::Arc;

use std::sync::atomic::{AtomicBool, Ordering};

use sdm_core::{LinkResolver, WholesaleDownloader};
use sdm_engine::{FfmpegMuxer, JsonStateStore, Muxer, ReqwestTransport, RetryPolicy, SystemTrash};
use sdm_resolve::{
    BinaryLocator, ManagedBinaries, ProcessRunner, ReqwestBinaryFetcher, SystemProcessRunner,
    YtDlpResolver, YtDlpWholesaleDownloader,
};

use crate::app::{App, Dependencies};
use crate::error::AppError;
use crate::grabber::GrabberController;
use crate::host::Host;
use crate::settings::{
    SettingsStore, channel_reader, cookie_provider, cookie_source_reader, playlist_cap_reader,
};

pub fn production_dependencies(host: &dyn Host) -> Dependencies {
    let data_dir = host.data_dir();
    let settings = SettingsStore::load(data_dir.join("settings.json"), host.default_download_dir());
    let shared = settings.shared();
    let bin_directory = data_dir.join("bin");
    let runner: Arc<dyn ProcessRunner> = Arc::new(SystemProcessRunner::new());

    // The managed `bin/` first, then the platform defaults.
    let mut search_paths = vec![bin_directory.clone()];
    search_paths.extend(BinaryLocator::default_search_paths());
    let locator = Arc::new(BinaryLocator::new().with_search_paths(search_paths));

    // A crash can leave cookie copies behind; none is in use yet.
    sdm_browser::lease::sweep_stale_leases(&data_dir);

    let resolver: Arc<dyn LinkResolver> = Arc::new(
        YtDlpResolver::new(
            Arc::clone(&runner),
            Arc::clone(&locator),
            cookie_source_reader(&shared),
            playlist_cap_reader(&shared),
            extra_arguments(&bin_directory),
        )
        .with_cookie_provider(cookie_provider(&shared, data_dir.clone())),
    );
    let wholesale: Arc<dyn WholesaleDownloader> = Arc::new(
        YtDlpWholesaleDownloader::new(
            Arc::clone(&runner),
            Arc::clone(&locator),
            cookie_source_reader(&shared),
            extra_arguments(&bin_directory),
        )
        .with_cookie_provider(cookie_provider(&shared, data_dir.clone())),
    );
    let muxer: Arc<dyn Muxer> =
        Arc::new(FfmpegMuxer::new(Arc::clone(&runner), Arc::clone(&locator)));

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
        channel_reader(&shared),
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

    let grabber = GrabberController::production(
        Some(Arc::clone(&resolver)),
        ffmpeg_available,
        Arc::clone(&shared),
    );

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
        settings: Some(settings),
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

/// The tokens every yt-dlp invocation carries: the managed `qjs` as its JS
/// runtime (yt-dlp enables only deno by default, and without a runtime
/// YouTube's `n` challenge fails and every format disappears) and the
/// managed `bin/` as the ffmpeg location.
fn extra_arguments(bin_directory: &std::path::Path) -> Arc<dyn Fn() -> Vec<String> + Send + Sync> {
    let arguments = yt_dlp_extra_arguments(bin_directory);
    Arc::new(move || arguments.clone())
}

fn yt_dlp_extra_arguments(bin_directory: &std::path::Path) -> Vec<String> {
    let qjs_name = if cfg!(windows) { "qjs.exe" } else { "qjs" };
    vec![
        "--js-runtimes".to_owned(),
        format!("quickjs:{}", bin_directory.join(qjs_name).display()),
        "--ffmpeg-location".to_owned(),
        bin_directory.display().to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::yt_dlp_extra_arguments;

    #[test]
    fn yt_dlp_gets_the_managed_js_runtime_and_ffmpeg_location() {
        let bin = Path::new("bin-dir");
        let arguments = yt_dlp_extra_arguments(bin);
        let runtime = arguments
            .iter()
            .position(|a| a == "--js-runtimes")
            .expect("--js-runtimes is passed");
        assert!(arguments[runtime + 1].starts_with("quickjs:"));
        assert!(arguments[runtime + 1].contains("bin-dir"));
        let ffmpeg = arguments
            .iter()
            .position(|a| a == "--ffmpeg-location")
            .expect("--ffmpeg-location is passed");
        assert_eq!(arguments[ffmpeg + 1], "bin-dir");
    }
}
