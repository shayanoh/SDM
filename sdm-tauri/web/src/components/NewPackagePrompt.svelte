<script lang="ts">
  import { errorMessage, useBridge, useUi } from "../lib/context";
  import Modal from "./Modal.svelte";

  let { ids }: { ids: string[] } = $props();

  const bridge = useBridge();
  const ui = useUi();

  let name = $state("");
  const trimmed = $derived(name.trim());

  function cancel() {
    ui.modal = null;
  }

  async function confirm() {
    if (!trimmed) return;
    // Read the props first: closing the modal unmounts this component.
    const wanted = ids;
    const packageName = trimmed;
    ui.modal = null;
    try {
      await bridge.moveItemsToNewPackage(wanted, packageName);
    } catch (error) {
      ui.report(errorMessage(error));
    }
  }
</script>

<Modal onCancel={cancel}>
  <h2 id="modal-title">Enter new package name:</h2>
  <input
    type="text"
    class="selectable"
    data-autofocus
    aria-label="Package name"
    bind:value={name}
  />
  <div class="buttons">
    <button type="button" data-default disabled={!trimmed} onclick={confirm}
      >Move to New Package</button
    >
    <button type="button" onclick={cancel}>Cancel</button>
  </div>
</Modal>

<style>
  h2 {
    margin: 0 0 12px;
    font-size: 15px;
  }
  input {
    width: 250px;
    padding: 5px 8px;
    color: var(--text-primary);
    background: var(--surface-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
  }
  button {
    padding: 4px 14px;
  }
</style>
