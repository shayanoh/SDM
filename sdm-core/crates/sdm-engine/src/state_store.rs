use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use sdm_core::DownloadPackage;
use serde::{Deserialize, Serialize};

use crate::resume_sidecar::write_atomically;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    pub format_version: i64,
    pub packages: Vec<DownloadPackage>,
}

impl PersistedState {
    pub const CURRENT_FORMAT_VERSION: i64 = 2;

    /// A state at the current format version.
    pub fn new(packages: Vec<DownloadPackage>) -> Self {
        Self {
            format_version: Self::CURRENT_FORMAT_VERSION,
            packages,
        }
    }

    /// Whether every package and item satisfies the invariants their own
    /// constructors enforce with `assert!` (non-empty `name` / `filename`).
    ///
    /// Deserialization does not route through those constructors, so a
    /// corrupt or hand-edited snapshot can decode into a model that violates
    /// them without the assertion ever firing. Validating here, once, after
    /// decode, is how the store closes that gap instead of trusting the
    /// derived decoder.
    fn is_valid(&self) -> bool {
        self.packages.iter().all(|package| {
            !package.name.is_empty()
                && package.items.iter().all(|item| {
                    !item.output_filename.is_empty()
                        && !item.components.is_empty()
                        && item.components.iter().all(|c| !c.part_filename.is_empty())
                })
        })
    }
}

impl Default for PersistedState {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

#[async_trait]
pub trait StateStore: Send + Sync {
    async fn load(&self) -> PersistedState;
    async fn save(&self, state: PersistedState);
    /// Writes any pending state immediately. Call on quit and on a debounce timer.
    ///
    /// `flush()` stays non-throwing by design: in Phase 1 there is no
    /// caller yet that could do anything useful with a save error (no UI
    /// feedback loop, no retry policy above this layer), so making it
    /// fallible would only push the "now what" decision onto call sites
    /// that can't act on it. What is not acceptable is losing the pending
    /// save silently. `JsonStateStore` honors that by only discarding
    /// `pending` once the write has actually succeeded — a failed write
    /// (disk full, permission denied, encode failure) leaves the save
    /// queued so the next `flush()` call retries it. Surfacing failures to
    /// a caller can be added later once one exists that would use it.
    async fn flush(&self);
    /// Whether a save is still waiting to be written: after `flush()`, true
    /// means the write failed and the save was kept. The engine uses this to
    /// try again a debounce window later instead of waiting for an unrelated
    /// change. A store that cannot fail to write leaves the default.
    async fn has_pending(&self) -> bool {
        false
    }
}

/// Durable state as a single atomically replaced JSON file.
///
/// Saves are coalesced in memory until `flush()`; the caller owns the debounce
/// timer, so no test has to wait on an internal one. See spec §4.2.
///
/// Swift's `actor`: serialized access is a `Mutex` around the pending save,
/// held for each (await-free) method body, `flush()`'s file write included.
#[derive(Clone)]
pub struct JsonStateStore {
    inner: Arc<JsonStateStoreInner>,
}

struct JsonStateStoreInner {
    file_path: PathBuf,
    pending: Mutex<Option<PersistedState>>,
}

impl JsonStateStore {
    pub fn new(file_path: impl Into<PathBuf>) -> Self {
        Self {
            inner: Arc::new(JsonStateStoreInner {
                file_path: file_path.into(),
                pending: Mutex::new(None),
            }),
        }
    }

    fn pending(&self) -> MutexGuard<'_, Option<PersistedState>> {
        self.inner.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn read_snapshot(path: &Path) -> Option<PersistedState> {
        let data = std::fs::read(path).ok()?;
        let mut state: PersistedState = serde_json::from_slice(&data).ok()?;
        if !(1..=PersistedState::CURRENT_FORMAT_VERSION).contains(&state.format_version)
            || !state.is_valid()
        {
            return None;
        }
        // A v1 file's items decode through `DownloadItem`'s legacy branch;
        // normalize the version now so a later save/flush writes it back as v2.
        state.format_version = PersistedState::CURRENT_FORMAT_VERSION;
        Some(state)
    }
}

#[async_trait]
impl StateStore for JsonStateStore {
    /// Loads the durable snapshot, returning an empty `PersistedState` when
    /// the file is missing, unreadable, garbage, written by a mismatched
    /// format version, or decodes into a package/item that violates the
    /// non-empty `name` / `filename` invariant those models otherwise
    /// enforce with assertions in their constructors. The bad file is left
    /// where it is (the next successful `flush()` overwrites it).
    ///
    /// Deliberately non-throwing, matching `ResumeSidecar::load`: an unusable
    /// snapshot always means "start from empty", never "retry the load".
    async fn load(&self) -> PersistedState {
        if let Some(pending) = self.pending().as_ref() {
            return pending.clone();
        }
        Self::read_snapshot(&self.inner.file_path).unwrap_or_default()
    }

    async fn save(&self, state: PersistedState) {
        *self.pending() = Some(state);
    }

    async fn has_pending(&self) -> bool {
        self.pending().is_some()
    }

    async fn flush(&self) {
        let mut pending = self.pending();
        let Some(state) = pending.as_ref() else {
            return;
        };
        // Encode/write failures leave `pending` untouched, so the next
        // `flush()` retries the very same save rather than losing it.
        let Ok(data) = serde_json::to_vec(state) else {
            return;
        };
        let path = &self.inner.file_path;
        let written = (|| {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            write_atomically(path, &data)
        })();
        if written.is_ok() {
            *pending = None;
        }
        // Otherwise: write failed (disk full, permission denied, etc.). Keep
        // `pending` so this save is retried on the next flush() rather than
        // silently discarded.
    }
}

/// Non-persistent store for tests and previews.
#[derive(Clone, Default)]
pub struct InMemoryStateStore {
    state: Arc<Mutex<PersistedState>>,
}

impl InMemoryStateStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl StateStore for InMemoryStateStore {
    async fn load(&self) -> PersistedState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    async fn save(&self, state: PersistedState) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = state;
    }

    async fn flush(&self) {}
}
