mod support;

use sdm_app::AppError;
use sdm_app::protocol::{ItemStateDto, StructureItem, StructurePackage};
use sdm_engine::{FakeOrigin, FakeOriginBehavior};
use support::*;

/// The packages of the newest structure the sink received.
fn packages(h: &Harness) -> Vec<StructurePackage> {
    h.sink
        .patches()
        .iter()
        .rev()
        .find_map(|p| p.structure.clone())
        .expect("a structure patch")
        .packages
}

fn items(h: &Harness) -> Vec<StructureItem> {
    packages(h).into_iter().flat_map(|p| p.items).collect()
}

fn state_of(h: &Harness, id: &str) -> ItemStateDto {
    items(h)
        .into_iter()
        .find(|i| i.id == id)
        .unwrap_or_else(|| panic!("no item {id}"))
        .state
}

/// Adds one URL per name and returns the item ids in the same order.
async fn add(h: &Harness, names: &[&str]) -> Vec<String> {
    let mut ids = Vec::new();
    for name in names {
        h.app
            .add_url(&format!("https://example.com/{name}"), None)
            .await
            .unwrap();
        let id = items(h)
            .into_iter()
            .find(|i| i.filename == *name)
            .unwrap_or_else(|| panic!("no item named {name}"))
            .id;
        ids.push(id);
    }
    ids
}

async fn tick_until_state(h: &Harness, id: &str, wanted: ItemStateDto) -> bool {
    let id = id.to_owned();
    tick_until(&h.app, 600, || {
        items(h).iter().any(|i| i.id == id && i.state == wanted)
    })
    .await
}

/// A payload large enough that an item is still running when a test looks.
const BIG: usize = 30_000_000;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stop_items_then_start_items_round_trips_state() {
    let h = harness(FakeOrigin::new(payload(BIG))).await;
    let ids = add(&h, &["a.bin", "b.bin"]).await;

    h.app.stop_items(&ids).await.unwrap();
    for id in &ids {
        assert_eq!(state_of(&h, id), ItemStateDto::Stopped);
    }
    h.app.start_items(&ids).await.unwrap();
    for id in &ids {
        assert!(
            matches!(
                state_of(&h, id),
                ItemStateDto::Queued | ItemStateDto::Running
            ),
            "{:?}",
            state_of(&h, id)
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bad_id_rejects_the_whole_batch() {
    let h = harness(FakeOrigin::new(payload(BIG))).await;
    let ids = add(&h, &["a.bin"]).await;
    let before = state_of(&h, &ids[0]);

    let result = h.app.stop_items(&[ids[0].clone(), "nope".to_owned()]).await;

    assert!(matches!(result, Err(AppError::InvalidArgument { .. })));
    assert_eq!(state_of(&h, &ids[0]), before, "nothing may change");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn duplicate_ids_are_acted_on_once() {
    let h = harness(FakeOrigin::new(payload(BIG))).await;
    let ids = add(&h, &["a.bin"]).await;
    h.app
        .stop_items(&[ids[0].clone(), ids[0].clone()])
        .await
        .unwrap();
    assert_eq!(state_of(&h, &ids[0]), ItemStateDto::Stopped);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn set_items_enabled_false_marks_items_disabled() {
    let h = harness(FakeOrigin::new(payload(BIG))).await;
    let ids = add(&h, &["a.bin", "b.bin"]).await;

    h.app.set_items_enabled(&ids, false).await.unwrap();
    assert!(items(&h).iter().all(|i| !i.is_enabled));
    assert!(items(&h).iter().all(|i| i.state == ItemStateDto::Stopped));

    h.app.set_items_enabled(&ids, true).await.unwrap();
    assert!(items(&h).iter().all(|i| i.is_enabled));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remove_items_without_files_keeps_the_file_on_disk() {
    let h = harness(FakeOrigin::new(payload(3000))).await;
    let ids = add(&h, &["keep.bin"]).await;
    assert!(tick_until_state(&h, &ids[0], ItemStateDto::Completed).await);

    h.app.remove_items(&ids, false).await.unwrap();

    assert!(items(&h).is_empty());
    assert!(
        h.downloads
            .path()
            .join("keep.bin")
            .join("keep.bin")
            .exists()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remove_items_with_files_trashes_the_file() {
    let h = harness(FakeOrigin::new(payload(3000))).await;
    let ids = add(&h, &["gone.bin"]).await;
    assert!(tick_until_state(&h, &ids[0], ItemStateDto::Completed).await);

    h.app.remove_items(&ids, true).await.unwrap();

    assert!(items(&h).is_empty());
    assert!(
        !h.downloads
            .path()
            .join("gone.bin")
            .join("gone.bin")
            .exists()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reset_items_discards_a_stopped_items_progress() {
    let h = harness(FakeOrigin::new(payload(3000))).await;
    let ids = add(&h, &["r.bin"]).await;
    assert!(tick_until_state(&h, &ids[0], ItemStateDto::Completed).await);

    h.app.reset_items(&ids).await.unwrap();

    assert!(
        !h.downloads.path().join("r.bin").join("r.bin").exists(),
        "reset trashes the finished file"
    );
    assert_ne!(state_of(&h, &ids[0]), ItemStateDto::Completed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reset_items_skips_a_running_item() {
    let h = harness(FakeOrigin::new(payload(BIG))).await;
    let ids = add(&h, &["run.bin"]).await;
    assert!(tick_until_state(&h, &ids[0], ItemStateDto::Running).await);

    h.app.reset_items(&ids).await.unwrap();

    h.app.tick().await.unwrap();
    assert_eq!(state_of(&h, &ids[0]), ItemStateDto::Running);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retry_items_restarts_a_failed_item() {
    let origin = FakeOrigin::with_behavior(
        payload(3000),
        FakeOriginBehavior {
            status_override: Some(404),
            ..FakeOriginBehavior::default()
        },
    );
    let h = harness(origin.clone()).await;
    let ids = add(&h, &["bad.bin"]).await;
    assert!(
        tick_until(&h.app, 600, || matches!(
            state_of(&h, &ids[0]),
            ItemStateDto::Failed { .. }
        ))
        .await,
        "the item never failed"
    );

    origin.set_behavior(FakeOriginBehavior::default());
    h.app.retry_items(&ids).await.unwrap();

    assert!(tick_until_state(&h, &ids[0], ItemStateDto::Completed).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retry_mux_items_is_a_no_op_for_a_non_mux_item() {
    let h = harness(FakeOrigin::new(payload(BIG))).await;
    let ids = add(&h, &["m.bin"]).await;
    let before = state_of(&h, &ids[0]);
    h.app.retry_mux_items(&ids).await.unwrap();
    assert_eq!(state_of(&h, &ids[0]), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_empty_id_list_is_ok_and_publishes_nothing() {
    let h = harness(FakeOrigin::new(payload(BIG))).await;
    add(&h, &["a.bin"]).await;
    h.sink.take();
    h.app.stop_items(&[]).await.unwrap();
    h.app.remove_items(&[], true).await.unwrap();
    assert!(h.sink.take().is_empty());
}
