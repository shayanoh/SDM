use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use sdm_app::host::{FakeHost, RecordingSink};
use sdm_app::{App, Dependencies};
use sdm_engine::{FakeOrigin, InMemoryStateStore, RetryPolicy, Trash};

/// Moves "trashed" files into a scratch folder, so no test reaches the real
/// Trash (the engine's trash-guard test forbids naming the system one).
struct ScratchTrash {
    directory: tempfile::TempDir,
    counter: AtomicUsize,
}

impl Trash for ScratchTrash {
    fn trash(&self, path: &Path) -> std::io::Result<()> {
        let slot = self.directory.path().join(format!(
            "{}-{}",
            self.counter.fetch_add(1, Ordering::SeqCst),
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        std::fs::rename(path, slot)
    }
}

// Each test binary uses a different subset of the harness.
#[allow(dead_code)]
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
    harness_with(origin, None).await
}

pub async fn harness_with(
    origin: FakeOrigin,
    grabber: Option<Arc<sdm_app::grabber::GrabberController>>,
) -> Harness {
    let downloads = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let host = Arc::new(FakeHost::new(
        data.path().to_path_buf(),
        downloads.path().to_path_buf(),
    ));
    let dependencies = Dependencies {
        transport: Arc::new(origin),
        state_store: Arc::new(InMemoryStateStore::new()),
        trash: Arc::new(ScratchTrash {
            directory: tempfile::tempdir().unwrap(),
            counter: AtomicUsize::new(0),
        }),
        resolver: None,
        muxer: None,
        wholesale_downloader: None,
        retry_policy: RetryPolicy::new(),
        binaries: None,
        grabber,
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

/// A grabber over a fake probe origin and an optional resolver, with a clock
/// the test moves by hand.
#[allow(dead_code)]
pub struct GrabberKit {
    pub harness: Harness,
    pub origin: Arc<sdm_grabber::FakeProbeOrigin>,
    pub now: Arc<std::sync::Mutex<std::time::SystemTime>>,
}

#[allow(dead_code)]
pub async fn grabber_kit(
    resolver: Option<Arc<dyn sdm_core::LinkResolver>>,
    origin_for_downloads: FakeOrigin,
) -> GrabberKit {
    let origin = Arc::new(sdm_grabber::FakeProbeOrigin::new());
    let now = Arc::new(std::sync::Mutex::new(std::time::SystemTime::UNIX_EPOCH));
    let clock_now = now.clone();
    let clock: sdm_grabber::Clock = Arc::new(move || *clock_now.lock().unwrap());
    let grabber = sdm_app::grabber::GrabberController::new(
        origin.clone(),
        resolver,
        Arc::new(std::sync::atomic::AtomicBool::new(true)),
        Some(clock),
    );
    let harness = harness_with(origin_for_downloads, Some(grabber)).await;
    GrabberKit {
        harness,
        origin,
        now,
    }
}

/// The newest grabber view the sink received.
#[allow(dead_code)]
pub fn grabber_view(h: &Harness) -> sdm_app::protocol::GrabberView {
    h.sink
        .patches()
        .iter()
        .rev()
        .find_map(|p| p.grabber.clone())
        .expect("a patch carrying the grabber view")
}

/// An origin answer that probes as `online`.
#[allow(dead_code)]
pub fn online() -> sdm_grabber::FakeProbeOriginBehavior {
    sdm_grabber::FakeProbeOriginBehavior {
        status_code: 200,
        headers: [
            ("content-type".to_owned(), "video/mp4".to_owned()),
            ("content-length".to_owned(), "5000000".to_owned()),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    }
}

#[allow(dead_code)]
pub fn su(text: &str) -> sdm_core::SourceUrl {
    sdm_core::SourceUrl::new(text).expect("test URL parses")
}
