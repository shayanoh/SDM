<script lang="ts">
  import { useBridge, useStore, useUi } from "../lib/context";
  import { runMenu } from "../lib/actions";
  import { toolbarEntries, type MenuId } from "../lib/menu";
  import { detectPlatform } from "../lib/platform";
  import { selectedItemIds, type ListView } from "../lib/views";
  import Icon from "./Icon.svelte";

  /**
   * The window's action bar: the item menu's actions as icon buttons, then the
   * Info Panel toggle. They act on the selection and grey out the same way
   * the context menu's entries do.
   */
  let { view }: { view: ListView } = $props();

  const bridge = useBridge();
  const store = useStore();
  const ui = useUi();
  const platform = detectPlatform();

  const ids = $derived(selectedItemIds(ui.selected, view));
  const chosen = $derived(ids.flatMap((id) => store.items.get(id) ?? []));
  const entries = $derived(toolbarEntries(chosen, { platform }));

  function run(id: MenuId) {
    void runMenu(id, {
      ids,
      items: store.items,
      bridge,
      ui,
      clipboard: navigator.clipboard,
    });
  }
</script>

<div class="toolbar" role="toolbar" aria-label="Actions">
  {#each entries as entry (entry.id)}
    {#if entry.id !== "divider" && entry.icon}
      <button
        type="button"
        class="tool"
        class:active={entry.id === "info" && ui.panelVisible}
        aria-label={entry.label}
        title={entry.shortcut
          ? `${entry.label} (${entry.shortcut})`
          : entry.label}
        disabled={entry.disabled}
        onclick={() => run(entry.id)}
      >
        <Icon name={entry.icon} size={18} strokeWidth={1.7} />
      </button>
    {/if}
  {/each}
</div>

<style>
  .toolbar {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    padding: 6px 12px;
    background: var(--surface-primary);
    border-bottom: 1px solid var(--border);
  }
  .tool {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 28px;
    padding: 0;
    color: var(--text-primary);
    background: none;
    border: none;
    border-radius: 6px;
  }
  .tool:hover:not(:disabled) {
    background: color-mix(in srgb, var(--text-primary) 10%, transparent);
  }
  .tool.active {
    background: color-mix(in srgb, var(--text-primary) 14%, transparent);
  }
  .tool:disabled {
    color: var(--text-secondary);
    opacity: 0.45;
  }
</style>
