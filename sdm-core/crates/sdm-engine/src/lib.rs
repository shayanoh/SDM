//! The download engine: workers, scheduler, retry policy, snapshots.

mod download_engine;
mod download_task;
mod engine_logging;
mod engine_snapshot;
mod http_transport;
mod retry_policy;
mod scheduler;
mod snapshot_aggregates;
mod sparse_file;
mod speed_sampler;
mod yt_dlp_artifacts;

pub use download_engine::EngineError;
pub use download_task::DownloadError;
pub use engine_snapshot::{EngineSnapshot, ItemSnapshot, PackageSnapshot};
pub use http_transport::TransportError;
pub use retry_policy::{FailureKind, RetryPolicy};
pub use scheduler::{Scheduler, SchedulerInput};
pub use snapshot_aggregates::{
    EngineFigures, LiveWindow, PackageAggregate, average_recent_speed, estimated_seconds_remaining,
};
pub use sparse_file::SparseFileError;
pub use speed_sampler::SpeedSampler;
pub use yt_dlp_artifacts::YtDlpArtifacts;

#[cfg(feature = "engine-logging")]
pub use engine_logging::{formatted, item_tag, worker_tag};

#[doc(hidden)]
#[cfg(feature = "engine-logging")]
pub use tracing as __tracing;
