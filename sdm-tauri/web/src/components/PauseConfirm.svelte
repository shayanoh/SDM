<script lang="ts">
  import { errorMessage, useBridge, useStore, useUi } from "../lib/context";
  import { canStop } from "../lib/eligibility";
  import Modal from "./Modal.svelte";

  /** Pausing a non-resumable download loses its progress for good: ask first. */
  let { ids }: { ids: string[] } = $props();

  const bridge = useBridge();
  const store = useStore();
  const ui = useUi();

  function cancel() {
    ui.modal = null;
  }

  async function confirm() {
    // Read the props first: closing the modal unmounts this component.
    const wanted = ids;
    ui.modal = null;
    // The selection may have changed state while the question was open.
    const targets = wanted.filter((id) => {
      const item = store.items.get(id);
      return item !== undefined && canStop(item);
    });
    if (targets.length === 0) return;
    try {
      await bridge.stopItems(targets);
    } catch (error) {
      ui.report(errorMessage(error));
    }
  }
</script>

<Modal onCancel={cancel}>
  <h2 id="modal-title">Pause non-resumable download?</h2>
  <p>
    One or more downloads cannot be resumed. Pausing them now will lose their
    progress permanently.
  </p>
  <div class="buttons">
    <button type="button" data-default onclick={confirm}>Pause</button>
    <button type="button" data-autofocus onclick={cancel}>Cancel</button>
  </div>
</Modal>

<style>
  h2 {
    margin: 0 0 8px;
    font-size: 17px;
  }
  p {
    margin: 0 0 16px;
    color: var(--text-secondary);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  button {
    padding: 4px 14px;
  }
</style>
