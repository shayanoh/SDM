<script lang="ts">
  /** A small determinate ring, like SwiftUI's `.circular` progress view. */
  let { fraction, size = 14 }: { fraction: number; size?: number } = $props();

  const STROKE = 2;
  const radius = $derived((size - STROKE) / 2);
  const circumference = $derived(2 * Math.PI * radius);
  const clamped = $derived(
    Number.isFinite(fraction) ? Math.min(1, Math.max(0, fraction)) : 0,
  );
</script>

<svg
  class="ring"
  width={size}
  height={size}
  viewBox="0 0 {size} {size}"
  aria-hidden="true"
>
  <circle
    class="track"
    cx={size / 2}
    cy={size / 2}
    r={radius}
    fill="none"
    stroke-width={STROKE}
  />
  <circle
    class="fill"
    cx={size / 2}
    cy={size / 2}
    r={radius}
    fill="none"
    stroke-width={STROKE}
    stroke-linecap="round"
    stroke-dasharray="{circumference * clamped} {circumference}"
    transform="rotate(-90 {size / 2} {size / 2})"
  />
</svg>

<style>
  .ring {
    flex: none;
  }
  .track {
    stroke: var(--border);
  }
  .fill {
    stroke: var(--progress-fill);
  }
</style>
