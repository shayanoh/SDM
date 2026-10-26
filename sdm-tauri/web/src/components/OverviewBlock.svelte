<script lang="ts">
  import { useStore } from "../lib/context";
  import { formatSpeed } from "../lib/format";
  import BandwidthGraph from "./BandwidthGraph.svelte";

  const store = useStore();
  const stats = $derived(store.stats);
  const active = $derived(stats?.runningCount ?? 0);
</script>

<div class="overview">
  {#if active === 0}
    <div class="headline muted">No running downloads</div>
  {:else}
    <div class="headline mono">{formatSpeed(stats?.bytesPerSecond ?? 0)}</div>
  {/if}
  <BandwidthGraph
    history={(stats?.history ?? []).map((sample) => sample ?? 0)}
  />
  <div class="caption muted">{active} active</div>
</div>

<style>
  .overview {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 4px 0;
  }
  .muted {
    color: var(--text-secondary);
  }
</style>
