import type { GrabberPackage, GrabberView } from "../bindings";
import { isAddable } from "./grabberRows";
import type { IconName } from "./icons";

export type GrabberMenuEntry =
  | { kind: "divider" }
  | {
      kind: "item";
      /** `recheck`, `copy`, `add`, `addStart`, `remove`, `rename`, `split` or `merge:<package id>`. */
      id: string;
      label: string;
      disabled?: boolean;
      destructive?: boolean;
      icon?: IconName;
    }
  | { kind: "submenu"; label: string; entries: GrabberMenuEntry[] };

const item = (
  id: string,
  label: string,
  extra: { disabled?: boolean; destructive?: boolean; icon?: IconName } = {},
): GrabberMenuEntry => ({ kind: "item", id, label, ...extra });

const DIVIDER: GrabberMenuEntry = { kind: "divider" };

/** The menu on a row or a selection of rows. Empty with nothing selected. */
export function rowMenu(view: GrabberView, ids: string[]): GrabberMenuEntry[] {
  if (ids.length === 0) return [];
  const chosen = new Set(ids);
  const canAdd = view.rows.some((row) => chosen.has(row.id) && isAddable(row));
  return [
    item("recheck", "Recheck", { icon: "refresh-cw" }),
    item("copy", "Copy URL", { disabled: ids.length !== 1, icon: "copy" }),
    DIVIDER,
    item("add", "Add to Downloads", { disabled: !canAdd }),
    item("addStart", "Add and Start", { disabled: !canAdd }),
    DIVIDER,
    item("remove", "Remove", { destructive: true, icon: "trash-2" }),
  ];
}

/** The menu on a package header. */
export function packageMenu(
  view: GrabberView,
  pkg: GrabberPackage,
): GrabberMenuEntry[] {
  const others = view.packages.filter((other) => other.id !== pkg.id);
  return [
    item("rename", "Rename…"),
    {
      kind: "submenu",
      label: "Merge into",
      entries:
        others.length === 0
          ? [item("none", "No Other Packages", { disabled: true })]
          : others.map((other) => item(`merge:${other.id}`, other.name)),
    },
    item("split", "Split"),
  ];
}
