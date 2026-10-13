//! Benchmark program for the Rust core. It is the twin of
//! `legacy/SDMKitSwift/Sources/SDMBench/main.swift`: the same two scenarios
//! with the same parameters, each against the core's own public
//! [`FakeOrigin`], an in-memory state store and identical [`EngineSettings`].
//! Run it from `scripts/bench.sh`.
//!
//! ```text
//! sdm-bench transfer   8 items x 256 MB in one package, run to completion
//! sdm-bench ticks      1,000 items in 20 packages, 8 running, 600 x (tick + snapshot)
//! ```
//!
//! `SDM_BENCH_SCALE` (a positive integer, default 1) divides the transfer item
//! size and the tick count for quick debugging; both programs read it
//! identically and the recorded run leaves it unset.

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use sdm_core::{DownloadItem, DownloadPackage, ItemState, SourceUrl};
use sdm_engine::{
    DownloadEngine, EngineSettings, FakeOrigin, FakeOriginBehavior, HttpTransport,
    InMemoryStateStore, RangeRequest, RangeResponse, RetryPolicy, TransportError,
};

/// Bytes per chunk the fake origin emits. 64 KiB on both sides (the fake's
/// default of 64 bytes would measure the stream plumbing, not the transfer).
const CHUNK_SIZE: usize = 64 * 1024;

/// Same bytes as the Swift side's `makePayload`: index-derived, so the origin
/// costs the same to build and any byte can be checked independently.
fn make_payload(count: usize) -> Bytes {
    let bytes: Vec<u8> = (0..count)
        .map(|i| (i.wrapping_mul(31).wrapping_add(7) % 251) as u8)
        .collect();
    Bytes::from(bytes)
}

/// Fronts a [`FakeOrigin`], and parks every body fetch forever when `gated`,
/// so running items stay running. The one-byte probe always passes through.
struct BenchTransport {
    origin: FakeOrigin,
    gated: bool,
}

#[async_trait]
impl HttpTransport for BenchTransport {
    async fn fetch(&self, request: RangeRequest) -> Result<RangeResponse, TransportError> {
        if self.gated {
            let is_probe = request
                .range
                .is_some_and(|range| range.start == 0 && range.end == 1);
            if !is_probe {
                std::future::pending::<()>().await;
            }
        }
        self.origin.fetch(request).await
    }
}

fn make_engine(payload: Bytes, folder: &Path, gated: bool) -> DownloadEngine {
    let origin = FakeOrigin::with_behavior(
        payload,
        FakeOriginBehavior {
            chunk_size: CHUNK_SIZE,
            ..FakeOriginBehavior::default()
        },
    );
    DownloadEngine::new(
        Arc::new(BenchTransport { origin, gated }),
        Arc::new(InMemoryStateStore::new()),
        EngineSettings::new(8, 8, 64, folder.to_path_buf()).with_max_connections_per_host(64),
        RetryPolicy::new(),
        None,
        None,
        None,
    )
}

fn scale() -> Result<usize, String> {
    match std::env::var("SDM_BENCH_SCALE") {
        Err(_) => Ok(1),
        Ok(raw) => match raw.parse::<usize>() {
            Ok(value) if value >= 1 => Ok(value),
            _ => Err(format!(
                "SDM_BENCH_SCALE must be a positive integer, got {raw}"
            )),
        },
    }
}

fn item(url: &str, filename: &str, position: i64) -> Result<DownloadItem, String> {
    let url = SourceUrl::new(url).map_err(|error| format!("bad URL {url}: {error}"))?;
    Ok(DownloadItem::http(url, filename).with_position(position))
}

async fn run_transfer(directory: &Path) -> Result<String, String> {
    let item_bytes = 256 * 1024 * 1024 / scale()?;
    let item_count = 8usize;
    let engine = make_engine(make_payload(item_bytes), directory, false);

    let mut items = Vec::new();
    for index in 0..item_count {
        items.push(item(
            &format!("https://example.com/file{index}.bin"),
            &format!("file{index}.bin"),
            index as i64,
        )?);
    }
    let names: Vec<String> = items
        .iter()
        .map(|item| item.output_filename.clone())
        .collect();
    engine
        .add(DownloadPackage::new("Batch").with_items(items))
        .await;

    loop {
        engine.tick().await;
        let snapshot = engine.snapshot().await;
        let mut completed = 0usize;
        for item in snapshot.packages.iter().flat_map(|package| &package.items) {
            match &item.state {
                ItemState::Failed { reason } => return Err(format!("an item failed: {reason}")),
                ItemState::Completed => completed += 1,
                _ => {}
            }
        }
        if completed == item_count {
            break;
        }
        tokio::task::yield_now().await;
    }

    let mut total = 0usize;
    for name in names {
        let path = directory.join("Batch").join(&name);
        let size = std::fs::metadata(&path)
            .map_err(|error| format!("{name}: {error}"))?
            .len() as usize;
        if size != item_bytes {
            return Err(format!("{name} is {size} bytes, expected {item_bytes}"));
        }
        total += size;
    }
    Ok(format!(
        "scenario=transfer ok=true items={item_count} bytes={total}"
    ))
}

async fn run_ticks(directory: &Path) -> Result<String, String> {
    let item_count = 1000usize;
    let package_count = 20usize;
    let item_bytes = 64 * 1024;
    let iterations = 600 / scale()?;
    let engine = make_engine(make_payload(item_bytes), directory, true);

    for package_index in 0..package_count {
        let per_package = item_count / package_count;
        let mut items = Vec::new();
        for index in 0..per_package {
            items.push(item(
                &format!("https://example.com/p{package_index}/file{index}.bin"),
                &format!("file{index}.bin"),
                index as i64,
            )?);
        }
        engine
            .add(
                DownloadPackage::new(format!("Package {package_index}"))
                    .with_items(items)
                    .with_position(package_index as i64),
            )
            .await;
    }

    let mut packages_seen = 0usize;
    for _ in 0..iterations {
        engine.tick().await;
        let snapshot = engine.snapshot().await;
        packages_seen += snapshot.packages.len();
    }
    if packages_seen != iterations * package_count {
        return Err(format!(
            "snapshots held {packages_seen} packages, expected {}",
            iterations * package_count
        ));
    }
    Ok(format!("scenario=ticks ok=true items={item_count} bytes=0"))
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let scenario = match arguments.as_slice() {
        [name] if name == "transfer" || name == "ticks" => name.clone(),
        _ => {
            eprintln!("usage: sdm-bench transfer|ticks");
            return ExitCode::from(2);
        }
    };

    // The directory is removed when `directory` drops, on success, on an
    // error return and on a panic unwinding out of the runtime.
    let directory = match tempfile::Builder::new().prefix("sdm-bench-").tempdir() {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("scenario={scenario} ok=false error=cannot create a temp directory: {error}");
            return ExitCode::from(1);
        }
    };
    let result = match tokio::runtime::Runtime::new() {
        Ok(runtime) => runtime.block_on(async {
            if scenario == "transfer" {
                run_transfer(directory.path()).await
            } else {
                run_ticks(directory.path()).await
            }
        }),
        Err(error) => Err(format!("cannot start the runtime: {error}")),
    };
    drop(directory);
    match result {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("scenario={scenario} ok=false error={error}");
            ExitCode::from(1)
        }
    }
}
