import { mount } from "svelte";
import "./theme.css";
import App from "./App.svelte";
import type { Bridge } from "./lib/bridge";
import { createContext } from "./lib/context";
import { MockBridge, loadSeed } from "./lib/mockBridge";
import { DownloadStore } from "./lib/store.svelte";
import { TauriBridge } from "./lib/tauriBridge";
import { UiState } from "./lib/ui.svelte";
import { wireSelectionPruning } from "./lib/wiring";

// Inside the Tauri window the real core answers; in a plain browser (dev,
// Playwright) an in-memory mock does.
const bridge: Bridge =
  "__TAURI_INTERNALS__" in window ? new TauriBridge() : mockFromQuery();
/** `?grabber=demo` pre-loads the link grabber (`&ffmpeg=off` for a machine without it); `?vibrancy=on` fakes a window effect; `?simulate=500:50` loads that many items, that many at once. */
function mockFromQuery(): Bridge {
  const query = new URLSearchParams(location.search);
  const [items, active] = (query.get("simulate") ?? "").split(":").map(Number);
  return new MockBridge({
    vibrancy: query.get("vibrancy") === "on",
    grabber: {
      ...(query.get("grabber") ? { text: query.get("grabber")! } : {}),
      ffmpeg: query.get("ffmpeg") !== "off",
    },
    ...(items > 0 && active > 0
      ? { seed: loadSeed(items), maxRunning: active }
      : {}),
  });
}

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
