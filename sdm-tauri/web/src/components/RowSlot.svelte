<script lang="ts">
  import type { Snippet } from "svelte";
  import { useBridge, useUi } from "../lib/context";
  import {
    beginDrag,
    dropOn,
    endDrag,
    leaveDrag,
    overDrag,
    type DragContext,
  } from "../lib/dragdrop";
  import type { ListView, Row } from "../lib/views";

  /**
   * One absolutely positioned row of the windowed list. It owns what the list
   * needs to see of a row — selection, the context menu and the background —
   * so nothing between the list and those can hide them.
   */
  let {
    row,
    top,
    view,
    reorderable,
    children,
  }: {
    row: Row;
    top: number;
    view: ListView;
    reorderable: boolean;
    children: Snippet;
  } = $props();

  const ui = useUi();
  const bridge = useBridge();
  const dnd = $derived<DragContext>({ ui, bridge, view });
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
  class:drop-target={ui.dragTargetId === row.id}
  style:top="{top}px"
  style:height="{row.height}px"
  role="option"
  tabindex="-1"
  aria-selected={selected}
  data-row-id={row.id}
  data-kind={row.kind}
  onclick={select}
  oncontextmenu={openMenu}
  draggable={reorderable}
  ondragstart={(event) => reorderable && beginDrag(row, event, dnd)}
  ondragover={(event) => reorderable && overDrag(row, event, dnd)}
  ondragleave={() => leaveDrag(row, dnd)}
  ondrop={(event) => reorderable && dropOn(row, event, dnd)}
  ondragend={() => endDrag(dnd)}
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
  /* A whole-row outline, as SwiftUI draws it, for the row a drag would land on. */
  .slot.drop-target {
    box-shadow: inset 0 0 0 2px var(--accent);
    border-radius: 4px;
  }
  /* The theme's selection tint, darkened by 20 % (SwiftUI's `.mix(with: .black, by: 0.2)`). */
  .slot.selected {
    background: color-mix(in srgb, var(--selection-tint), black 20%);
  }
</style>
