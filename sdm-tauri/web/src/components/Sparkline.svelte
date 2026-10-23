<script lang="ts">
  let { samples }: { samples: number[] } = $props();

  const WIDTH = 64;
  const HEIGHT = 16;

  const points = $derived.by(() => {
    if (samples.length < 2) return "";
    const peak = Math.max(1, ...samples);
    const step = WIDTH / (samples.length - 1);
    return samples
      .map((value, index) => {
        const y = HEIGHT - (Math.max(0, value) / peak) * HEIGHT;
        return `${(index * step).toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
  });
</script>

<svg
  viewBox="0 0 {WIDTH} {HEIGHT}"
  preserveAspectRatio="none"
  aria-hidden="true"
>
  {#if points}
    <polyline {points} fill="none" stroke="var(--graph-stroke)" />
  {/if}
</svg>

<style>
  svg {
    display: block;
    width: 64px;
    height: 16px;
  }
  polyline {
    stroke-width: 1;
    vector-effect: non-scaling-stroke;
  }
</style>
