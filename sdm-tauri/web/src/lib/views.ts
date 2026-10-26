import { HEADER_HEIGHT, ITEM_HEIGHT } from "./layout";
import type { ItemModel, PackageOrder } from "./store.svelte";

/** The packages (and their items, in order) one tab shows. */
export interface ListView {
  packages: PackageOrder[];
}

export interface Row {
  kind: "package" | "item";
  id: string;
  height: number;
  packageId: string;
  /** The package's index in the view, or the item's index within its package (zebra striping). */
  index: number;
}

/**
 * The Completed tab: a filtered view of the same list, not a second store.
 * Only completed items, and only packages that still have one.
 */
export function completedView(
  order: PackageOrder[],
  items: ReadonlyMap<string, ItemModel>,
): ListView {
  const packages: PackageOrder[] = [];
  for (const pkg of order) {
    const itemIds = pkg.itemIds.filter(
      (id) => items.get(id)?.state.kind === "completed",
    );
    if (itemIds.length > 0) packages.push({ id: pkg.id, itemIds });
  }
  return { packages };
}

/** A header's figures when it lists only some of a package's items. */
export function completedAggregate(
  itemIds: string[],
  items: ReadonlyMap<string, ItemModel>,
): { completedBytes: number; totalBytes: number } {
  let completedBytes = 0;
  let totalBytes = 0;
  for (const id of itemIds) {
    const item = items.get(id);
    if (!item) continue;
    completedBytes += item.completedBytes;
    totalBytes += item.totalBytes ?? 0;
  }
  return { completedBytes, totalBytes };
}

/** Header, then items, for every package; a collapsed package shows only its header. */
export function flatRows(
  view: ListView,
  collapsed: ReadonlySet<string>,
): Row[] {
  const rows: Row[] = [];
  view.packages.forEach((pkg, packageIndex) => {
    rows.push({
      kind: "package",
      id: pkg.id,
      height: HEADER_HEIGHT,
      packageId: pkg.id,
      index: packageIndex,
    });
    if (collapsed.has(pkg.id)) return;
    pkg.itemIds.forEach((id, itemIndex) =>
      rows.push({
        kind: "item",
        id,
        height: ITEM_HEIGHT,
        packageId: pkg.id,
        index: itemIndex,
      }),
    );
  });
  return rows;
}

/** Every item id of the view, in list order (collapsed packages included). */
export const itemOrder = (view: ListView): string[] =>
  view.packages.flatMap((pkg) => pkg.itemIds);

/** The items a selection stands for: package keys expand to their items. List order, no repeats. */
export function selectedItemIds(
  selection: ReadonlySet<string>,
  view: ListView,
): string[] {
  const wanted = new Set<string>();
  for (const pkg of view.packages) {
    if (selection.has(pkg.id)) pkg.itemIds.forEach((id) => wanted.add(id));
    else
      pkg.itemIds
        .filter((id) => selection.has(id))
        .forEach((id) => wanted.add(id));
  }
  return itemOrder(view).filter((id) => wanted.has(id));
}
