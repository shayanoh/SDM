//! The ninth test of Swift's `CheckpointTickTests.swift`, the one that drives
//! a `DownloadEngine` (the other eight are in `checkpoint_tick_tests.rs`).
mod support;

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use async_trait::async_trait;
use sdm_core::DownloadPackage;
use sdm_engine::{
    FakeOrigin, FakeOriginBehavior, HttpTransport, RangeRequest, RangeResponse, TransportError,
};
use support::{
    engine_settings, example_item, make_scratch_directory, new_engine, test_payload, within,
};

/// Makes the download's folder read-only at the moment the *body* request is
/// issued — i.e. after `prepare()` has already removed any stale sidecar and
/// opened the `.incomplete` file, but before the first checkpoint.
///
/// The open descriptor keeps working (`pwrite` doesn't consult directory
/// permissions), so bytes still land correctly at their absolute offsets; only
/// creating the new `.sdmpart` file fails. That is the real-world shape of the
/// bug — the download appears healthy while its resume state is silently
/// being lost.
struct FolderLockingOrigin {
    inner: FakeOrigin,
    folder: PathBuf,
}

#[async_trait]
impl HttpTransport for FolderLockingOrigin {
    async fn fetch(&self, request: RangeRequest) -> Result<RangeResponse, TransportError> {
        let range = request.range;
        let response = self.inner.fetch(request).await?;
        if range.is_some_and(|range| range.length() > 1) {
            let _ = std::fs::set_permissions(&self.folder, std::fs::Permissions::from_mode(0o500));
        }
        Ok(response)
    }
}

/// Restores write permission on drop so the scratch directory can be removed.
struct Unlock(PathBuf);

impl Drop for Unlock {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

/// The engine carries the reason through to the snapshot the UI consumes, and
/// keeps it after the runner has retired — "this download's resume state could
/// not be written" is precisely the thing that must still be visible once the
/// download has stopped.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn checkpoint_failure_reaches_the_engine_snapshot() {
    let dir = make_scratch_directory();
    let folder = dir.path().join("Batch");
    let _unlock = Unlock(folder.clone());

    let behavior = FakeOriginBehavior {
        drop_after_bytes: Some(300),
        ..Default::default()
    };
    let engine = new_engine(
        FolderLockingOrigin {
            inner: FakeOrigin::with_behavior(test_payload(4000), behavior),
            folder,
        },
        engine_settings(1, 1, 8, dir.path()).with_checkpoint_interval_bytes(128),
    );
    engine
        .add(DownloadPackage::new("Batch").with_items(vec![example_item("a.bin")]))
        .await;
    within(engine.run_until_idle()).await.unwrap();

    let snapshot = engine.snapshot().await;
    let item = &snapshot.packages[0].items[0];
    let reason = item
        .checkpoint_failure
        .as_ref()
        .expect("a checkpoint failure");
    assert!(reason.contains("a.bin"), "{reason}");
}
