<script lang="ts">
  import { tick } from "svelte";
  import type { GrabberMenuEntry } from "../lib/grabberMenu";
  import Icon from "./Icon.svelte";

  /**
   * A popup menu at a point: flips to stay inside the window, closes on
   * Escape, on a click outside and when the window loses focus, and opens a
   * submenu beside its entry. Entries come from a pure builder, so this holds
   * no rules of its own.
   */
  let {
    x,
    y,
    entries,
    onChoose,
    onClose,
  }: {
    x: number;
    y: number;
    entries: GrabberMenuEntry[];
    onChoose: (id: string) => void;
    onClose: () => void;
  } = $props();

  let element: HTMLDivElement | undefined = $state();
  let left = $state(0);
  let top = $state(0);
  let openSubmenu = $state<number | null>(null);

  $effect(() => {
    left = x;
    top = y;
    void tick().then(() => {
      if (!element) return;
      const box = element.getBoundingClientRect();
      if (x + box.width > window.innerWidth) left = Math.max(0, x - box.width);
      if (y + box.height > window.innerHeight)
        top = Math.max(0, y - box.height);
      element
        .querySelector<HTMLElement>("[role=menuitem]:not([aria-disabled=true])")
        ?.focus();
    });
  });

  function choose(id: string) {
    onChoose(id);
    onClose();
  }

  function onWindowKey(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }

  function onMenuKey(event: KeyboardEvent) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const items = [
      ...(element?.querySelectorAll<HTMLElement>(
        ":scope > [role=menuitem]:not([aria-disabled=true])",
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
    if (element && !element.contains(event.target as Node)) onClose();
  }
</script>

<svelte:window
  onkeydown={onWindowKey}
  onpointerdown={onOutside}
  onblur={onClose}
/>

{#snippet list(items: GrabberMenuEntry[], nested: boolean)}
  {#each items as entry, index (index)}
    {#if entry.kind === "divider"}
      <div class="divider" role="separator"></div>
    {:else if entry.kind === "submenu"}
      <div
        class="entry submenu-anchor"
        role="menuitem"
        tabindex="-1"
        aria-haspopup="menu"
        aria-expanded={openSubmenu === index}
        onmouseenter={() => (openSubmenu = index)}
        onclick={() => (openSubmenu = index)}
        onkeydown={(event) => {
          if (event.key === "ArrowRight" || event.key === "Enter")
            openSubmenu = index;
        }}
      >
        <span class="icon"></span>
        <span class="label">{entry.label}</span>
        <span class="shortcut">▸</span>
        {#if openSubmenu === index}
          <div class="menu sub" role="menu">
            {@render list(entry.entries, true)}
          </div>
        {/if}
      </div>
    {:else}
      <button
        type="button"
        class="entry"
        class:destructive={entry.destructive}
        role="menuitem"
        aria-disabled={entry.disabled === true}
        onmouseenter={() => {
          if (!nested) openSubmenu = null;
        }}
        onclick={() => {
          if (!entry.disabled) choose(entry.id);
        }}
      >
        <span class="icon"
          >{#if entry.icon}<Icon name={entry.icon} size={14} />{/if}</span
        >
        <span class="label">{entry.label}</span>
      </button>
    {/if}
  {/each}
{/snippet}

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
  {@render list(entries, false)}
</div>

<style>
  .menu {
    position: fixed;
    z-index: 100;
    min-width: 200px;
    padding: 5px;
    background: color-mix(in srgb, var(--surface-secondary) 96%, transparent);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 8px 28px rgb(0 0 0 / 0.35);
  }
  .menu.sub {
    position: absolute;
    left: 100%;
    top: -6px;
  }
  .entry {
    position: relative;
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
  .entry.destructive {
    color: var(--status-failed);
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
