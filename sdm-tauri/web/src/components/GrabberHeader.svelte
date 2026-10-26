<script lang="ts">
  import type { GrabberView } from "../bindings";
  import { heldBackText } from "../lib/grabberRows";

  /**
   * The grabber's top bar: determinate progress, the `checked / total`
   * caption, four static verdict readouts (not filters), and the bulk actions.
   */
  let {
    view,
    onAddAll,
    onCancel,
    onRecheckAll,
    onClear,
  }: {
    view: GrabberView;
    onAddAll: (start: boolean) => void;
    onCancel: () => void;
    onRecheckAll: () => void;
    onClear: () => void;
  } = $props();

  const fraction = $derived(
    view.totalCount > 0 ? view.checkedCount / view.totalCount : 0,
  );
  const empty = $derived(view.packages.length === 0);
  const pills = $derived([
    { label: "Online", count: view.summary.online, role: "online" },
    { label: "Faulty", count: view.summary.faulty, role: "faulty" },
    { label: "Offline", count: view.summary.offline, role: "offline" },
    { label: "Check failed", count: view.summary.failed, role: "failed" },
  ]);
</script>

<div class="header">
  <progress max="1" value={fraction} aria-label="Links checked" class="progress"
  ></progress>
  <div class="line">
    <span class="caption secondary"
      >{view.checkedCount} / {view.totalCount} checked</span
    >
    <span class="spacer"></span>
    {#each pills as pill (pill.label)}
      <span class="pill caption {pill.role}">{pill.label} {pill.count}</span>
    {/each}
    <button
      type="button"
      class="small"
      disabled={empty}
      onclick={() => onAddAll(false)}>Add all to downloads</button
    >
    <button
      type="button"
      class="small"
      disabled={empty}
      onclick={() => onAddAll(true)}>Add all and start</button
    >
    {#if view.summary.isChecking}
      <button type="button" class="small destructive" onclick={onCancel}
        >Cancel Checks</button
      >
    {:else}
      <button
        type="button"
        class="small"
        disabled={view.summary.recheckable === 0}
        onclick={onRecheckAll}>Recheck All</button
      >
    {/if}
    <button
      type="button"
      class="small destructive"
      disabled={empty}
      onclick={onClear}>Clear</button
    >
  </div>
  {#if view.heldBack !== null}
    <div class="caption faulty-text">{heldBackText(view.heldBack)}</div>
  {/if}
  {#if view.tools}
    <div
      class="caption"
      class:error={view.tools.isError}
      class:secondary={!view.tools.isError}
      role="status"
    >
      {view.tools.text}
    </div>
  {/if}
</div>

<style>
  .header {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 16px;
    background: var(--surface-secondary);
    border-bottom: 1px solid var(--border);
  }
  .progress {
    width: 100%;
    height: 4px;
    appearance: none;
    border: none;
    border-radius: 2px;
    background: var(--surface-tertiary);
    overflow: hidden;
  }
  .progress::-webkit-progress-bar {
    background: var(--surface-tertiary);
  }
  .progress::-webkit-progress-value {
    background: var(--progress-fill);
    transition: width 0.2s;
  }
  .progress::-moz-progress-bar {
    background: var(--progress-fill);
  }
  .line {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }
  .spacer {
    flex: 1;
  }
  .secondary {
    color: var(--text-secondary);
  }
  .pill {
    padding: 3px 8px;
    white-space: nowrap;
    background: var(--surface-tertiary);
    border-radius: 999px;
  }
  .pill.online {
    color: var(--status-online);
  }
  .pill.faulty {
    color: var(--status-faulty);
  }
  .pill.offline {
    color: var(--status-offline);
  }
  .pill.failed {
    color: var(--status-failed);
  }
  .small {
    padding: 1px 8px;
    font-size: 11px;
  }
  .destructive:not(:disabled) {
    color: var(--status-failed);
  }
  .faulty-text {
    color: var(--status-faulty);
  }
  .error {
    color: var(--status-failed);
  }
  :global(html[data-vibrancy="on"]) .header {
    background: color-mix(in srgb, var(--surface-secondary) 78%, transparent);
  }
</style>
