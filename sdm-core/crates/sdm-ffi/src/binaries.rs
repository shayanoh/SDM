//! `ManagedBinaries`: the FFI wrapper over `sdm_resolve::ManagedBinaries`,
//! which owns the app's `bin/` directory (inflates the bundled ffmpeg / qjs
//! blobs, downloads and self-updates yt-dlp).
//!
//! It replaces Swift's `ManagedBinaries` and shared `BinaryLocator` pair. The
//! locator part is handled in `shared`: when the binaries change, every
//! locator the engine and grabber built is invalidated.
//!
//! Swift's `notify` closure becomes a queue the app drains: the controller's
//! one-second loop already calls `tick` and `status`, and calls
//! [`ManagedBinaries::take_notifications`] with them.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use sdm_resolve::{
    BinaryFetching, CheckOutcome, CheckReason, ManagedBinariesStatus, ProcessRunner,
    ReqwestBinaryFetcher, SystemProcessRunner, VendorAsset, YtDlpChannel,
};

use crate::error::CoreError;
use crate::runtime::{guard, run};
use crate::shared::{invalidate_all_locators, lock};

/// How to build a production [`ManagedBinaries`].
#[derive(Debug, Clone, uniffi::Record)]
pub struct ManagedBinariesConfig {
    /// `~/Library/Application Support/SDM/bin`.
    pub bin_directory: PathBuf,
    /// The compressed binaries shipped in the app bundle.
    pub vendor_assets: Vec<VendorAsset>,
    /// The yt-dlp release channel to follow; change it with `set_channel`.
    pub channel: YtDlpChannel,
}

/// Owns the managed `bin/` directory.
#[derive(uniffi::Object)]
pub struct ManagedBinaries {
    inner: sdm_resolve::ManagedBinaries,
    channel: Arc<Mutex<YtDlpChannel>>,
    notifications: Arc<Mutex<Vec<String>>>,
}

impl ManagedBinaries {
    /// Managed binaries over an injected fetcher and process runner, for
    /// tests. `new` builds the production ones and calls this.
    pub fn with_dependencies(
        config: ManagedBinariesConfig,
        fetcher: Arc<dyn BinaryFetching>,
        runner: Arc<dyn ProcessRunner>,
    ) -> Result<Arc<Self>, CoreError> {
        if config.bin_directory.as_os_str().is_empty() {
            return Err(CoreError::invalid(
                "binDirectory",
                "the directory path is empty",
            ));
        }
        let channel = Arc::new(Mutex::new(config.channel));
        let notifications = Arc::new(Mutex::new(Vec::new()));
        let assets = config.vendor_assets;
        let channel_source = Arc::clone(&channel);
        let queue = Arc::clone(&notifications);
        let inner = guard(|| {
            sdm_resolve::ManagedBinaries::new(
                config.bin_directory,
                fetcher,
                runner,
                move || assets.clone(),
                move || *lock(&channel_source),
            )
            .with_on_binaries_changed(|| {
                Box::pin(async {
                    invalidate_all_locators();
                })
            })
            .with_notify(move |message| lock(&queue).push(message))
        })?;
        Ok(Arc::new(Self {
            inner,
            channel,
            notifications,
        }))
    }
}

#[uniffi::export]
impl ManagedBinaries {
    #[uniffi::constructor]
    pub fn new(config: ManagedBinariesConfig) -> Result<Arc<Self>, CoreError> {
        Self::with_dependencies(
            config,
            Arc::new(ReqwestBinaryFetcher::new()),
            Arc::new(SystemProcessRunner::new()),
        )
    }

    /// Follows a different release channel from the next check on. Call
    /// `check_now` with `CheckReason::ChannelChanged` to act on it.
    pub fn set_channel(&self, channel: YtDlpChannel) -> Result<(), CoreError> {
        guard(|| *lock(&self.channel) = channel)
    }

    /// Inflates any bundled blob whose version differs from the installed
    /// one. Cheap; safe on every launch.
    pub async fn provision_bundled_if_needed(&self) -> Result<(), CoreError> {
        let inner = self.inner.clone();
        run(async move { inner.provision_bundled_if_needed().await }).await
    }

    /// Advances the update clock one tick; call about once a second.
    pub async fn tick(&self) -> Result<(), CoreError> {
        let inner = self.inner.clone();
        run(async move { inner.tick().await }).await
    }

    /// Checks for a newer yt-dlp and downloads it if needed. Coalesces with
    /// any check already running.
    pub async fn check_now(&self, reason: CheckReason) -> Result<CheckOutcome, CoreError> {
        let inner = self.inner.clone();
        run(async move { inner.check_now(reason).await }).await
    }

    pub fn status(&self) -> Result<ManagedBinariesStatus, CoreError> {
        guard(|| self.inner.status())
    }

    /// The messages ("yt-dlp updated to ...") produced since the last call,
    /// oldest first.
    pub fn take_notifications(&self) -> Result<Vec<String>, CoreError> {
        guard(|| std::mem::take(&mut *lock(&self.notifications)))
    }
}
