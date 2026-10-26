<script lang="ts">
  import type { GrabberMediaRow } from "../bindings";
  import {
    canPick,
    mediaBadge,
    pickerLabel,
    sizeText,
  } from "../lib/grabberRows";
  import FormatPicker from "./FormatPicker.svelte";
  import Icon from "./Icon.svelte";

  let {
    row,
    onPick,
  }: { row: GrabberMediaRow; onPick: (id: string, optionId: string) => void } =
    $props();

  const badge = $derived(mediaBadge(row));
  const picker = $derived(pickerLabel(row));
  const size = $derived(sizeText(row.sizeBytes));
</script>

<div class="row">
  <span class="kind"><Icon name="square-play" size={14} /></span>
  <span class="name" dir="auto" title={row.sourceUrl}>{row.title}</span>
  {#if row.isDuplicate}<span class="caption tag">duplicate</span>{/if}
  <span class="spacer"></span>
  {#if canPick(row)}
    <FormatPicker
      options={row.options}
      label={picker.text}
      role={picker.role}
      onPick={(optionId) => onPick(row.id, optionId)}
    />
  {:else}
    <span class="badge caption {badge.role}" title={badge.title}>
      {#if badge.spinner}<span class="spinner" aria-hidden="true"></span>{/if}
      {badge.text}
    </span>
  {/if}
  <span class="size mono caption">{size}</span>
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 100%;
    padding: 2px 16px 2px 12px;
  }
  .kind {
    display: inline-flex;
    color: var(--text-secondary);
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
    min-width: 0;
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
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
    color: var(--text-secondary);
    white-space: nowrap;
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
