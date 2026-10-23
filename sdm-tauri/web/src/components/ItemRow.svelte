<script lang="ts">
  import { errorMessage, useBridge, useUi } from "../lib/context";
  import { formatBytes, formatSpeed } from "../lib/format";
  import type { ItemModel } from "../lib/store.svelte";
  import SegmentedBar from "./SegmentedBar.svelte";
  import Sparkline from "./Sparkline.svelte";

  let { item, selected }: { item: ItemModel; selected: boolean } = $props();

  const bridge = useBridge();
  const ui = useUi();

  const kind = $derived(item.state.kind);
  const label = $derived.by(() => {
    const state = item.state;
    switch (state.kind) {
      case "running":
        return "Running";
      case "queued":
        return "Queued";
      case "stopped":
        return "Paused";
      case "completed":
        return "Done";
      case "failed":
        return `Failed: ${state.reason}`;
    }
  });
  const active = $derived(kind === "running" || kind === "queued");

  async function run(action: () => Promise<void>) {
    try {
      await action();
    } catch (error) {
      ui.report(errorMessage(error));
    }
  }
</script>

<div class="item" class:selected data-state={kind}>
  <div class="top">
    <span class="name" title={item.url}>{item.filename}</span>
    <span class="figures">
      {formatBytes(item.completedBytes)} / {item.totalBytes === null
        ? "—"
        : formatBytes(item.totalBytes)}
    </span>
    <span class="figures">{formatSpeed(item.bytesPerSecond)}</span>
    <span class="state">{label}</span>
    <Sparkline samples={item.speedHistory} />
    <span class="actions">
      {#if kind !== "completed"}
        <button
          type="button"
          onclick={() =>
            run(() =>
              active
                ? bridge.stopItems([item.id])
                : bridge.startItems([item.id]),
            )}
        >
          {active ? "Pause" : "Resume"}
        </button>
      {/if}
      {#if kind === "failed"}
        <button
          type="button"
          onclick={() => run(() => bridge.retryItems([item.id]))}
        >
          Retry
        </button>
      {/if}
      <button
        type="button"
        onclick={() => run(() => bridge.removeItems([item.id], false))}
      >
        Remove
      </button>
    </span>
  </div>
  <SegmentedBar completed={item.completed} total={item.totalBytes} />
</div>

<style>
  .item {
    height: var(--row-height);
    box-sizing: border-box;
    padding: 6px 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    border-bottom: 1px solid var(--border);
  }
  .item.selected {
    background: var(--selection-tint);
  }
  .top {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .figures,
  .state {
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
  }
  [data-state="failed"] .state {
    color: var(--status-failed);
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  button {
    padding: 1px 8px;
  }
</style>
