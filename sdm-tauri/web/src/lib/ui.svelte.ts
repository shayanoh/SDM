import type { DeletionInfo, DeletionTarget } from "../bindings";
import { SelectionModel } from "./selection";

export type Tab = "downloads" | "completed" | "linkgrabber";
export type ListTab = "downloads" | "completed";

export type ModalState =
  | { kind: "delete"; target: DeletionTarget; info: DeletionInfo }
  | { kind: "pause"; ids: string[] }
  | { kind: "newPackage"; ids: string[] };

export interface MenuState {
  x: number;
  y: number;
  /** The row (item or package id) the menu was opened on. */
  targetId: string;
}

export const MIN_PANEL_HEIGHT = 100;
export const MAX_PANEL_HEIGHT = 600;

/** Window-level state that is not part of the download list. */
export class UiState {
  /** The error line shown in the banner, or null. */
  error = $state<string | null>(null);

  tab = $state<Tab>("downloads");
  panelVisible = $state(true);
  panelHeight = $state(150);
  modal = $state.raw<ModalState | null>(null);
  menu = $state.raw<MenuState | null>(null);
  collapsed = $state.raw<Record<ListTab, ReadonlySet<string>>>({
    downloads: new Set(),
    completed: new Set(),
  });

  #selected = $state.raw<ReadonlySet<string>>(new Set());
  /** Selection logic; `selected` follows it reactively. */
  readonly selection = new SelectionModel((next) => {
    this.#selected = next;
  });

  get selected(): ReadonlySet<string> {
    return this.#selected;
  }

  report(message: string) {
    this.error = message;
  }

  dismiss() {
    this.error = null;
  }

  setTab(tab: Tab) {
    if (tab === this.tab) return;
    this.tab = tab;
    this.selection.clear();
    this.menu = null;
  }

  setPanelHeight(height: number) {
    if (!Number.isFinite(height)) return;
    this.panelHeight = Math.min(MAX_PANEL_HEIGHT, Math.max(MIN_PANEL_HEIGHT, height));
  }

  isCollapsed(tab: ListTab, packageId: string): boolean {
    return this.collapsed[tab].has(packageId);
  }

  toggleCollapsed(tab: ListTab, packageId: string) {
    const next = new Set(this.collapsed[tab]);
    if (!next.delete(packageId)) next.add(packageId);
    this.collapsed = { ...this.collapsed, [tab]: next };
  }
}
