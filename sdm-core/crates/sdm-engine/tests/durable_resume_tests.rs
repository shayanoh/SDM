mod support;

use std::path::Path;
use std::sync::Arc;

use sdm_core::{ByteRange, RangeSet};
use sdm_engine::{
    DownloadTask, DownloadTaskConfiguration, FakeOrigin, FakeOriginBehavior, ResumeSidecar,
    SparseFile,
};
use support::{make_scratch_directory, test_configuration, test_payload, test_source, within};
use uuid::Uuid;

// The partial file is flushed to the device about once a minute, not on every
// checkpoint, so a sidecar records both what was written and what was
// flushed. These pin which of the two a resume trusts.

fn full_range() -> RangeSet {
    RangeSet::from_ranges(&[ByteRange::new(0, 600)])
}

fn flushed_range() -> RangeSet {
    RangeSet::from_ranges(&[ByteRange::new(0, 200)])
}

fn sidecar(durable: Option<RangeSet>, boot: Option<&str>) -> ResumeSidecar {
    let mut sidecar = ResumeSidecar::new(test_source(), None, 1000, None, full_range());
    sidecar.durable_completed = durable;
    sidecar.boot_session_id = boot.map(str::to_string);
    sidecar
}

#[test]
fn sidecar_without_a_durable_set_is_trusted_whole() {
    // Written by a version that flushed before every sidecar.
    assert_eq!(
        sidecar(None, None).resumable_ranges(Some("now")),
        full_range()
    );
}

#[test]
fn sidecar_from_the_same_boot_session_is_trusted_whole() {
    let written = sidecar(Some(flushed_range()), Some("boot-1"));
    assert_eq!(written.resumable_ranges(Some("boot-1")), full_range());
}

#[test]
fn sidecar_from_an_earlier_boot_session_is_trusted_only_as_far_as_it_was_flushed() {
    let written = sidecar(Some(flushed_range()), Some("boot-1"));
    assert_eq!(written.resumable_ranges(Some("boot-2")), flushed_range());
}

#[test]
fn sidecar_is_trusted_only_as_far_as_flushed_when_the_boot_session_is_unknown() {
    assert_eq!(
        sidecar(Some(flushed_range()), Some("boot-1")).resumable_ranges(None),
        flushed_range()
    );
    assert_eq!(
        sidecar(Some(flushed_range()), None).resumable_ranges(Some("boot-1")),
        flushed_range()
    );
}

#[test]
fn sidecar_written_before_the_durable_fields_existed_still_loads() {
    let dir = make_scratch_directory();
    let path = dir.path().join("old.sdmpart");
    let json = r#"
        {"formatVersion":1,"sourceURL":"https://example.com/file.bin","totalBytes":1000,
         "validator":"v","completed":{"ranges":[{"start":0,"end":600}]}}
        "#;
    std::fs::write(&path, json).unwrap();

    let loaded = ResumeSidecar::load(&path).expect("the old sidecar loads");
    assert_eq!(loaded.durable_completed, None);
    assert_eq!(loaded.resumable_ranges(Some("now")), full_range());
}

// MARK: - DownloadTask

/// Lays down an interrupted download by hand: an `.incomplete` file holding
/// only `on_disk` bytes of the payload (zeros elsewhere), and a sidecar that
/// claims `written` was written but only `flushed` was flushed.
fn stage_interrupted_download(
    payload: &[u8],
    destination: &Path,
    on_disk: std::ops::Range<usize>,
    written: std::ops::Range<usize>,
    flushed: std::ops::Range<usize>,
    boot: &str,
) {
    let mut bytes = vec![0u8; payload.len()];
    bytes[on_disk.clone()].copy_from_slice(&payload[on_disk]);
    std::fs::write(SparseFile::incomplete_path(destination), bytes).unwrap();
    ResumeSidecar::new(
        test_source(),
        None,
        payload.len() as i64,
        None,
        RangeSet::from_ranges(&[ByteRange::new(written.start as i64, written.end as i64)]),
    )
    .with_durable_completed(RangeSet::from_ranges(&[ByteRange::new(
        flushed.start as i64,
        flushed.end as i64,
    )]))
    .with_boot_session_id(boot)
    .save(&ResumeSidecar::path_for(destination))
    .unwrap();
}

async fn resume(
    payload: &[u8],
    destination: &Path,
    boot: &str,
) -> (std::path::PathBuf, Vec<ByteRange>) {
    let origin = FakeOrigin::new(payload.to_vec());
    let mut configuration = test_configuration(1, 64);
    configuration.boot_session_id = Some(boot.to_string());
    let task = DownloadTask::new(
        Uuid::new_v4(),
        test_source(),
        destination.to_path_buf(),
        Arc::new(origin.clone()),
        configuration,
    );
    let result = within(task.start()).await.unwrap();
    (result, origin.requested_ranges())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resume_after_a_restart_refetches_bytes_that_were_never_flushed() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");
    let payload = test_payload(8000);
    // A power cut: the sidecar says 6000 bytes were written, but only the
    // 2000 that had been flushed actually reached the disk.
    stage_interrupted_download(
        &payload,
        &destination,
        0..2000,
        0..6000,
        0..2000,
        "before-the-power-cut",
    );

    let (result, requested) = resume(&payload, &destination, "after").await;

    // Byte identity is the point: trusting the 6000 would have left zeros.
    assert_eq!(std::fs::read(result).unwrap(), payload);
    assert!(requested.iter().any(|range| range.start == 2000));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resume_in_the_same_boot_session_keeps_everything_that_was_written() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");
    let payload = test_payload(8000);
    // A crash or force quit: unflushed bytes are still there.
    stage_interrupted_download(&payload, &destination, 0..6000, 0..6000, 0..2000, "same");

    let (result, requested) = resume(&payload, &destination, "same").await;

    assert_eq!(std::fs::read(result).unwrap(), payload);
    // Apart from the one-byte probe, nothing before 6000 is asked for again.
    assert!(
        !requested
            .iter()
            .any(|range| range.length() > 1 && range.start < 6000)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_run_flushes_so_its_sidecar_vouches_for_everything_written() {
    let dir = make_scratch_directory();
    let destination = dir.path().join("out.bin");
    let payload = test_payload(8000);
    let origin = FakeOrigin::new(payload);
    origin.set_behavior(FakeOriginBehavior {
        drop_after_bytes: Some(3000),
        ..Default::default()
    });
    let task = DownloadTask::new(
        Uuid::new_v4(),
        test_source(),
        destination.clone(),
        Arc::new(origin),
        DownloadTaskConfiguration::new(1, 64, 128, None),
    );
    let _ = within(task.start()).await;

    // No minute has passed, so none of the byte-triggered checkpoints
    // flushed — the closing one on the failure path is what does.
    let saved = ResumeSidecar::load(&ResumeSidecar::path_for(&destination))
        .expect("a sidecar after the failed run");
    assert_eq!(saved.completed.total_bytes(), 3000);
    assert_eq!(saved.durable_completed.as_ref(), Some(&saved.completed));
    assert_eq!(
        saved.boot_session_id,
        ResumeSidecar::current_boot_session_id()
    );
}
