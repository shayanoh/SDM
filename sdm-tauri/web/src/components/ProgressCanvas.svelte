<script lang="ts">
  let {
    completed,
    total,
  }: {
    completed: { start: number; end: number }[];
    total: number | null;
  } = $props();

  const HEIGHT = 8;
  let canvas: HTMLCanvasElement | undefined = $state();
  let width = $state(0);

  function color(styles: CSSStyleDeclaration, name: string): string | null {
    const value = styles.getPropertyValue(name).trim();
    return value === "" ? null : value;
  }

  $effect(() => {
    // Reads `completed`, `total` and `width`, so any of them redraws the bar.
    if (!canvas || !(width > 0)) return;
    const ratio = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(HEIGHT * ratio);
    const context = canvas.getContext("2d");
    if (!context) return;
    context.scale(ratio, ratio);
    const styles = getComputedStyle(canvas);

    const track = color(styles, "--surface-tertiary");
    if (track) {
      context.fillStyle = track;
      context.fillRect(0, 0, width, HEIGHT);
    }
    const fill = color(styles, "--completed-segment-fill");
    if (!fill || total === null || !(total > 0)) return;
    context.fillStyle = fill;
    for (const range of completed) {
      const start = Math.max(0, range.start);
      const end = Math.min(total, range.end);
      if (!Number.isFinite(start) || !Number.isFinite(end) || end <= start)
        continue;
      const x = (start / total) * width;
      context.fillRect(
        x,
        0,
        Math.max(1, ((end - start) / total) * width),
        HEIGHT,
      );
    }
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
    border-radius: 4px;
  }
</style>
