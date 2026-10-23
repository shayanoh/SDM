<script lang="ts">
  import { useStore, useUi } from "../lib/context";
  import { completedView } from "../lib/views";
  import Banner from "./Banner.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import BottomBar from "./BottomBar.svelte";
  import PackageList from "./PackageList.svelte";
  import LinkgrabberPlaceholder from "./LinkgrabberPlaceholder.svelte";
  import Sidebar from "./Sidebar.svelte";
  import Toolbar from "./Toolbar.svelte";

  const MIN_SIDEBAR = 200;
  const MAX_SIDEBAR = 360;

  const store = useStore();
  const ui = useUi();

  let sidebarWidth = $state(220);

  const downloads = $derived({ packages: store.order });
  const completed = $derived(completedView(store.order, store.items));
  const onDownloads = $derived(ui.tab === "downloads");

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
      <BottomBar
        view={onDownloads ? downloads : completed}
        showsPauseResume={onDownloads}
        showsLiveAggregates={onDownloads}
      />
    {/if}
  </section>
</div>
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
  .detail {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--surface-primary);
  }
</style>
