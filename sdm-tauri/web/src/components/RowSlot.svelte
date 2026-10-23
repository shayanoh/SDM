<script lang="ts">
  import type { Snippet } from "svelte";
  import { useUi } from "../lib/context";
  import type { Row } from "../lib/views";

  /**
   * One absolutely positioned row of the windowed list. It owns what the list
   * needs to see of a row — selection, the context menu and the background —
   * so nothing between the list and those can hide them.
   */
  let { row, top, children }: { row: Row; top: number; children: Snippet } =
    $props();

  const ui = useUi();
  const selected = $derived(ui.selected.has(row.id));

  function select(event: MouseEvent) {
    if (event.ctrlKey || event.metaKey) ui.selection.toggle(row.id);
    else if (event.shiftKey) ui.selection.range(row.id);
    else ui.selection.click(row.id);
  }

  function openMenu(event: MouseEvent) {
    event.preventDefault();
    ui.selection.contextTarget(row.id);
    ui.menu = { x: event.clientX, y: event.clientY, targetId: row.id };
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="slot {row.kind}"
  class:odd={row.index % 2 === 1}
  class:selected
  style:top="{top}px"
  style:height="{row.height}px"
  role="option"
  tabindex="-1"
  aria-selected={selected}
  data-row-id={row.id}
  data-kind={row.kind}
  onclick={select}
  oncontextmenu={openMenu}
>
  {@render children()}
</div>

<style>
  .slot {
    position: absolute;
    left: 0;
    right: 0;
    overflow: hidden;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 50%, transparent);
  }
  .item {
    background: var(--surface-primary);
  }
  .item.odd {
    background: var(--surface-secondary);
  }
  .package {
    background: color-mix(in srgb, var(--surface-secondary) 50%, transparent);
  }
  .package.odd {
    background: color-mix(in srgb, var(--surface-secondary) 90%, transparent);
  }
  /* The theme's selection tint, darkened by 20 % (SwiftUI's `.mix(with: .black, by: 0.2)`). */
  .slot.selected {
    background: color-mix(in srgb, var(--selection-tint), black 20%);
  }
</style>
