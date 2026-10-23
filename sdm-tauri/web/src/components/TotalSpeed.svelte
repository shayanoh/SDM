<script lang="ts">
  import { useStore } from "../lib/context";
  import { formatSpeed } from "../lib/format";

  // A leaf of its own so the per-tick `stats` read re-renders only this text.
  const store = useStore();
</script>

<span class="speed" data-testid="total-speed">
  {formatSpeed(store.stats?.bytesPerSecond ?? 0)}
</span>

<style>
  .speed {
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
  }
</style>
