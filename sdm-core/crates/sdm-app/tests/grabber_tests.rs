mod support;

use std::time::Duration;

use sdm_app::AppError;
use sdm_app::protocol::{GrabberRow, VerdictDto};
use sdm_engine::FakeOrigin;
use sdm_grabber::FakeProbeOriginBehavior;
use support::*;

fn link_ids(h: &Harness) -> Vec<String> {
    grabber_view(h)
        .rows
        .iter()
        .filter_map(|r| match r {
            GrabberRow::Link(l) => Some(l.id.clone()),
            _ => None,
        })
        .collect()
}

fn link_verdicts(h: &Harness) -> Vec<Option<VerdictDto>> {
    grabber_view(h)
        .rows
        .iter()
        .filter_map(|r| match r {
            GrabberRow::Link(l) => Some(l.verdict.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ingest_text_adds_rows_that_probe_to_online() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    kit.origin
        .set_behavior(online(), &su("https://a.example.com/movie.mp4"));
    let app = &kit.harness.app;
    app.grabber_ingest_text("watch https://a.example.com/movie.mp4 now")
        .await
        .unwrap();
    let view = grabber_view(&kit.harness);
    assert_eq!(view.total_count, 1);
    assert_eq!(view.checked_count, 1);
    assert_eq!(link_verdicts(&kit.harness), vec![Some(VerdictDto::Online)]);
    assert_eq!(view.summary.online, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_blob_without_links_adds_nothing_and_does_not_error() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    kit.harness.sink.take();
    kit.harness
        .app
        .grabber_ingest_text("nothing to see here, no links at all")
        .await
        .unwrap();
    kit.harness.app.grabber_ingest_text("").await.unwrap();
    assert!(kit.harness.sink.take().iter().all(|p| p.grabber.is_none()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_same_link_twice_is_one_row() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    for _ in 0..2 {
        kit.harness
            .app
            .grabber_ingest_text("https://a.example.com/x.zip")
            .await
            .unwrap();
    }
    assert_eq!(grabber_view(&kit.harness).total_count, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_idle_grabber_sends_no_grabber_field() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    kit.harness.sink.take();
    for _ in 0..5 {
        kit.harness.app.tick().await.unwrap();
    }
    assert!(kit.harness.sink.take().iter().all(|p| p.grabber.is_none()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_changed_row_sends_the_view_once() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    kit.harness.sink.take();
    kit.harness
        .app
        .grabber_ingest_text("https://a.example.com/x.zip")
        .await
        .unwrap();
    for _ in 0..3 {
        kit.harness.app.tick().await.unwrap();
    }
    let with_view = kit
        .harness
        .sink
        .take()
        .into_iter()
        .filter(|p| p.grabber.is_some())
        .count();
    assert!(with_view >= 1);
    for _ in 0..3 {
        kit.harness.app.tick().await.unwrap();
    }
    assert!(kit.harness.sink.take().iter().all(|p| p.grabber.is_none()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_full_patch_always_carries_the_view() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    let fresh = std::sync::Arc::new(sdm_app::host::RecordingSink::default());
    kit.harness.app.attach(fresh.clone()).await.unwrap();
    let first = fresh.patches().into_iter().next().unwrap();
    assert!(first.full);
    assert!(first.grabber.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_malformed_id_errors_and_changes_nothing() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    kit.harness
        .app
        .grabber_ingest_text("https://a.example.com/x.zip")
        .await
        .unwrap();
    let app = &kit.harness.app;
    let bad = vec!["not-an-id".to_owned()];
    assert!(matches!(
        app.grabber_remove_links(&bad).await,
        Err(AppError::InvalidArgument { .. })
    ));
    assert!(app.grabber_remove_package("nope").await.is_err());
    assert!(app.grabber_recheck(&bad).await.is_err());
    assert!(app.grabber_split_package("").await.is_err());
    assert!(app.grabber_move_link("x", "y").await.is_err());
    assert_eq!(grabber_view(&kit.harness).total_count, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_empty_id_list_is_a_no_op() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    kit.harness
        .app
        .grabber_ingest_text("https://a.example.com/x.zip")
        .await
        .unwrap();
    kit.harness.app.grabber_remove_links(&[]).await.unwrap();
    kit.harness.app.grabber_recheck(&[]).await.unwrap();
    assert_eq!(grabber_view(&kit.harness).total_count, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remove_links_and_clear_work() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    let app = &kit.harness.app;
    app.grabber_ingest_text("https://a.example.com/1.zip https://b.example.com/2.zip")
        .await
        .unwrap();
    let ids = link_ids(&kit.harness);
    assert_eq!(ids.len(), 2);
    app.grabber_remove_links(&ids[..1]).await.unwrap();
    assert_eq!(link_ids(&kit.harness), vec![ids[1].clone()]);
    app.grabber_clear().await.unwrap();
    assert_eq!(grabber_view(&kit.harness).total_count, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rename_move_merge_split_and_remove_a_package() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    let app = &kit.harness.app;
    app.grabber_ingest_text(
        "https://a.example.com/Show.S01E01.mkv https://a.example.com/Show.S01E02.mkv \
         https://b.example.com/other.zip",
    )
    .await
    .unwrap();
    let view = grabber_view(&kit.harness);
    assert!(view.packages.len() >= 2, "{:?}", view.packages);
    let first = view.packages[0].clone();

    app.grabber_rename_package(&first.id, "  Renamed  ")
        .await
        .unwrap();
    assert!(
        grabber_view(&kit.harness)
            .packages
            .iter()
            .any(|p| p.name == "Renamed")
    );
    assert!(matches!(
        app.grabber_rename_package(&first.id, "   ").await,
        Err(AppError::InvalidArgument { .. })
    ));

    // Package ids are made fresh by every recluster, so read them again.
    let view = grabber_view(&kit.harness);
    let first = view
        .packages
        .iter()
        .find(|p| p.name == "Renamed")
        .unwrap()
        .clone();
    let second = view
        .packages
        .iter()
        .find(|p| p.name != "Renamed")
        .unwrap()
        .clone();
    app.grabber_move_link(&second.row_ids[0], &first.id)
        .await
        .unwrap();
    let moved = grabber_view(&kit.harness);
    let target = moved.packages.iter().find(|p| p.name == "Renamed").unwrap();
    assert!(target.row_ids.contains(&second.row_ids[0]));

    let target_id = target.id.clone();
    app.grabber_split_package(&target_id).await.unwrap();
    let after_split = grabber_view(&kit.harness);
    assert!(after_split.packages.len() >= moved.packages.len());

    let before = grabber_view(&kit.harness).packages;
    if before.len() >= 2 {
        app.grabber_merge_packages(&before[0].id, &before[1].id)
            .await
            .unwrap();
        assert_eq!(grabber_view(&kit.harness).packages.len(), before.len() - 1);
    }
    let remaining = grabber_view(&kit.harness).packages;
    app.grabber_remove_package(&remaining[0].id).await.unwrap();
    assert_eq!(
        grabber_view(&kit.harness).packages.len(),
        remaining.len() - 1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recheck_failed_retries_check_failed_rows() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    let url = su("https://a.example.com/x.zip");
    kit.origin.set_behavior(
        FakeProbeOriginBehavior {
            error: Some(sdm_grabber::ProbeError::MalformedResponse),
            ..Default::default()
        },
        &url,
    );
    let app = &kit.harness.app;
    app.grabber_ingest_text(url.as_str()).await.unwrap();
    assert_eq!(
        link_verdicts(&kit.harness),
        vec![Some(VerdictDto::CheckFailed)]
    );
    assert_eq!(grabber_view(&kit.harness).summary.recheckable, 1);

    kit.origin.set_behavior(online(), &url);
    app.grabber_recheck_failed().await.unwrap();
    assert_eq!(link_verdicts(&kit.harness), vec![Some(VerdictDto::Online)]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recheck_retries_exactly_the_given_rows() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    let url = su("https://a.example.com/x.zip");
    kit.origin.set_behavior(online(), &url);
    let app = &kit.harness.app;
    app.grabber_ingest_text(url.as_str()).await.unwrap();
    let ids = link_ids(&kit.harness);
    kit.origin.set_behavior(
        FakeProbeOriginBehavior {
            status_code: 404,
            ..Default::default()
        },
        &url,
    );
    app.grabber_recheck(&ids).await.unwrap();
    assert_eq!(link_verdicts(&kit.harness), vec![Some(VerdictDto::Offline)]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_checks_settles_the_waiting_rows_as_failed() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    let held = FakeProbeOriginBehavior {
        holds_until_released: true,
        ..online()
    };
    // Twelve links on one host: eight probe at once (the per-host cap), four wait.
    let urls: Vec<_> = (0..12)
        .map(|i| su(&format!("https://one.example.com/file{i}.zip")))
        .collect();
    for url in &urls {
        kit.origin.set_behavior(held.clone(), url);
    }
    let text = urls
        .iter()
        .map(|u| u.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let app = kit.harness.app.clone();
    let ingest = tokio::spawn(async move { app.grabber_ingest_text(&text).await });
    // Let the eight in-flight probes start.
    tokio::time::sleep(Duration::from_millis(200)).await;
    kit.harness.app.grabber_cancel_checks().await.unwrap();
    for url in &urls {
        kit.origin.release(url);
    }
    ingest.await.unwrap().unwrap();
    let verdicts = link_verdicts(&kit.harness);
    assert_eq!(verdicts.len(), 12);
    assert!(
        verdicts.contains(&Some(VerdictDto::CheckFailed)),
        "{verdicts:?}"
    );
    assert!(!grabber_view(&kit.harness).summary.is_checking);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn known_downloads_mark_duplicates() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    let url = "https://a.example.com/movie.mp4";
    kit.origin.set_behavior(online(), &su(url));
    let app = &kit.harness.app;
    app.grabber_ingest_text(url).await.unwrap();
    app.add_url(url, None).await.unwrap();
    app.tick().await.unwrap();
    app.tick().await.unwrap();
    let view = grabber_view(&kit.harness);
    assert!(matches!(&view.rows[0], GrabberRow::Link(l) if l.is_duplicate));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn prune_removes_rows_older_than_thirty_minutes() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    let app = &kit.harness.app;
    app.grabber_ingest_text("https://a.example.com/old.zip")
        .await
        .unwrap();
    *kit.now.lock().unwrap() += Duration::from_secs(31 * 60);
    app.grabber_ingest_text("https://a.example.com/new.zip")
        .await
        .unwrap();
    assert_eq!(grabber_view(&kit.harness).total_count, 2);
    // The prune runs once a minute: 60 seconds of ticks at the engine rate.
    let per_minute = sdm_core::AppTiming::TICKS_PER_SECOND * 60;
    let h = &kit.harness;
    assert!(
        tick_until(app, per_minute + 5, || grabber_view(h).total_count == 1).await,
        "the old row should have been pruned"
    );
    let GrabberRow::Link(left) = &grabber_view(h).rows[0] else {
        panic!()
    };
    assert!(left.filename.contains("new"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn thousands_of_links_ingest_without_blocking() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(1000))).await;
    let text = (0..2000)
        .map(|i| format!("https://h{}.example.com/file{i}.zip", i % 50))
        .collect::<Vec<_>>()
        .join("\n");
    tokio::time::timeout(
        Duration::from_secs(60),
        kit.harness.app.grabber_ingest_text(&text),
    )
    .await
    .expect("ingest finished")
    .unwrap();
    assert_eq!(grabber_view(&kit.harness).total_count, 2000);
    kit.harness.app.tick().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_a_grabber_every_action_is_a_clear_error() {
    let h = harness(FakeOrigin::new(payload(1000))).await;
    assert!(matches!(
        h.app.grabber_ingest_text("https://a.example.com/x").await,
        Err(AppError::Failed { .. })
    ));
    assert!(h.app.grabber_clear().await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_tool_download_shows_an_error_notice_in_the_view() {
    use sdm_resolve::testing::{FakeBinaryFetcher, FakeProcessRunner};
    use sdm_resolve::{
        ArchiveKind, CheckReason, ManagedBinaries, PlatformTool, ToolMember, UpdatePolicy,
        VersionSource, YtDlpChannel,
    };

    let tool = PlatformTool {
        name: "qjs".into(),
        release_api: "https://api.example/qjs/releases/latest".into(),
        asset_name: "qjs-linux".into(),
        archive: ArchiveKind::Raw,
        members: vec![ToolMember::new("", "qjs")],
        policy: UpdatePolicy::WhenVersionChanges,
        version_source: VersionSource::Tag,
    };
    let fetcher = std::sync::Arc::new(FakeBinaryFetcher::new());
    // The release claims one checksum; the bytes it serves are another.
    fetcher.set_response(
        Ok(format!(
            "{{\"tag_name\":\"v1\",\"published_at\":null,\"assets\":[{{\"name\":\"qjs-linux\",\"browser_download_url\":\"https://dl.example/qjs\",\"digest\":\"sha256:{}\"}}]}}",
            "0".repeat(64)
        )
        .into_bytes()),
        &url::Url::parse("https://api.example/qjs/releases/latest").unwrap(),
    );
    fetcher.set_response(
        Ok(b"TAMPERED".to_vec()),
        &url::Url::parse("https://dl.example/qjs").unwrap(),
    );
    let bin = tempfile::tempdir().unwrap();
    let binaries = ManagedBinaries::new(
        bin.path().join("bin"),
        fetcher,
        std::sync::Arc::new(FakeProcessRunner::new()),
        Vec::new,
        || YtDlpChannel::Stable,
    )
    .with_platform_tools(vec![tool]);
    let origin = std::sync::Arc::new(sdm_grabber::FakeProbeOrigin::new());
    let grabber = sdm_app::grabber::GrabberController::new(
        origin,
        None,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        None,
    );
    let h = harness_full(
        FakeOrigin::new(payload(10)),
        Some(grabber),
        Some(binaries.clone()),
    )
    .await;
    binaries.check_now(CheckReason::Launch).await;
    h.app
        .grabber_ingest_text("https://a.example.com/x.zip")
        .await
        .unwrap();
    let notice = grabber_view(&h).tools.expect("a tools notice");
    assert!(notice.is_error);
    assert!(
        notice.text.starts_with("Could not fetch qjs"),
        "{}",
        notice.text
    );
}

/// Not a pass/fail gate: prints what one full-view patch costs for 2,000 links
/// (`cargo test -p sdm-app --test grabber_tests view_cost -- --ignored --nocapture`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "a measurement, not a test"]
async fn view_cost_for_two_thousand_links() {
    let kit = grabber_kit(None, FakeOrigin::new(payload(10))).await;
    for i in 0..2000 {
        kit.origin.set_behavior(
            online(),
            &su(&format!("https://h{}.example.com/file{i}.zip", i % 50)),
        );
    }
    let text = (0..2000)
        .map(|i| format!("https://h{}.example.com/file{i}.zip", i % 50))
        .collect::<Vec<_>>()
        .join("\n");
    kit.harness.app.grabber_ingest_text(&text).await.unwrap();
    let view = grabber_view(&kit.harness);
    let started = std::time::Instant::now();
    let rounds = 20;
    let mut bytes = 0;
    for _ in 0..rounds {
        // What the channel does with a patch: serialize it.
        bytes = serde_json::to_vec(&view_as_json(&view)).unwrap().len();
    }
    println!(
        "2000 links: {} KB per view, {:?} per serialization",
        bytes / 1024,
        started.elapsed() / rounds
    );
}

fn view_as_json(view: &sdm_app::protocol::GrabberView) -> serde_json::Value {
    // `GrabberView` only derives `Serialize` for the wire; a round trip through
    // `serde_json::to_value` is what the Tauri channel does.
    serde_json::to_value(view).unwrap()
}
