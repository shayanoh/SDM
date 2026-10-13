mod support;

use std::sync::Arc;

use sdm_ffi::{CoreError, ManagedBinaries, ManagedBinariesConfig};
use sdm_resolve::testing::{FakeBinaryFetcher, FakeProcessRunner};
use sdm_resolve::{BinaryFetchError, CheckOutcome, CheckReason, VendorAsset, YtDlpChannel};
use support::{scratch_directory, within};

fn config(dir: &std::path::Path, channel: YtDlpChannel) -> ManagedBinariesConfig {
    ManagedBinariesConfig {
        bin_directory: dir.join("bin"),
        vendor_assets: vec![],
        channel,
    }
}

fn offline_fetcher() -> Arc<FakeBinaryFetcher> {
    // Nothing registered: every request fails like a dropped connection.
    Arc::new(FakeBinaryFetcher::new())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn new_builds_production_managed_binaries() {
    within(async {
        let dir = scratch_directory();
        let managed = ManagedBinaries::new(config(dir.path(), YtDlpChannel::Stable))
            .expect("build managed binaries");
        let status = managed.status().unwrap();
        assert_eq!(status.channel, YtDlpChannel::Stable);
        assert_eq!(status.yt_dlp_version, None);
        // No bundled assets: provisioning is a no-op and touches no network.
        managed.provision_bundled_if_needed().await.unwrap();
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_exported_method_runs_once_over_fakes() {
    within(async {
        let dir = scratch_directory();
        let managed = ManagedBinaries::with_dependencies(
            config(dir.path(), YtDlpChannel::Stable),
            offline_fetcher(),
            Arc::new(FakeProcessRunner::new()),
        )
        .expect("build managed binaries");

        managed.provision_bundled_if_needed().await.unwrap();
        managed.tick().await.unwrap();

        let outcome = managed.check_now(CheckReason::Manual).await.unwrap();
        assert!(
            matches!(outcome, CheckOutcome::NoNetwork | CheckOutcome::Failed(_)),
            "an offline fetcher cannot update anything: {outcome:?}"
        );

        managed.set_channel(YtDlpChannel::Nightly).unwrap();
        assert_eq!(managed.status().unwrap().channel, YtDlpChannel::Nightly);

        // A check that fails leaves nothing to announce.
        assert!(managed.take_notifications().unwrap().is_empty());
        // And draining twice is fine.
        assert!(managed.take_notifications().unwrap().is_empty());
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_status_reflects_the_manifest_after_a_failed_check() {
    within(async {
        let dir = scratch_directory();
        let fetcher = offline_fetcher();
        let managed = ManagedBinaries::with_dependencies(
            config(dir.path(), YtDlpChannel::Stable),
            fetcher.clone(),
            Arc::new(FakeProcessRunner::new()),
        )
        .unwrap();
        managed.check_now(CheckReason::Launch).await.unwrap();
        assert!(fetcher.request_count() >= 1, "the check asked the fetcher");
        let status = managed.status().unwrap();
        assert!(status.yt_dlp_version.is_none());
        let _ = BinaryFetchError::Http(500); // the error type is part of the fake's API
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_empty_bin_directory_is_an_error() {
    let refused = ManagedBinaries::with_dependencies(
        ManagedBinariesConfig {
            bin_directory: std::path::PathBuf::new(),
            vendor_assets: vec![VendorAsset {
                name: "qjs".to_owned(),
                compressed_path: "/nonexistent".into(),
                version: "1".to_owned(),
            }],
            channel: YtDlpChannel::Stable,
        },
        offline_fetcher(),
        Arc::new(FakeProcessRunner::new()),
    );
    assert!(matches!(refused, Err(CoreError::InvalidArgument { .. })));
}
