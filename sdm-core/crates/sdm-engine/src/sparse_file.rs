use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use thiserror::Error;

/// Swift's `SparseFile.FileError`. `errno` values are the raw OS error codes.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SparseFileError {
    #[error("could not open {path} (errno {errno})")]
    CouldNotOpen { path: String, errno: i32 },
    #[error("write failed (errno {errno})")]
    WriteFailed { errno: i32 },
    #[error("truncate failed (errno {errno})")]
    TruncateFailed { errno: i32 },
}

/// Why [`SparseFile::finalize`] failed: the flush (a [`SparseFileError`]) or
/// the rename (Swift throws a Cocoa error from `FileManager.moveItem`; an
/// existing destination is an `io::Error` of kind `AlreadyExists`, which is
/// what `RetryPolicy::classify` looks for). Callers that classify the error
/// should hand `classify` the inner error, see [`FinalizeError::into_boxed`].
#[derive(Debug, Error)]
pub enum FinalizeError {
    #[error(transparent)]
    Sync(#[from] SparseFileError),
    #[error(transparent)]
    Move(#[from] io::Error),
}

impl FinalizeError {
    /// The underlying error, unwrapped so `downcast_ref` finds the concrete
    /// `SparseFileError` / `io::Error` like a Swift `as?` cast would.
    pub fn into_boxed(self) -> Box<dyn std::error::Error + Send + Sync> {
        match self {
            Self::Sync(error) => Box::new(error),
            Self::Move(error) => Box::new(error),
        }
    }
}

/// Reported when `pwrite` returns 0 (Swift reports a stale `errno` there,
/// which is meaningless) or an `io::Error` carries no OS code.
const EIO: i32 = libc::EIO;
const EBADF: i32 = libc::EBADF;

fn errno_of(error: &io::Error) -> i32 {
    error.raw_os_error().unwrap_or(EIO)
}

/// `fsync(2)`, not `File::sync_all`: on macOS std's `sync_all`/`sync_data`
/// issue `fcntl(F_FULLFSYNC)`, a much stronger and slower flush than the
/// plain `fsync` Swift calls.
fn fsync(file: &File) -> io::Result<()> {
    // SAFETY: `file` owns an open descriptor for the duration of the call.
    let result = unsafe { libc::fsync(file.as_raw_fd()) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The on-disk destination for a download, written at arbitrary offsets by
/// multiple concurrent workers.
///
/// Wraps a file descriptor guarded by an internal lock; positional writes to
/// disjoint offsets are safe across threads.
pub struct SparseFile {
    pub final_path: PathBuf,
    pub incomplete_path: PathBuf,
    /// `None` once closed.
    file: Mutex<Option<File>>,
}

impl SparseFile {
    pub fn incomplete_path(final_path: &Path) -> PathBuf {
        let mut path: OsString = final_path.as_os_str().to_owned();
        path.push(".incomplete");
        PathBuf::from(path)
    }

    /// Opens (creating if needed) the `.incomplete` file and preallocates it.
    ///
    /// # Panics
    ///
    /// Panics if `total_bytes` is negative ("totalBytes must be non-negative").
    pub fn open(final_path: &Path, total_bytes: i64) -> Result<Self, SparseFileError> {
        assert!(total_bytes >= 0, "totalBytes must be non-negative");
        let incomplete_path = Self::incomplete_path(final_path);

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o644)
            .open(&incomplete_path)
            .map_err(|error| SparseFileError::CouldNotOpen {
                path: incomplete_path.to_string_lossy().into_owned(),
                errno: errno_of(&error),
            })?;

        // Sparse on APFS: allocates no blocks until bytes are actually written.
        // (The descriptor is closed by dropping `file` on the error path.)
        file.set_len(total_bytes as u64)
            .map_err(|error| SparseFileError::TruncateFailed {
                errno: errno_of(&error),
            })?;

        Ok(Self {
            final_path: final_path.to_path_buf(),
            incomplete_path,
            file: Mutex::new(Some(file)),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<File>> {
        self.file.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Writes all of `data` at the absolute `offset`, looping over short
    /// writes. Writing after `close()` fails with `EBADF`, as `pwrite` on a
    /// closed descriptor does.
    ///
    /// # Panics
    ///
    /// Panics if `offset` is negative ("offset must be non-negative").
    pub fn write(&self, data: &[u8], offset: i64) -> Result<(), SparseFileError> {
        assert!(offset >= 0, "offset must be non-negative");
        if data.is_empty() {
            return Ok(());
        }

        let guard = self.lock();
        let Some(file) = guard.as_ref() else {
            return Err(SparseFileError::WriteFailed { errno: EBADF });
        };

        let mut written = 0usize;
        while written < data.len() {
            let result = file
                .write_at(&data[written..], offset as u64 + written as u64)
                .map_err(|error| SparseFileError::WriteFailed {
                    errno: errno_of(&error),
                })?;
            if result == 0 {
                return Err(SparseFileError::WriteFailed { errno: EIO });
            }
            written += result;
        }
        Ok(())
    }

    /// Forces everything written so far out to the storage device.
    ///
    /// A no-op once closed, and legitimately so: `close()` fsyncs before
    /// releasing the descriptor, and no write can follow it. Failing with
    /// `EBADF` there instead would make `DownloadTask::checkpoint()`'s "sync,
    /// then write the sidecar" sequence fail on an already-finished task.
    pub fn sync(&self) -> Result<(), SparseFileError> {
        let guard = self.lock();
        let Some(file) = guard.as_ref() else {
            return Ok(());
        };
        fsync(file).map_err(|error| SparseFileError::WriteFailed {
            errno: errno_of(&error),
        })
    }

    /// Flushes, closes, and renames the file to its final name. Refuses to
    /// overwrite an existing destination (`FileManager.moveItem` semantics),
    /// failing with an `io::Error` of kind `AlreadyExists`.
    pub fn finalize(&self) -> Result<PathBuf, FinalizeError> {
        self.sync()?;
        self.close();
        if std::fs::symlink_metadata(&self.final_path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} already exists", self.final_path.display()),
            )
            .into());
        }
        std::fs::rename(&self.incomplete_path, &self.final_path)?;
        Ok(self.final_path.clone())
    }

    /// Flushes and releases the descriptor. Idempotent.
    ///
    /// The `fsync` is best-effort (there is no caller that could act on a
    /// failure here) but it is what makes `sync()` safe to treat as a no-op
    /// once closed: `close(2)` alone does not push anything to the device, so
    /// without it a descriptor closed on a failure path would leave bytes in
    /// the page cache that a later sidecar might claim are durable.
    pub fn close(&self) {
        let mut guard = self.lock();
        if let Some(file) = guard.take() {
            let _ = fsync(&file);
            drop(file);
        }
    }
}

impl Drop for SparseFile {
    fn drop(&mut self) {
        self.close();
    }
}
