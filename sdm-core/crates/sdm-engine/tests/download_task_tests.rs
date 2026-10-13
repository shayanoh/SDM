mod support;

use std::sync::Arc;

use sdm_core::ByteRange;
use sdm_core::SourceUrl;
use sdm_engine::{
    DownloadError, DownloadTask, DownloadTaskConfiguration, FakeOrigin, FakeOriginBehavior,
    ResumeSidecar, SparseFile, TransportError,
};
use support::{make_scratch_directory, test_configuration, test_payload, test_source, within};
use uuid::Uuid;

fn task_for(
    destination: std::path::PathBuf,
    origin: FakeOrigin,
    configuration: DownloadTaskConfiguration,
) -> DownloadTask {
    DownloadTask::new(
        Uuid::new_v4(),
        test_source(),
        destination,
        Arc::new(origin),
        configuration,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_worker_downloads_complete_file() {
    let dir = make_scratch_directory();
    let payload = test_payload(4000);
    let task = task_for(
        dir.path().join("out.bin"),
        FakeOrigin::new(payload.clone()),
        test_configuration(1, 64),
    );

    let result = within(task.start()).await.unwrap();
    assert_eq!(std::fs::read(result).unwrap(), payload);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completed_file_has_no_incomplete_or_sidecar_left_behind() {
    let dir = make_scratch_directory();
    let task = task_for(
        dir.path().join("out.bin"),
        FakeOrigin::new(test_payload(1000)),
        test_configuration(1, 64),
    );
    within(task.start()).await.unwrap();

    let remaining: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(remaining, vec!["out.bin".to_string()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn range_set_covers_whole_file_after_completion() {
    let dir = make_scratch_directory();
    let task = task_for(
        dir.path().join("out.bin"),
        FakeOrigin::new(test_payload(1000)),
        test_configuration(1, 64),
    );
    within(task.start()).await.unwrap();

    assert_eq!(task.completed_ranges().ranges(), &[ByteRange::new(0, 1000)]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn truncated_body_fails_without_retrying_forever() {
    let dir = make_scratch_directory();
    // A single worker's claim is always the whole free gap now, so with a
    // 100-byte payload the truncation point below lands inside that one
    // claim rather than at its edge.
    let behavior = FakeOriginBehavior {
        truncate_after_bytes: Some(50),
        ..Default::default()
    };
    let task = task_for(
        dir.path().join("out.bin"),
        FakeOrigin::with_behavior(test_payload(100), behavior),
        test_configuration(1, 64),
    );

    let error = within(task.start()).await.unwrap_err();
    assert_eq!(
        error,
        DownloadError::TruncatedResponse {
            expected: 100,
            received: 50
        }
    );

    // The short read must not have lost the bytes it did receive.
    assert_eq!(task.completed_ranges().ranges(), &[ByteRange::new(0, 50)]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mid_stream_failure_leaves_incomplete_file_and_sidecar_resumable() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");
    // chunk_size defaults to 64 and checkpoint_interval is 128, so a
    // checkpoint fires (at 128 bytes) before the drop at 512 — the sidecar
    // is guaranteed to exist when the connection dies.
    let behavior = FakeOriginBehavior {
        drop_after_bytes: Some(512),
        ..Default::default()
    };
    let task = task_for(
        destination.clone(),
        FakeOrigin::with_behavior(test_payload(4000), behavior),
        test_configuration(1, 64),
    );

    let error = within(task.start()).await.unwrap_err();
    assert_eq!(
        error,
        DownloadError::Transport(TransportError::ConnectionDropped)
    );

    assert!(SparseFile::incomplete_path(&destination).exists());
    assert!(ResumeSidecar::path_for(&destination).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn error_status_fails_the_download() {
    let dir = make_scratch_directory();
    let behavior = FakeOriginBehavior {
        status_override: Some(404),
        ..Default::default()
    };
    let task = task_for(
        dir.path().join("out.bin"),
        FakeOrigin::with_behavior(test_payload(100), behavior),
        test_configuration(1, 64),
    );

    let error = within(task.start()).await.unwrap_err();
    assert_eq!(error, DownloadError::ServerError { status: 404 });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn expired_url_on_a_refreshable_component_throws_url_expired() {
    let behavior = FakeOriginBehavior {
        status_override: Some(403),
        ..Default::default()
    };
    let origin = FakeOrigin::with_behavior(test_payload(2000), behavior);
    let dir = make_scratch_directory();
    let task = DownloadTask::new(
        Uuid::new_v4(),
        SourceUrl::new("https://gv/v137").unwrap(),
        dir.path().join("v.f137.mp4"),
        Arc::new(origin),
        DownloadTaskConfiguration::new(1, 64, 128, None).with_refreshable_format_id("137"),
    );

    let error = within(task.start()).await.unwrap_err();
    assert_eq!(
        error,
        DownloadError::UrlExpired {
            format_id: "137".to_string()
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn expired_url_without_a_refreshable_format_is_a_plain_server_error() {
    let behavior = FakeOriginBehavior {
        status_override: Some(403),
        ..Default::default()
    };
    let origin = FakeOrigin::with_behavior(test_payload(2000), behavior);
    let dir = make_scratch_directory();
    let task = DownloadTask::new(
        Uuid::new_v4(),
        SourceUrl::new("https://x/f").unwrap(),
        dir.path().join("f.bin"),
        Arc::new(origin),
        DownloadTaskConfiguration::new(1, 64, 128, None),
    );

    let error = within(task.start()).await.unwrap_err();
    assert_eq!(error, DownloadError::ServerError { status: 403 });
}
