import type { DownloadStore } from "./store.svelte";
import type { UiState } from "./ui.svelte";

/** Keeps the selection to rows that still exist after every structure change. */
export function wireSelectionPruning(store: DownloadStore, ui: UiState) {
  store.onStructure = () => ui.selection.prune(store.liveKeys());
}
