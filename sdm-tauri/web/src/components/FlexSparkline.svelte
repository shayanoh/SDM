<script lang="ts">
  import Sparkline from "./Sparkline.svelte";

  /** A sparkline that takes the width of its container. */
  let { samples, height = 48 }: { samples: number[]; height?: number } =
    $props();

  let width = $state(0);
</script>

<div class="flex" bind:clientWidth={width} style:height="{height}px">
  <Sparkline {samples} width={Math.max(0, Math.floor(width))} {height} />
</div>

<style>
  .flex {
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }
</style>
