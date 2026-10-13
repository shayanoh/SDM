//! Port of Swift's `MetadataSnapshotTests.swift` (1 test).
mod support;

use sdm_core::{DownloadItem, DownloadPackage};
use sdm_engine::FakeOrigin;
use support::{engine_settings, make_scratch_directory, new_engine, snapshot_item, test_source};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn item_metadata_is_surfaced_in_the_snapshot() {
    let dir = make_scratch_directory();
    let engine = new_engine(
        FakeOrigin::new(Vec::new()),
        engine_settings(1, 1, 8, dir.path()),
    );

    let with_meta =
        DownloadItem::http(test_source(), "a.mkv").with_metadata("1080p · WEB-DL · H.264");
    let without_meta = DownloadItem::http(test_source(), "b.zip");
    let (with_id, without_id) = (with_meta.id, without_meta.id);
    engine
        .add(DownloadPackage::new("P").with_items(vec![with_meta, without_meta]))
        .await;

    assert_eq!(
        snapshot_item(with_id, &engine).await.unwrap().metadata,
        Some("1080p · WEB-DL · H.264".to_string())
    );
    assert_eq!(
        snapshot_item(without_id, &engine).await.unwrap().metadata,
        None
    );
}
