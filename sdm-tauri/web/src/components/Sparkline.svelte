<script lang="ts">
  import { sparklinePoints } from "../lib/graphs";

  /** One 1 px `graph-stroke` path, y-scaled to its own maximum, clipped to its box. */
  let {
    samples,
    width = 64,
    height = 16,
  }: { samples: number[]; width?: number; height?: number } = $props();

  const points = $derived(sparklinePoints(samples, width, height));
</script>

<svg {width} {height} viewBox="0 0 {width} {height}" aria-hidden="true">
  {#if points}
    <polyline
      {points}
      fill="none"
      stroke="var(--graph-stroke)"
      stroke-width="1"
    />
  {/if}
</svg>

<style>
  svg {
    display: block;
    flex: none;
    overflow: hidden;
  }
</style>
