mod support;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use sdm_resolve::testing::{FakeBinaryFetcher, FakeProcessRunner};
use sdm_resolve::{
    BinariesChangedFuture, BinaryFetchError, CheckOutcome, CheckReason, ManagedBinaries,
    YtDlpChannel,
};
use support::sha256_hex;

/// A fetcher primed for a successful stable-channel yt-dlp download of `tag`.
fn primed_fetcher(tag: &str, binary: &[u8]) -> Arc<FakeBinaryFetcher> {
    let f = Arc::new(FakeBinaryFetcher::new());
    let ch = YtDlpChannel::Stable;
    f.set_response(
        Ok(format!("{{\"tag_name\":\"{tag}\"}}").into_bytes()),
        &ch.releases_api(),
    );
    f.set_response(Ok(binary.to_vec()), &ch.asset_url(tag));
    f.set_response(
        Ok(format!("{}  yt-dlp_macos\n", sha256_hex(binary)).into_bytes()),
        &ch.sums_url(tag),
    );
    f
}

fn make_managed(bin: &Path, fetcher: &Arc<FakeBinaryFetcher>) -> ManagedBinaries {
    ManagedBinaries::new(
        bin.to_path_buf(),
        fetcher.clone(),
        Arc::new(FakeProcessRunner::new()),
        Vec::new,
        || YtDlpChannel::Stable,
    )
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fresh_install_downloads_yt_dlp() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let payload = b"YTDLP-BINARY-2026.08.19".to_vec();
    let fetcher = primed_fetcher("2026.08.19", &payload);

    let changed = Arc::new(AtomicUsize::new(0));
    let counter = changed.clone();
    let mb = make_managed(&bin, &fetcher).with_on_binaries_changed(move || {
        let counter = counter.clone();
        Box::pin(async move {
            counter.fetch_add(1, Ordering::SeqCst);
        }) as BinariesChangedFuture
    });
    let outcome = mb.check_now(CheckReason::Launch).await;

    assert_eq!(outcome, CheckOutcome::Updated("2026.08.19".into()));
    let installed = bin.join("yt-dlp");
    assert!(is_executable(&installed));
    assert_eq!(std::fs::read(&installed).unwrap(), payload);
    assert_eq!(
        mb.manifest_for_testing().yt_dlp_version.as_deref(),
        Some("2026.08.19")
    );
    assert_eq!(changed.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn up_to_date_skips_download() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let fetcher = primed_fetcher("2026.08.19", b"x");
    let mb = make_managed(&bin, &fetcher);
    mb.check_now(CheckReason::Launch).await; // installs
    let count_after_install = fetcher.request_count();

    let outcome = mb.check_now(CheckReason::Timer).await;
    assert_eq!(outcome, CheckOutcome::UpToDate(Some("2026.08.19".into())));
    assert_eq!(fetcher.request_count(), count_after_install + 1); // only the release-info call
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn newer_remote_replaces_binary() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");

    let mb1 = make_managed(&bin, &primed_fetcher("2026.08.19", b"old"));
    mb1.check_now(CheckReason::Launch).await;

    let mb2 = make_managed(&bin, &primed_fetcher("2026.09.01", b"new"));
    let outcome = mb2.check_now(CheckReason::Timer).await;
    assert_eq!(outcome, CheckOutcome::Updated("2026.09.01".into()));
    assert_eq!(std::fs::read(bin.join("yt-dlp")).unwrap(), b"new".to_vec());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nightly_tag_beats_stable_date() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let mb1 = make_managed(&bin, &primed_fetcher("2026.08.19", b"a"));
    mb1.check_now(CheckReason::Launch).await;

    let f2 = primed_fetcher("2026.08.19.120000", b"b");
    let mb2 = make_managed(&bin, &f2);
    assert_eq!(
        mb2.check_now(CheckReason::Timer).await,
        CheckOutcome::Updated("2026.08.19.120000".into())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn checksum_mismatch_aborts() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let sentinel = bin.join("yt-dlp");
    std::fs::write(&sentinel, b"SENTINEL").unwrap();
    std::fs::set_permissions(&sentinel, std::fs::Permissions::from_mode(0o755)).unwrap();

    let f = Arc::new(FakeBinaryFetcher::new());
    let ch = YtDlpChannel::Stable;
    f.set_response(
        Ok(b"{\"tag_name\":\"2026.09.01\"}".to_vec()),
        &ch.releases_api(),
    );
    f.set_response(Ok(b"corrupt".to_vec()), &ch.asset_url("2026.09.01"));
    f.set_response(
        Ok(b"deadbeef  yt-dlp_macos\n".to_vec()),
        &ch.sums_url("2026.09.01"),
    );

    let mb = make_managed(&bin, &f);
    let outcome = mb.check_now(CheckReason::Timer).await;
    assert!(
        matches!(outcome, CheckOutcome::Failed(_)),
        "expected .failed, got {outcome:?}"
    );
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"SENTINEL".to_vec());
    assert!(mb.manifest_for_testing().last_error.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn offline_reports_no_network() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let f = Arc::new(FakeBinaryFetcher::new());
    f.set_response(
        Err(BinaryFetchError::Transport("offline".into())),
        &YtDlpChannel::Stable.releases_api(),
    );
    let mb = make_managed(&bin, &f);
    assert_eq!(
        mb.check_now(CheckReason::Launch).await,
        CheckOutcome::NoNetwork
    );
    assert!(mb.manifest_for_testing().last_error.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_checks_coalesce() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let fetcher = primed_fetcher("2026.08.19", b"x");
    let mb = make_managed(&bin, &fetcher);

    tokio::join!(
        mb.check_now(CheckReason::Launch),
        mb.check_now(CheckReason::Timer),
        mb.check_now(CheckReason::Manual),
        mb.check_now(CheckReason::ResolveNeeded),
        mb.check_now(CheckReason::Timer),
    );

    // release-info + asset + sums == 3 requests for a single download.
    assert_eq!(fetcher.request_count(), 3);
}
