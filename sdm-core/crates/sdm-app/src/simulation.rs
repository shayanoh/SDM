//! A load harness: an app whose "network" is a slow in-process origin, so a UI
//! can be measured with hundreds of items and dozens of live transfers and no
//! real server. Wired to `--simulate <items> <active>` in debug builds and in
//! builds with the shell's `simulate` feature.
//!
//! The origin paces its bodies with a real-time interval. That is acceptable
//! here because no test asserts timing against it (the tests only count items).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use sdm_core::{DownloadItem, DownloadPackage, SourceUrl};
use sdm_engine::{
    BodyStream, HttpTransport, InMemoryStateStore, RangeRequest, RangeResponse, RetryPolicy,
    TransportError, Trash,
};

use crate::app::{App, Dependencies};
use crate::error::AppError;
use crate::grabber::GrabberController;
use crate::host::{Host, HostError};
use crate::notify::Notification;

/// Largest simulation accepted: more items than any real list.
const MAX_ITEMS: usize = 5000;
/// Items per package; 500 items become ten packages.
const ITEMS_PER_PACKAGE: usize = 50;
/// Size each simulated file claims to have (it lasts several minutes).
const FILE_BYTES: i64 = 64 * 1024 * 1024;
/// Bytes a body stream emits per tick. With four segments an item moves
/// about 160 KiB/s, so fifty live items write only a few MB/s to disk.
const CHUNK: usize = 8 * 1024;
const TICK: Duration = Duration::from_millis(200);

/// Serves `FILE_BYTES` of zeros for any URL, a slow trickle per request.
struct SlowOrigin {
    block: Bytes,
}

#[async_trait]
impl HttpTransport for SlowOrigin {
    async fn fetch(&self, request: RangeRequest) -> Result<RangeResponse, TransportError> {
        let (start, end, status) = match request.range {
            Some(range) => (
                range.start.clamp(0, FILE_BYTES),
                range.end.clamp(0, FILE_BYTES),
                206,
            ),
            None => (0, FILE_BYTES, 200),
        };
        let remaining = usize::try_from((end - start).max(0)).unwrap_or(0);
        let block = self.block.clone();
        let interval = tokio::time::interval(TICK);
        let body: BodyStream = Box::pin(futures::stream::unfold(
            (remaining, interval),
            move |(remaining, mut interval)| {
                let block = block.clone();
                async move {
                    if remaining == 0 {
                        return None;
                    }
                    interval.tick().await;
                    let size = remaining.min(block.len());
                    Some((Ok(block.slice(..size)), (remaining - size, interval)))
                }
            },
        ));
        Ok(RangeResponse {
            status_code: status,
            total_size: Some(FILE_BYTES),
            accepts_ranges: true,
            validator: Some("simulated".to_owned()),
            resolved_url: request.url,
            body,
        })
    }
}

/// Answers every grabber probe as a reachable 5 MB file, after a short pause,
/// so the grabber view can be exercised with no network.
struct SimulatedProbe;

#[async_trait]
impl sdm_grabber::ProbeTransport for SimulatedProbe {
    async fn send(
        &self,
        request: sdm_grabber::ProbeRequest,
    ) -> Result<sdm_grabber::ProbeResponse, sdm_grabber::ProbeError> {
        tokio::time::sleep(Duration::from_millis(150)).await;
        Ok(sdm_grabber::ProbeResponse {
            status_code: 200,
            final_url: request.url,
            headers: [
                ("content-type".to_owned(), "application/zip".to_owned()),
                ("content-length".to_owned(), "5000000".to_owned()),
            ]
            .into_iter()
            .collect(),
            body: Vec::new(),
        })
    }
}

/// Never touches the user's Trash.
struct NoTrash;

impl Trash for NoTrash {
    fn trash(&self, _path: &Path) -> std::io::Result<()> {
        Ok(())
    }
}

/// Keeps the simulation's data and downloads under one scratch folder, so a
/// run never reads or writes the user's real data directory.
pub struct SimulationHost {
    root: PathBuf,
}

impl SimulationHost {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl Host for SimulationHost {
    fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }
    fn default_download_dir(&self) -> PathBuf {
        self.root.join("downloads")
    }
    fn reveal_in_file_manager(&self, _path: &Path) -> Result<(), HostError> {
        Ok(())
    }
    fn open_path(&self, _path: &Path) -> Result<(), HostError> {
        Ok(())
    }
    fn notify(&self, _notification: Notification) -> Result<(), HostError> {
        Ok(())
    }
}

/// The engine's collaborators for a simulation: the slow origin, a state store
/// that is never written to disk, and no resolver, muxer or binaries.
pub fn simulated_dependencies(_host: &dyn Host, _items: usize, _active: usize) -> Dependencies {
    Dependencies {
        transport: Arc::new(SlowOrigin {
            block: Bytes::from(vec![0u8; CHUNK]),
        }),
        state_store: Arc::new(InMemoryStateStore::new()),
        trash: Arc::new(NoTrash),
        resolver: None,
        muxer: None,
        wholesale_downloader: None,
        retry_policy: RetryPolicy::new(),
        binaries: None,
        grabber: Some(GrabberController::new(
            Arc::new(SimulatedProbe),
            None,
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            None,
        )),
    }
}

fn check_counts(items: usize, active: usize) -> Result<(), AppError> {
    if items == 0 || active == 0 || active > items || items > MAX_ITEMS {
        return Err(AppError::invalid(
            "simulate",
            format!(
                "need 1 <= active <= items <= {MAX_ITEMS}, got {items} items and {active} active"
            ),
        ));
    }
    Ok(())
}

/// Adds `items` simulated downloads in packages of fifty. The app must have
/// been built with `App::new_with_concurrency(.., active)` so only `active`
/// of them run at once.
pub async fn populate(app: &App, items: usize, active: usize) -> Result<(), AppError> {
    check_counts(items, active)?;
    let numbers: Vec<usize> = (0..items).collect();
    for (index, group) in numbers.chunks(ITEMS_PER_PACKAGE).enumerate() {
        let mut children = Vec::with_capacity(group.len());
        for number in group {
            let name = format!("file-{number}.bin");
            let url = SourceUrl::new(format!("https://sim.invalid/{name}"))
                .map_err(|_| AppError::failed("could not build a simulated address"))?;
            children.push(DownloadItem::http(url, name));
        }
        let package = DownloadPackage::new(format!("Simulated {}", index + 1)).with_items(children);
        app.add_package(package).await?;
    }
    Ok(())
}

/// `Some((items, active))` when `args` holds `--simulate <items> <active>`,
/// `None` when the flag is absent, an error when it is malformed.
pub fn parse_simulate_args(args: &[String]) -> Result<Option<(usize, usize)>, AppError> {
    let Some(position) = args.iter().position(|arg| arg == "--simulate") else {
        return Ok(None);
    };
    let number = |offset: usize, what: &str| -> Result<usize, AppError> {
        args.get(position + offset)
            .and_then(|text| text.parse::<usize>().ok())
            .ok_or_else(|| {
                AppError::invalid(
                    "simulate",
                    format!("--simulate needs the {what} as a number"),
                )
            })
    };
    let items = number(1, "item count")?;
    let active = number(2, "active count")?;
    check_counts(items, active)?;
    Ok(Some((items, active)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{FakeHost, RecordingSink};
    use std::sync::Arc;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_simulation_loads_the_requested_items_with_the_requested_active_count() {
        let data = tempfile::tempdir().unwrap();
        let downloads = tempfile::tempdir().unwrap();
        let host = Arc::new(FakeHost::new(data.path().into(), downloads.path().into()));
        let app = crate::App::new_with_concurrency(
            host.clone(),
            simulated_dependencies(host.as_ref(), 40, 5),
            5,
        )
        .unwrap();
        populate(&app, 40, 5).await.unwrap();
        let sink = Arc::new(RecordingSink::default());
        app.attach(sink.clone()).await.unwrap();
        for _ in 0..3 {
            app.tick().await.unwrap();
        }
        let patches = sink.patches();
        let structure = patches[0].structure.as_ref().unwrap();
        let total: usize = structure.packages.iter().map(|p| p.items.len()).sum();
        assert_eq!(total, 40);
        let running = |patch: &crate::protocol::Patch| {
            patch
                .structure
                .iter()
                .flat_map(|s| &s.packages)
                .flat_map(|p| &p.items)
                .filter(|i| matches!(i.state, crate::protocol::ItemStateDto::Running))
                .count()
        };
        for patch in &patches {
            assert!(running(patch) <= 5, "never more than the active count");
        }
        assert!(
            patches.iter().map(running).max().unwrap_or(0) > 0,
            "some items must be running after a few ticks"
        );
    }

    #[test]
    fn bad_arguments_are_rejected() {
        assert!(parse_simulate_args(&["--simulate".into(), "0".into(), "0".into()]).is_err());
        assert!(parse_simulate_args(&["--simulate".into(), "10".into(), "20".into()]).is_err());
        assert!(parse_simulate_args(&["--simulate".into(), "x".into(), "1".into()]).is_err());
        assert!(parse_simulate_args(&["--simulate".into(), "6000".into(), "1".into()]).is_err());
        assert!(parse_simulate_args(&["--simulate".into(), "5".into()]).is_err());
        assert_eq!(
            parse_simulate_args(&["--simulate".into(), "500".into(), "50".into()]).unwrap(),
            Some((500, 50))
        );
        assert_eq!(parse_simulate_args(&[]).unwrap(), None);
    }
}
