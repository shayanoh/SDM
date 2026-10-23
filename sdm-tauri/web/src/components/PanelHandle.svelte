<script lang="ts">
  import { useUi } from "../lib/context";

  /** The 40 x 5 capsule on the panel's top edge: drag it up to grow the panel. */
  const ui = useUi();

  let dragging: { startY: number; startHeight: number } | null = null;

  function down(event: PointerEvent) {
    dragging = { startY: event.clientY, startHeight: ui.panelHeight };
    (event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId);
  }

  function move(event: PointerEvent) {
    if (!dragging) return;
    ui.setPanelHeight(dragging.startHeight - (event.clientY - dragging.startY));
  }

  function up() {
    dragging = null;
  }
</script>

<div
  class="handle"
  role="separator"
  aria-orientation="horizontal"
  aria-label="Resize the details panel"
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
>
  <span class="capsule"></span>
</div>

<style>
  .handle {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 14px;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    cursor: ns-resize;
    touch-action: none;
  }
  .capsule {
    width: 40px;
    height: 5px;
    margin-top: 0;
    background: var(--text-primary);
    border-radius: 999px;
    opacity: 0.85;
  }
</style>
