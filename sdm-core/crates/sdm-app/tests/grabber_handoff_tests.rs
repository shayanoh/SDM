mod support;

use sdm_app::AppError;
use sdm_app::protocol::{
    GrabberMediaRow, GrabberRow, HandoffTarget, ItemStateDto, MediaStateDto, StructureItem,
    StructurePackage,
};
use sdm_engine::FakeOrigin;
use sdm_grabber::FakeProbeOriginBehavior;
use support::*;

fn engine_packages(h: &Harness) -> Vec<StructurePackage> {
    h.sink
        .patches()
        .iter()
        .rev()
        .find_map(|p| p.structure.clone())
        .map(|s| s.packages)
        .unwrap_or_default()
}

fn engine_items(h: &Harness) -> Vec<StructureItem> {
    engine_packages(h)
        .into_iter()
        .flat_map(|p| p.items)
        .collect()
}

fn media_rows(h: &Harness) -> Vec<GrabberMediaRow> {
    grabber_view(h)
        .rows
        .into_iter()
        .filter_map(|r| match r {
            GrabberRow::Media(m) => Some(m),
            _ => None,
        })
        .collect()
}

fn row_count(h: &Harness) -> usize {
    grabber_view(h).rows.len()
}

async fn kit_with_links(urls: &[(&str, FakeProbeOriginBehavior)]) -> GrabberKit {
    let kit = grabber_kit(Some(standard_resolver()), FakeOrigin::new(payload(1000))).await;
    for (url, behavior) in urls {
        kit.origin.set_behavior(behavior.clone(), &su(url));
    }
    let text = urls.iter().map(|(u, _)| *u).collect::<Vec<_>>().join(" ");
    kit.harness.app.grabber_ingest_text(&text).await.unwrap();
    kit
}

fn offline() -> FakeProbeOriginBehavior {
    FakeProbeOriginBehavior {
        status_code: 404,
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_selection_hands_off_only_addable_links() {
    let kit = kit_with_links(&[
        ("https://a.example.com/good.zip", online()),
        ("https://b.example.com/gone.zip", offline()),
    ])
    .await;
    let h = &kit.harness;
    let all_ids: Vec<String> = grabber_view(h)
        .rows
        .iter()
        .map(|r| match r {
            GrabberRow::Link(l) => l.id.clone(),
            GrabberRow::Media(m) => m.id.clone(),
        })
        .collect();
    h.app
        .grabber_add_to_downloads(HandoffTarget::Rows { ids: all_ids }, false)
        .await
        .unwrap();
    let items = engine_items(h);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].filename, "good.zip");
    // The offline row stays; the handed-off one left.
    assert_eq!(row_count(h), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn add_leaves_items_stopped_and_add_and_start_queues_them() {
    let kit = kit_with_links(&[("https://a.example.com/one.zip", online())]).await;
    let h = &kit.harness;
    let package = grabber_view(h).packages[0].id.clone();
    h.app
        .grabber_add_to_downloads(HandoffTarget::Package { id: package }, false)
        .await
        .unwrap();
    assert_eq!(engine_items(h)[0].state, ItemStateDto::Stopped);

    kit.origin
        .set_behavior(online(), &su("https://a.example.com/two.zip"));
    h.app
        .grabber_ingest_text("https://a.example.com/two.zip")
        .await
        .unwrap();
    h.app
        .grabber_add_to_downloads(HandoffTarget::All, true)
        .await
        .unwrap();
    let two = engine_items(h)
        .into_iter()
        .find(|i| i.filename == "two.zip")
        .unwrap();
    assert_ne!(two.state, ItemStateDto::Stopped);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_resolved_media_row_is_handed_off_and_an_unselected_one_is_held_back() {
    let kit = kit_with_links(&[
        ("https://youtu.be/ok1", online()),
        ("https://youtu.be/big1", online()),
    ])
    .await;
    let h = &kit.harness;
    let rows = media_rows(h);
    assert_eq!(rows.len(), 2);
    let ok = rows.iter().find(|r| r.source_url.ends_with("ok1")).unwrap();
    let big = rows
        .iter()
        .find(|r| r.source_url.ends_with("big1"))
        .unwrap();
    assert_eq!(ok.state, MediaStateDto::Resolved);
    assert_eq!(big.state, MediaStateDto::Unselected);

    h.app
        .grabber_add_to_downloads(HandoffTarget::All, false)
        .await
        .unwrap();
    assert_eq!(engine_items(h).len(), 1);
    let view = grabber_view(h);
    assert_eq!(view.held_back, Some(1));
    let left = media_rows(h);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].state, MediaStateDto::Unselected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_held_back_message_clears_on_the_next_clean_hand_off() {
    let kit = kit_with_links(&[
        ("https://youtu.be/big1", online()),
        ("https://a.example.com/x.zip", online()),
    ])
    .await;
    let h = &kit.harness;
    h.app
        .grabber_add_to_downloads(HandoffTarget::All, false)
        .await
        .unwrap();
    assert_eq!(grabber_view(h).held_back, Some(1));
    // Choose a format for the waiting row; the next hand-off has nothing held back.
    let row = media_rows(h).remove(0);
    h.app
        .grabber_set_format(&row.id, &row.options[0].id)
        .await
        .unwrap();
    h.app
        .grabber_add_to_downloads(HandoffTarget::All, false)
        .await
        .unwrap();
    assert_eq!(grabber_view(h).held_back, None);
    assert_eq!(row_count(h), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn add_all_hands_off_every_package_without_losing_any() {
    let kit = kit_with_links(&[
        ("https://a.example.com/Show.S01E01.mkv", online()),
        ("https://a.example.com/Show.S01E02.mkv", online()),
        ("https://b.example.com/other.zip", online()),
        ("https://c.example.com/Another.Thing.zip", online()),
    ])
    .await;
    let h = &kit.harness;
    let before = grabber_view(h).packages.len();
    assert!(before >= 2);
    h.app
        .grabber_add_to_downloads(HandoffTarget::All, false)
        .await
        .unwrap();
    assert_eq!(engine_items(h).len(), 4);
    assert_eq!(engine_packages(h).len(), before);
    assert_eq!(row_count(h), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn standalone_selected_rows_go_under_their_own_filename() {
    // Handing a single selected link off names the package after the link
    // when it is in a package of its own.
    let kit = kit_with_links(&[("https://a.example.com/solo.zip", online())]).await;
    let h = &kit.harness;
    let id = match &grabber_view(h).rows[0] {
        GrabberRow::Link(l) => l.id.clone(),
        _ => panic!(),
    };
    h.app
        .grabber_add_to_downloads(HandoffTarget::Rows { ids: vec![id] }, false)
        .await
        .unwrap();
    assert_eq!(engine_items(h)[0].filename, "solo.zip");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_selection_with_nothing_addable_does_nothing() {
    let kit = kit_with_links(&[("https://b.example.com/gone.zip", offline())]).await;
    let h = &kit.harness;
    let id = match &grabber_view(h).rows[0] {
        GrabberRow::Link(l) => l.id.clone(),
        _ => panic!(),
    };
    h.app
        .grabber_add_to_downloads(HandoffTarget::Rows { ids: vec![id] }, true)
        .await
        .unwrap();
    assert!(engine_items(h).is_empty());
    assert_eq!(row_count(h), 1);
    // Bad ids are rejected, unknown package ids are a clear error.
    assert!(matches!(
        h.app
            .grabber_add_to_downloads(
                HandoffTarget::Rows {
                    ids: vec!["x".into()]
                },
                false
            )
            .await,
        Err(AppError::InvalidArgument { .. })
    ));
    assert!(
        h.app
            .grabber_add_to_downloads(
                HandoffTarget::Package {
                    id: uuid::Uuid::new_v4().to_string()
                },
                false
            )
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn set_format_applies_the_option_and_rejects_a_stale_one() {
    let kit = kit_with_links(&[("https://youtu.be/big1", online())]).await;
    let h = &kit.harness;
    let row = media_rows(h).remove(0);
    assert_eq!(row.state, MediaStateDto::Unselected);
    h.app
        .grabber_set_format(&row.id, &row.options[0].id)
        .await
        .unwrap();
    let after = media_rows(h).remove(0);
    assert_eq!(after.state, MediaStateDto::Resolved);
    assert!(after.options.iter().any(|o| o.selected));
    assert!(after.summary.is_some());

    let before = grabber_view(h);
    let stale = h.app.grabber_set_format(&row.id, "no-such-format").await;
    assert!(matches!(stale, Err(AppError::InvalidArgument { .. })));
    assert_eq!(grabber_view(h), before);
    assert!(
        h.app
            .grabber_set_format(&uuid::Uuid::new_v4().to_string(), "x")
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_resolve_shows_the_reason_and_is_not_addable() {
    let kit = kit_with_links(&[("https://youtu.be/missing", online())]).await;
    let h = &kit.harness;
    let row = media_rows(h).remove(0);
    assert_eq!(row.state, MediaStateDto::Failed);
    assert!(row.state_text.is_some());
    h.app
        .grabber_add_to_downloads(HandoffTarget::Rows { ids: vec![row.id] }, false)
        .await
        .unwrap();
    assert!(engine_items(h).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn copy_url_returns_the_final_url_or_the_source_page() {
    let kit = kit_with_links(&[
        ("https://a.example.com/x.zip", online()),
        ("https://youtu.be/ok1", online()),
    ])
    .await;
    let h = &kit.harness;
    for row in grabber_view(h).rows {
        match row {
            GrabberRow::Link(l) => {
                assert_eq!(h.app.grabber_copy_url(&l.id).unwrap(), l.final_url)
            }
            GrabberRow::Media(m) => {
                assert_eq!(h.app.grabber_copy_url(&m.id).unwrap(), m.source_url)
            }
        }
    }
    assert!(h.app.grabber_copy_url("junk").is_err());
    assert!(
        h.app
            .grabber_copy_url(&uuid::Uuid::new_v4().to_string())
            .is_err()
    );
}
