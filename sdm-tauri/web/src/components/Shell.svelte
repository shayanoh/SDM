<script lang="ts">
  import { useBridge, useStore, useUi } from "../lib/context";
  import { runMenu } from "../lib/actions";
  import { detectPlatform } from "../lib/platform";
  import { handleKey } from "../lib/shortcuts";
  import { completedView, itemOrder, selectedItemIds } from "../lib/views";
  import Banner from "./Banner.svelte";
  import DetailsPanel from "./DetailsPanel.svelte";
  import PanelHandle from "./PanelHandle.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import BottomBar from "./BottomBar.svelte";
  import PackageList from "./PackageList.svelte";
  import ModalHost from "./ModalHost.svelte";
  import LinkgrabberPlaceholder from "./LinkgrabberPlaceholder.svelte";
  import Sidebar from "./Sidebar.svelte";
  import Toolbar from "./Toolbar.svelte";

  const MIN_SIDEBAR = 200;
  const MAX_SIDEBAR = 360;

  const bridge = useBridge();
  const store = useStore();
  const ui = useUi();
  const platform = detectPlatform();

  let sidebarWidth = $state(220);

  const downloads = $derived({ packages: store.order });
  const completed = $derived(completedView(store.order, store.items));
  const onDownloads = $derived(ui.tab === "downloads");
  const view = $derived(onDownloads ? downloads : completed);
  const shownIds = $derived(selectedItemIds(ui.selected, view));

  function editable(element: Element | null): boolean {
    if (!(element instanceof HTMLElement)) return false;
    return (
      element.isContentEditable ||
      ["INPUT", "TEXTAREA", "SELECT"].includes(element.tagName)
    );
  }

  function onKeyDown(event: KeyboardEvent) {
    // The shortcuts belong to the two list tabs.
    if (ui.tab === "linkgrabber") return;
    handleKey(event, {
      platform,
      modalOpen: ui.modal !== null,
      menuOpen: ui.menu !== null,
      focusInEditable:
        editable(event.target as Element | null) ||
        editable(document.activeElement),
      run: (id) =>
        void runMenu(id, {
          ids: shownIds,
          items: store.items,
          bridge,
          ui,
          clipboard: navigator.clipboard,
        }),
      selectAll: () => ui.selection.selectAll(itemOrder(view)),
      moveSelection: (delta, extend) => {
        const moved = ui.selection.move(delta, extend);
        if (moved !== null) ui.reveal = moved;
      },
      closeMenu: () => (ui.menu = null),
    });
  }

  function dragDivider(event: PointerEvent) {
    const handle = event.currentTarget as HTMLElement;
    const startX = event.clientX;
    const startWidth = sidebarWidth;
    handle.setPointerCapture(event.pointerId);
    const move = (moved: PointerEvent) => {
      sidebarWidth = Math.min(
        MAX_SIDEBAR,
        Math.max(MIN_SIDEBAR, startWidth + moved.clientX - startX),
      );
    };
    const stop = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", stop);
      handle.removeEventListener("pointercancel", stop);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", stop);
    handle.addEventListener("pointercancel", stop);
  }
</script>

<svelte:window onkeydown={onKeyDown} />

<div class="window" style:--sidebar-width="{sidebarWidth}px">
  <aside class="side"><Sidebar /></aside>
  <div
    class="divider"
    role="separator"
    aria-orientation="vertical"
    aria-label="Resize the sidebar"
    onpointerdown={dragDivider}
  ></div>
  <section class="detail">
    <Banner />
    {#if ui.tab === "linkgrabber"}
      <LinkgrabberPlaceholder />
    {:else}
      <Toolbar view={onDownloads ? downloads : completed} />
      <PackageList
        view={onDownloads ? downloads : completed}
        tab={onDownloads ? "downloads" : "completed"}
        allowsReordering={onDownloads}
        showsLiveAggregates={onDownloads}
      />
      {#if ui.panelVisible}
        <div class="panel" style:height="{ui.panelHeight}px">
          <DetailsPanel itemIds={shownIds} />
          <PanelHandle />
        </div>
      {/if}
      <BottomBar
        view={onDownloads ? downloads : completed}
        showsPauseResume={onDownloads}
        showsLiveAggregates={onDownloads}
      />
    {/if}
  </section>
</div>
<ModalHost />
<ContextMenu view={onDownloads ? downloads : completed} />

<style>
  .window {
    display: grid;
    grid-template-columns: var(--sidebar-width) 1px minmax(0, 1fr);
    height: 100vh;
    min-width: 0;
  }
  .side {
    min-width: 0;
    overflow: hidden;
  }
  .divider {
    position: relative;
    background: var(--border);
    cursor: col-resize;
  }
  /* A wider grab area than the 1 px line. */
  .divider::before {
    content: "";
    position: absolute;
    inset: 0 -3px;
  }
  .panel {
    position: relative;
    flex: none;
    border-top: 1px solid var(--border);
  }
  .detail {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--surface-primary);
  }
</style>
