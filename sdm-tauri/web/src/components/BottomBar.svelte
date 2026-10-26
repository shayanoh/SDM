<script lang="ts">
  import { errorMessage, useBridge, useStore, useUi } from "../lib/context";
  import { allDownloadableStopped, downloadable } from "../lib/eligibility";
  import { formatSpeed } from "../lib/format";
  import type { ListView } from "../lib/views";
  import Icon from "./Icon.svelte";

  /**
   * The Pause/Resume All bar, the global speed and the totals. `showsPauseResume`
   * and `showsLiveAggregates` are off on the Completed tab, where nothing transfers.
   */
  let {
    view,
    showsPauseResume,
    showsLiveAggregates,
  }: {
    view: ListView;
    showsPauseResume: boolean;
    showsLiveAggregates: boolean;
  } = $props();

  const bridge = useBridge();
  const store = useStore();
  const ui = useUi();

  const items = $derived(
    view.packages
      .flatMap((pkg) => pkg.itemIds.map((id) => store.items.get(id)))
      .filter((i) => i !== undefined),
  );
  const toggleable = $derived(downloadable(items));
  const stopped = $derived(allDownloadableStopped(items));

  async function toggle() {
    try {
      await (stopped ? bridge.resumeAll() : bridge.pauseAll());
    } catch (error) {
      ui.report(errorMessage(error));
    }
  }
</script>

<div class="bar">
  {#if showsPauseResume}
    <button
      type="button"
      class="toggle"
      disabled={toggleable.length === 0}
      onclick={toggle}
    >
      <Icon name={stopped ? "play" : "pause"} size={13} />
      {stopped ? "Resume All" : "Pause All"}
    </button>
    <span class="divider"></span>
  {/if}
  <span class="speed title3 mono">
    {showsLiveAggregates
      ? formatSpeed(store.stats?.bytesPerSecond ?? 0)
      : formatSpeed(0)}
  </span>
  <span class="spacer"></span>
  <span class="totals"
    >{view.packages.length} packages, {items.length} items</span
  >
</div>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    background: var(--surface-secondary);
    border-top: 1px solid var(--border);
  }
  .toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .toggle :global(svg) {
    fill: currentColor;
  }
  .divider {
    width: 1px;
    height: 16px;
    background: var(--border);
  }
  .spacer {
    flex: 1;
  }
  .totals {
    color: var(--text-secondary);
  }
</style>
