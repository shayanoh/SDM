//! Port of Swift's `URLRefreshTests.swift` (3 tests). `FakeResolver` lives in
//! `support/fake_resolver.rs`.
mod support;

use std::sync::Arc;

use sdm_core::{
    ComponentOrigin, DownloadItem, DownloadPackage, FileComponent, ItemState, RefreshedFormat,
    SourceUrl,
};
use sdm_engine::{DownloadEngine, FakeOriginBehavior, InMemoryStateStore, RetryPolicy};
use support::{
    FakeResolver, RoutingTransport, engine_settings, make_scratch_directory, snapshot_item,
    test_payload, within,
};

fn url(string: &str) -> SourceUrl {
    SourceUrl::new(string).unwrap()
}

fn resolved_item(url: SourceUrl, total_bytes: Option<i64>) -> DownloadItem {
    let mut component =
        FileComponent::new(url, "clip.f137.mp4").with_origin(ComponentOrigin::Resolved {
            format_id: "137".to_string(),
        });
    component.total_bytes = total_bytes;
    DownloadItem::new(vec![component], "clip.mp4")
}

fn stale_behavior() -> FakeOriginBehavior {
    FakeOriginBehavior {
        status_override: Some(403),
        ..Default::default()
    }
}

fn engine_with(
    router: RoutingTransport,
    dir: &std::path::Path,
    resolver: Option<Arc<FakeResolver>>,
) -> DownloadEngine {
    DownloadEngine::new(
        Arc::new(router),
        Arc::new(InMemoryStateStore::new()),
        engine_settings(1, 2, 8, dir),
        RetryPolicy::new(),
        resolver.map(|resolver| resolver as Arc<dyn sdm_core::LinkResolver>),
        None,
        None,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn expired_component_url_is_refreshed_and_the_download_finishes() {
    let dir = make_scratch_directory();
    let payload = test_payload(8000);
    let router = RoutingTransport::new(
        "stale.example",
        payload.clone(),
        stale_behavior(),
        "fresh.example",
        payload.clone(),
        FakeOriginBehavior::default(),
    );
    let resolver = Arc::new(FakeResolver::new(|format_id| {
        Ok(RefreshedFormat::new(
            url("https://fresh.example/f137"),
            None,
            format_id,
        ))
    }));
    let engine = engine_with(router, dir.path(), Some(resolver));
    let item = resolved_item(url("https://stale.example/f137"), None);
    let id = item.id;
    engine
        .add(DownloadPackage::new("P").with_items(vec![item]))
        .await;
    within(engine.run_until_idle()).await.unwrap();

    let snapshot = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(snapshot.state, ItemState::Completed);
    assert_eq!(
        std::fs::read(dir.path().join("P/clip.mp4")).unwrap(),
        payload
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn refresh_returning_a_changed_size_fails_the_item() {
    let dir = make_scratch_directory();
    let router = RoutingTransport::new(
        "stale.example",
        test_payload(8000),
        stale_behavior(),
        "fresh.example",
        test_payload(8000),
        FakeOriginBehavior::default(),
    );
    let resolver = Arc::new(FakeResolver::new(|format_id| {
        Ok(RefreshedFormat::new(
            url("https://fresh.example/f137"),
            Some(9999),
            format_id,
        ))
    }));
    let engine = engine_with(router, dir.path(), Some(resolver));
    let item = resolved_item(url("https://stale.example/f137"), Some(8000));
    let id = item.id;
    engine
        .add(DownloadPackage::new("P").with_items(vec![item]))
        .await;
    within(engine.run_until_idle()).await.unwrap();

    let snapshot = snapshot_item(id, &engine).await.unwrap();
    let ItemState::Failed { reason } = snapshot.state else {
        panic!("expected Failed, got {:?}", snapshot.state);
    };
    assert!(reason.contains("size"), "{reason}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn expired_component_with_no_resolver_fails_rather_than_looping() {
    let dir = make_scratch_directory();
    let router = RoutingTransport::new(
        "stale.example",
        test_payload(8000),
        stale_behavior(),
        "fresh.example",
        test_payload(8000),
        FakeOriginBehavior::default(),
    );
    let engine = engine_with(router, dir.path(), None);
    let item = resolved_item(url("https://stale.example/f137"), None);
    let id = item.id;
    engine
        .add(DownloadPackage::new("P").with_items(vec![item]))
        .await;
    // No resolver: UrlExpired -> ServerError(403) -> transient backoff, held
    // out of the desired set. `run_until_idle` returns without looping.
    within(engine.run_until_idle()).await.unwrap();
    let snapshot = snapshot_item(id, &engine).await.unwrap();
    assert_ne!(snapshot.state, ItemState::Completed);
}

// MARK: - Rust-only: a refresh that outlives its run installs nothing

/// A resolver that parks inside `refresh` until `release()`, and optionally
/// ignores the cancellation token (a resolver that does not notice a Stop).
struct ParkingResolver {
    arrived: tokio::sync::Notify,
    has_arrived: std::sync::atomic::AtomicBool,
    released: tokio::sync::Notify,
    is_released: std::sync::atomic::AtomicBool,
    honours_cancel: bool,
}

impl ParkingResolver {
    fn new(honours_cancel: bool) -> Self {
        Self {
            arrived: tokio::sync::Notify::new(),
            has_arrived: std::sync::atomic::AtomicBool::new(false),
            released: tokio::sync::Notify::new(),
            is_released: std::sync::atomic::AtomicBool::new(false),
            honours_cancel,
        }
    }

    async fn wait_until_arrived(&self) {
        within(async {
            loop {
                let notified = self.arrived.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self.has_arrived.load(std::sync::atomic::Ordering::SeqCst) {
                    return;
                }
                notified.await;
            }
        })
        .await;
    }

    fn release(&self) {
        self.is_released
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.released.notify_waiters();
    }

    async fn wait_until_released(&self) {
        loop {
            let notified = self.released.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_released.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            notified.await;
        }
    }
}

#[async_trait::async_trait]
impl sdm_core::LinkResolver for ParkingResolver {
    fn can_handle(&self, _url: &SourceUrl) -> bool {
        false
    }

    async fn resolve(
        &self,
        _url: &SourceUrl,
    ) -> Result<sdm_core::ResolvedTarget, sdm_core::ResolveError> {
        Err(sdm_core::ResolveError::Unsupported)
    }

    async fn refresh(
        &self,
        _source_url: &SourceUrl,
        format_id: &str,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<RefreshedFormat, sdm_core::ResolveError> {
        self.has_arrived
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.arrived.notify_waiters();
        if self.honours_cancel {
            tokio::select! {
                () = self.wait_until_released() => {}
                () = cancel.cancelled() => return Err(sdm_core::ResolveError::Cancelled),
            }
        } else {
            self.wait_until_released().await;
        }
        Ok(RefreshedFormat::new(
            url("https://fresh.example/f137"),
            None,
            format_id,
        ))
    }
}

/// Stops an item while its refresh is in flight and lets the refresh return
/// afterwards; the stale component URL must stay, the item must land stopped,
/// and no attempt may be charged as a failure.
async fn stop_during_refresh(honours_cancel: bool) {
    let dir = make_scratch_directory();
    let payload = test_payload(8000);
    let router = RoutingTransport::new(
        "stale.example",
        payload.clone(),
        stale_behavior(),
        "fresh.example",
        payload,
        FakeOriginBehavior::default(),
    );
    let resolver = Arc::new(ParkingResolver::new(honours_cancel));
    let store = Arc::new(InMemoryStateStore::new());
    let engine = DownloadEngine::new(
        Arc::new(router),
        store.clone(),
        engine_settings(1, 2, 8, dir.path()),
        RetryPolicy::new(),
        Some(resolver.clone() as Arc<dyn sdm_core::LinkResolver>),
        None,
        None,
    );
    let item = resolved_item(url("https://stale.example/f137"), None);
    let id = item.id;
    engine
        .add(DownloadPackage::new("P").with_items(vec![item]))
        .await;

    resolver.wait_until_arrived().await;
    engine.stop_item(id).await;
    resolver.release();
    within(engine.run_until_idle()).await.unwrap();

    let snapshot = snapshot_item(id, &engine).await.unwrap();
    assert_eq!(snapshot.state, ItemState::Stopped);
    assert!(!dir.path().join("P/clip.mp4").exists());
    let saved = sdm_engine::StateStore::load(&*store).await;
    assert_eq!(
        saved.packages[0].items[0].components[0].url.as_str(),
        "https://stale.example/f137"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stopping_an_item_during_its_refresh_ends_the_refresh_and_installs_nothing() {
    stop_during_refresh(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refresh_that_returns_after_a_stop_installs_no_new_task() {
    stop_during_refresh(false).await;
}
