<script lang="ts">
  import { themeColor } from "../lib/colors";

  /**
   * The segmented progress bar: the completed byte ranges drawn directly, so
   * it stays right at any segment count. A 6 px fully rounded track in
   * `surface-tertiary` at 60 %, the ranges (at least half a pixel wide) in
   * `progress-fill`, clipped to the track.
   */
  let {
    completed,
    total,
  }: {
    completed: { start: number; end: number }[];
    total: number | null;
  } = $props();

  const HEIGHT = 6;
  let canvas: HTMLCanvasElement | undefined = $state();
  let width = $state(0);

  $effect(() => {
    // Reads `completed`, `total` and `width`, so any of them redraws the bar.
    if (!canvas || !(width > 0)) return;
    const ratio = window.devicePixelRatio || 1;
    const pixelWidth = Math.round(width * ratio);
    const pixelHeight = Math.round(HEIGHT * ratio);
    // Resizing reallocates the backing store, so only do it when the size changed.
    if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
      canvas.width = pixelWidth;
      canvas.height = pixelHeight;
    }
    const context = canvas.getContext("2d");
    if (!context) return;
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, width, HEIGHT);
    context.save();
    context.beginPath();
    context.roundRect(0, 0, width, HEIGHT, HEIGHT / 2);
    context.clip();

    const track = themeColor(canvas, "--surface-tertiary");
    if (track) {
      context.globalAlpha = 0.6;
      context.fillStyle = track;
      context.fillRect(0, 0, width, HEIGHT);
      context.globalAlpha = 1;
    }
    const fill = themeColor(canvas, "--progress-fill");
    if (fill && total !== null && total > 0) {
      context.fillStyle = fill;
      for (const range of completed) {
        const start = Math.max(0, range.start);
        const end = Math.min(total, range.end);
        if (!Number.isFinite(start) || !Number.isFinite(end) || end <= start)
          continue;
        context.fillRect(
          (start / total) * width,
          0,
          Math.max(0.5, ((end - start) / total) * width),
          HEIGHT,
        );
      }
    }
    context.restore();
  });
</script>

<canvas
  bind:this={canvas}
  bind:clientWidth={width}
  style:height="{HEIGHT}px"
  aria-hidden="true"
></canvas>

<style>
  canvas {
    display: block;
    width: 100%;
  }
</style>
