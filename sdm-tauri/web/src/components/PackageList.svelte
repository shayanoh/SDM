<script lang="ts">
  import { useStore, useUi } from "../lib/context";
  import { HEADER_HEIGHT, OVERSCAN } from "../lib/layout";
  import type { ListTab } from "../lib/ui.svelte";
  import { completedAggregate, flatRows, type ListView } from "../lib/views";
  import { buildOffsets, visibleRange } from "../lib/windowing";
  import ItemRow from "./ItemRow.svelte";
  import PackageHeader from "./PackageHeader.svelte";
  import RowSlot from "./RowSlot.svelte";

  /**
   * The one downloads list: collapsible packages, zebra rows and multi-select.
   * Downloads and Completed both render through it, so they cannot drift.
   * Windowed: only the rows near the viewport exist in the DOM.
   */
  let {
    view,
    tab,
    allowsReordering,
    showsLiveAggregates,
  }: {
    view: ListView;
    tab: ListTab;
    allowsReordering: boolean;
    showsLiveAggregates: boolean;
  } = $props();

  const store = useStore();
  const ui = useUi();

  let viewport = $state(0);
  let scrollTop = $state(0);

  const rows = $derived(flatRows(view, ui.collapsed[tab]));
  const offsets = $derived(buildOffsets(rows.map((row) => row.height)));
  const range = $derived(
    visibleRange({ offsets, scrollTop, viewport, overscan: OVERSCAN }),
  );
  const visible = $derived(
    rows
      .slice(range.start, range.end)
      .map((row, offset) => ({ row, index: range.start + offset })),
  );
  const itemIdsByPackage = $derived(
    new Map(view.packages.map((pkg) => [pkg.id, pkg.itemIds])),
  );

  let scroller: HTMLDivElement | undefined = $state();

  // Scrolls a row the keyboard moved to into view, using the row offsets (it may not be rendered).
  $effect(() => {
    const key = ui.reveal;
    if (key === null || !scroller) return;
    const at = rows.findIndex((row) => row.id === key);
    if (at >= 0) {
      const top = offsets[at];
      const bottom = offsets[at + 1];
      if (top < scroller.scrollTop) scroller.scrollTop = top;
      else if (bottom > scroller.scrollTop + scroller.clientHeight)
        scroller.scrollTop = bottom - scroller.clientHeight;
    }
    ui.reveal = null;
  });

  // Shift-click and the keyboard walk the rows in display order.
  $effect(() => {
    ui.selection.setOrder(rows.map((row) => row.id));
  });
</script>

<div
  class="scroller"
  role="listbox"
  aria-multiselectable="true"
  aria-label={tab === "downloads" ? "Downloads" : "Completed"}
  data-reorderable={allowsReordering}
  bind:this={scroller}
  bind:clientHeight={viewport}
  onscroll={(event) => (scrollTop = event.currentTarget.scrollTop)}
>
  <div class="inner" style:height="{offsets[offsets.length - 1] ?? 0}px">
    {#each visible as { row, index } (row.kind + row.id)}
      <RowSlot {row} top={offsets[index]} {view} reorderable={allowsReordering}>
        {#if row.kind === "package"}
          {@const pkg = store.packages.get(row.id)}
          {#if pkg}
            {@const sums = completedAggregate(
              itemIdsByPackage.get(row.id) ?? [],
              store.items,
            )}
            <PackageHeader
              {pkg}
              {tab}
              live={showsLiveAggregates}
              completedBytes={showsLiveAggregates
                ? pkg.completedBytes
                : sums.completedBytes}
              totalBytes={showsLiveAggregates
                ? pkg.totalBytes
                : sums.totalBytes}
            />
          {/if}
        {:else}
          {@const item = store.items.get(row.id)}
          {#if item}<ItemRow {item} />{/if}
        {/if}
      </RowSlot>
    {/each}
  </div>
  {#if rows.length === 0}
    <p class="empty">
      {tab === "downloads"
        ? "No downloads yet."
        : "No completed downloads yet."}
    </p>
  {/if}
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
  .empty {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    margin: 0;
    color: var(--text-secondary);
  }
</style>
