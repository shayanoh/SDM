<script lang="ts">
  import type { GrabberLinkRow } from "../bindings";
  import { linkBadge, sizeText } from "../lib/grabberRows";
  import Icon from "./Icon.svelte";

  let {
    row,
    onRemove,
  }: { row: GrabberLinkRow; onRemove: (id: string) => void } = $props();

  const badge = $derived(linkBadge(row));
  const size = $derived(sizeText(row.sizeBytes));
</script>

<div class="row">
  <span class="name" dir="auto" title={row.finalUrl}>{row.filename}</span>
  {#if row.isDuplicate}<span class="caption tag">duplicate</span>{/if}
  <span class="spacer"></span>
  <span class="badge caption {badge.role}">
    {#if badge.spinner}<span class="spinner" aria-hidden="true"></span>{/if}
    {badge.text}
  </span>
  <span class="size mono">{size}</span>
  {#if row.removable}
    <button
      type="button"
      class="trash"
      aria-label="Remove {row.filename}"
      onclick={(event) => {
        event.stopPropagation();
        onRemove(row.id);
      }}><Icon name="trash-2" size={14} /></button
    >
  {/if}
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 100%;
    padding: 2px 16px 2px 12px;
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tag {
    color: var(--status-faulty);
  }
  .spacer {
    flex: 1;
  }
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
  }
  .badge.online {
    color: var(--status-online);
  }
  .badge.offline {
    color: var(--status-offline);
  }
  .badge.failed {
    color: var(--status-failed);
  }
  .badge.faulty {
    color: var(--status-faulty);
  }
  .badge.secondary {
    color: var(--text-secondary);
  }
  .size {
    white-space: nowrap;
  }
  .trash {
    display: inline-flex;
    padding: 0;
    color: var(--status-failed);
    background: none;
    border: none;
  }
  .spinner {
    width: 10px;
    height: 10px;
    border: 1.5px solid var(--text-secondary);
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
