import {
  canDisable,
  canEnable,
  canOpen,
  canReset,
  canRetry,
  canRetryMux,
  canStart,
  canStop,
  type RowItem,
} from "./eligibility";
import type { IconName } from "./icons";
import { fileManagerName, type Platform } from "./platform";

export type MenuId =
  | "open"
  | "show"
  | "copy"
  | "start"
  | "pause"
  | "enable"
  | "disable"
  | "retry"
  | "retryMux"
  | "reset"
  | "moveToNew"
  | "removeFromList"
  | "removeAndDelete"
  | "info";

export type MenuEntry =
  | {
      id: MenuId;
      label: string;
      icon?: IconName;
      shortcut?: string;
      disabled: boolean;
    }
  | { id: "divider" };

export interface MenuOptions {
  platform: Platform;
}

const DIVIDER: MenuEntry = { id: "divider" };

function shortcuts(platform: Platform) {
  return platform === "mac"
    ? { open: "↩", show: "⌘↩", remove: "⌫", removeAndDelete: "⌘⌫" }
    : {
        open: "Enter",
        show: "Ctrl+Enter",
        remove: "Del",
        removeAndDelete: "Ctrl+Del",
      };
}

/**
 * The item context menu, in the order and wording of `itemsContextMenu`
 * (`PackagesListView.swift`). An entry is disabled when nothing selected can
 * take its action; Open and Show need exactly one item.
 */
export function itemMenu(
  items: RowItem[],
  { platform }: MenuOptions,
): MenuEntry[] {
  const some = (predicate: (item: RowItem) => boolean) => items.some(predicate);
  const multiple = items.length > 1;
  const s = multiple ? "s" : "";
  const keys = shortcuts(platform);
  const empty = items.length === 0;
  return [
    {
      id: "open",
      label: "Open File",
      icon: "eye",
      shortcut: keys.open,
      disabled: items.length !== 1 || !canOpen(items[0]),
    },
    {
      id: "show",
      label: `Show in ${fileManagerName(platform)}`,
      icon: "folder-open",
      shortcut: keys.show,
      disabled: items.length !== 1,
    },
    {
      id: "copy",
      label: `Copy URL${s} to Clipboard`,
      icon: "copy",
      disabled: empty,
    },
    DIVIDER,
    {
      id: "start",
      label: `Start Download${s}`,
      icon: "play",
      disabled: !some(canStart),
    },
    {
      id: "pause",
      label: `Pause Download${s}`,
      icon: "pause",
      disabled: !some(canStop),
    },
    DIVIDER,
    {
      id: "enable",
      label: multiple ? "Enable All" : "Enable",
      icon: "toggle-left",
      disabled: !some(canEnable),
    },
    {
      id: "disable",
      label: multiple ? "Disable All" : "Disable",
      icon: "toggle-right",
      disabled: !some(canDisable),
    },
    {
      id: "retry",
      label: `Retry Failed Item${s}`,
      icon: "refresh-cw",
      disabled: !some(canRetry),
    },
    {
      id: "retryMux",
      label: `Retry Mux${s}`,
      icon: "rotate-cw",
      disabled: !some(canRetryMux),
    },
    {
      id: "reset",
      label: `Reset Download${s}`,
      icon: "arrow-left-to-line",
      disabled: !some(canReset),
    },
    DIVIDER,
    {
      id: "moveToNew",
      label: "Move to New Package",
      icon: "folder-plus",
      disabled: empty,
    },
    {
      id: "removeFromList",
      label: "Remove from List",
      icon: "eraser",
      shortcut: keys.remove,
      disabled: empty,
    },
    {
      id: "removeAndDelete",
      label: `Remove and Delete File${s}`,
      icon: "trash-2",
      shortcut: keys.removeAndDelete,
      disabled: empty,
    },
  ];
}

/** The toolbar: the item menu's entries without dividers, then the Info Panel toggle. */
export function toolbarEntries(
  items: RowItem[],
  options: MenuOptions,
): MenuEntry[] {
  return [
    ...itemMenu(items, options).filter((entry) => entry.id !== "divider"),
    { id: "info", label: "Info Panel", icon: "panel-bottom", disabled: false },
  ];
}
