<script lang="ts">
  import { onDestroy } from "svelte";
  import { errorMessage, useBridge, useStore, useUi } from "../lib/context";
  import { statusText, summarize } from "../lib/details";
  import { canOpen } from "../lib/eligibility";
  import { formatBytes, formatSpeed } from "../lib/format";
  import { fileManagerName, detectPlatform } from "../lib/platform";
  import FlexSparkline from "./FlexSparkline.svelte";
  import SegmentedBar from "./SegmentedBar.svelte";

  /**
   * The bottom panel: details of the one selected item, the aggregate of
   * several, or a hint. It tells the engine which items it shows, so it gets
   * their full speed history, and stops asking when it goes away.
   */
  let { itemIds }: { itemIds: string[] } = $props();

  const OWNER = "main-details";

  const bridge = useBridge();
  const store = useStore();
  const ui = useUi();
  const manager = fileManagerName(detectPlatform());

  const items = $derived(itemIds.flatMap((id) => store.items.get(id) ?? []));
  const single = $derived(items.length === 1 ? items[0] : null);
  const summary = $derived(items.length > 1 ? summarize(items) : null);
  const singlePackage = $derived(
    single ? store.packageOfItem(single.id) : undefined,
  );
  const sharedFolder = $derived.by(() => {
    const folders = new Set(
      items.flatMap((item) => store.packageOfItem(item.id)?.folder ?? []),
    );
    return folders.size === 1 ? [...folders][0] : null;
  });
  const sharedPackage = $derived(
    items.length > 0 ? store.packageOfItem(items[0].id) : undefined,
  );

  $effect(() => {
    const ids = itemIds;
    bridge
      .setDetailItems(OWNER, ids)
      .catch((error) => ui.report(errorMessage(error)));
  });
  onDestroy(() => {
    bridge.setDetailItems(OWNER, []).catch(() => {});
  });

  async function act(action: () => Promise<void>) {
    try {
      await action();
    } catch (error) {
      ui.report(errorMessage(error));
    }
  }

  const percent = (fraction: number) => (fraction * 100).toFixed(1);
</script>

<div class="panel">
  {#if items.length === 0}
    <p class="hint">Select an item to view its details here</p>
  {:else if single}
    {@const status = statusText(single)}
    {@const total = single.totalBytes ?? 0}
    <div class="grid">
      <div class="wide graph">
        <FlexSparkline samples={single.detailHistory ?? single.speedHistory} />
        <span class="caption mono speed"
          >{formatSpeed(single.bytesPerSecond)}</span
        >
      </div>
      <div class="wide">
        <SegmentedBar completed={single.completed} total={single.totalBytes} />
      </div>

      <span>Status</span>
      <span class="selectable status tone-{status.tone}">{status.text}</span>

      <span>Filename</span>
      <div class="value">
        <span class="text selectable">{single.filename}</span>
        <button
          type="button"
          disabled={!canOpen(single)}
          onclick={() => act(() => bridge.openItem(single.id))}>Open</button
        >
      </div>

      {#if single.assembly === "mux"}
        <span>Assembly</span>
        <span class="muted"
          >{single.isAssembling
            ? "Running ffmpeg…"
            : "Video + audio, muxed with ffmpeg"}</span
        >
      {/if}

      <span>Destination Folder</span>
      <div class="value">
        <span class="text selectable">{singlePackage?.folder ?? "—"}</span>
        <button
          type="button"
          disabled={!singlePackage}
          onclick={() =>
            singlePackage && act(() => bridge.revealPackage(singlePackage.id))}
        >
          Show in {manager}
        </button>
      </div>

      <span>URL</span>
      <div class="value">
        <span class="text selectable">{single.url}</span>
        <button
          type="button"
          onclick={() => act(() => navigator.clipboard.writeText(single.url))}
          >Copy URL</button
        >
      </div>

      <span>Media Info</span>
      <span class="muted selectable"
        >{single.metadata ? single.metadata : "—"}</span
      >

      <span>Progress</span>
      <span class="mono">
        {formatBytes(single.completedBytes)}/{formatBytes(total)} - {percent(
          total > 0 ? single.completedBytes / total : 0,
        )}%
      </span>

      <span>Segments</span>
      <span>{single.activeSegments}/{single.configuredSegments}</span>

      <span>Can Resume?</span>
      <span
        >{single.isResumable === null
          ? "-"
          : single.isResumable
            ? "Yes"
            : "No"}</span
      >
    </div>
  {:else if summary}
    <div class="grid">
      <div class="wide graph">
        <FlexSparkline samples={summary.speedHistory} />
        <span class="caption mono speed"
          >{formatSpeed(summary.bytesPerSecond)}</span
        >
      </div>
      <div class="wide">
        <SegmentedBar
          completed={summary.completed}
          total={summary.totalBytes}
        />
      </div>

      <span>Item States</span>
      <div class="states">
        {#if summary.states.completed > 0}<span class="online"
            >{summary.states.completed} Completed</span
          >{/if}
        {#if summary.states.running > 0}<span class="accent"
            >{summary.states.running} Running</span
          >{/if}
        {#if summary.states.retrying > 0}<span class="faulty"
            >{summary.states.retrying} Retrying</span
          >{/if}
        {#if summary.states.queued > 0}<span class="muted"
            >{summary.states.queued} Queued</span
          >{/if}
        {#if summary.states.stopped > 0}<span class="muted"
            >{summary.states.stopped} Stopped</span
          >{/if}
        {#if summary.states.failed > 0}<span class="failed"
            >{summary.states.failed} Failed</span
          >{/if}
      </div>

      {#if sharedFolder}
        <span>Destination Folder</span>
        <div class="value">
          <span class="text selectable">{sharedFolder}</span>
          <button
            type="button"
            disabled={!sharedPackage}
            onclick={() =>
              sharedPackage &&
              act(() => bridge.revealPackage(sharedPackage.id))}
          >
            Show in {manager}
          </button>
        </div>
      {/if}

      <span>Total Progress</span>
      <span class="mono">
        {formatBytes(summary.downloadedBytes)}/{formatBytes(summary.totalBytes)} -
        {percent(summary.averageFraction)}%
      </span>

      <span>Total Segments</span>
      <span>{summary.activeSegments}/{summary.configuredSegments}</span>
    </div>
  {/if}
</div>

<style>
  .panel {
    height: 100%;
    overflow-y: auto;
    background: var(--surface-secondary);
  }
  .hint {
    display: grid;
    place-items: center;
    height: 100%;
    margin: 0;
    padding: 20px;
  }
  .grid {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 4px 10px;
    padding: 12px 14px 8px;
    align-items: center;
  }
  .wide {
    grid-column: 1 / -1;
  }
  .graph {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .speed {
    min-width: 64px;
    text-align: right;
  }
  .value {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .text {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .status {
    white-space: pre-line;
  }
  .tone-failed {
    color: var(--status-failed);
  }
  .tone-faulty {
    color: var(--status-faulty);
  }
  .muted {
    color: var(--text-secondary);
  }
  .states {
    display: flex;
    gap: 10px;
  }
  .online {
    color: var(--status-online);
  }
  .accent {
    color: var(--accent);
  }
  .faulty {
    color: var(--status-faulty);
  }
  .failed {
    color: var(--status-failed);
  }
</style>
