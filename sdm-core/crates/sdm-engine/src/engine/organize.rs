//! Organizing the list: reordering and moving items and packages, removing
//! them, and resetting a download.

use crate::safe_join::JoinName;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sdm_core::{DownloadItem, DownloadPackage, ItemState, RangeSet};
use uuid::Uuid;

use super::state::EngineState;
use super::{DownloadEngine, Inner, RetireIntent};
use crate::resume_sidecar::ResumeSidecar;
use crate::sparse_file::SparseFile;
use crate::yt_dlp_artifacts::YtDlpArtifacts;

/// Where files go when the engine "deletes" them. Swift calls
/// `FileManager.trashItem` directly; the engine takes this instead so tests
/// can keep their files out of the user's real Trash.
pub trait Trash: Send + Sync {
    /// Moves `path` (a file or a folder) to the trash.
    fn trash(&self, path: &Path) -> std::io::Result<()>;
}

/// The user's Trash, through the `trash` crate.
///
/// On macOS it asks `NSFileManager.trashItem`, which is what Swift's
/// `FileManager.trashItem` does. The crate's default, the Finder, would need
/// the user's Automation permission and is slower.
pub struct SystemTrash;

impl Trash for SystemTrash {
    fn trash(&self, path: &Path) -> std::io::Result<()> {
        #[cfg(target_os = "macos")]
        {
            use trash::macos::{DeleteMethod, TrashContextExtMacos};
            let mut context = trash::TrashContext::default();
            context.set_delete_method(DeleteMethod::NsFileManager);
            context.delete(path).map_err(std::io::Error::other)
        }
        #[cfg(not(target_os = "macos"))]
        {
            trash::delete(path).map_err(std::io::Error::other)
        }
    }
}

/// The engine's default [`Trash`]: it moves nothing and reports an error.
///
/// `DownloadEngine::new` cannot know which trash the caller wants, and the
/// wrong default is destructive (the real Trash, in a test run). So the
/// default refuses, and a caller that wants files trashed says so with
/// `with_trash`. Only the production constructor in `sdm-ffi` passes
/// [`SystemTrash`].
pub(crate) struct RefusingTrash;

impl Trash for RefusingTrash {
    fn trash(&self, path: &Path) -> std::io::Result<()> {
        Err(std::io::Error::other(format!(
            "no Trash was configured for this engine; refusing to move {}",
            path.display()
        )))
    }
}

/// Applies `ids` as the new order of `existing`, writing each placed element's
/// `position` as its index in `ids`. Anything not named keeps its relative
/// order, appended at the end. An id that is not in `existing`, or that
/// repeats, is skipped (but still counts toward the next index).
///
/// Swift appends the unnamed ones in `Dictionary.values` order, which is
/// unspecified; keeping their relative order is what its doc comment promises.
fn apply_order<T>(
    existing: Vec<T>,
    ids: &[Uuid],
    id_of: impl Fn(&T) -> Uuid,
    set_position: impl Fn(&mut T, i64),
) -> Vec<T> {
    let mut by_id: HashMap<Uuid, usize> = HashMap::new();
    for (index, element) in existing.iter().enumerate() {
        by_id.entry(id_of(element)).or_insert(index);
    }
    let mut slots: Vec<Option<T>> = existing.into_iter().map(Some).collect();
    let mut reordered = Vec::with_capacity(slots.len());
    for (position, id) in ids.iter().enumerate() {
        let Some(index) = by_id.remove(id) else {
            continue;
        };
        if let Some(mut element) = slots[index].take() {
            set_position(&mut element, position as i64);
            reordered.push(element);
        }
    }
    reordered.extend(slots.into_iter().flatten());
    reordered
}

fn is_sidecar(path: &Path) -> bool {
    path.extension() == Some(OsStr::new("sdmpart"))
}

/// Every on-disk artefact an item can leave in its package folder: the final
/// output file, and — for a multi-component item — each component's part file.
/// For each, the base URL plus its `.incomplete` sparse file and its `.sdmpart`
/// resume sidecar. A one-component HTTP item's part filename equals its output
/// filename, so this collapses to the old single-file set for it.
fn item_artefact_urls(item: &DownloadItem, folder: &Path) -> Vec<PathBuf> {
    let mut bases = vec![folder.join_name(&item.output_filename)];
    for component in &item.components {
        bases.push(folder.join_name(&component.part_filename));
    }
    let mut urls = Vec::new();
    for base in bases {
        let incomplete = SparseFile::incomplete_path(&base);
        let sidecar = ResumeSidecar::path_for(&base);
        urls.extend([base, incomplete, sidecar]);
    }
    // A wholesale (yt-dlp-as-downloader) item can leave `.ytdl` / `.fragN` /
    // pre-merge stream files a killed yt-dlp did not clean up.
    if item.has_wholesale_component() {
        urls.extend(YtDlpArtifacts::scratch_files(folder, &item.output_filename));
    }
    urls
}

/// Moves whichever of an item's on-disk files currently exist — the finished
/// file, the `.incomplete` sparse file, the `.sdmpart` resume sidecar — from
/// one package folder to another. Best-effort per file, same as
/// `trash_if_exists`/`remove_folder_if_empty` elsewhere in this file: a missing
/// source file (the common case — most of these three exist for any given
/// item, not all of them) is simply skipped rather than treated as an error.
fn relocate_item_files(item: &DownloadItem, source_folder: &Path, destination_folder: &Path) {
    let _ = std::fs::create_dir_all(destination_folder);
    let sources = item_artefact_urls(item, source_folder);
    let destinations = item_artefact_urls(item, destination_folder);
    for (from, to) in sources.iter().zip(&destinations) {
        if from.exists() {
            let _ = std::fs::rename(from, to);
        }
    }
}

/// Keeps items out of the scheduler (`EngineState::removing`) while a removal
/// or reset works on their files. Dropping it lifts the guard, so an early
/// return or a cancelled future cannot leave an item unschedulable forever;
/// the operations drop it explicitly before their final `reconcile()`.
struct RemovalGuard {
    inner: Arc<Inner>,
    ids: Vec<Uuid>,
}

impl RemovalGuard {
    /// Marks `ids` as being removed. `state` is the locked state of the scope
    /// that decided the operation, so the marking is atomic with the decision.
    fn new(inner: &Arc<Inner>, state: &mut EngineState, ids: Vec<Uuid>) -> Self {
        for id in &ids {
            *state.removing.entry(*id).or_insert(0) += 1;
        }
        Self {
            inner: Arc::clone(inner),
            ids,
        }
    }
}

impl Drop for RemovalGuard {
    fn drop(&mut self) {
        let mut state = self.inner.lock();
        for id in &self.ids {
            if let Some(count) = state.removing.get_mut(id) {
                *count -= 1;
                if *count == 0 {
                    state.removing.remove(id);
                }
            }
        }
    }
}

impl Inner {
    fn trash_if_exists(&self, path: &Path) {
        if path.exists() {
            let _ = self.trash.trash(path);
        }
    }

    fn remove_folder_if_empty(&self, folder: &Path) {
        let Ok(mut contents) = std::fs::read_dir(folder) else {
            return;
        };
        if contents.next().is_none() {
            let _ = self.trash.trash(folder);
        }
    }

    /// Cancels and awaits an item's runner if it has one, mirroring
    /// `shutdown()`'s stop sequence for a single item: cancel the job, pause
    /// the task so its file descriptor closes, then await the job so
    /// `finish_item` has already run and the runner is gone from `runners`
    /// before the caller touches disk. That also means the old job can no
    /// longer write state back onto the item: its `finish_item` is done, and a
    /// later run has a different `run_id`.
    ///
    /// Blocking here is deliberate and safe for this method's callers
    /// (`remove_item`, `remove_package`, `reset_download`): they are about to
    /// touch the same file on disk and must not race the worker that has it
    /// open, and each already overwrites the item's landing state (or removes
    /// the item outright) immediately after, so whatever the job lands on in
    /// the meantime is transient. Marking `Preempted` (rather than leaving
    /// `None`) matters independently of that overwrite though: without it, the
    /// cancellation was charged to the item's retry budget — the job's failure
    /// handling would classify it as a real failure and could leave the item
    /// silently held in `retry_hold_ticks` backoff, invisible in `state` but
    /// enough to make a later restart sit `Queued` and never actually get
    /// picked up until the backoff aged out on its own.
    ///
    /// User-facing Stop/Disable do **not** go through this method — see
    /// `retire_runner_nonblocking`.
    async fn stop_runner_if_running(&self, item_id: Uuid) {
        let runner = {
            let mut state = self.lock();
            let Some(runner) = state.runners().get(&item_id).cloned() else {
                return;
            };
            if let Some(live) = state.runners_mut().get_mut(&item_id) {
                live.retire_intent = RetireIntent::Preempted;
            }
            runner
        };
        runner.cancel.cancel();
        runner.pause_all_components().await;
        runner.done.wait().await;
    }

    /// The body of `move_items_to_package`, on a locked state. Returns whether
    /// the move went ahead.
    fn move_items(
        &self,
        state: &mut EngineState,
        item_ids: &[Uuid],
        package_id: Uuid,
        at_index: Option<usize>,
    ) -> bool {
        // A repeated id would be removed twice below; the first mention wins.
        let mut seen = HashSet::new();
        let ids: Vec<Uuid> = item_ids
            .iter()
            .copied()
            .filter(|id| seen.insert(*id))
            .collect();
        if ids.is_empty() {
            return false;
        }
        let Some(destination) = state
            .packages()
            .iter()
            .position(|package| package.id == package_id)
        else {
            return false;
        };

        let mut sources: HashMap<Uuid, (usize, usize)> = HashMap::new();
        for id in &ids {
            let Some(source) = state.location_of(*id) else {
                return false;
            };
            if state.packages()[source.0].items[source.1].state == ItemState::Running {
                return false;
            }
            sources.insert(*id, source);
        }

        // Snapshot the moved items themselves before any removal below
        // touches the arrays they currently sit in.
        let moved_items: Vec<DownloadItem> = ids
            .iter()
            .map(|id| {
                let (package_index, item_index) = sources[id];
                state.packages()[package_index].items[item_index].clone()
            })
            .collect();

        // How many of the batch sit ahead of `index` within the destination
        // package itself — same "removing it first shifts everything after it
        // down by one" adjustment `move_item` made for a single item,
        // generalized to however many of the batch are also sourced from the
        // destination.
        let mut insertion_index = at_index.unwrap_or(state.packages()[destination].items.len());
        if let Some(index) = at_index {
            let preceding_within_destination = ids
                .iter()
                .filter(|id| sources[id].0 == destination && sources[id].1 < index)
                .count();
            insertion_index = index - preceding_within_destination;
        }

        let download_folder = state.settings().download_folder.clone();
        let destination_folder = download_folder.join_name(&state.packages()[destination].name);
        for (id, item) in ids.iter().zip(&moved_items) {
            let source = sources[id];
            if source.0 == destination {
                continue;
            }
            let source_folder = download_folder.join_name(&state.packages()[source.0].name);
            relocate_item_files(item, &source_folder, &destination_folder);
        }

        // Removed highest-index-first within each source package so an earlier
        // removal never invalidates a later one's stored index.
        let mut by_package: HashMap<usize, Vec<usize>> = HashMap::new();
        for id in &ids {
            let (package_index, item_index) = sources[id];
            by_package
                .entry(package_index)
                .or_default()
                .push(item_index);
        }
        for (package_index, item_indices) in &mut by_package {
            item_indices.sort_unstable_by(|a, b| b.cmp(a));
            for item_index in item_indices.iter() {
                state.packages_mut()[*package_index]
                    .items
                    .remove(*item_index);
            }
        }

        let destination_items = &mut state.packages_mut()[destination].items;
        let insertion_index = insertion_index.min(destination_items.len());
        destination_items.splice(insertion_index..insertion_index, moved_items);
        state.renumber_positions(destination);

        // Descending so removing an emptied source package doesn't shift the
        // index of another not-yet-processed one still pending below it in
        // this same loop.
        let source_packages: BTreeSet<usize> = by_package.keys().copied().collect();
        let mut empty_source_folders = Vec::new();
        for package_index in source_packages.into_iter().rev() {
            if package_index == destination {
                continue;
            }
            if state.packages()[package_index].items.is_empty() {
                empty_source_folders
                    .push(download_folder.join_name(&state.packages()[package_index].name));
                state.packages_mut().remove(package_index);
            } else {
                state.renumber_positions(package_index);
            }
        }
        for folder in empty_source_folders {
            self.remove_folder_if_empty(&folder);
        }
        true
    }
}

impl DownloadEngine {
    /// Applies a new item order within one package. Spec §9.3: "Dropping
    /// between rows reorders," and reordering writes the position field the
    /// scheduler's rank already reads — no separate re-scheduling step.
    pub async fn reorder_items(&self, item_ids: &[Uuid], package_id: Uuid) {
        let inner = &self.inner;
        {
            let mut state = inner.lock();
            let Some(package_index) = state
                .packages()
                .iter()
                .position(|package| package.id == package_id)
            else {
                return;
            };
            let package = &mut state.packages_mut()[package_index];
            let existing = std::mem::take(&mut package.items);
            package.items = apply_order(
                existing,
                item_ids,
                |item| item.id,
                |item, position| item.position = position,
            );
        }
        inner.persist().await;
        inner.reconcile().await;
    }

    /// Moves an item to a position — possibly within its own package.
    pub async fn move_item(&self, item_id: Uuid, to_package: Uuid, at_index: Option<usize>) {
        self.move_items_to_package(&[item_id], to_package, at_index)
            .await;
    }

    /// Moves items into a new package.
    ///
    /// If the move is then refused (an item is missing or running) the new,
    /// empty package stays behind, as in Swift.
    pub async fn move_items_to_new_package(&self, item_ids: &[Uuid], package_name: &str) {
        if item_ids.is_empty() {
            return;
        }
        let inner = &self.inner;
        let moved = {
            let mut state = inner.lock();
            let new_package = DownloadPackage::new(sdm_core::safe_path_component(package_name));
            let new_package_id = new_package.id;
            state.packages_mut().push(new_package);
            inner.move_items(&mut state, item_ids, new_package_id, Some(0))
        };
        if moved {
            inner.persist().await;
            inner.reconcile().await;
        }
    }

    /// `move_item`'s multi-select counterpart: moves an ordered batch of items
    /// to a destination package in one atomic step, rather than one
    /// `move_item` call per item (which would re-run `reconcile()`/`persist()`
    /// once per item and let each move's index arithmetic see the others'
    /// already-applied shifts instead of the pre-drop layout the operator
    /// actually dragged from).
    ///
    /// `item_ids` is trusted to already be in the exact relative order they
    /// should land in — this does not re-sort them. The caller is the one place
    /// that knows "list order" as opposed to whatever order a multi-selection
    /// drag session happens to hand back, which is unspecified.
    ///
    /// All-or-nothing, generalizing `move_item`'s single-item refusal: if any
    /// named item does not exist or is `Running`, the whole batch is refused
    /// rather than silently moving a confusing subset.
    ///
    /// `at_index` positions the *whole batch* as one contiguous run within the
    /// destination package's item list, indexed against that list before any
    /// of the batch is removed from wherever it currently sits — same "insert
    /// before whatever is currently at this index" semantics as `move_item`'s
    /// `at_index`. `None` appends the batch at the end.
    pub async fn move_items_to_package(
        &self,
        item_ids: &[Uuid],
        package_id: Uuid,
        at_index: Option<usize>,
    ) {
        let inner = &self.inner;
        let moved = {
            let mut state = inner.lock();
            inner.move_items(&mut state, item_ids, package_id, at_index)
        };
        if moved {
            inner.persist().await;
            inner.reconcile().await;
        }
    }

    /// Applies a new package order. Same idiom as `reorder_items`: any id not
    /// present in `package_ids` keeps its relative order, appended at the end.
    ///
    /// Like Swift's, this persists but does not reconcile.
    pub async fn reorder_packages(&self, package_ids: &[Uuid]) {
        let inner = &self.inner;
        {
            let mut state = inner.lock();
            let existing = std::mem::take(state.packages_mut());
            *state.packages_mut() = apply_order(
                existing,
                package_ids,
                |package| package.id,
                |package, position| package.position = position,
            );
        }
        inner.persist().await;
    }

    /// Removes one item: stops its runner if it is currently running, always
    /// drops its resume sidecar (a sidecar with no owning list entry is just an
    /// orphan), and — only when `delete_file` is true — trashes whatever bytes
    /// it wrote (both the in-progress `.incomplete` file and, if it had already
    /// finished, the final file) and, if that was the last item in its
    /// package, trashes the now-empty package folder too.
    ///
    /// `delete_file == false` is deliberately non-destructive: it only edits
    /// the list, so "remove from list" can never surprise someone by taking
    /// their downloaded bytes with it.
    pub async fn remove_item(&self, item_id: Uuid, delete_file: bool) {
        let inner = &self.inner;
        let (item, folder, guard) = {
            let mut state = inner.lock();
            let Some((package, item)) = state.find_item(item_id) else {
                return;
            };
            let found = (
                item.clone(),
                state.settings().download_folder.join_name(&package.name),
            );
            let guard = RemovalGuard::new(inner, &mut state, vec![item_id]);
            (found.0, found.1, guard)
        };

        inner.stop_runner_if_running(item_id).await;

        // Sidecars go regardless of `delete_file` (an orphan sidecar is
        // useless); bytes only when asked.
        for url in item_artefact_urls(&item, &folder) {
            if is_sidecar(&url) {
                ResumeSidecar::remove(&url);
            } else if delete_file {
                inner.trash_if_exists(&url);
            }
        }

        let package_became_empty = {
            let mut state = inner.lock();
            let Some((package_index, item_index)) = state.location_of(item_id) else {
                return;
            };
            state.packages_mut()[package_index].items.remove(item_index);
            let became_empty = state.packages()[package_index].items.is_empty();
            if became_empty {
                state.packages_mut().remove(package_index);
            }
            state.clear_item_bookkeeping(item_id);
            became_empty
        };
        drop(guard);
        if delete_file && package_became_empty {
            inner.remove_folder_if_empty(&folder);
        }

        inner.persist().await;
        inner.reconcile().await;
    }

    /// Removes an entire package. `delete_files` trashes the whole package
    /// folder in one move rather than per-item — the folder holds every item's
    /// bytes and sidecar together, so this is both simpler and atomic compared
    /// to removing them one at a time.
    pub async fn remove_package(&self, package_id: Uuid, delete_files: bool) {
        let inner = &self.inner;
        let (package, folder, guard) = {
            let mut state = inner.lock();
            let Some(package) = state
                .packages()
                .iter()
                .find(|package| package.id == package_id)
            else {
                return;
            };
            let found = (
                package.clone(),
                state.settings().download_folder.join_name(&package.name),
            );
            let ids = found.0.items.iter().map(|item| item.id).collect();
            let guard = RemovalGuard::new(inner, &mut state, ids);
            (found.0, found.1, guard)
        };
        for item in &package.items {
            inner.stop_runner_if_running(item.id).await;
        }

        if delete_files {
            for item in &package.items {
                for url in item_artefact_urls(item, &folder)
                    .into_iter()
                    .filter(|url| is_sidecar(url))
                {
                    ResumeSidecar::remove(&url);
                }
            }
            inner.trash_if_exists(&folder);
        }

        {
            let mut state = inner.lock();
            for item in &package.items {
                state.clear_item_bookkeeping(item.id);
            }
            state
                .packages_mut()
                .retain(|existing| existing.id != package_id);
        }
        drop(guard);
        inner.persist().await;
        inner.reconcile().await;
    }

    /// Restarts a download from zero: stops it if running, discards its
    /// sidecar and any partial bytes, and resets it — leaving it exactly where
    /// it was in the schedule rather than starting it.
    ///
    /// A reset must not itself start a download: `Queued` stays `Queued` and
    /// `Stopped` stays `Stopped` (an enabled item was previously
    /// unconditionally re-queued here, which auto-started it the moment its
    /// slot came up — surprising for a reset, which is about discarding
    /// progress, not scheduling). A disabled item still lands `Stopped`
    /// regardless of what it was reset from, matching `set_enabled`'s own
    /// "disabled means not running" rule. `Running` is stopped first as
    /// always, then treated as `Queued` — the UI is expected to disable the
    /// Reset action on a running item in the first place, so this is a
    /// backstop, not the normal path.
    pub async fn reset_download(&self, item_id: Uuid) {
        let inner = &self.inner;
        let (item, folder, prior_state, guard) = {
            let mut state = inner.lock();
            let Some((package, item)) = state.find_item(item_id) else {
                return;
            };
            let found = (
                item.clone(),
                state.settings().download_folder.join_name(&package.name),
                item.state.clone(),
            );
            let guard = RemovalGuard::new(inner, &mut state, vec![item_id]);
            (found.0, found.1, found.2, guard)
        };

        inner.stop_runner_if_running(item_id).await;

        // A reset discards *all* progress: every component's part file,
        // `.incomplete`, and `.sdmpart`, plus any finalized output. Missing
        // one (the old code only touched the output filename) left stale
        // parts that made the next resume fail with "file already exists".
        for url in item_artefact_urls(&item, &folder) {
            if is_sidecar(&url) {
                ResumeSidecar::remove(&url);
            } else {
                inner.trash_if_exists(&url);
            }
        }

        {
            let mut state = inner.lock();
            state.failed_attempts_mut().remove(&item_id);
            state.retry_hold_ticks_mut().remove(&item_id);
            state.last_failure_mut().remove(&item_id);
            state.sampled_bytes.remove(&item_id);
            state.checkpoint_failures_mut().remove(&item_id);
            state.attempt_start_bytes.remove(&item_id);
            state.missing_files_mut().remove(&item_id);

            state.mutate_item(item_id, |item| {
                for component in &mut item.components {
                    component.completed = RangeSet::new();
                    component.total_bytes = None;
                    component.is_resumable = None;
                    component.validator = None;
                }
                item.state = if item.is_enabled
                    && matches!(prior_state, ItemState::Queued | ItemState::Running)
                {
                    ItemState::Queued
                } else {
                    ItemState::Stopped
                };
            });
        }
        // Schedulable again; the `reconcile()` below is what may restart it.
        drop(guard);
        inner.persist().await;
        inner.reconcile().await;
    }
}
