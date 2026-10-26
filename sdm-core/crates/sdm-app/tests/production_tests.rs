use std::sync::Arc;

use sdm_app::App;
use sdm_app::host::FakeHost;

#[tokio::test(flavor = "multi_thread")]
async fn the_production_app_builds_with_a_managed_bin_directory_and_no_network() {
    let data = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let host = Arc::new(FakeHost::new(
        data.path().to_path_buf(),
        downloads.path().to_path_buf(),
    ));
    let app = App::production(host).expect("builds");
    // No yt-dlp has been provisioned yet: the status is empty, not an error.
    let status = app.binaries_status();
    assert_eq!(status.yt_dlp_version, None);
    assert!(data.path().join("bin").parent().unwrap().exists());
}
