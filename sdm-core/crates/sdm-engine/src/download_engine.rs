use thiserror::Error;

/// Failures the engine itself produces, as opposed to the transport or the
/// per-download task. Routed through `RetryPolicy::classify` like any other
/// error so they reach a terminal `Failed { reason }` instead of a retry loop.
///
/// Task 16 adds `DownloadEngine` itself to this module; the error enum lives
/// here first because `RetryPolicy` classifies it.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EngineError {
    /// The per-package output folder could not be created — permissions, a
    /// read-only volume, a file sitting where the folder should be.
    #[error("could not create the folder {path}: {underlying}")]
    DestinationFolderUnavailable { path: String, underlying: String },
}
