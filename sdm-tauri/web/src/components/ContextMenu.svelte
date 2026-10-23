<script lang="ts">
  import { tick } from "svelte";
  import { runMenu } from "../lib/actions";
  import { useBridge, useStore, useUi } from "../lib/context";
  import {
    itemMenu,
    packageMenu,
    type MenuEntry,
    type MenuId,
  } from "../lib/menu";
  import { detectPlatform } from "../lib/platform";
  import { selectedItemIds, type ListView } from "../lib/views";
  import Icon from "./Icon.svelte";

  /**
   * The right-click menu: the item menu on an item (acting on the whole
   * selection when the item is part of it), the package menu on a header.
   */
  let { view }: { view: ListView } = $props();

  const bridge = useBridge();
  const store = useStore();
  const ui = useUi();
  const platform = detectPlatform();

  let element: HTMLDivElement | undefined = $state();
  let left = $state(0);
  let top = $state(0);

  const menu = $derived(ui.menu);
  const isPackage = $derived(
    menu !== null && store.packages.has(menu.targetId),
  );
  const ids = $derived(selectedItemIds(ui.selected, view));
  const entries = $derived<MenuEntry[]>(
    isPackage
      ? packageMenu()
      : itemMenu(
          ids.flatMap((id) => store.items.get(id) ?? []),
          { platform },
        ),
  );

  // Keep the menu inside the window: flip it left/up when it would overflow.
  $effect(() => {
    if (!menu) return;
    left = menu.x;
    top = menu.y;
    void tick().then(() => {
      if (!element) return;
      const box = element.getBoundingClientRect();
      if (menu.x + box.width > window.innerWidth)
        left = Math.max(0, menu.x - box.width);
      if (menu.y + box.height > window.innerHeight)
        top = Math.max(0, menu.y - box.height);
      element
        .querySelector<HTMLElement>("[role=menuitem]:not([aria-disabled=true])")
        ?.focus();
    });
  });

  // A menu whose row has gone (removed by a structure patch) closes.
  $effect(() => {
    if (menu && !store.liveKeys().has(menu.targetId)) ui.menu = null;
  });

  function close() {
    ui.menu = null;
  }

  function choose(entry: Exclude<MenuEntry, { id: "divider" }>) {
    if (entry.disabled) return;
    const target = menu?.targetId;
    close();
    const packageItemIds = isPackage
      ? (view.packages.find((pkg) => pkg.id === target)?.itemIds ?? [])
      : undefined;
    void runMenu(entry.id as MenuId, {
      ids: isPackage ? [] : ids,
      items: store.items,
      bridge,
      ui,
      clipboard: navigator.clipboard,
      packageId: isPackage ? target : undefined,
      packageItemIds,
    });
  }

  function onWindowKey(event: KeyboardEvent) {
    if (!menu) return;
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  }

  function onMenuKey(event: KeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const items = [
      ...(element?.querySelectorAll<HTMLElement>(
        "[role=menuitem]:not([aria-disabled=true])",
      ) ?? []),
    ];
    if (items.length === 0) return;
    const at = items.indexOf(document.activeElement as HTMLElement);
    const next =
      event.key === "ArrowDown"
        ? (at + 1) % items.length
        : (at - 1 + items.length) % items.length;
    items[next].focus();
  }

  function onOutside(event: PointerEvent) {
    if (menu && element && !element.contains(event.target as Node)) close();
  }
</script>

<svelte:window
  onkeydown={onWindowKey}
  onpointerdown={onOutside}
  onblur={close}
/>

{#if menu}
  <div
    class="menu"
    role="menu"
    tabindex="-1"
    bind:this={element}
    style:left="{left}px"
    style:top="{top}px"
    onkeydown={onMenuKey}
    oncontextmenu={(event) => event.preventDefault()}
  >
    {#each entries as entry, index (index)}
      {#if entry.id === "divider"}
        <div class="divider" role="separator"></div>
      {:else}
        <button
          type="button"
          class="entry"
          role="menuitem"
          aria-disabled={entry.disabled}
          onclick={() => choose(entry)}
        >
          <span class="icon"
            >{#if entry.icon}<Icon name={entry.icon} size={14} />{/if}</span
          >
          <span class="label">{entry.label}</span>
          {#if entry.shortcut}<span class="shortcut">{entry.shortcut}</span
            >{/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .menu {
    position: fixed;
    z-index: 100;
    min-width: 220px;
    padding: 5px;
    background: color-mix(in srgb, var(--surface-secondary) 96%, transparent);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 8px 28px rgb(0 0 0 / 0.35);
  }
  .entry {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 4px 10px 4px 6px;
    text-align: left;
    color: var(--text-primary);
    background: none;
    border: none;
    border-radius: 5px;
  }
  .entry:hover:not([aria-disabled="true"]),
  .entry:focus-visible {
    background: var(--selection-tint);
    outline: none;
  }
  .entry[aria-disabled="true"] {
    color: var(--text-secondary);
    opacity: 0.5;
  }
  .icon {
    display: inline-flex;
    width: 16px;
    justify-content: center;
  }
  .label {
    flex: 1;
  }
  .shortcut {
    color: var(--text-secondary);
    font-size: 11px;
  }
  .divider {
    height: 1px;
    margin: 4px 6px;
    background: var(--border);
  }
</style>
