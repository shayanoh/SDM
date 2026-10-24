import type { Bridge } from "./bridge";
import type { Tab, UiState } from "./ui.svelte";

const TABS: readonly Tab[] = ["downloads", "completed", "linkgrabber"];

/**
 * Switches the window to the tab the backend asks for (a clicked
 * notification, the tray's Review button). Returns the way to stop.
 */
export function wireNavigation(bridge: Bridge, ui: UiState): () => void {
  return bridge.onNavigate((route) => {
    const tab = TABS.find((known) => known === route);
    if (tab) ui.setTab(tab);
  });
}
