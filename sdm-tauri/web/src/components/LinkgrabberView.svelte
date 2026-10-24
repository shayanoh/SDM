<script lang="ts">
  import type { GrabberPackage } from "../bindings";
  import {
    addToDownloads,
    ingestDropped,
    renamePackage,
    runPackageMenu,
    runRowMenu,
    simple,
  } from "../lib/grabberActions";
  import {
    packageMenu,
    rowMenu,
    type GrabberMenuEntry,
  } from "../lib/grabberMenu";
  import { useBridge, useStore, useUi } from "../lib/context";
  import AddLinksSheet from "./AddLinksSheet.svelte";
  import GrabberHeader from "./GrabberHeader.svelte";
  import GrabberList from "./GrabberList.svelte";
  import Icon from "./Icon.svelte";
  import MenuSurface from "./MenuSurface.svelte";
  import RenamePackagePrompt from "./RenamePackagePrompt.svelte";
  import TemporaryTools from "./TemporaryTools.svelte";

  /**
   * The link grabber, as SwiftUI lays it out: an Add Links action, the
   * progress header, then the rows. It renders the controller's view and sends
   * ids; every rule (what is addable, what is held back) is in Rust.
   */
  const bridge = useBridge();
  const store = useStore();
  const ui = useUi();

  const view = $derived(store.grabber);
  const ctx = $derived({
    bridge,
    ui,
    clipboard: navigator.clipboard,
  });

  type Menu = {
    x: number;
    y: number;
    entries: GrabberMenuEntry[];
    choose: (id: string) => void;
  };
  let menu = $state.raw<Menu | null>(null);
  let adding = $state(false);
  /** The package being renamed, with the action bound to it when the prompt opened (the prompt closes before it applies). */
  let renaming = $state.raw<{
    name: string;
    apply: (name: string) => void;
  } | null>(null);

  function openRowMenu(event: MouseEvent, ids: string[]) {
    const entries = rowMenu(view, ids);
    if (entries.length === 0) return;
    menu = {
      x: event.clientX,
      y: event.clientY,
      entries,
      choose: (id) => void runRowMenu(id, ids, ctx),
    };
  }

  function openPackageMenu(event: MouseEvent, pkg: GrabberPackage) {
    menu = {
      x: event.clientX,
      y: event.clientY,
      entries: packageMenu(view, pkg),
      choose: (id) =>
        void runPackageMenu(id, pkg, ctx).then((outcome) => {
          if (outcome === "rename")
            renaming = {
              name: pkg.name,
              apply: (name) => void renamePackage(pkg, name, ctx),
            };
        }),
    };
  }

  /** Text or a link dragged in from another window. A drag of a row inside the list is not one. */
  function isExternalDrop(event: DragEvent): boolean {
    const types = event.dataTransfer?.types ?? [];
    return (
      !types.includes("application/x-sdm-grabber-row") &&
      (types.includes("text/uri-list") || types.includes("text/plain"))
    );
  }

  function onDragOver(event: DragEvent) {
    if (isExternalDrop(event)) event.preventDefault();
  }

  function onDrop(event: DragEvent) {
    if (!isExternalDrop(event)) return;
    event.preventDefault();
    const transfer = event.dataTransfer;
    const text =
      transfer?.getData("text/uri-list") ||
      transfer?.getData("text/plain") ||
      "";
    void ingestDropped(text, ctx);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="grabber" ondragover={onDragOver} ondrop={onDrop}>
  <div class="toolbar" role="toolbar" aria-label="Link grabber actions">
    <button type="button" class="add" onclick={() => (adding = true)}>
      <Icon name="link" size={16} strokeWidth={1.7} /> Add Links
    </button>
  </div>
  <GrabberHeader
    {view}
    onAddAll={(start) => void addToDownloads({ kind: "all" }, start, ctx)}
    onCancel={() => void simple((b) => b.grabberCancelChecks(), ctx)}
    onRecheckAll={() => void simple((b) => b.grabberRecheckFailed(), ctx)}
    onClear={() => void simple((b) => b.grabberClear(), ctx)}
  />
  {#if view.rows.length === 0}
    <div class="empty">
      <p class="message">No links yet.</p>
      <p class="muted">Paste or drop links, or choose Add Links.</p>
      <TemporaryTools />
    </div>
  {:else}
    <GrabberList
      {view}
      onRowMenu={openRowMenu}
      onPackageMenu={openPackageMenu}
      onAddPackage={(id, start) =>
        void addToDownloads({ kind: "package", id }, start, ctx)}
      onRemovePackage={(id) =>
        void simple((b) => b.grabberRemovePackage(id), ctx)}
      onRemoveLink={(id) => void simple((b) => b.grabberRemoveLinks([id]), ctx)}
      onPick={(id, optionId) =>
        void simple((b) => b.grabberSetFormat(id, optionId), ctx)}
      onMove={(linkId, packageId) =>
        void simple((b) => b.grabberMoveLink(linkId, packageId), ctx)}
    />
  {/if}
</div>

{#if menu}
  <MenuSurface
    x={menu.x}
    y={menu.y}
    entries={menu.entries}
    onChoose={menu.choose}
    onClose={() => (menu = null)}
  />
{/if}

{#if adding}
  <AddLinksSheet
    onCancel={() => (adding = false)}
    onAdd={(text) => void simple((b) => b.grabberIngestText(text), ctx)}
  />
{/if}

{#if renaming}
  <RenamePackagePrompt
    name={renaming.name}
    onCancel={() => (renaming = null)}
    onRename={renaming.apply}
  />
{/if}

<style>
  .grabber {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    min-width: 0;
  }
  .toolbar {
    display: flex;
    justify-content: flex-end;
    padding: 6px 12px;
    background: var(--surface-primary);
    border-bottom: 1px solid var(--border);
  }
  .add {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: none;
    border: none;
  }
  .add:hover {
    background: color-mix(in srgb, var(--text-primary) 10%, transparent);
  }
  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 24px;
  }
  .message {
    margin: 0;
    font-size: 15px;
  }
  .muted {
    margin: 0 0 12px;
    color: var(--text-secondary);
  }
  :global(html[data-vibrancy="on"]) .toolbar {
    background: color-mix(in srgb, var(--surface-primary) 78%, transparent);
  }
</style>
