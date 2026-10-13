//! Port of `FakeWholesaleDownloader.swift`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use sdm_core::{SourceUrl, WholesaleDownloader, WholesaleError, WholesaleProgress};
use tokio::sync::{Notify, oneshot};
use tokio_util::sync::CancellationToken;

type ProgressCallback = Arc<dyn Fn(WholesaleProgress) + Send + Sync>;

/// In-process `WholesaleDownloader` a test drives step by step — mirrors
/// `FakeOrigin`. `download` blocks until `finish_success()` /
/// `finish_failure()`; `emit` pushes a progress report; cancellation resolves
/// cleanly (as `WholesaleError::Cancelled`, the trait's contract).
#[derive(Default)]
pub struct FakeWholesaleDownloader {
    state: Mutex<FakeState>,
    started: Notify,
}

#[derive(Default)]
struct FakeState {
    on_progress: Option<ProgressCallback>,
    finish: Option<oneshot::Sender<Result<(), WholesaleError>>>,
    destination: Option<PathBuf>,
    call_count: usize,
}

impl FakeWholesaleDownloader {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many times `download` has been called.
    pub fn call_count(&self) -> usize {
        self.state.lock().unwrap().call_count
    }

    /// Suspends until `download` has been called and registered its callback.
    pub async fn wait_until_started(&self) {
        loop {
            let notified = self.started.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.state.lock().unwrap().on_progress.is_some() {
                return;
            }
            notified.await;
        }
    }

    pub fn emit(&self, progress: WholesaleProgress) {
        let callback = self.state.lock().unwrap().on_progress.clone();
        if let Some(callback) = callback {
            callback(progress);
        }
    }

    /// Writes the file `destination` names and completes the call.
    pub fn finish_success(&self) {
        let destination = self.state.lock().unwrap().destination.clone();
        if let Some(destination) = destination {
            std::fs::write(destination, b"wholesale").expect("write the fake output");
        }
        self.resume(Ok(()));
    }

    /// Completes the call successfully without writing the output file.
    pub fn finish_without_output(&self) {
        self.resume(Ok(()));
    }

    pub fn finish_failure(&self, error: WholesaleError) {
        self.resume(Err(error));
    }

    fn resume(&self, result: Result<(), WholesaleError>) {
        let finish = self.state.lock().unwrap().finish.take();
        if let Some(finish) = finish {
            let _ = finish.send(result);
        }
    }
}

/// Clears the callback when the call ends, however it ends.
struct ClearOnDrop<'a>(&'a FakeWholesaleDownloader);

impl Drop for ClearOnDrop<'_> {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap();
        state.on_progress = None;
        state.finish = None;
    }
}

#[async_trait]
impl WholesaleDownloader for FakeWholesaleDownloader {
    async fn download(
        &self,
        _page_url: &SourceUrl,
        _format_selector: &str,
        destination: &Path,
        on_progress: Arc<dyn Fn(WholesaleProgress) + Send + Sync>,
        cancel: &CancellationToken,
    ) -> Result<(), WholesaleError> {
        let (sender, receiver) = oneshot::channel();
        {
            let mut state = self.state.lock().unwrap();
            state.on_progress = Some(on_progress);
            state.destination = Some(destination.to_path_buf());
            state.finish = Some(sender);
            state.call_count += 1;
        }
        self.started.notify_waiters();
        let _clear = ClearOnDrop(self);
        tokio::select! {
            result = receiver => result.unwrap_or(Err(WholesaleError::Cancelled)),
            () = cancel.cancelled() => Err(WholesaleError::Cancelled),
        }
    }
}
