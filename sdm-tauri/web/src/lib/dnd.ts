export type DragPayload = { kind: "item" | "package"; id: string }[];

/**
 * The item ids a drag carries, in list order (a drag session hands them back
 * in no useful order). `null` when the payload mixes in packages, carries
 * nothing, or names no item that exists: the drop is then refused.
 */
export function orderedItemIds(
  payload: DragPayload,
  itemOrder: string[],
): string[] | null {
  return ordered(payload, "item", itemOrder);
}

/** The package counterpart of [`orderedItemIds`]. */
export function orderedPackageIds(
  payload: DragPayload,
  packageOrder: string[],
): string[] | null {
  return ordered(payload, "package", packageOrder);
}

function ordered(
  payload: DragPayload,
  kind: "item" | "package",
  order: string[],
): string[] | null {
  if (payload.length === 0 || payload.some((entry) => entry.kind !== kind))
    return null;
  const wanted = new Set(payload.map((entry) => entry.id));
  const result = order.filter((id) => wanted.has(id));
  return result.length > 0 ? result : null;
}

/**
 * Where a drop lands within a row's package: before the row when the pointer
 * is in its top half, after it in the bottom half (there is no row after the
 * last one to hover, so this is the only way to append).
 */
export const dropIndex = (
  rowIndex: number,
  offsetY: number,
  rowHeight: number,
) => (offsetY > rowHeight / 2 ? rowIndex + 1 : rowIndex);

/**
 * The full package order after moving `dragged` (already in list order) next
 * to `targetId`: before it, or after it when `preferAfter`. `null` when the
 * target is itself being dragged (dropping a selection on one of its own
 * members is a no-op) or does not exist.
 */
export function reorderedPackageIds(
  order: string[],
  dragged: string[],
  targetId: string,
  preferAfter: boolean,
): string[] | null {
  const draggedSet = new Set(dragged);
  if (draggedSet.has(targetId)) return null;
  const remaining = order.filter((id) => !draggedSet.has(id));
  const anchor = remaining.indexOf(targetId);
  if (anchor < 0) return null;
  remaining.splice(preferAfter ? anchor + 1 : anchor, 0, ...dragged);
  return remaining;
}
