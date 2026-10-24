<script lang="ts">
  import Modal from "./Modal.svelte";

  /** The paste field: feeds the same extraction as the clipboard watcher will. */
  let {
    onAdd,
    onCancel,
  }: { onAdd: (text: string) => void; onCancel: () => void } = $props();

  let text = $state("");
  const blank = $derived(text.trim().length === 0);

  function add() {
    if (blank) return;
    // Close first, then hand the text over (the sheet unmounts on close).
    const pasted = text;
    const apply = onAdd;
    onCancel();
    apply(pasted);
  }

  function onKey(event: KeyboardEvent) {
    // Enter is a new line in a multi-line field; Ctrl/Cmd+Enter adds.
    if (event.key === "Enter") {
      event.stopPropagation();
      if (event.ctrlKey || event.metaKey) {
        event.preventDefault();
        add();
      }
    }
  }
</script>

<Modal {onCancel}>
  <h2 id="modal-title" class="headline">Add links</h2>
  <textarea
    class="selectable"
    data-autofocus
    aria-label="Links"
    spellcheck="false"
    bind:value={text}
    onkeydown={onKey}></textarea>
  <div class="buttons">
    <button type="button" onclick={onCancel}>Cancel</button>
    <button type="button" data-default disabled={blank} onclick={add}
      >Add</button
    >
  </div>
</Modal>

<style>
  h2 {
    margin: 0 0 12px;
  }
  textarea {
    display: block;
    width: 420px;
    max-width: 100%;
    min-height: 200px;
    padding: 6px 8px;
    font:
      13px/1.35 ui-monospace,
      "Cascadia Mono",
      Menlo,
      monospace;
    color: var(--text-primary);
    background: var(--surface-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    resize: vertical;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 12px;
  }
  button {
    padding: 4px 14px;
  }
</style>
