<script lang="ts">
  import type { DeletionInfo, DeletionTarget } from "../bindings";
  import { errorMessage, useBridge, useUi } from "../lib/context";
  import { formatBytes } from "../lib/format";
  import { detectPlatform } from "../lib/platform";
  import Icon from "./Icon.svelte";
  import MiddleEllipsis from "./MiddleEllipsis.svelte";
  import Modal from "./Modal.svelte";

  /**
   * "Remove and Delete": says exactly how many files and bytes will be trashed
   * and lists their names, instead of asking to trust a generic prompt.
   */
  let { target, info }: { target: DeletionTarget; info: DeletionInfo } =
    $props();

  const bridge = useBridge();
  const ui = useUi();
  const bin = detectPlatform() === "windows" ? "Recycle Bin" : "Trash";

  const count = $derived(info.names.length);

  function cancel() {
    ui.modal = null;
  }

  async function confirm() {
    // Read the props first: closing the modal unmounts this component.
    const chosen = target;
    ui.modal = null;
    try {
      if (chosen.kind === "items") await bridge.removeItems(chosen.ids, true);
      else await bridge.removePackage(chosen.id, true);
    } catch (error) {
      ui.report(errorMessage(error));
    }
  }
</script>

<Modal onCancel={cancel}>
  <div class="head">
    <span class="trash"
      ><Icon name="trash-2" size={34} strokeWidth={1.6} /></span
    >
    <div>
      <h2 id="modal-title">{info.title}</h2>
      {#if count === 0}
        <p class="sub">No files will be moved to the {bin}</p>
      {:else}
        <p class="sub">
          {count} file{count === 1 ? "" : "s"} · {formatBytes(info.totalBytes ?? 0)} will
          be moved to the {bin}.
        </p>
      {/if}
    </div>
  </div>
  {#if count > 0}
    <div class="names">
      {#each info.names as name (name)}
        <div class="file"><MiddleEllipsis text={name} /></div>
      {/each}
    </div>
  {/if}
  <div class="buttons">
    <button type="button" data-autofocus onclick={cancel}>Cancel</button>
    <button type="button" class="danger" data-default onclick={confirm}>
      {count === 0 ? "Remove from List" : `Move to ${bin}`}
    </button>
  </div>
</Modal>

<style>
  .head {
    display: flex;
    gap: 14px;
    align-items: flex-start;
  }
  .trash {
    color: var(--status-failed);
  }
  h2 {
    margin: 0 0 4px;
    font-size: 17px;
  }
  .sub {
    margin: 0;
    color: var(--text-secondary);
  }
  .names {
    max-height: 180px;
    margin-top: 16px;
    padding: 10px;
    overflow-y: auto;
    background: color-mix(in srgb, var(--surface-primary) 50%, transparent);
    border-radius: 8px;
  }
  .file {
    padding: 2px 0;
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
  .danger {
    color: var(--text-primary);
    background: var(--accent);
    border-color: transparent;
  }
</style>
