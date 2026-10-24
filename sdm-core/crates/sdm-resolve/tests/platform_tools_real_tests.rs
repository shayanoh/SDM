//! Downloads the real pinned artifacts. Network and about 350 MB: run by hand
//! with `cargo test -p sdm-resolve --test platform_tools_real_tests -- --ignored`.
#![cfg(all(
    any(target_os = "windows", target_os = "linux"),
    target_arch = "x86_64"
))]

use std::process::Command;
use std::sync::Arc;

use sdm_resolve::testing::FakeProcessRunner;
use sdm_resolve::{CheckReason, ManagedBinaries, ReqwestBinaryFetcher, YtDlpChannel};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "downloads the real ffmpeg, ffprobe and qjs"]
async fn real_latest_tools_download_install_and_run() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let mb = ManagedBinaries::new(
        bin.clone(),
        Arc::new(ReqwestBinaryFetcher::new()),
        Arc::new(FakeProcessRunner::new()),
        Vec::new,
        || YtDlpChannel::Stable,
    );
    mb.check_now(CheckReason::Manual).await;
    assert_eq!(
        mb.tools_activity(),
        sdm_resolve::ToolsActivity::Idle,
        "{:?}",
        mb.manifest_for_testing().last_error
    );
    let exe = if cfg!(windows) { ".exe" } else { "" };
    for (tool, flag) in [
        ("ffmpeg", "-version"),
        ("ffprobe", "-version"),
        ("qjs", "--help"),
    ] {
        let output = Command::new(bin.join(format!("{tool}{exe}")))
            .arg(flag)
            .output()
            .unwrap_or_else(|e| panic!("{tool} did not run: {e}"));
        assert!(
            output.status.success(),
            "{tool} exited with {:?}",
            output.status
        );
    }
}
