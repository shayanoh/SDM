<script lang="ts">
  import { formatBytes, formatEta, formatSpeed } from "../lib/format";
  import type { PackageModel } from "../lib/store.svelte";

  let { pkg }: { pkg: PackageModel } = $props();

  const eta = $derived(formatEta(pkg.etaSeconds));
</script>

<div class="package">
  <span class="name">{pkg.name}</span>
  <span class="figures">
    {formatBytes(pkg.completedBytes)} / {formatBytes(pkg.totalBytes)}
  </span>
  <span class="figures">{formatSpeed(pkg.bytesPerSecond)}</span>
  {#if eta}<span class="figures">{eta} left</span>{/if}
</div>

<style>
  .package {
    height: var(--row-height);
    box-sizing: border-box;
    padding: 0 12px;
    display: flex;
    align-items: center;
    gap: 12px;
    font-weight: 600;
    background: var(--surface-secondary);
    border-bottom: 1px solid var(--border);
  }
  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .figures {
    font-weight: 400;
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
  }
</style>
