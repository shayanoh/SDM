use thiserror::Error;

/// Swift's `SparseFile.FileError`. Task 14 adds `SparseFile` itself to this
/// module; the error enum lives here first because `RetryPolicy` classifies
/// it. `errno` values are the raw OS error codes.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SparseFileError {
    #[error("could not open {path} (errno {errno})")]
    CouldNotOpen { path: String, errno: i32 },
    #[error("write failed (errno {errno})")]
    WriteFailed { errno: i32 },
    #[error("truncate failed (errno {errno})")]
    TruncateFailed { errno: i32 },
}
