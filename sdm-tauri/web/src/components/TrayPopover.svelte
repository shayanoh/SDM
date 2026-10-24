<script lang="ts">
  import { onMount } from "svelte";
  import type { TrayStatus } from "../bindings";
  import { errorMessage, useBridge } from "../lib/context";
  import { formatSpeed } from "../lib/format";
  import { fitWindow } from "../lib/windowControl";
  import BandwidthGraph from "./BandwidthGraph.svelte";
  import ProgressRing from "./ProgressRing.svelte";

  /** The SwiftUI menu bar panel: speed, graph, running downloads, waiting links, actions. */
  const WIDTH = 280;
  const LISTED = 5;

  const bridge = useBridge();

  let status = $state<TrayStatus | null>(null);
  let error = $state<string | null>(null);
  let height = $state(0);

  const items = $derived((status?.items ?? []).slice(0, LISTED));
  const waiting = $derived(status?.linksWaiting ?? 0);

  async function guarded(action: () => Promise<void>) {
    try {
      await action();
    } catch (failure) {
      error = errorMessage(failure);
    }
  }

  onMount(() => {
    const stop = bridge.onTrayStatus((next) => (status = next));
    void guarded(async () => {
      status = await bridge.trayStatus();
    });
    return stop;
  });

  // The window is as tall as the panel, and stays against the tray icon.
  $effect(() => {
    if (height > 0) void fitWindow(WIDTH, height, { center: false });
  });
</script>

<div class="panel" bind:clientHeight={height}>
  {#if status}
    <div class="head">
      <span class="headline mono"
        >{formatSpeed(status.bytesPerSecond ?? 0)}</span
      >
      <span class="caption muted">{status.runningCount} active</span>
    </div>
    <BandwidthGraph
      history={status.history.map((sample) => sample ?? 0)}
      height={32}
    />
    {#if items.length > 0}
      <hr />
      {#each items as item (item.name)}
        <div class="row">
          <ProgressRing fraction={item.fraction ?? 0} />
          <span class="name caption">{item.name}</span>
          <span class="caption mono"
            >{formatSpeed(item.bytesPerSecond ?? 0)}</span
          >
        </div>
      {/each}
    {/if}
    {#if waiting > 0}
      <hr />
      <div class="row">
        <span class="caption"
          >{waiting} {waiting === 1 ? "link" : "links"} waiting</span
        >
        <span class="spacer"></span>
        <button
          class="small"
          onclick={() => guarded(() => bridge.showMain("linkgrabber"))}
          >Review</button
        >
      </div>
    {/if}
    <hr />
    <div class="row">
      <button onclick={() => guarded(() => bridge.pauseAll())}>Pause all</button
      >
      <span class="spacer"></span>
      <button onclick={() => guarded(() => bridge.showMain())}>Open SDM</button>
      <button onclick={() => guarded(() => bridge.requestQuit())}>Quit</button>
    </div>
    {#if error}
      <div class="caption problem">{error}</div>
    {/if}
  {/if}
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 16px;
    width: 100%;
  }
  .head,
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .head {
    justify-content: space-between;
  }
  .row .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .spacer {
    flex: 1;
  }
  .muted {
    color: var(--text-secondary);
  }
  .problem {
    color: var(--status-failed);
  }
  hr {
    width: 100%;
    margin: 0;
    border: 0;
    border-top: 1px solid var(--border);
  }
  button.small {
    padding: 1px 8px;
    font-size: 11px;
  }
</style>
