mod support;

use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use sdm_resolve::testing::{FakeBinaryFetcher, FakeProcessRunner};
use sdm_resolve::{
    ArchiveKind, BinariesChangedFuture, BinaryFetchError, CheckReason, ManagedBinaries,
    PlatformTool, ToolMember, ToolSource, ToolsActivity, UpdatePolicy, VersionSource, YtDlpChannel,
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
    zip_of(&[
        ("build-2026-10-23/bin/ffmpeg", b"FFMPEG"),
        ("build-2026-10-23/bin/ffprobe", b"FFPROBE"),
    ])
}

const FFMPEG_API: &str = "https://api.example/ffmpeg/releases/tags/latest";
const QJS_API: &str = "https://api.example/qjs/releases/latest";

fn ffmpeg_tool() -> PlatformTool {
    PlatformTool {
        name: "ffmpeg".into(),
        source: ToolSource::GitHub {
            release_api: FFMPEG_API.into(),
            asset_name: "ffmpeg-latest.zip".into(),
        },
        archive: ArchiveKind::Zip,
        members: vec![
            ToolMember::new("bin/ffmpeg", "ffmpeg"),
            ToolMember::new("bin/ffprobe", "ffprobe"),
        ],
        policy: UpdatePolicy::IfMissing,
        version_source: VersionSource::PublishedDate,
    }
}

fn qjs_tool() -> PlatformTool {
    PlatformTool {
        name: "qjs".into(),
        source: ToolSource::GitHub {
            release_api: QJS_API.into(),
            asset_name: "qjs-linux".into(),
        },
        archive: ArchiveKind::Raw,
        members: vec![ToolMember::new("", "qjs")],
        policy: UpdatePolicy::WhenVersionChanges,
        version_source: VersionSource::Tag,
    }
}

/// Publishes `body` as the release's asset and the release JSON that names it.
/// `digest` is the checksum the release claims (`None`: it publishes none).
fn publish(
    fetcher: &FakeBinaryFetcher,
    api: &str,
    tag: &str,
    published_at: &str,
    asset: &str,
    body: &[u8],
    digest: Option<String>,
) {
    let asset_url = format!("https://dl.example/{tag}/{asset}");
    let digest_json = match digest {
        Some(d) => format!("\"sha256:{d}\""),
        None => "null".to_owned(),
    };
    let json = format!(
        "{{\"tag_name\":\"{tag}\",\"published_at\":\"{published_at}\",\"assets\":[{{\"name\":\"{asset}\",\"browser_download_url\":\"{asset_url}\",\"digest\":{digest_json}}}]}}"
    );
    fetcher.set_response(Ok(json.into_bytes()), &Url::parse(api).unwrap());
    fetcher.set_response(Ok(body.to_vec()), &Url::parse(&asset_url).unwrap());
}

fn publish_ok(
    fetcher: &FakeBinaryFetcher,
    api: &str,
    tag: &str,
    published_at: &str,
    asset: &str,
    body: &[u8],
) {
    publish(
        fetcher,
        api,
        tag,
        published_at,
        asset,
        body,
        Some(sha256_hex(body)),
    );
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
        || YtDlpChannel::Stable,
    )
    .with_platform_tools(tools)
}

fn requests(fetcher: &FakeBinaryFetcher, needle: &str) -> usize {
    fetcher
        .requested_urls()
        .iter()
        .filter(|u| u.contains(needle))
        .count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn missing_tools_are_fetched_from_the_newest_release_and_recorded() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let archive = ffmpeg_zip();
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    publish_ok(
        &fetcher,
        FFMPEG_API,
        "latest",
        "2026-10-23T13:25:09Z",
        "ffmpeg-latest.zip",
        &archive,
    );
    publish_ok(
        &fetcher,
        QJS_API,
        "v0.17.0",
        "2026-09-18T21:39:55Z",
        "qjs-linux",
        b"QJS",
    );
    // yt-dlp's release lookup answers 404: the tools must still be installed.
    let mb = managed(&bin, &fetcher, vec![ffmpeg_tool(), qjs_tool()]);
    mb.check_now(CheckReason::Launch).await;

    assert_eq!(std::fs::read(bin.join("ffmpeg")).unwrap(), b"FFMPEG");
    assert_eq!(std::fs::read(bin.join("ffprobe")).unwrap(), b"FFPROBE");
    assert_eq!(std::fs::read(bin.join("qjs")).unwrap(), b"QJS");
    let manifest = mb.manifest_for_testing();
    assert_eq!(manifest.ffmpeg_version.as_deref(), Some("2026-10-23"));
    assert_eq!(manifest.qjs_version.as_deref(), Some("v0.17.0"));
    assert_eq!(mb.tools_activity(), ToolsActivity::Idle);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ffmpeg_and_ffprobe_share_one_download() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = ffmpeg_zip();
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    publish_ok(
        &fetcher,
        FFMPEG_API,
        "latest",
        "2026-10-23T13:25:09Z",
        "ffmpeg-latest.zip",
        &archive,
    );
    let mb = managed(&tmp.path().join("bin"), &fetcher, vec![ffmpeg_tool()]);
    mb.check_now(CheckReason::Launch).await;
    assert_eq!(requests(&fetcher, "dl.example"), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_installed_ffmpeg_costs_no_network_and_a_deleted_one_is_fetched_again() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let archive = ffmpeg_zip();
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    publish_ok(
        &fetcher,
        FFMPEG_API,
        "latest",
        "2026-10-23T13:25:09Z",
        "ffmpeg-latest.zip",
        &archive,
    );
    let mb = managed(&bin, &fetcher, vec![ffmpeg_tool()]);
    mb.check_now(CheckReason::Launch).await;
    assert_eq!(requests(&fetcher, "api.example"), 1);

    // The release is rebuilt every day; an installed ffmpeg is left alone.
    publish_ok(
        &fetcher,
        FFMPEG_API,
        "latest",
        "2026-10-24T13:25:09Z",
        "ffmpeg-latest.zip",
        &archive,
    );
    mb.check_now(CheckReason::Timer).await;
    assert_eq!(requests(&fetcher, "api.example"), 1);
    assert_eq!(requests(&fetcher, "dl.example"), 1);

    std::fs::remove_file(bin.join("ffprobe")).unwrap();
    mb.check_now(CheckReason::Timer).await;
    assert_eq!(requests(&fetcher, "dl.example"), 2);
    assert!(bin.join("ffprobe").exists());
    assert_eq!(
        mb.manifest_for_testing().ffmpeg_version.as_deref(),
        Some("2026-10-24")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn qjs_follows_its_release_tag() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    publish_ok(
        &fetcher,
        QJS_API,
        "v1",
        "2026-01-01T00:00:00Z",
        "qjs-linux",
        b"QJS-1",
    );
    let mb = managed(&bin, &fetcher, vec![qjs_tool()]);
    mb.check_now(CheckReason::Launch).await;
    assert_eq!(std::fs::read(bin.join("qjs")).unwrap(), b"QJS-1");

    // Same tag: the release is read, the file is not downloaded again.
    mb.check_now(CheckReason::Timer).await;
    assert_eq!(requests(&fetcher, "dl.example"), 1);

    publish_ok(
        &fetcher,
        QJS_API,
        "v2",
        "2026-02-01T00:00:00Z",
        "qjs-linux",
        b"QJS-2",
    );
    mb.check_now(CheckReason::Timer).await;
    assert_eq!(std::fs::read(bin.join("qjs")).unwrap(), b"QJS-2");
    assert_eq!(mb.manifest_for_testing().qjs_version.as_deref(), Some("v2"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_checksum_mismatch_records_the_error_and_leaves_bin_clean() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    // The release claims the digest of the real file; the bytes served differ.
    publish(
        &fetcher,
        QJS_API,
        "v1",
        "2026-01-01T00:00:00Z",
        "qjs-linux",
        b"TAMPERED",
        Some(sha256_hex(b"QJS")),
    );
    let mb = managed(&bin, &fetcher, vec![qjs_tool()]);
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
async fn a_release_without_a_checksum_or_the_asset_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    publish(
        &fetcher,
        QJS_API,
        "v1",
        "2026-01-01T00:00:00Z",
        "qjs-linux",
        b"QJS",
        None,
    );
    let mb = managed(&bin, &fetcher, vec![qjs_tool()]);
    mb.check_now(CheckReason::Launch).await;
    assert!(!bin.join("qjs").exists());
    assert_eq!(
        mb.manifest_for_testing().last_error.as_deref(),
        Some("fetch qjs: the release has no checksum for qjs-linux")
    );

    publish_ok(
        &fetcher,
        QJS_API,
        "v1",
        "2026-01-01T00:00:00Z",
        "other-asset",
        b"QJS",
    );
    mb.check_now(CheckReason::Timer).await;
    assert_eq!(
        mb.manifest_for_testing().last_error.as_deref(),
        Some("fetch qjs: the release has no qjs-linux")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_fetch_is_retried_by_the_next_check() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    fetcher.set_response(
        Err(BinaryFetchError::Transport("offline".into())),
        &Url::parse(QJS_API).unwrap(),
    );
    let mb = managed(&bin, &fetcher, vec![qjs_tool()]);
    mb.check_now(CheckReason::Launch).await;
    assert!(matches!(mb.tools_activity(), ToolsActivity::Failed(_)));
    assert!(!bin.join("qjs").exists());

    publish_ok(
        &fetcher,
        QJS_API,
        "v1",
        "2026-01-01T00:00:00Z",
        "qjs-linux",
        b"QJS",
    );
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
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    publish_ok(
        &fetcher,
        FFMPEG_API,
        "latest",
        "2026-10-23T13:25:09Z",
        "ffmpeg-latest.zip",
        &archive,
    );
    publish_ok(
        &fetcher,
        QJS_API,
        "v1",
        "2026-01-01T00:00:00Z",
        "qjs-linux",
        b"QJS",
    );
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let mb = managed(&bin, &fetcher, vec![ffmpeg_tool(), qjs_tool()]).with_on_binaries_changed(
        move || {
            let counter = counter.clone();
            Box::pin(async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }) as BinariesChangedFuture
        },
    );
    mb.check_now(CheckReason::Launch).await;
    assert_eq!(count.load(Ordering::SeqCst), 1);
    mb.check_now(CheckReason::Timer).await;
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_empty_table_does_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    let mb = managed(&tmp.path().join("bin"), &fetcher, Vec::new());
    mb.check_now(CheckReason::Launch).await;
    assert_eq!(mb.tools_activity(), ToolsActivity::Idle);
    assert_eq!(requests(&fetcher, "example"), 0);
}

const SITE_LATEST: &str = "https://site.example/redirect/latest/macos/arm64/release";
const SITE_BUILD: &str = "https://site.example/download/macos/arm64/1792_N-127";

/// The macOS shape: ffmpeg and ffprobe are separate one-file zips, found
/// behind a `latest` address that redirects to a build.
fn site_tool(file: &str) -> PlatformTool {
    PlatformTool {
        name: "ffmpeg".into(),
        source: ToolSource::Redirect {
            latest_url: format!("{SITE_LATEST}/{file}.zip"),
        },
        archive: ArchiveKind::Zip,
        members: vec![ToolMember::new(file, file)],
        policy: UpdatePolicy::IfMissing,
        version_source: VersionSource::BuildId,
    }
}

fn publish_site_build(fetcher: &FakeBinaryFetcher, file: &str, body: &[u8], sums: Option<&str>) {
    let zip = zip_of(&[(file, body)]);
    let latest = Url::parse(&format!("{SITE_LATEST}/{file}.zip")).unwrap();
    let build = Url::parse(&format!("{SITE_BUILD}/{file}.zip")).unwrap();
    fetcher.set_redirect(&latest, &build);
    let sums = sums.map_or_else(
        || format!("{}  {file}.zip\n", sha256_hex(&zip)),
        str::to_owned,
    );
    fetcher.set_response(
        Ok(sums.into_bytes()),
        &Url::parse(&format!("{SITE_BUILD}/{file}.zip.sha256")).unwrap(),
    );
    fetcher.set_response(Ok(zip), &build);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_build_behind_a_latest_address_is_checked_against_the_checksum_beside_it() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    publish_site_build(&fetcher, "ffmpeg", b"FFMPEG", None);
    publish_site_build(&fetcher, "ffprobe", b"FFPROBE", None);
    let mb = managed(
        &bin,
        &fetcher,
        vec![site_tool("ffmpeg"), site_tool("ffprobe")],
    );
    mb.check_now(CheckReason::Launch).await;

    assert_eq!(std::fs::read(bin.join("ffmpeg")).unwrap(), b"FFMPEG");
    assert_eq!(std::fs::read(bin.join("ffprobe")).unwrap(), b"FFPROBE");
    assert_eq!(
        mb.manifest_for_testing().ffmpeg_version.as_deref(),
        Some("1792_N-127")
    );
    assert_eq!(mb.tools_activity(), ToolsActivity::Idle);
    // Installed and never replaced: the next check asks nothing.
    let asked = fetcher.request_count();
    mb.check_now(CheckReason::Timer).await;
    assert_eq!(
        fetcher
            .requested_urls()
            .iter()
            .skip(asked)
            .filter(|u| u.contains("site.example"))
            .count(),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_build_whose_checksum_does_not_match_installs_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    publish_site_build(
        &fetcher,
        "ffmpeg",
        b"FFMPEG",
        Some(&format!("{}  x\n", "0".repeat(64))),
    );
    let mb = managed(&bin, &fetcher, vec![site_tool("ffmpeg")]);
    mb.check_now(CheckReason::Launch).await;

    assert!(!bin.join("ffmpeg").exists());
    assert!(matches!(mb.tools_activity(), ToolsActivity::Failed(_)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_checksum_file_without_a_checksum_installs_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let fetcher = Arc::new(FakeBinaryFetcher::new());
    publish_site_build(
        &fetcher,
        "ffmpeg",
        b"FFMPEG",
        Some("<html>not found</html>"),
    );
    let mb = managed(&bin, &fetcher, vec![site_tool("ffmpeg")]);
    mb.check_now(CheckReason::Launch).await;

    assert!(!bin.join("ffmpeg").exists());
    assert!(matches!(mb.tools_activity(), ToolsActivity::Failed(_)));
}
