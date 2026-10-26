<script lang="ts">
  import { themeColor } from "../lib/colors";
  import { runningAverage, windowed } from "../lib/graphs";

  /** Samples per second the engine publishes (`AppTiming::TICKS_PER_SECOND`). */
  const TICKS_PER_SECOND = 5;
  const DEFAULT_HEIGHT = 40;

  /**
   * The sidebar's global graph: the speed history as an area at 25 % opacity
   * under a 1.5 px two-second running-average line, one sample per pixel of
   * width, as `BandwidthGraph.swift` draws it.
   */
  let {
    history,
    height = DEFAULT_HEIGHT,
  }: { history: number[]; height?: number } = $props();
  const HEIGHT = $derived(height);

  let canvas: HTMLCanvasElement | undefined = $state();
  let width = $state(0);

  $effect(() => {
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
    const samples = windowed(history, width);
    if (samples.length < 2) return;
    const peak = Math.max(1, ...samples);
    const step = width / (samples.length - 1);
    const y = (value: number) => HEIGHT - (value / peak) * HEIGHT;

    const stroke = themeColor(canvas, "--graph-stroke");
    if (stroke) {
      context.beginPath();
      context.moveTo(0, HEIGHT);
      samples.forEach((value, index) => context.lineTo(index * step, y(value)));
      context.lineTo(width, HEIGHT);
      context.closePath();
      context.globalAlpha = 0.25;
      context.fillStyle = stroke;
      context.fill();
      context.globalAlpha = 1;
    }
    const average = themeColor(canvas, "--graph-average-stroke");
    if (average) {
      const line = runningAverage(samples, TICKS_PER_SECOND * 2);
      context.beginPath();
      line.forEach((value, index) =>
        index === 0
          ? context.moveTo(0, y(value))
          : context.lineTo(index * step, y(value)),
      );
      context.lineWidth = 1.5;
      context.strokeStyle = average;
      context.stroke();
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
  }
</style>
