use std::sync::Arc;

use sdm_app::host::{FakeHost, RecordingSink};
use sdm_app::{App, Dependencies};
use sdm_engine::{FakeOrigin, InMemoryStateStore, RetryPolicy, SystemTrash};

pub struct Harness {
    pub app: Arc<App>,
    pub sink: Arc<RecordingSink>,
    pub host: Arc<FakeHost>,
    pub downloads: tempfile::TempDir,
    pub data: tempfile::TempDir,
}

pub fn payload(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i.wrapping_mul(31).wrapping_add(7) % 251) as u8)
        .collect()
}

pub async fn harness(origin: FakeOrigin) -> Harness {
    let downloads = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let host = Arc::new(FakeHost::new(
        data.path().to_path_buf(),
        downloads.path().to_path_buf(),
    ));
    let dependencies = Dependencies {
        transport: Arc::new(origin),
        state_store: Arc::new(InMemoryStateStore::new()),
        trash: Arc::new(SystemTrash),
        resolver: None,
        muxer: None,
        wholesale_downloader: None,
        retry_policy: RetryPolicy::new(),
        binaries: None,
    };
    let app = App::new(host.clone(), dependencies).expect("app");
    let sink = Arc::new(RecordingSink::default());
    app.attach(sink.clone()).await.expect("attach");
    Harness {
        app,
        sink,
        host,
        downloads,
        data,
    }
}

/// Ticks until `condition` holds or `max_ticks` passed. There is no clock:
/// each iteration is one engine tick plus a yield to let download tasks run.
pub async fn tick_until(app: &App, max_ticks: usize, mut condition: impl FnMut() -> bool) -> bool {
    for _ in 0..max_ticks {
        app.tick().await.unwrap();
        if condition() {
            return true;
        }
        tokio::task::yield_now().await;
    }
    false
}
