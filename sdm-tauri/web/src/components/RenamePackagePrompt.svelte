<script lang="ts">
  import { untrack } from "svelte";
  import Modal from "./Modal.svelte";

  /** SwiftUI's "Rename package" alert: a name field, Rename and Cancel. */
  let {
    name,
    onRename,
    onCancel,
  }: {
    name: string;
    onRename: (name: string) => void;
    onCancel: () => void;
  } = $props();

  let value = $state(untrack(() => name));
  const trimmed = $derived(value.trim());

  function rename() {
    if (!trimmed) return;
    // Read the props first: closing the prompt unmounts it and clears what they point at.
    const wanted = trimmed;
    const apply = onRename;
    onCancel();
    apply(wanted);
  }
</script>

<Modal {onCancel}>
  <h2 id="modal-title">Rename package</h2>
  <input
    type="text"
    class="selectable"
    data-autofocus
    aria-label="Name"
    placeholder="Name"
    bind:value
  />
  <div class="buttons">
    <button type="button" data-default disabled={!trimmed} onclick={rename}
      >Rename</button
    >
    <button type="button" onclick={onCancel}>Cancel</button>
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
