import type { Bridge } from "./bridge";
import { errorMessage } from "./context";
import {
  dropIndex,
  orderedItemIds,
  orderedPackageIds,
  reorderedPackageIds,
  type DragPayload,
} from "./dnd";
import type { UiState } from "./ui.svelte";
import { itemOrder, type ListView, type Row } from "./views";

export interface DragContext {
  ui: UiState;
  bridge: Bridge;
  view: ListView;
}

/** Marks a drag that started in this list, so files dragged in from the desktop are not mistaken for one. */
export const DRAG_TYPE = "application/x-sdm-rows";

/** What dropping `payload` on `row` would do, or `null` when the drop is refused. */
function plan(
  payload: DragPayload,
  row: Row,
  view: ListView,
  offsetY: number,
):
  | { kind: "moveItems"; ids: string[]; packageId: string; index?: number }
  | { kind: "reorderPackages"; order: string[] }
  | null {
  const items = orderedItemIds(payload, itemOrder(view));
  if (items) {
    return row.kind === "item"
      ? {
          kind: "moveItems",
          ids: items,
          packageId: row.packageId,
          index: dropIndex(row.index, offsetY, row.height),
        }
      : { kind: "moveItems", ids: items, packageId: row.id };
  }
  const packages = orderedPackageIds(
    payload,
    view.packages.map((pkg) => pkg.id),
  );
  if (!packages || row.kind !== "package") return null;
  const order = reorderedPackageIds(
    view.packages.map((pkg) => pkg.id),
    packages,
    row.id,
    offsetY > row.height / 2,
  );
  return order ? { kind: "reorderPackages", order } : null;
}

/** Starts dragging `row`: the whole selection if the row is part of it, else just the row. */
export function beginDrag(
  row: Row,
  event: DragEvent,
  { ui, view }: DragContext,
) {
  const packageIds = new Set(view.packages.map((pkg) => pkg.id));
  const keys = ui.selected.has(row.id) ? [...ui.selected] : [row.id];
  const payload: DragPayload = keys.map((id) => ({
    kind: packageIds.has(id) ? "package" : "item",
    id,
  }));
  ui.dragPayload = payload;
  const transfer = event.dataTransfer;
  if (transfer) {
    transfer.setData(DRAG_TYPE, JSON.stringify(payload));
    transfer.effectAllowed = "move";
  }
}

/** Marks `row` as the drop target when the drag in progress could land on it. */
export function overDrag(
  row: Row,
  event: DragEvent,
  { ui, view }: DragContext,
) {
  const payload = ui.dragPayload;
  if (!payload) return;
  // Only whether the drop is allowed matters here; the position is decided on drop.
  if (plan(payload, row, view, 0)) {
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    ui.dragTargetId = row.id;
  } else if (ui.dragTargetId === row.id) {
    ui.dragTargetId = null;
  }
}

export function leaveDrag(row: Row, { ui }: DragContext) {
  if (ui.dragTargetId === row.id) ui.dragTargetId = null;
}

export function endDrag({ ui }: DragContext) {
  ui.dragPayload = null;
  ui.dragTargetId = null;
}

/** Applies the drop on `row`. Anything that did not start in this list, or that has no meaning, does nothing. */
export async function dropOn(row: Row, event: DragEvent, context: DragContext) {
  const { ui, bridge, view } = context;
  const payload = ui.dragPayload;
  if (!payload) return;
  const element = event.currentTarget as HTMLElement;
  const offsetY = event.clientY - element.getBoundingClientRect().top;
  const action = plan(payload, row, view, offsetY);
  if (!action) return;
  event.preventDefault();
  endDrag(context);
  try {
    if (action.kind === "moveItems") {
      if (action.index === undefined)
        await bridge.moveItems(action.ids, action.packageId);
      else await bridge.moveItems(action.ids, action.packageId, action.index);
    } else {
      await bridge.reorderPackages(action.order);
    }
  } catch (error) {
    ui.report(errorMessage(error));
  }
}
