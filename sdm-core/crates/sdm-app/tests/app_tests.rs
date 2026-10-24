mod support;

use sdm_app::AppError;
use sdm_app::protocol::{ItemStateDto, Patch};
use sdm_engine::FakeOrigin;
use support::*;

fn last_structure_states(patches: &[Patch]) -> Vec<ItemStateDto> {
    patches
        .iter()
        .rev()
        .find_map(|p| p.structure.as_ref())
        .map(|s| {
            s.packages
                .iter()
                .flat_map(|p| p.items.iter().map(|i| i.state.clone()))
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn attaching_sends_one_full_patch() {
    let h = harness(FakeOrigin::new(payload(10))).await;
    let patches = h.sink.take();
    assert_eq!(patches.len(), 1);
    assert!(patches[0].full);
    assert!(patches[0].structure.is_some());
    assert!(patches[0].stats.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_idle_engine_emits_nothing() {
    let h = harness(FakeOrigin::new(payload(10))).await;
    h.sink.take();
    for _ in 0..10 {
        h.app.tick().await.unwrap();
    }
    assert!(
        h.sink.take().is_empty(),
        "an idle tick must not produce a patch"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn adding_a_url_downloads_it_and_reports_completion() {
    let body = payload(5000);
    let h = harness(FakeOrigin::new(body.clone())).await;
    h.sink.take();
    h.app
        .add_url("https://example.com/files/data.bin", None)
        .await
        .unwrap();

    let done = tick_until(&h.app, 400, || {
        last_structure_states(&h.sink.patches()).contains(&ItemStateDto::Completed)
    })
    .await;
    assert!(done, "the item never completed");

    let file = h.downloads.path().join("data.bin").join("data.bin");
    let on_disk = std::fs::read(&file)
        .or_else(|_| std::fs::read(h.downloads.path().join("data.bin")))
        .expect("the downloaded file exists in the package folder");
    assert_eq!(on_disk, body);
    assert!(h.host.notifications().iter().any(|n| matches!(n,
        sdm_app::notify::Notification::Finished { filename } if filename == "data.bin")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn seq_increases_by_exactly_one_per_patch() {
    let h = harness(FakeOrigin::new(payload(3000))).await;
    h.app
        .add_url("https://example.com/a.bin", None)
        .await
        .unwrap();
    tick_until(&h.app, 400, || {
        last_structure_states(&h.sink.patches()).contains(&ItemStateDto::Completed)
    })
    .await;
    let seqs: Vec<u32> = h.sink.patches().iter().map(|p| p.seq).collect();
    for pair in seqs.windows(2) {
        assert_eq!(pair[1], pair[0].wrapping_add(1), "{seqs:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn full_state_equals_the_patches_applied_so_far() {
    let h = harness(FakeOrigin::new(payload(4000))).await;
    h.app
        .add_url("https://example.com/a.bin", None)
        .await
        .unwrap();
    for _ in 0..5 {
        h.app.tick().await.unwrap();
    }
    h.sink.take();
    h.app.full_state().await.unwrap();
    let full = h.sink.take().remove(0);
    assert!(full.full);
    let structure = full.structure.expect("structure in a full patch");
    assert_eq!(structure.packages.len(), 1);
    assert_eq!(full.items.len(), structure.packages[0].items.len());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_send_makes_the_next_patch_full() {
    let h = harness(FakeOrigin::new(payload(2000))).await;
    h.sink.take();
    h.sink.fail_next();
    h.app
        .add_url("https://example.com/a.bin", None)
        .await
        .unwrap(); // this patch is lost
    // The sink was dropped on failure; the shell re-attaches (a reloaded window).
    h.app.attach(h.sink.clone()).await.unwrap();
    let first = h.sink.take().remove(0);
    assert!(first.full, "after a lost patch the next one must be full");
    assert!(
        first
            .structure
            .unwrap()
            .packages
            .iter()
            .any(|p| !p.items.is_empty())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hidden_list_emits_nothing_and_showing_it_sends_a_full_patch() {
    let h = harness(FakeOrigin::new(payload(2000))).await;
    h.sink.take();
    h.app.set_list_visible(false).await.unwrap();
    h.app
        .add_url("https://example.com/a.bin", None)
        .await
        .unwrap();
    for _ in 0..5 {
        h.app.tick().await.unwrap();
    }
    assert!(
        h.sink.take().is_empty(),
        "no patches while the list is hidden"
    );
    h.app.set_list_visible(true).await.unwrap();
    let patches = h.sink.take();
    assert_eq!(patches.len(), 1);
    assert!(patches[0].full);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn detail_histories_are_sent_only_for_reported_items_and_ignore_bad_ids() {
    let h = harness(FakeOrigin::new(payload(200_000))).await;
    h.app
        .add_url("https://example.com/big.bin", None)
        .await
        .unwrap();
    for _ in 0..3 {
        h.app.tick().await.unwrap();
    }
    let id = h
        .sink
        .patches()
        .iter()
        .rev()
        .find_map(|p| p.structure.as_ref())
        .map(|s| s.packages[0].items[0].id.clone())
        .expect("an item exists");

    let error = h
        .app
        .set_detail_items("w1", &["not-a-uuid".to_owned()])
        .await
        .expect_err("bad id");
    assert!(matches!(error, AppError::InvalidArgument { .. }));

    h.app
        .set_detail_items("w1", std::slice::from_ref(&id))
        .await
        .unwrap();
    h.sink.take();
    h.app.tick().await.unwrap();
    let with_history: Vec<_> = h
        .sink
        .take()
        .into_iter()
        .flat_map(|p| p.histories)
        .collect();
    assert!(with_history.iter().all(|history| history.id == id));

    h.app.set_detail_items("w1", &[]).await.unwrap();
    h.sink.take();
    h.app.tick().await.unwrap();
    assert!(h.sink.take().iter().all(|p| p.histories.is_empty()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn commands_reject_bad_ids_and_urls_without_panicking() {
    let h = harness(FakeOrigin::new(payload(10))).await;
    for bad in ["", "zzz", "00000000-0000-0000-0000-000000000000"] {
        // A well-formed but unknown id is not an error: the engine ignores it.
        let result = h.app.stop_items(&[bad.to_owned()]).await;
        if bad == "00000000-0000-0000-0000-000000000000" {
            assert!(result.is_ok());
        } else {
            assert!(
                matches!(result, Err(AppError::InvalidArgument { .. })),
                "{bad}"
            );
        }
    }
    for url in ["", "nope", "ftp://x/y"] {
        assert!(
            matches!(
                h.app.add_url(url, None).await,
                Err(AppError::InvalidArgument { .. })
            ),
            "{url}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn settings_updates_apply_to_the_engine_and_bad_ones_are_rejected() {
    let h = harness(FakeOrigin::new(payload(1500))).await;
    let new_folder = h.data.path().join("new-downloads");
    h.app
        .update_settings(sdm_app::protocol::SettingsDto {
            download_folder: Some(new_folder.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        h.app.get_settings().download_folder.as_deref(),
        Some(new_folder.to_string_lossy().as_ref())
    );

    let before = h.app.get_settings();
    let bad = h
        .app
        .update_settings(sdm_app::protocol::SettingsDto {
            download_folder: Some("".into()),
            ..Default::default()
        })
        .await;
    assert!(bad.is_err());
    assert_eq!(h.app.get_settings(), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_panic_inside_guarded_work_becomes_an_internal_error() {
    let h = harness(FakeOrigin::new(payload(10))).await;
    let result: Result<(), AppError> = h.app.guarded(|_| async { panic!("boom") }).await;
    assert!(matches!(result, Err(AppError::Internal { message }) if message.contains("boom")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn removing_an_item_with_open_details_stops_its_histories() {
    let h = harness(FakeOrigin::new(payload(200_000))).await;
    h.app
        .add_url("https://example.com/big.bin", None)
        .await
        .unwrap();
    for _ in 0..3 {
        h.app.tick().await.unwrap();
    }
    let id = h
        .sink
        .patches()
        .iter()
        .rev()
        .find_map(|p| p.structure.as_ref())
        .map(|s| s.packages[0].items[0].id.clone())
        .expect("an item exists");
    h.app
        .set_detail_items("w1", std::slice::from_ref(&id))
        .await
        .unwrap();

    h.app
        .remove_items(std::slice::from_ref(&id), false)
        .await
        .unwrap();
    h.sink.take();
    for _ in 0..3 {
        h.app.tick().await.unwrap();
    }
    assert!(
        h.sink.take().iter().all(|p| p.histories.is_empty()),
        "a removed item must not keep producing histories"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn package_folder_follows_the_download_folder_setting() {
    let h = harness(FakeOrigin::new(payload(2000))).await;
    h.app
        .add_url("https://example.com/a.bin", Some("Pack"))
        .await
        .unwrap();
    let first_folder = h
        .sink
        .patches()
        .iter()
        .rev()
        .find_map(|p| p.structure.as_ref())
        .map(|s| s.packages[0].folder.clone())
        .expect("a structure with the package");
    assert!(
        first_folder.starts_with(&*h.downloads.path().to_string_lossy()),
        "{first_folder}"
    );

    let elsewhere = tempfile::tempdir().unwrap();
    h.sink.take();
    h.app
        .update_settings(sdm_app::protocol::SettingsDto {
            download_folder: Some(elsewhere.path().to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await
        .unwrap();
    h.app.tick().await.unwrap();
    let moved = h
        .sink
        .patches()
        .iter()
        .rev()
        .find_map(|p| p.structure.as_ref())
        .map(|s| s.packages[0].folder.clone())
        .expect("changing the folder re-sends the structure");
    assert!(
        moved.starts_with(&*elsewhere.path().to_string_lossy()),
        "{moved}"
    );
}
