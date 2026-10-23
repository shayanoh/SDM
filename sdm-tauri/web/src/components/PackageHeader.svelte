<script lang="ts">
  import { useUi } from "../lib/context";
  import { formatBytes, formatEta, formatSpeed } from "../lib/format";
  import type { PackageModel } from "../lib/store.svelte";
  import type { ListTab } from "../lib/ui.svelte";
  import Icon from "./Icon.svelte";
  import Sparkline from "./Sparkline.svelte";

  /**
   * A package's disclosure header. On Downloads the figures are the engine's
   * live aggregate; on Completed (`live` off) they are summed by the caller
   * over just the completed items.
   */
  let {
    pkg,
    tab,
    live,
    completedBytes,
    totalBytes,
  }: {
    pkg: PackageModel;
    tab: ListTab;
    live: boolean;
    completedBytes: number;
    totalBytes: number;
  } = $props();

  const ui = useUi();
  const collapsed = $derived(ui.isCollapsed(tab, pkg.id));
  const eta = $derived(live ? formatEta(pkg.etaSeconds) : "");

  function toggle(event: MouseEvent) {
    event.stopPropagation();
    ui.toggleCollapsed(tab, pkg.id);
  }
</script>

<div class="header">
  <button
    type="button"
    class="chevron"
    aria-label="{collapsed ? 'Expand' : 'Collapse'} {pkg.name}"
    aria-expanded={!collapsed}
    onclick={toggle}
  >
    <Icon
      name={collapsed ? "chevron-right" : "chevron-down"}
      size={12}
      strokeWidth={2.5}
    />
  </button>
  <div class="text">
    <div class="name title3" dir="auto">{pkg.name}</div>
    <div class="figures">
      <span class="caption mono muted"
        >{formatBytes(completedBytes)} / {formatBytes(totalBytes)}</span
      >
      {#if eta}<span>ETA: {eta}</span>{/if}
    </div>
  </div>
  <span class="spacer"></span>
  <span class="speed mono"
    >{live ? formatSpeed(pkg.bytesPerSecond) : formatSpeed(0)}</span
  >
  <Sparkline samples={live ? pkg.speedHistory : []} width={60} height={20} />
</div>

<style>
  .header {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 100%;
    padding: 6px 16px 6px 12px;
  }
  .chevron {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    padding: 0;
    color: var(--text-secondary);
    background: none;
    border: none;
  }
  .text {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .name {
    font-weight: 700;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .figures {
    display: flex;
    gap: 8px;
    align-items: baseline;
    white-space: nowrap;
  }
  .muted {
    color: var(--text-secondary);
  }
  .spacer {
    flex: 1;
  }
  .speed {
    white-space: nowrap;
  }
</style>
