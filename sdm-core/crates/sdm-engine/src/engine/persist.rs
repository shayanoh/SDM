use sdm_core::ItemState;

use super::Inner;
use crate::state_store::PersistedState;

impl Inner {
    /// Queues the current package graph for durable storage and starts (or
    /// leaves running) the debounce window `tick()` drains.
    ///
    /// `Queued`/`Running` are squashed to `Stopped` in what actually gets
    /// written — nothing "in flight" or "about to run" is ever true of a
    /// process that isn't currently alive to act on it, so the durable
    /// snapshot should not claim otherwise. This is also what makes restart
    /// behavior fall out of existing primitives with no dedicated mechanism:
    /// every restored item lands `Stopped` (or terminal), and "resume
    /// downloads automatically on launch" is then just `resume_all()` called
    /// once after `restore()` — see `EngineController`. Live in-memory
    /// `packages` is untouched; only the copy handed to the store is
    /// transformed.
    pub(super) async fn persist(&self) {
        // Swift issues `stateStore.save` from inside the actor, so saves reach
        // the store in the order their snapshots were taken. Taking the
        // snapshot and saving it under one gate keeps that order here.
        let Ok(_ordering) = self.persist_gate.acquire().await else {
            return;
        };
        let mut for_storage = self.lock().packages().to_vec();
        for package in &mut for_storage {
            for item in &mut package.items {
                if matches!(item.state, ItemState::Queued | ItemState::Running) {
                    item.state = ItemState::Stopped;
                }
            }
        }
        self.state_store
            .save(PersistedState::new(for_storage))
            .await;
        let mut state = self.lock();
        if state.ticks_since_pending_change.is_none() {
            state.ticks_since_pending_change = Some(0);
        }
    }
}
