//! Port of Swift's `ReorderingTests.swift` (10 tests).
mod support;

use sdm_core::{DownloadItem, DownloadPackage, ItemState};
use support::{
    WorkerGate, WorkerGatedOrigin, engine_settings, example_item, make_scratch_directory,
    new_engine, snapshot_item, test_payload, wait_until_async, within,
};
use uuid::Uuid;

fn disabled_item(name: &str, position: i64) -> DownloadItem {
    example_item(name)
        .with_enabled(false)
        .with_position(position)
}

async fn item_ids_of(engine: &sdm_engine::DownloadEngine, package_id: Uuid) -> Option<Vec<Uuid>> {
    engine
        .snapshot()
        .await
        .packages
        .iter()
        .find(|package| package.id == package_id)
        .map(|package| package.items.iter().map(|item| item.id).collect())
}

async fn package_exists(engine: &sdm_engine::DownloadEngine, package_id: Uuid) -> bool {
    item_ids_of(engine, package_id).await.is_some()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reorder_items_applies_the_given_order_within_a_package() {
    let dir = make_scratch_directory();
    let engine = new_engine(
        sdm_engine::FakeOrigin::new(test_payload(10)),
        engine_settings(1, 1, 8, dir.path()),
    );
    let items: Vec<DownloadItem> = (0..3)
        .map(|index| example_item(&format!("{index}.bin")).with_position(index))
        .collect();
    let package_id = Uuid::new_v4();
    engine
        .add(
            DownloadPackage::new("Batch")
                .with_id(package_id)
                .with_items(items.clone()),
        )
        .await;

    let new_order = [items[2].id, items[0].id, items[1].id];
    engine.reorder_items(&new_order, package_id).await;

    let ordered: Vec<Uuid> = engine.snapshot().await.packages[0]
        .items
        .iter()
        .map(|item| item.id)
        .collect();
    assert_eq!(ordered, new_order);

    within(engine.run_until_idle()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn move_item_relocates_a_queued_item_into_another_package() {
    let dir = make_scratch_directory();
    let engine = new_engine(
        sdm_engine::FakeOrigin::new(test_payload(10)),
        engine_settings(1, 1, 8, dir.path()),
    );
    let item = example_item("a.bin").with_enabled(false);
    let package_a = Uuid::new_v4();
    let package_b = Uuid::new_v4();
    engine
        .add(
            DownloadPackage::new("A")
                .with_id(package_a)
                .with_items(vec![item.clone()]),
        )
        .await;
    engine
        .add(DownloadPackage::new("B").with_id(package_b))
        .await;

    engine.move_item(item.id, package_b, None).await;

    // Package A had only this one item, so moving it out empties A — and,
    // same as `remove_item`/`remove_package`, an emptied package is dropped
    // entirely rather than left behind as an empty husk.
    assert!(!package_exists(&engine, package_a).await);
    assert_eq!(item_ids_of(&engine, package_b).await, Some(vec![item.id]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn move_item_relocates_the_finished_file_to_the_destination_packages_folder() {
    let dir = make_scratch_directory();
    let payload = test_payload(200);
    let engine = new_engine(
        sdm_engine::FakeOrigin::new(payload.clone()),
        engine_settings(1, 1, 8, dir.path()),
    );
    let item = example_item("a.bin");
    let package_a = Uuid::new_v4();
    let package_b = Uuid::new_v4();
    engine
        .add(
            DownloadPackage::new("A")
                .with_id(package_a)
                .with_items(vec![item.clone()]),
        )
        .await;
    engine
        .add(DownloadPackage::new("B").with_id(package_b))
        .await;
    within(engine.run_until_idle()).await.unwrap();

    let source_path = dir.path().join("A").join("a.bin");
    let destination_path = dir.path().join("B").join("a.bin");
    assert_eq!(std::fs::read(&source_path).unwrap(), payload);

    engine.move_item(item.id, package_b, None).await;

    assert!(!source_path.exists());
    assert_eq!(std::fs::read(&destination_path).unwrap(), payload);
    // Package A had only this one item, so moving it out empties — and
    // trashes — A's folder, same as `remove_item`/`remove_package` do.
    assert!(!package_exists(&engine, package_a).await);
    assert!(!dir.path().join("A").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn move_item_at_index_inserts_at_that_position_in_the_destination_package() {
    let dir = make_scratch_directory();
    let engine = new_engine(
        sdm_engine::FakeOrigin::new(test_payload(10)),
        engine_settings(1, 1, 8, dir.path()),
    );
    let moving = example_item("moving.bin").with_enabled(false);
    let existing: Vec<DownloadItem> = (0..2)
        .map(|index| disabled_item(&format!("{index}.bin"), index))
        .collect();
    let package_a = Uuid::new_v4();
    let package_b = Uuid::new_v4();
    engine
        .add(
            DownloadPackage::new("A")
                .with_id(package_a)
                .with_items(vec![moving.clone()]),
        )
        .await;
    engine
        .add(
            DownloadPackage::new("B")
                .with_id(package_b)
                .with_items(existing.clone()),
        )
        .await;

    engine.move_item(moving.id, package_b, Some(1)).await;

    assert_eq!(
        item_ids_of(&engine, package_b).await,
        Some(vec![existing[0].id, moving.id, existing[1].id])
    );
}

/// `move_item` is also how same-package drag reordering works (the app's list
/// view uses it instead of `.onMove`). `at_index` means "insert before
/// whatever is currently at this index in the pre-move list," which forward
/// and backward moves resolve to different post-removal insertion points
/// for — this pins both directions.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn move_item_within_the_same_package_reorders_in_place() {
    let dir = make_scratch_directory();
    let engine = new_engine(
        sdm_engine::FakeOrigin::new(test_payload(10)),
        engine_settings(1, 1, 8, dir.path()),
    );
    let items: Vec<DownloadItem> = (0..4)
        .map(|index| disabled_item(&format!("{index}.bin"), index))
        .collect();
    let package_id = Uuid::new_v4();
    engine
        .add(
            DownloadPackage::new("A")
                .with_id(package_id)
                .with_items(items.clone()),
        )
        .await;

    // Drop item 0 onto item 2's row: item 0 should end up immediately
    // before wherever item 2 ends up, i.e. [1, 0, 2, 3].
    engine.move_item(items[0].id, package_id, Some(2)).await;
    assert_eq!(
        item_ids_of(&engine, package_id).await,
        Some(vec![items[1].id, items[0].id, items[2].id, items[3].id])
    );

    // Drop item 3 (now last) onto the first row: item 3 should land at the
    // front.
    engine.move_item(items[3].id, package_id, Some(0)).await;
    assert_eq!(
        item_ids_of(&engine, package_id).await,
        Some(vec![items[3].id, items[1].id, items[0].id, items[2].id])
    );
}

/// `move_items_to_package` is what a multi-selection drag uses — it moves an
/// ordered batch to one contiguous run at the target index in a single atomic
/// step, rather than one `move_item` call per item.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn move_items_inserts_the_whole_batch_as_one_contiguous_run_at_the_target_index() {
    let dir = make_scratch_directory();
    let engine = new_engine(
        sdm_engine::FakeOrigin::new(test_payload(10)),
        engine_settings(1, 1, 8, dir.path()),
    );
    let moving: Vec<DownloadItem> = (0..2)
        .map(|index| example_item(&format!("moving{index}.bin")).with_enabled(false))
        .collect();
    let existing: Vec<DownloadItem> = (0..2)
        .map(|index| disabled_item(&format!("{index}.bin"), index))
        .collect();
    let package_a = Uuid::new_v4();
    let package_b = Uuid::new_v4();
    engine
        .add(
            DownloadPackage::new("A")
                .with_id(package_a)
                .with_items(moving.clone()),
        )
        .await;
    engine
        .add(
            DownloadPackage::new("B")
                .with_id(package_b)
                .with_items(existing.clone()),
        )
        .await;

    // Moving both A items in, dropped between B's two existing items.
    let moving_ids: Vec<Uuid> = moving.iter().map(|item| item.id).collect();
    engine
        .move_items_to_package(&moving_ids, package_b, Some(1))
        .await;

    assert_eq!(
        item_ids_of(&engine, package_b).await,
        Some(vec![
            existing[0].id,
            moving[0].id,
            moving[1].id,
            existing[1].id
        ])
    );
    // A had only these two items, so moving both out empties — and drops
    // — it, same as `move_item` does for a single item.
    assert!(!package_exists(&engine, package_a).await);
}

/// The batch can straddle its own destination: some members already live in
/// the destination package, others are moving in from elsewhere — the "how
/// many of the batch sit ahead of the target index within the destination
/// itself" adjustment has to account for both at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn move_items_handles_a_batch_straddling_its_own_destination() {
    let dir = make_scratch_directory();
    let engine = new_engine(
        sdm_engine::FakeOrigin::new(test_payload(10)),
        engine_settings(1, 1, 8, dir.path()),
    );
    let from_other = example_item("other.bin").with_enabled(false);
    let destination_items: Vec<DownloadItem> = (0..3)
        .map(|index| disabled_item(&format!("{index}.bin"), index))
        .collect();
    let package_a = Uuid::new_v4();
    let package_b = Uuid::new_v4();
    engine
        .add(
            DownloadPackage::new("A")
                .with_id(package_a)
                .with_items(vec![from_other.clone()]),
        )
        .await;
    engine
        .add(
            DownloadPackage::new("B")
                .with_id(package_b)
                .with_items(destination_items.clone()),
        )
        .await;

    // Move item 0 (already in B) and `from_other` (from A) to land right
    // before B's item 2 — item 0 sitting ahead of index 2 in B's own
    // pre-move list means the effective insertion point shifts back by one
    // once it's removed from its old spot.
    engine
        .move_items_to_package(
            &[destination_items[0].id, from_other.id],
            package_b,
            Some(2),
        )
        .await;

    assert_eq!(
        item_ids_of(&engine, package_b).await,
        Some(vec![
            destination_items[1].id,
            destination_items[0].id,
            from_other.id,
            destination_items[2].id,
        ])
    );
    assert!(!package_exists(&engine, package_a).await);
}

/// A batch drawn from multiple distinct source packages must empty and drop
/// every one of them that ends up with no items left, not just the first.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn move_items_drops_every_emptied_source_package() {
    let dir = make_scratch_directory();
    let engine = new_engine(
        sdm_engine::FakeOrigin::new(test_payload(10)),
        engine_settings(1, 1, 8, dir.path()),
    );
    let item_a = example_item("a.bin").with_enabled(false);
    let item_b = example_item("b.bin").with_enabled(false);
    let package_a = Uuid::new_v4();
    let package_b = Uuid::new_v4();
    let package_c = Uuid::new_v4();
    engine
        .add(
            DownloadPackage::new("A")
                .with_id(package_a)
                .with_items(vec![item_a.clone()]),
        )
        .await;
    engine
        .add(
            DownloadPackage::new("B")
                .with_id(package_b)
                .with_items(vec![item_b.clone()]),
        )
        .await;
    engine
        .add(DownloadPackage::new("C").with_id(package_c))
        .await;

    engine
        .move_items_to_package(&[item_a.id, item_b.id], package_c, None)
        .await;

    assert!(!package_exists(&engine, package_a).await);
    assert!(!package_exists(&engine, package_b).await);
    assert_eq!(
        item_ids_of(&engine, package_c).await,
        Some(vec![item_a.id, item_b.id])
    );
}

/// Same all-or-nothing refusal `move_item` gives a single running item,
/// generalized: one running item anywhere in the batch refuses the whole move
/// rather than silently moving the rest.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn move_items_refuses_the_whole_batch_if_any_member_is_running() {
    let dir = make_scratch_directory();
    let gate = WorkerGate::new();
    let engine = new_engine(
        WorkerGatedOrigin::new(test_payload(4000), gate.clone()),
        engine_settings(2, 1, 8, dir.path()),
    );
    let running = example_item("a.bin");
    let queued = example_item("b.bin").with_enabled(false);
    let package_a = Uuid::new_v4();
    let package_b = Uuid::new_v4();
    engine
        .add(
            DownloadPackage::new("A")
                .with_id(package_a)
                .with_items(vec![running.clone(), queued.clone()]),
        )
        .await;
    engine
        .add(DownloadPackage::new("B").with_id(package_b))
        .await;

    wait_until_async(|| async {
        snapshot_item(running.id, &engine)
            .await
            .is_some_and(|snapshot| snapshot.state == ItemState::Running)
    })
    .await;

    engine
        .move_items_to_package(&[running.id, queued.id], package_b, None)
        .await;

    assert_eq!(
        item_ids_of(&engine, package_a).await,
        Some(vec![running.id, queued.id])
    );
    assert_eq!(item_ids_of(&engine, package_b).await, Some(vec![]));

    gate.open();
    within(engine.run_until_idle()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn move_item_is_a_no_op_for_a_running_item() {
    let dir = make_scratch_directory();
    let gate = WorkerGate::new();
    let engine = new_engine(
        WorkerGatedOrigin::new(test_payload(4000), gate.clone()),
        engine_settings(1, 1, 8, dir.path()),
    );
    let item = example_item("a.bin");
    let package_a = Uuid::new_v4();
    let package_b = Uuid::new_v4();
    engine
        .add(
            DownloadPackage::new("A")
                .with_id(package_a)
                .with_items(vec![item.clone()]),
        )
        .await;
    engine
        .add(DownloadPackage::new("B").with_id(package_b))
        .await;

    wait_until_async(|| async {
        snapshot_item(item.id, &engine)
            .await
            .is_some_and(|snapshot| snapshot.state == ItemState::Running)
    })
    .await;

    engine.move_item(item.id, package_b, None).await;
    assert_eq!(item_ids_of(&engine, package_a).await, Some(vec![item.id]));
    assert_eq!(item_ids_of(&engine, package_b).await, Some(vec![]));

    gate.open();
    within(engine.run_until_idle()).await.unwrap();
}
