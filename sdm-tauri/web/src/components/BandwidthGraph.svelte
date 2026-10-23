<script lang="ts">
  import { runningAverage, windowed } from "../lib/graphs";

  /** Samples per second the engine publishes (`AppTiming::TICKS_PER_SECOND`). */
  const TICKS_PER_SECOND = 5;
  const HEIGHT = 40;

  /**
   * The sidebar's global graph: the speed history as an area at 25 % opacity
   * under a 1.5 px two-second running-average line, one sample per pixel of
   * width, as `BandwidthGraph.swift` draws it.
   */
  let { history }: { history: number[] } = $props();

  let canvas: HTMLCanvasElement | undefined = $state();
  let width = $state(0);

  function color(styles: CSSStyleDeclaration, name: string): string | null {
    const value = styles.getPropertyValue(name).trim();
    return value === "" ? null : value;
  }

  $effect(() => {
    if (!canvas || !(width > 0)) return;
    const ratio = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(HEIGHT * ratio);
    const context = canvas.getContext("2d");
    if (!context) return;
    context.scale(ratio, ratio);
    const samples = windowed(history, width);
    if (samples.length < 2) return;
    const styles = getComputedStyle(canvas);
    const peak = Math.max(1, ...samples);
    const step = width / (samples.length - 1);
    const y = (value: number) => HEIGHT - (value / peak) * HEIGHT;

    const stroke = color(styles, "--graph-stroke");
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
    const average = color(styles, "--graph-average-stroke");
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
