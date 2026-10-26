import type { Bridge } from "./bridge";
import type { Tab, UiState } from "./ui.svelte";

const TABS: readonly Tab[] = ["downloads", "completed", "linkgrabber"];

/**
 * Switches the window to the tab the backend asks for (a clicked
 * notification, the tray's Review button, a download captured by the Chrome
 * extension). Returns the way to stop.
 */
export function wireNavigation(bridge: Bridge, ui: UiState): () => void {
  const apply = (route: string | null | undefined) => {
    const tab = TABS.find((known) => known === route);
    if (tab) ui.setTab(tab);
  };
  const stop = bridge.onNavigate(apply);
  // A request made while the page was still loading (the app started by the
  // Chrome extension) was parked: pick it up now that this listener is on.
  void bridge
    .takePendingRoute()
    .then(apply)
    .catch(() => {});
  return stop;
}
