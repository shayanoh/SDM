<script lang="ts">
  import type { FormatOptionDto } from "../bindings";
  import type { GrabberMenuEntry } from "../lib/grabberMenu";
  import type { TextRole } from "../lib/grabberRows";
  import { splitOptions } from "../lib/grabberRows";
  import MenuSurface from "./MenuSurface.svelte";

  /**
   * The borderless menu button on a media row: matching formats first, then a
   * divider, then the ones outside the preferences marked with a warning sign.
   */
  let {
    options,
    label,
    role,
    onPick,
  }: {
    options: FormatOptionDto[];
    label: string;
    role: TextRole;
    onPick: (optionId: string) => void;
  } = $props();

  let anchor = $state<{ x: number; y: number } | null>(null);

  const entries = $derived.by<GrabberMenuEntry[]>(() => {
    const { matching, nonMatching } = splitOptions(options);
    const result: GrabberMenuEntry[] = matching.map((o) => ({
      kind: "item",
      id: o.id,
      label: o.label,
    }));
    if (matching.length > 0 && nonMatching.length > 0)
      result.push({ kind: "divider" });
    for (const o of nonMatching)
      result.push({ kind: "item", id: o.id, label: `⚠ ${o.label}` });
    return result;
  });

  function open(event: MouseEvent) {
    event.stopPropagation();
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    anchor = { x: box.left, y: box.bottom + 2 };
  }
</script>

<button
  type="button"
  class="picker caption {role}"
  aria-haspopup="menu"
  onclick={open}
  onpointerdown={(event) => event.stopPropagation()}
  >{label} <span class="chevron">⌄</span></button
>

{#if anchor}
  <MenuSurface
    x={anchor.x}
    y={anchor.y}
    {entries}
    onChoose={onPick}
    onClose={() => (anchor = null)}
  />
{/if}

<style>
  .picker {
    padding: 0;
    background: none;
    border: none;
    border-radius: 0;
    white-space: nowrap;
  }
  .picker.online {
    color: var(--status-online);
  }
  .picker.faulty {
    color: var(--status-faulty);
  }
  .picker.offline {
    color: var(--status-offline);
  }
  .picker.failed {
    color: var(--status-failed);
  }
  .picker.secondary {
    color: var(--text-secondary);
  }
  .chevron {
    font-size: 9px;
  }
</style>
