<script lang="ts">
  import type { GrabberPackage, GrabberView } from "../bindings";
  import { useUi } from "../lib/context";
  import { flatLines, selectableIds } from "../lib/grabberList";
  import { OVERSCAN } from "../lib/layout";
  import { buildOffsets, visibleRange } from "../lib/windowing";
  import Icon from "./Icon.svelte";
  import LinkRow from "./LinkRow.svelte";
  import MediaRow from "./MediaRow.svelte";

  /**
   * The grabber's rows: unpackaged ones first, then each package's header and
   * rows. Windowed like the downloads list (two heights, only the rows near
   * the viewport exist). A row drags onto a package header to move there.
   */
  let {
    view,
    onRowMenu,
    onPackageMenu,
    onAddPackage,
    onRemovePackage,
    onRemoveLink,
    onPick,
    onMove,
  }: {
    view: GrabberView;
    onRowMenu: (event: MouseEvent, ids: string[]) => void;
    onPackageMenu: (event: MouseEvent, pkg: GrabberPackage) => void;
    onAddPackage: (id: string, start: boolean) => void;
    onRemovePackage: (id: string) => void;
    onRemoveLink: (id: string) => void;
    onPick: (id: string, optionId: string) => void;
    onMove: (linkId: string, packageId: string) => void;
  } = $props();

  const ui = useUi();

  const DRAG_TYPE = "application/x-sdm-grabber-row";

  let viewport = $state(0);
  let scrollTop = $state(0);
  let dropTarget = $state<string | null>(null);
  let dragging = $state<string | null>(null);

  const lines = $derived(flatLines(view));
  const offsets = $derived(buildOffsets(lines.map((line) => line.height)));
  const range = $derived(
    visibleRange({ offsets, scrollTop, viewport, overscan: OVERSCAN }),
  );
  const visible = $derived(
    lines
      .slice(range.start, range.end)
      .map((line, offset) => ({ line, index: range.start + offset })),
  );

  $effect(() => {
    ui.selection.setOrder(selectableIds(lines));
  });

  // Rows that left the grabber leave the selection too.
  $effect(() => {
    ui.selection.prune(new Set(view.rows.map((row) => row.id)));
  });

  function select(event: MouseEvent, id: string) {
    if (event.ctrlKey || event.metaKey) ui.selection.toggle(id);
    else if (event.shiftKey) ui.selection.range(id);
    else ui.selection.click(id);
  }

  function rowMenu(event: MouseEvent, id: string) {
    event.preventDefault();
    onRowMenu(event, ui.selection.contextTarget(id));
  }

  function dragStart(event: DragEvent, id: string) {
    dragging = id;
    event.dataTransfer?.setData(DRAG_TYPE, id);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  }

  function dragOver(event: DragEvent, key: string) {
    if (dragging === null) return;
    event.preventDefault();
    dropTarget = key;
  }

  function drop(event: DragEvent, pkg: GrabberPackage) {
    const id = dragging;
    dragging = null;
    dropTarget = null;
    if (id === null) return;
    event.preventDefault();
    event.stopPropagation();
    onMove(id, pkg.id);
  }
</script>

<div
  class="scroller"
  role="listbox"
  aria-multiselectable="true"
  aria-label="Links"
  bind:clientHeight={viewport}
  onscroll={(event) => (scrollTop = event.currentTarget.scrollTop)}
>
  <div class="inner" style:height="{offsets[offsets.length - 1] ?? 0}px">
    {#each visible as { line, index } (line.key)}
      {#if line.kind === "header"}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="slot header"
          class:odd={line.index % 2 === 1}
          class:drop-target={dropTarget === line.key}
          style:top="{offsets[index]}px"
          style:height="{line.height}px"
          data-package-id={line.pkg.id}
          oncontextmenu={(event) => {
            event.preventDefault();
            onPackageMenu(event, line.pkg);
          }}
          ondragover={(event) => dragOver(event, line.key)}
          ondragleave={() => {
            if (dropTarget === line.key) dropTarget = null;
          }}
          ondrop={(event) => drop(event, line.pkg)}
        >
          <span class="name title3" dir="auto">{line.pkg.name}</span>
          <span class="spacer"></span>
          <button
            type="button"
            class="small"
            onclick={() => onAddPackage(line.pkg.id, false)}
            >Add to downloads</button
          >
          <button
            type="button"
            class="small"
            onclick={() => onAddPackage(line.pkg.id, true)}
            >Add and start</button
          >
          <button
            type="button"
            class="trash"
            aria-label="Remove package {line.pkg.name}"
            onclick={() => onRemovePackage(line.pkg.id)}
            ><Icon name="trash-2" size={14} /></button
          >
        </div>
      {:else}
        {@const row = line.row}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class="slot row"
          class:odd={line.index % 2 === 1}
          class:selected={ui.selected.has(row.id)}
          style:top="{offsets[index]}px"
          style:height="{line.height}px"
          role="option"
          tabindex="-1"
          aria-selected={ui.selected.has(row.id)}
          data-row-id={row.id}
          draggable="true"
          onclick={(event) => select(event, row.id)}
          oncontextmenu={(event) => rowMenu(event, row.id)}
          ondragstart={(event) => dragStart(event, row.id)}
          ondragend={() => {
            dragging = null;
            dropTarget = null;
          }}
        >
          {#if row.kind === "link"}
            <LinkRow {row} onRemove={onRemoveLink} />
          {:else}
            <MediaRow {row} {onPick} />
          {/if}
        </div>
      {/if}
    {/each}
  </div>
</div>

<style>
  .scroller {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    position: relative;
    background: var(--surface-primary);
  }
  .inner {
    position: relative;
  }
  .slot {
    position: absolute;
    left: 0;
    right: 0;
    overflow: hidden;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 50%, transparent);
  }
  .row {
    background: var(--surface-primary);
  }
  .row.odd {
    background: var(--surface-secondary);
  }
  .row.selected {
    background: color-mix(in srgb, var(--selection-tint), black 20%);
  }
  .header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    /* The same translucent stripes as the Downloads package headers. */
    background: color-mix(in srgb, var(--surface-secondary) 50%, transparent);
  }
  .header.odd {
    background: color-mix(in srgb, var(--surface-secondary) 90%, transparent);
  }
  .header.drop-target {
    box-shadow: inset 0 0 0 2px var(--accent);
    border-radius: 4px;
  }
  .name {
    font-weight: 700;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .spacer {
    flex: 1;
  }
  .small {
    padding: 1px 8px;
    font-size: 11px;
  }
  .trash {
    display: inline-flex;
    padding: 0;
    color: var(--status-failed);
    background: none;
    border: none;
  }
</style>
