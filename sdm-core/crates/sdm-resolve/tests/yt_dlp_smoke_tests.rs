use std::path::Path;
use std::time::Duration;

use sdm_resolve::{BinaryLocator, ProcessRunner, SystemProcessRunner};
use tokio_util::sync::CancellationToken;

fn yt_dlp_on_disk() -> bool {
    use std::os::unix::fs::PermissionsExt;
    ["/usr/local/bin/yt-dlp", "/opt/homebrew/bin/yt-dlp"]
        .iter()
        .any(|p| {
            Path::new(p)
                .metadata()
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        })
}

/// Exercises the real `BinaryLocator` + `SystemProcessRunner` wiring against
/// the installed yt-dlp — `--version` only, no network. Skipped when yt-dlp
/// is not installed (parent spec §10.2).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn yt_dlp_version_runs_through_system_runner() {
    if !yt_dlp_on_disk() {
        eprintln!("skipped: yt-dlp is not installed in /usr/local/bin or /opt/homebrew/bin");
        return;
    }
    let locator = BinaryLocator::new();
    let ytdlp = locator.locate("yt-dlp").expect("yt-dlp located");
    let out = SystemProcessRunner::new()
        .run(
            &ytdlp,
            &["--version".to_string()],
            Duration::from_secs(10),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(out.exit_code, 0);
    assert!(!out.stdout.is_empty());
}
