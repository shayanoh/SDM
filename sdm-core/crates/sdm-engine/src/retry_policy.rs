use std::error::Error;
use std::io;
use std::time::Duration;

use sdm_core::WholesaleError;

use crate::download_task::DownloadError;
use crate::engine::EngineError;
use crate::http_transport::TransportError;
use crate::sparse_file::SparseFileError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureKind {
    Transient,
    Permanent { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: usize,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl RetryPolicy {
    /// Swift's initializer defaults: 5 attempts, 1 s base delay, 60 s ceiling.
    pub fn new() -> Self {
        Self {
            max_attempts: 5,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
        }
    }

    /// # Panics
    ///
    /// Panics if `max_attempts` is zero ("maxAttempts must be at least 1").
    pub fn with_max_attempts(mut self, max_attempts: usize) -> Self {
        assert!(max_attempts >= 1, "maxAttempts must be at least 1");
        self.max_attempts = max_attempts;
        self
    }

    pub fn with_base_delay(mut self, base_delay: Duration) -> Self {
        self.base_delay = base_delay;
        self
    }

    pub fn with_max_delay(mut self, max_delay: Duration) -> Self {
        self.max_delay = max_delay;
        self
    }

    /// Classifies any error by downcasting, as the Swift does with `as?`
    /// casts; an error of an unrecognised type is transient.
    pub fn classify(&self, error: &(dyn Error + 'static)) -> FailureKind {
        if let Some(transport) = error.downcast_ref::<TransportError>() {
            return match transport {
                TransportError::ConnectionDropped
                | TransportError::Network { .. }
                | TransportError::MalformedResponse => FailureKind::Transient,
                TransportError::Http { status } => Self::classify_status(*status),
            };
        }
        if let Some(wholesale) = error.downcast_ref::<WholesaleError>() {
            return match wholesale {
                WholesaleError::BinaryMissing => FailureKind::Permanent {
                    reason: "yt-dlp is not installed".to_string(),
                },
                WholesaleError::AuthRequired => FailureKind::Permanent {
                    reason: "The site requires sign-in — set cookies in Settings → Media Sites"
                        .to_string(),
                },
                WholesaleError::Unavailable => FailureKind::Permanent {
                    reason: "The video is unavailable or DRM-protected".to_string(),
                },
                // A one-shot yt-dlp download failure: retry a couple of times
                // (transient network / rate limit), then it hits the cap.
                WholesaleError::Cancelled | WholesaleError::Failed { .. } => FailureKind::Transient,
            };
        }
        if let Some(download) = error.downcast_ref::<DownloadError>() {
            return match download {
                DownloadError::ServerError { status } => Self::classify_status(*status),
                DownloadError::UnknownSize => FailureKind::Permanent {
                    reason: "Server did not report a size".to_string(),
                },
                DownloadError::IncompleteAfterWorkersFinished => FailureKind::Transient,
                DownloadError::TruncatedResponse { .. } => FailureKind::Transient,
                DownloadError::RangeNotHonored { .. } => FailureKind::Transient,
                // If the engine has a resolver it refreshes the URL and never
                // reaches here; without one, treat it like any expired signed
                // URL — transient, so it backs off and eventually terminates.
                DownloadError::UrlExpired { .. } => FailureKind::Transient,
                DownloadError::RefreshFailed { reason } => FailureKind::Permanent {
                    reason: reason.clone(),
                },
                // Swift lets the underlying error propagate out of `start()`
                // and classifies that; these variants carry it.
                DownloadError::Transport(inner) => return self.classify(inner),
                DownloadError::File(inner) => return self.classify(inner),
                DownloadError::Move { kind, .. } => {
                    if *kind == io::ErrorKind::AlreadyExists {
                        FailureKind::Permanent {
                            reason: "A file already exists at the destination".to_string(),
                        }
                    } else {
                        FailureKind::Transient
                    }
                }
                DownloadError::InvalidUrl { url } => FailureKind::Permanent {
                    reason: format!("{url} is not a valid URL"),
                },
            };
        }
        // A destination that already exists is the one local failure that
        // re-attempting can never clear. `DownloadTask::finalize()` refuses to
        // overwrite (deliberately — that behavior is tested), so without this
        // the resulting error fell through to `Transient`, the item went
        // back to `Queued`, the next tick resumed it to "complete" instantly
        // from its sidecar, and it failed at the same rename again. Forever.
        //
        // Swift matches `CocoaError.fileWriteFileExists`; the Rust twin of
        // that failure is an `io::Error` of kind `AlreadyExists`.
        if let Some(io_error) = error.downcast_ref::<io::Error>()
            && io_error.kind() == io::ErrorKind::AlreadyExists
        {
            return FailureKind::Permanent {
                reason: "A file already exists at the destination".to_string(),
            };
        }
        // Likewise for a destination folder that cannot be opened or created:
        // permissions and read-only volumes do not fix themselves, and
        // retrying produces a request storm with no diagnosis.
        if let Some(file) = error.downcast_ref::<SparseFileError>() {
            return match file {
                SparseFileError::CouldNotOpen { path, errno } => FailureKind::Permanent {
                    reason: format!("Could not open {path} (errno {errno})"),
                },
                SparseFileError::TruncateFailed { errno } => FailureKind::Permanent {
                    reason: format!("Could not preallocate the file (errno {errno})"),
                },
                // Could be a full disk that the user then frees up.
                SparseFileError::WriteFailed { errno } => {
                    if io::Error::from_raw_os_error(*errno).kind() == io::ErrorKind::StorageFull {
                        FailureKind::Permanent {
                            reason: "The disk is full".to_string(),
                        }
                    } else {
                        FailureKind::Transient
                    }
                }
            };
        }
        if let Some(engine) = error.downcast_ref::<EngineError>() {
            return match engine {
                EngineError::DestinationFolderUnavailable { path, underlying } => {
                    FailureKind::Permanent {
                        reason: format!("Could not create the folder {path}: {underlying}"),
                    }
                }
            };
        }
        FailureKind::Transient
    }

    /// 403 is transient on purpose: signed media URLs (googlevideo and
    /// friends) expire, and the correct response is to refresh and retry.
    fn classify_status(status: u16) -> FailureKind {
        match status {
            403 | 408 | 425 | 429 | 500..=599 => FailureKind::Transient,
            _ => FailureKind::Permanent {
                reason: format!("HTTP {status}"),
            },
        }
    }

    /// Exponential backoff with deterministic jitter derived from the attempt
    /// number, so retry timing is reproducible in tests.
    pub fn delay_for_attempt(&self, attempt: usize) -> Duration {
        let factor = (1u64 << attempt.min(20)) as f64;
        // Swift's `attempt &* 37` wraps; the remainder is taken on the
        // wrapped value.
        let jitter = 1.0 + (attempt.wrapping_mul(37) % 25) as f64 / 100.0;
        let seconds = self.base_delay.as_secs_f64() * factor * jitter;
        match Duration::try_from_secs_f64(seconds) {
            Ok(delay) => delay.min(self.max_delay),
            // Out of `Duration`'s range, so certainly over any ceiling.
            Err(_) => self.max_delay,
        }
    }
}
