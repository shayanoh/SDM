use std::ffi::OsString;
use std::fs;
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::binary_fetching::{BinaryFetchError, BinaryFetching};
use crate::binary_locator::is_executable_file;
use crate::lzfse;
use crate::managed_binaries_types::{
    BinariesManifest, CheckOutcome, CheckReason, ManagedBinariesStatus, VendorAsset, YtDlpChannel,
    YtDlpVersion,
};
use crate::process_runner::ProcessRunner;

/// The future `on_binaries_changed` returns (Swift's `async` closure).
pub type BinariesChangedFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

type VendorAssets = Arc<dyn Fn() -> Vec<VendorAsset> + Send + Sync>;
type ChannelSource = Arc<dyn Fn() -> YtDlpChannel + Send + Sync>;
type OnBinariesChanged = Arc<dyn Fn() -> BinariesChangedFuture + Send + Sync>;
type Notify = Arc<dyn Fn(String) + Send + Sync>;

/// Owns `~/Library/Application Support/SDM/bin/`: inflates the bundled
/// `ffmpeg` / `qjs` blobs, downloads and self-updates `yt-dlp`. Time is a
/// tick counter advanced ~1 Hz by the app -- never a wall clock -- so tests
/// drive cadence by calling `tick()` directly. See
/// `docs/superpowers/specs/2026-09-03-managed-binaries-design.md`.
///
/// The Swift type is an actor. The mutex guards only the counters and the
/// in-flight handle; it is locked for each synchronous stretch and never
/// held across an `.await`.
#[derive(Clone)]
pub struct ManagedBinaries {
    inner: Arc<Inner>,
}

struct Inner {
    bin_directory: PathBuf,
    fetcher: Arc<dyn BinaryFetching>,
    runner: Arc<dyn ProcessRunner>,
    vendor_assets: VendorAssets,
    channel: ChannelSource,
    on_binaries_changed: OnBinariesChanged,
    notify: Notify,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    current_tick: i64,
    last_check_at_tick: i64,
    latest_known: Option<String>,
    /// The running check, if any; waiters coalesce onto its result.
    in_flight: Option<watch::Receiver<Option<CheckOutcome>>>,
}

/// Twin of Swift `ManagedBinariesError`. `Display` is the case name because
/// the manifest's `lastError` stores Swift's interpolation of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ManagedBinariesError {
    #[error("checksumMismatch")]
    ChecksumMismatch,
    #[error("checksumLineMissing")]
    ChecksumLineMissing,
}

/// Everything the download path can fail with, so the `lastError` text and
/// the `CheckOutcome` follow Swift's catch order.
#[derive(Debug, thiserror::Error)]
enum DownloadError {
    #[error(transparent)]
    Fetch(#[from] BinaryFetchError),
    #[error(transparent)]
    Checksum(#[from] ManagedBinariesError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
}

impl ManagedBinaries {
    /// 6 h at 1 Hz.
    pub const PRESENT_INTERVAL: i64 = 21_600;
    /// 15 min at 1 Hz.
    pub const ABSENT_INTERVAL: i64 = 900;

    pub fn new(
        bin_directory: PathBuf,
        fetcher: Arc<dyn BinaryFetching>,
        runner: Arc<dyn ProcessRunner>,
        vendor_assets: impl Fn() -> Vec<VendorAsset> + Send + Sync + 'static,
        channel: impl Fn() -> YtDlpChannel + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                bin_directory,
                fetcher,
                runner,
                vendor_assets: Arc::new(vendor_assets),
                channel: Arc::new(channel),
                on_binaries_changed: Arc::new(|| Box::pin(async {})),
                notify: Arc::new(|_| {}),
                state: Mutex::new(State::default()),
            }),
        }
    }

    /// Swift's `onBinariesChanged` init argument (default: does nothing).
    /// Builder-style: call before the first use.
    pub fn with_on_binaries_changed(
        mut self,
        f: impl Fn() -> BinariesChangedFuture + Send + Sync + 'static,
    ) -> Self {
        self.configure(|inner| inner.on_binaries_changed = Arc::new(f));
        self
    }

    /// Swift's `notify` init argument (default: does nothing).
    /// Builder-style: call before the first use.
    pub fn with_notify(mut self, f: impl Fn(String) + Send + Sync + 'static) -> Self {
        self.configure(|inner| inner.notify = Arc::new(f));
        self
    }

    fn configure(&mut self, apply: impl FnOnce(&mut Inner)) {
        let inner = Arc::get_mut(&mut self.inner)
            .expect("configure ManagedBinaries before cloning or using it");
        apply(inner);
    }

    // MARK: - Bundled inflation

    /// Decode any bundled blob whose version differs from what the manifest
    /// records as installed. Cheap, safe to call on every launch.
    ///
    /// Blocking file reads, LZFSE decoding of tens of megabytes and `fsync`
    /// run on tokio's blocking pool, not on an async worker. The caller still
    /// resumes only once every blob has been handled, as in Swift.
    pub async fn provision_bundled_if_needed(&self) {
        let inner = Arc::clone(&self.inner);
        let joined = tokio::task::spawn_blocking(move || inner.provision_bundled_if_needed()).await;
        if let Err(error) = joined
            && error.is_panic()
        {
            std::panic::resume_unwind(error.into_panic());
        }
    }

    // MARK: - Cadence

    /// Advance the clock one tick. Runs a check inline when the interval has
    /// elapsed and none is running. Called ~1 Hz by the app.
    pub async fn tick(&self) {
        let inner = &self.inner;
        let present = is_executable_file(&inner.yt_dlp_path());
        let interval = if present {
            Self::PRESENT_INTERVAL
        } else {
            Self::ABSENT_INTERVAL
        };
        {
            let mut state = inner.state.lock().unwrap();
            state.current_tick += 1;
            if state.in_flight.is_some() || state.current_tick - state.last_check_at_tick < interval
            {
                return;
            }
        }
        self.check_now(CheckReason::Timer).await;
    }

    // MARK: - Update

    /// Check for a newer `yt-dlp` and download it if needed. Coalesces with
    /// any in-flight check -- never runs two at once.
    ///
    /// The check runs on its own task (Swift's unstructured `Task`), so a
    /// caller that stops waiting does not cancel it.
    pub async fn check_now(&self, reason: CheckReason) -> CheckOutcome {
        let mut receiver = {
            let mut state = self.inner.state.lock().unwrap();
            match &state.in_flight {
                Some(receiver) => receiver.clone(),
                None => {
                    let (sender, receiver) = watch::channel(None);
                    state.in_flight = Some(receiver.clone());
                    let inner = Arc::clone(&self.inner);
                    tokio::spawn(async move {
                        let clear = ClearInFlight(Arc::clone(&inner));
                        let outcome = inner.perform_check(reason).await;
                        drop(clear);
                        let _ = sender.send(Some(outcome));
                    });
                    receiver
                }
            }
        };
        match receiver.wait_for(|outcome| outcome.is_some()).await {
            Ok(outcome) => outcome.clone().expect("waited for Some"),
            Err(_) => CheckOutcome::Failed("check task ended unexpectedly".into()),
        }
    }

    pub fn status(&self) -> ManagedBinariesStatus {
        let inner = &self.inner;
        let manifest = inner.load_manifest();
        let latest_known = inner.state.lock().unwrap().latest_known.clone();
        ManagedBinariesStatus {
            yt_dlp_version: manifest.yt_dlp_version.clone(),
            latest_known: latest_known.or_else(|| manifest.yt_dlp_version.clone()),
            channel: (inner.channel)(),
            ffmpeg_version: manifest.ffmpeg_version.clone(),
            qjs_version: manifest.qjs_version.clone(),
            last_check_tick: (manifest.last_check_tick != 0).then_some(manifest.last_check_tick),
            last_error: manifest.last_error.clone(),
        }
    }

    /// Swift's `manifestForTesting`.
    pub fn manifest_for_testing(&self) -> BinariesManifest {
        self.inner.load_manifest()
    }
}

/// Clears `in_flight` when the check task finishes -- or panics, so a
/// crashed check cannot wedge every later `check_now`.
struct ClearInFlight(Arc<Inner>);

impl Drop for ClearInFlight {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().in_flight = None;
    }
}

impl Inner {
    fn yt_dlp_path(&self) -> PathBuf {
        self.bin_directory.join("yt-dlp")
    }

    fn manifest_path(&self) -> PathBuf {
        self.bin_directory.join("manifest.json")
    }

    fn provision_bundled_if_needed(&self) {
        let mut manifest = self.load_manifest();
        for asset in (self.vendor_assets)() {
            if !asset.compressed_path.exists() {
                continue;
            }
            let target = self.bin_directory.join(&asset.name);
            let installed = manifest.version_for(&asset.name).map(str::to_owned);
            let present = is_executable_file(&target);
            if present && installed.as_deref() == Some(asset.version.as_str()) {
                continue;
            }
            let result = (|| -> Result<(), String> {
                let blob = fs::read(&asset.compressed_path).map_err(|e| e.to_string())?;
                let binary = lzfse::decode(&blob).map_err(|e| e.to_string())?;
                self.write_executable(&binary, &target)
                    .map_err(|e| e.to_string())?;
                match asset.name.as_str() {
                    "ffmpeg" | "ffprobe" => manifest.ffmpeg_version = Some(asset.version.clone()),
                    "qjs" => manifest.qjs_version = Some(asset.version.clone()),
                    _ => {}
                }
                self.save_manifest(&manifest).map_err(|e| e.to_string())
            })();
            if let Err(error) = result {
                manifest.last_error = Some(format!("inflate {}: {error}", asset.name));
                let _ = self.save_manifest(&manifest);
            }
        }
    }

    async fn perform_check(&self, reason: CheckReason) -> CheckOutcome {
        let _ = reason; // Swift only logs it.
        let channel = (self.channel)();
        let mut manifest = self.load_manifest();
        {
            let mut state = self.state.lock().unwrap();
            manifest.last_check_tick = state.current_tick;
            state.last_check_at_tick = state.current_tick;
        }

        let tag = match self.fetcher.data(&channel.releases_api()).await {
            Ok(info) => match serde_json::from_slice::<GitHubRelease>(&info) {
                Ok(release) => release.tag_name,
                Err(error) => {
                    manifest.last_error = Some(format!("release info: {error}"));
                    let _ = self.save_manifest(&manifest);
                    return CheckOutcome::Failed(error.to_string());
                }
            },
            Err(error) => {
                if matches!(error, BinaryFetchError::Transport(_)) {
                    manifest.last_error = Some(format!("offline: {error}"));
                    let _ = self.save_manifest(&manifest);
                    return CheckOutcome::NoNetwork;
                }
                manifest.last_error = Some(format!("release info: {error}"));
                let _ = self.save_manifest(&manifest);
                return CheckOutcome::Failed(error.to_string());
            }
        };
        self.state.lock().unwrap().latest_known = Some(tag.clone());

        let have_binary = is_executable_file(&self.yt_dlp_path());
        let needs_download = if !have_binary {
            true
        } else if let (Some(installed), Some(remote)) = (
            manifest
                .yt_dlp_version
                .as_deref()
                .and_then(YtDlpVersion::new),
            YtDlpVersion::new(&tag),
        ) {
            remote > installed
        } else {
            manifest.yt_dlp_version.as_deref() != Some(tag.as_str())
        };

        if !needs_download {
            manifest.last_error = None;
            let _ = self.save_manifest(&manifest);
            return CheckOutcome::UpToDate(manifest.yt_dlp_version.clone());
        }

        match self.download(channel, &tag, &mut manifest).await {
            Ok(()) => {
                (self.on_binaries_changed)().await;
                (self.notify)(format!("yt-dlp updated to {tag}"));
                CheckOutcome::Updated(tag)
            }
            Err(DownloadError::Fetch(error)) => {
                manifest.last_error = Some(format!("download: {error}"));
                let _ = self.save_manifest(&manifest);
                if matches!(error, BinaryFetchError::Transport(_)) {
                    CheckOutcome::NoNetwork
                } else {
                    CheckOutcome::Failed(error.to_string())
                }
            }
            Err(error) => {
                manifest.last_error = Some(error.to_string());
                let _ = self.save_manifest(&manifest);
                CheckOutcome::Failed(error.to_string())
            }
        }
    }

    /// Download, verify, stage, ad-hoc sign, promote, and record the new
    /// version. The checksum is verified before anything touches disk.
    async fn download(
        &self,
        channel: YtDlpChannel,
        tag: &str,
        manifest: &mut BinariesManifest,
    ) -> Result<(), DownloadError> {
        let bytes = self.fetcher.data(&channel.asset_url(tag)).await?;
        let sums_bytes = self.fetcher.data(&channel.sums_url(tag)).await?;
        let sums = String::from_utf8_lossy(&sums_bytes);
        verify(&bytes, &sums, "yt-dlp_macos")?;

        let yt_dlp = self.yt_dlp_path();
        let mut staging_name = OsString::from(yt_dlp.as_os_str());
        staging_name.push(".new");
        let staging = PathBuf::from(staging_name);
        self.write_executable(&bytes, &staging)?;
        // Swift `try?`: a failed or missing xattr/codesign is not fatal.
        let cancel = CancellationToken::new();
        let _ = self
            .runner
            .run(
                Path::new("/usr/bin/xattr"),
                &["-c".to_owned(), staging.to_string_lossy().into_owned()],
                Duration::from_secs(20),
                &cancel,
            )
            .await;
        let _ = self
            .runner
            .run(
                Path::new("/usr/bin/codesign"),
                &[
                    "-s".to_owned(),
                    "-".to_owned(),
                    "--force".to_owned(),
                    staging.to_string_lossy().into_owned(),
                ],
                Duration::from_secs(30),
                &cancel,
            )
            .await;
        promote(&staging, &yt_dlp)?;

        manifest.yt_dlp_version = Some(tag.to_owned());
        manifest.yt_dlp_channel = channel;
        manifest.last_error = None;
        self.save_manifest(manifest)?;
        Ok(())
    }

    fn write_executable(&self, data: &[u8], path: &Path) -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        fs::create_dir_all(&self.bin_directory)?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        write_atomically(path, data)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
    }

    fn load_manifest(&self) -> BinariesManifest {
        fs::read(self.manifest_path())
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_else(BinariesManifest::empty)
    }

    /// Byte-for-byte what Swift's `JSONEncoder` (`.prettyPrinted`,
    /// `.sortedKeys`) writes, so either core can read the other's file.
    fn save_manifest(&self, manifest: &BinariesManifest) -> std::io::Result<()> {
        fs::create_dir_all(&self.bin_directory)?;
        write_atomically(&self.manifest_path(), &manifest.to_foundation_json())
    }
}

/// Checks `bytes` against the line of a `SHA2-256SUMS` file naming `filename`.
fn verify(bytes: &[u8], sums: &str, filename: &str) -> Result<(), ManagedBinariesError> {
    let digest = hex::encode(Sha256::digest(bytes));
    // Swift `isNewline` covers \n \v \f \r, NEL, LS and PS.
    let is_newline = |c: char| {
        matches!(
            c,
            '\n' | '\u{b}' | '\u{c}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'
        )
    };
    for line in sums.split(is_newline) {
        let parts: Vec<&str> = line.split(' ').filter(|p| !p.is_empty()).collect();
        if parts.len() < 2 {
            continue;
        }
        let name = parts[parts.len() - 1].trim_matches(|c: char| c.is_whitespace());
        if name == filename || name.strip_prefix('*') == Some(filename) {
            if parts[0].to_lowercase() != digest {
                return Err(ManagedBinariesError::ChecksumMismatch);
            }
            return Ok(());
        }
    }
    Err(ManagedBinariesError::ChecksumLineMissing)
}

/// Swift `replaceItemAt` / `moveItem`: on POSIX, `rename(2)` replaces an
/// existing destination atomically, so one call covers both branches.
fn promote(staging: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::rename(staging, destination)?;
    let _ = fs::set_permissions(destination, fs::Permissions::from_mode(0o755));
    Ok(())
}

/// Swift `Data.write(to:options: .atomic)`: write a sibling temporary file,
/// then rename it over `path`.
fn write_atomically(path: &Path, data: &[u8]) -> std::io::Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = path.with_file_name(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = fs::File::create_new(&temp)?;
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
