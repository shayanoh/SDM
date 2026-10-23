mod support;

use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use sdm_resolve::testing::{FakeBinaryFetcher, FakeProcessRunner};
use sdm_resolve::{
    ArchiveKind, BinariesChangedFuture, BinaryFetchError, CheckReason, ManagedBinaries,
    PlatformTool, ToolMember, ToolsActivity, YtDlpChannel,
};
use support::sha256_hex;
use url::Url;

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut out);
        for (name, data) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
    }
    out.into_inner()
}

fn ffmpeg_zip() -> Vec<u8> {
    zip_of(&[("d/ffmpeg", b"FFMPEG"), ("d/ffprobe", b"FFPROBE")])
}

fn ffmpeg_tool(version: &str, archive: &[u8]) -> PlatformTool {
    PlatformTool {
        name: "ffmpeg".into(),
        version: version.into(),
        url: "https://tools.example/ffmpeg.zip".into(),
        sha256: sha256_hex(archive),
        archive: ArchiveKind::Zip,
        members: vec![
            ToolMember::new("d/ffmpeg", "ffmpeg"),
            ToolMember::new("d/ffprobe", "ffprobe"),
        ],
    }
}

fn qjs_tool(version: &str, bytes: &[u8]) -> PlatformTool {
    PlatformTool {
        name: "qjs".into(),
        version: version.into(),
        url: "https://tools.example/qjs".into(),
        sha256: sha256_hex(bytes),
        archive: ArchiveKind::Raw,
        members: vec![ToolMember::new("", "qjs")],
    }
}

fn serve(fetcher: &FakeBinaryFetcher, tool: &PlatformTool, body: &[u8]) {
    fetcher.set_response(Ok(body.to_vec()), &Url::parse(&tool.url).unwrap());
}

fn managed(
    bin: &Path,
    fetcher: &Arc<FakeBinaryFetcher>,
    tools: Vec<PlatformTool>,
) -> ManagedBinaries {
    ManagedBinaries::new(
        bin.to_path_buf(),
        fetcher.clone(),
        Arc::new(FakeProcessRunner::new()),
        Vec::new,
        || YtDlpChannel::Stable,
    )
    .with_platform_tools(tools)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn missing_tools_are_fetched_and_recorded() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let archive = ffmpeg_zip();
    let (ffmpeg, qjs) = (ffmpeg_tool("f1", &archive), qjs_tool("q1", b"QJS"));
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    serve(&fetcher, &ffmpeg, &archive);
    serve(&fetcher, &qjs, b"QJS");
    // yt-dlp's release lookup answers 404: the tools must still be installed.
    let mb = managed(&bin, &fetcher, vec![ffmpeg, qjs]);
    mb.check_now(CheckReason::Launch).await;

    assert_eq!(std::fs::read(bin.join("ffmpeg")).unwrap(), b"FFMPEG");
    assert_eq!(std::fs::read(bin.join("ffprobe")).unwrap(), b"FFPROBE");
    assert_eq!(std::fs::read(bin.join("qjs")).unwrap(), b"QJS");
    let manifest = mb.manifest_for_testing();
    assert_eq!(manifest.ffmpeg_version.as_deref(), Some("f1"));
    assert_eq!(manifest.qjs_version.as_deref(), Some("q1"));
    assert_eq!(mb.tools_activity(), ToolsActivity::Idle);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ffmpeg_and_ffprobe_share_one_download() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let archive = ffmpeg_zip();
    let ffmpeg = ffmpeg_tool("f1", &archive);
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    serve(&fetcher, &ffmpeg, &archive);
    let mb = managed(&bin, &fetcher, vec![ffmpeg]);
    mb.check_now(CheckReason::Launch).await;
    let tool_requests = fetcher
        .requested_urls()
        .iter()
        .filter(|u| u.contains("tools.example"))
        .count();
    assert_eq!(tool_requests, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn present_tools_with_the_current_version_are_not_refetched() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let qjs = qjs_tool("q1", b"QJS");
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    serve(&fetcher, &qjs, b"QJS");
    let mb = managed(&bin, &fetcher, vec![qjs]);
    mb.check_now(CheckReason::Launch).await;
    let after_first = fetcher.requested_urls().len();
    mb.check_now(CheckReason::Timer).await;
    let tool_requests = fetcher
        .requested_urls()
        .iter()
        .filter(|u| u.contains("tools.example"))
        .count();
    assert_eq!(tool_requests, 1);
    assert!(
        fetcher.requested_urls().len() > after_first,
        "yt-dlp lookup still runs"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_version_bump_refetches() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    let v1 = qjs_tool("q1", b"QJS-1");
    serve(&fetcher, &v1, b"QJS-1");
    managed(&bin, &fetcher, vec![v1])
        .check_now(CheckReason::Launch)
        .await;
    let v2 = qjs_tool("q2", b"QJS-2");
    serve(&fetcher, &v2, b"QJS-2");
    let mb = managed(&bin, &fetcher, vec![v2]);
    mb.check_now(CheckReason::Launch).await;
    assert_eq!(std::fs::read(bin.join("qjs")).unwrap(), b"QJS-2");
    assert_eq!(mb.manifest_for_testing().qjs_version.as_deref(), Some("q2"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_checksum_mismatch_records_the_error_and_leaves_bin_clean() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let qjs = qjs_tool("q1", b"QJS");
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    serve(&fetcher, &qjs, b"TAMPERED");
    let mb = managed(&bin, &fetcher, vec![qjs]);
    mb.check_now(CheckReason::Launch).await;
    assert!(!bin.join("qjs").exists());
    let manifest = mb.manifest_for_testing();
    assert!(manifest.qjs_version.is_none());
    assert_eq!(
        manifest.last_error.as_deref(),
        Some("fetch qjs: checksumMismatch")
    );
    assert!(matches!(mb.tools_activity(), ToolsActivity::Failed(_)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_fetch_is_retried_by_the_next_check() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let qjs = qjs_tool("q1", b"QJS");
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    fetcher.set_response(
        Err(BinaryFetchError::Transport("offline".into())),
        &Url::parse(&qjs.url).unwrap(),
    );
    let mb = managed(&bin, &fetcher, vec![qjs.clone()]);
    mb.check_now(CheckReason::Launch).await;
    assert!(matches!(mb.tools_activity(), ToolsActivity::Failed(_)));
    assert!(!bin.join("qjs").exists());

    serve(&fetcher, &qjs, b"QJS");
    mb.check_now(CheckReason::Timer).await;
    assert_eq!(mb.tools_activity(), ToolsActivity::Idle);
    assert_eq!(std::fs::read(bin.join("qjs")).unwrap(), b"QJS");
    assert!(mb.manifest_for_testing().last_error.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_changed_hook_fires_once_after_tools_land() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let archive = ffmpeg_zip();
    let (ffmpeg, qjs) = (ffmpeg_tool("f1", &archive), qjs_tool("q1", b"QJS"));
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    serve(&fetcher, &ffmpeg, &archive);
    serve(&fetcher, &qjs, b"QJS");
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let mb = managed(&bin, &fetcher, vec![ffmpeg, qjs]).with_on_binaries_changed(move || {
        let counter = counter.clone();
        Box::pin(async move {
            counter.fetch_add(1, Ordering::SeqCst);
        }) as BinariesChangedFuture
    });
    mb.check_now(CheckReason::Launch).await;
    assert_eq!(count.load(Ordering::SeqCst), 1);
    mb.check_now(CheckReason::Timer).await;
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_empty_table_does_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    let mb = managed(&bin, &fetcher, Vec::new());
    mb.check_now(CheckReason::Launch).await;
    assert_eq!(mb.tools_activity(), ToolsActivity::Idle);
    assert!(
        fetcher
            .requested_urls()
            .iter()
            .all(|u| !u.contains("tools.example"))
    );
}
