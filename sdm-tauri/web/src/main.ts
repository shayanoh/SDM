import { mount } from "svelte";
import "./theme.css";
import App from "./App.svelte";
import type { Bridge } from "./lib/bridge";
import { createContext } from "./lib/context";
import { MockBridge } from "./lib/mockBridge";
import { DownloadStore } from "./lib/store.svelte";
import { TauriBridge } from "./lib/tauriBridge";
import { UiState } from "./lib/ui.svelte";
import { wireSelectionPruning } from "./lib/wiring";

// Inside the Tauri window the real core answers; in a plain browser (dev,
// Playwright) an in-memory mock does.
const bridge: Bridge =
  "__TAURI_INTERNALS__" in window ? new TauriBridge() : new MockBridge();
const ui = new UiState();
const store = new DownloadStore(() => {
  bridge.fullState().catch((error) => ui.report(String(error)));
});

wireSelectionPruning(store, ui);

const target = document.getElementById("app");
if (!target) throw new Error("missing #app element");

export default mount(App, {
  target,
  context: createContext(bridge, store, ui),
});
