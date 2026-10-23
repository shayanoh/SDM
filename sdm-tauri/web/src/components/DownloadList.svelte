<script lang="ts">
  import { useStore } from "../lib/context";
  import { OVERSCAN, ROW_HEIGHT } from "../lib/layout";
  import { buildOffsets, visibleRange } from "../lib/windowing";
  import ItemRow from "./ItemRow.svelte";
  import PackageHeader from "./PackageHeader.svelte";

  type Row = { kind: "package" | "item"; id: string };

  const store = useStore();

  let viewport = $state(0);
  let scrollTop = $state(0);
  let selectedId = $state<string | null>(null);

  const rows = $derived.by<Row[]>(() =>
    store.order.flatMap((pkg) => [
      { kind: "package" as const, id: pkg.id },
      ...pkg.itemIds.map((id) => ({ kind: "item" as const, id })),
    ]),
  );
  const offsets = $derived(buildOffsets(rows.map(() => ROW_HEIGHT)));
  const range = $derived(
    visibleRange({ offsets, scrollTop, viewport, overscan: OVERSCAN }),
  );
  const visible = $derived(
    rows
      .slice(range.start, range.end)
      .map((row, offset) => ({ row, index: range.start + offset })),
  );
</script>

<div
  class="scroller"
  bind:clientHeight={viewport}
  onscroll={(event) => (scrollTop = event.currentTarget.scrollTop)}
>
  <div class="inner" style:height="{rows.length * ROW_HEIGHT}px">
    {#each visible as { row, index } (row.kind + row.id)}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div
        class="slot"
        style:top="{index * ROW_HEIGHT}px"
        onclick={() => (selectedId = row.kind === "item" ? row.id : null)}
      >
        {#if row.kind === "package"}
          {@const pkg = store.packages.get(row.id)}
          {#if pkg}<PackageHeader {pkg} />{/if}
        {:else}
          {@const item = store.items.get(row.id)}
          {#if item}<ItemRow {item} selected={selectedId === row.id} />{/if}
        {/if}
      </div>
    {/each}
  </div>
  {#if rows.length === 0}
    <p class="empty">No downloads yet. Paste a link above.</p>
  {/if}
</div>

<style>
  .scroller {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    position: relative;
  }
  .inner {
    position: relative;
  }
  .slot {
    position: absolute;
    left: 0;
    right: 0;
    height: var(--row-height);
  }
  .empty {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--text-tertiary);
  }
</style>
