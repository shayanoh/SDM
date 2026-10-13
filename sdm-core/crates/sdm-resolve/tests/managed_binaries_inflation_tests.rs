use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use sdm_resolve::lzfse;
use sdm_resolve::testing::{FakeBinaryFetcher, FakeProcessRunner};
use sdm_resolve::{ManagedBinaries, VendorAsset, YtDlpChannel};

fn make_managed(bin: &Path, assets: Vec<VendorAsset>) -> ManagedBinaries {
    ManagedBinaries::new(
        bin.to_path_buf(),
        Arc::new(FakeBinaryFetcher::new()),
        Arc::new(FakeProcessRunner::new()),
        move || assets.clone(),
        || YtDlpChannel::Stable,
    )
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inflates_bundled_ffmpeg_once() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let blob = tmp.path().join("ffmpeg.lzfse");
    let payload = b"#!/bin/sh\necho ff\n".to_vec();
    std::fs::write(&blob, lzfse::encode(&payload).unwrap()).unwrap();

    let mb = make_managed(
        &bin,
        vec![VendorAsset {
            name: "ffmpeg".into(),
            compressed_path: blob,
            version: "7.1".into(),
        }],
    );
    mb.provision_bundled_if_needed().await;

    let ff = bin.join("ffmpeg");
    assert!(is_executable(&ff));
    assert_eq!(std::fs::read(&ff).unwrap(), payload);
    assert_eq!(
        mb.manifest_for_testing().ffmpeg_version.as_deref(),
        Some("7.1")
    );

    // Same version -> not rewritten: a local edit survives a second pass.
    std::fs::write(&ff, b"touched").unwrap();
    mb.provision_bundled_if_needed().await;
    assert_eq!(std::fs::read(&ff).unwrap(), b"touched".to_vec());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inflates_bundled_ffprobe_under_the_ffmpeg_version() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let blob = tmp.path().join("ffprobe.lzfse");
    let payload = b"#!/bin/sh\necho fp\n".to_vec();
    std::fs::write(&blob, lzfse::encode(&payload).unwrap()).unwrap();

    let mb = make_managed(
        &bin,
        vec![VendorAsset {
            name: "ffprobe".into(),
            compressed_path: blob,
            version: "7.1".into(),
        }],
    );
    mb.provision_bundled_if_needed().await;

    let fp = bin.join("ffprobe");
    assert!(is_executable(&fp));
    assert_eq!(std::fs::read(&fp).unwrap(), payload);
    assert_eq!(
        mb.manifest_for_testing().ffmpeg_version.as_deref(),
        Some("7.1")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reinflates_when_bundled_version_bumps() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");

    let v1 = tmp.path().join("q1.lzfse");
    std::fs::write(&v1, lzfse::encode(b"one").unwrap()).unwrap();
    let mb1 = make_managed(
        &bin,
        vec![VendorAsset {
            name: "qjs".into(),
            compressed_path: v1,
            version: "0.9.0".into(),
        }],
    );
    mb1.provision_bundled_if_needed().await;
    assert_eq!(std::fs::read(bin.join("qjs")).unwrap(), b"one".to_vec());

    let v2 = tmp.path().join("q2.lzfse");
    std::fs::write(&v2, lzfse::encode(b"two").unwrap()).unwrap();
    let mb2 = make_managed(
        &bin,
        vec![VendorAsset {
            name: "qjs".into(),
            compressed_path: v2,
            version: "0.10.0".into(),
        }],
    );
    mb2.provision_bundled_if_needed().await;
    assert_eq!(std::fs::read(bin.join("qjs")).unwrap(), b"two".to_vec());
    assert_eq!(
        mb2.manifest_for_testing().qjs_version.as_deref(),
        Some("0.10.0")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn missing_blob_is_skipped_not_fatal() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    let mb = make_managed(
        &bin,
        vec![VendorAsset {
            name: "ffmpeg".into(),
            compressed_path: tmp.path().join("does-not-exist.lzfse"),
            version: "7.1".into(),
        }],
    );
    mb.provision_bundled_if_needed().await;
    assert!(!bin.join("ffmpeg").exists());
    assert_eq!(mb.manifest_for_testing().last_error, None);
}
