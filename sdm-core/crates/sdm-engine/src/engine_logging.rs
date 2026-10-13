//! Mirrors `EngineLogging.swift`: compiled out unless the `engine-logging`
//! feature (Swift's `SDM_ENGINE_LOGGING` flag) is on.

/// Logs at debug level through `tracing` when the `engine-logging` feature is
/// on; expands to nothing otherwise (its arguments are not evaluated).
#[cfg(feature = "engine-logging")]
#[macro_export]
macro_rules! engine_log {
    ($($arg:tt)+) => {
        $crate::__tracing::debug!(target: "sdm_engine", $($arg)+)
    };
}

/// Logs at debug level through `tracing` when the `engine-logging` feature is
/// on; expands to nothing otherwise (its arguments are not evaluated).
#[cfg(not(feature = "engine-logging"))]
#[macro_export]
macro_rules! engine_log {
    ($($arg:tt)*) => {};
}

#[cfg(feature = "engine-logging")]
mod enabled {
    use std::time::Duration;

    use uuid::Uuid;

    /// Short, grep-friendly tag for a download item: `[<id8> <filename>]`.
    /// `id8` disambiguates items that happen to share a filename.
    pub fn item_tag(item_id: Uuid, filename: &str) -> String {
        let id = item_id.to_string().to_uppercase();
        format!("[{} {filename}]", &id[..8])
    }

    pub fn worker_tag(item_id: Uuid, filename: &str, worker: usize) -> String {
        format!("{} [w{worker}]", item_tag(item_id, filename))
    }

    pub fn formatted(duration: Duration) -> String {
        format!("{}ms", (duration.as_secs_f64() * 1000.0) as i64)
    }
}

#[cfg(feature = "engine-logging")]
pub use enabled::{formatted, item_tag, worker_tag};
