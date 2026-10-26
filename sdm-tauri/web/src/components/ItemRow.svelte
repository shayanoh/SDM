<script lang="ts">
  import { useStore } from "../lib/context";
  import { formatBytes, formatEta, formatSpeed } from "../lib/format";
  import { describeItem, itemEta, statusTone } from "../lib/rowText";
  import type { ItemModel } from "../lib/store.svelte";
  import Icon from "./Icon.svelte";
  import SegmentedBar from "./SegmentedBar.svelte";
  import Sparkline from "./Sparkline.svelte";
  import StateIcon from "./StateIcon.svelte";

  let { item }: { item: ItemModel } = $props();

  const store = useStore();

  const fraction = $derived(
    item.totalBytes !== null && item.totalBytes > 0
      ? item.completedBytes / item.totalBytes
      : 0,
  );
  const status = $derived(
    describeItem(item, item.isRetrying ? item.retryHoldSeconds : null),
  );
  const tone = $derived(statusTone(item));
  const eta = $derived(
    item.state.kind === "running" || item.state.kind === "queued"
      ? itemEta(item, store.globalRecentSpeed)
      : 0,
  );
</script>

<div class="item" class:disabled={!item.isEnabled} data-state={item.state.kind}>
  <div class="icon">
    <StateIcon
      state={item.state}
      {fraction}
      isRetrying={item.isRetrying}
      isAssembling={item.isAssembling}
    />
  </div>
  <div class="body">
    <div class="line">
      <span
        class="name"
        class:struck={!item.isEnabled}
        dir="auto"
        title={item.url}
      >
        {item.filename}
      </span>
      {#if item.isResumable === false}
        <span class="caption muted">not resumable</span>
      {/if}
      {#if item.fileMissing}
        <span class="caption faulty">
          <Icon name="triangle-alert" size={11} /> file missing
        </span>
      {/if}
      <span class="spacer"></span>
      <span class="caption mono muted"
        >{item.activeSegments}/{item.configuredSegments} seg</span
      >
      <span class="caption mono">{formatSpeed(item.bytesPerSecond)}</span>
      <Sparkline samples={item.speedHistory} width={48} height={16} />
    </div>
    <SegmentedBar completed={item.completed} total={item.totalBytes} />
    <div class="line">
      <span class="caption status tone-{tone}">{status}</span>
      {#if item.checkpointFailure !== null}
        <span class="caption faulty status">{item.checkpointFailure}</span>
      {/if}
      {#if eta > 0}
        <span class="caption muted">ETA: {formatEta(eta)}</span>
      {/if}
      <span class="spacer"></span>
      <span class="caption mono muted">
        {formatBytes(item.completedBytes)} / {formatBytes(item.totalBytes ?? 0)}
      </span>
    </div>
  </div>
</div>

<style>
  .item {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    height: 100%;
    padding: 8px 20px 8px 43px;
  }
  .item.disabled {
    opacity: 0.55;
  }
  .icon {
    flex: none;
    width: 20px;
    padding-top: 1px;
  }
  .body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name.struck {
    color: var(--text-secondary);
    text-decoration: line-through;
  }
  .spacer {
    flex: 1;
  }
  .muted {
    color: var(--text-secondary);
  }
  .faulty,
  .tone-faulty {
    color: var(--status-faulty);
  }
  .tone-failed {
    color: var(--status-failed);
  }
  .tone-secondary {
    color: var(--text-secondary);
  }
  .status {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
  }
</style>
