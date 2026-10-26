mod support;

use std::sync::Arc;

use sdm_resolve::testing::{FakeBinaryFetcher, FakeProcessRunner};
use sdm_resolve::{BinaryFetchError, CheckReason, ManagedBinaries, YtDlpChannel};
use support::sha256_hex;

async fn tick(mb: &ManagedBinaries, n: i64) {
    for _ in 0..n {
        mb.tick().await;
    }
}

fn managed(bin: std::path::PathBuf, fetcher: &Arc<FakeBinaryFetcher>) -> ManagedBinaries {
    ManagedBinaries::new(
        bin,
        fetcher.clone(),
        Arc::new(FakeProcessRunner::new()),
        || YtDlpChannel::Stable,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_check_before_present_interval() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let payload = b"ytdlp".to_vec();
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    let ch = YtDlpChannel::Stable;
    fetcher.set_response(
        Ok(b"{\"tag_name\":\"2026.08.19\"}".to_vec()),
        &ch.releases_api(),
    );
    fetcher.set_response(Ok(payload.clone()), &ch.asset_url("2026.08.19"));
    fetcher.set_response(
        Ok(format!("{}  yt-dlp_macos\n", sha256_hex(&payload)).into_bytes()),
        &ch.sums_url("2026.08.19"),
    );

    let mb = managed(bin, &fetcher);
    mb.check_now(CheckReason::Launch).await; // installs; resets the interval clock
    let baseline = fetcher.request_count();

    tick(&mb, ManagedBinaries::PRESENT_INTERVAL - 1).await;
    assert_eq!(fetcher.request_count(), baseline);

    tick(&mb, 1).await;
    assert_eq!(fetcher.request_count(), baseline + 1); // one release-info call, then .upToDate
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn absent_retries_every_short_interval() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    fetcher.set_response(
        Err(BinaryFetchError::Transport("offline".into())),
        &YtDlpChannel::Stable.releases_api(),
    );
    let mb = managed(bin, &fetcher);

    tick(&mb, ManagedBinaries::ABSENT_INTERVAL - 1).await;
    assert_eq!(fetcher.request_count(), 0);

    tick(&mb, 1).await;
    assert_eq!(fetcher.request_count(), 1);

    tick(&mb, ManagedBinaries::ABSENT_INTERVAL).await;
    assert_eq!(fetcher.request_count(), 2);
}
