//! An origin that advertises `Accept-Ranges` but answers a ranged request
//! with `200` and the whole body must never produce a wrong file: the body of
//! a `200` starts at byte 0, so writing it at a claim's offset would corrupt
//! the download while the size still looked right.
mod support;

use async_trait::async_trait;
use sdm_core::{ByteRange, DownloadItem, DownloadPackage, ItemState};
use sdm_engine::{
    FakeOrigin, FakeOriginBehavior, HttpTransport, RangeRequest, RangeResponse, SparseFile,
    TransportError,
};
use support::{
    engine_settings, make_scratch_directory, new_engine, snapshot_item, test_payload, test_source,
    within,
};

/// Honours the one-byte probe (so the download starts as resumable and
/// segmented) and then serves every other request as `200` with the whole
/// body, as a mirror behind a balancer or a CDN edge can.
struct AdvertisesThenIgnores {
    honest: FakeOrigin,
    ignoring: FakeOrigin,
}

#[async_trait]
impl HttpTransport for AdvertisesThenIgnores {
    async fn fetch(&self, request: RangeRequest) -> Result<RangeResponse, TransportError> {
        if request.range == Some(ByteRange::new(0, 1)) {
            self.honest.fetch(request).await
        } else {
            self.ignoring.fetch(request).await
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_200_answer_to_a_ranged_request_never_corrupts_the_file() {
    let dir = make_scratch_directory();
    let payload = test_payload(64 * 1024);
    let transport = AdvertisesThenIgnores {
        honest: FakeOrigin::new(payload.clone()),
        ignoring: FakeOrigin::with_behavior(
            payload.clone(),
            FakeOriginBehavior {
                ignores_ranges: true,
                ..Default::default()
            },
        ),
    };
    // Four workers: some claim starts past byte 0.
    let engine = new_engine(
        transport,
        engine_settings(1, 4, 8, dir.path()).with_min_segment_size_bytes(1024),
    );
    let item = DownloadItem::http(test_source(), "a.bin");
    let id = item.id;
    engine
        .add(DownloadPackage::new("P").with_items(vec![item]))
        .await;
    within(engine.run_until_idle()).await.unwrap();

    let destination = dir.path().join("P/a.bin");
    let snapshot = snapshot_item(id, &engine).await.unwrap();
    if destination.exists() {
        // If a file was delivered it is the right one.
        assert!(
            std::fs::read(&destination).unwrap() == payload,
            "a delivered file differs from the source"
        );
    } else {
        assert_ne!(snapshot.state, ItemState::Completed);
    }
    // Whatever the engine recorded as written is what is on disk.
    let incomplete = SparseFile::incomplete_path(&destination);
    if let Ok(on_disk) = std::fs::read(&incomplete) {
        for range in snapshot.completed.ranges() {
            let (start, end) = (range.start as usize, range.end as usize);
            assert!(
                on_disk[start..end] == payload[start..end],
                "recorded range {range:?} holds the wrong bytes"
            );
        }
    }
    assert_ne!(
        snapshot.state,
        ItemState::Completed,
        "a download whose ranged requests were answered with 200 cannot have completed"
    );
}
