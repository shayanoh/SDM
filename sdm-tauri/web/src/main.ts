import { mount } from "svelte";
import "./theme.css";
import App from "./App.svelte";
import ChromeSetup from "./components/ChromeSetup.svelte";
import SettingsWindow from "./components/settings/SettingsWindow.svelte";
import TrayPopover from "./components/TrayPopover.svelte";
import { startTheming } from "./lib/theme";
import type { Bridge } from "./lib/bridge";
import { createContext } from "./lib/context";
import { MockBridge, demoTrayStatus, loadSeed } from "./lib/mockBridge";
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
  const mock = new MockBridge({
    vibrancy: query.get("vibrancy") === "on",
    grabber: {
      ...(query.get("grabber") ? { text: query.get("grabber")! } : {}),
      ffmpeg: query.get("ffmpeg") !== "off",
    },
    ...(items > 0 && active > 0
      ? { seed: loadSeed(items), maxRunning: active }
      : {}),
  });
  if (query.get("tray") === "running") mock.setTrayStatus(demoTrayStatus());
  return mock;
}

const ui = new UiState();
const store = new DownloadStore(() => {
  bridge.fullState().catch((error) => ui.report(String(error)));
});

wireSelectionPruning(store, ui);

const target = document.getElementById("app");
if (!target) throw new Error("missing #app element");

// The Settings window is a second window on this same bundle.
const query = new URLSearchParams(location.search);
const windowName = query.get("window");
const isSettings = windowName === "settings";
const isTray = windowName === "tray";
const isChromeSetup = windowName === "chrome-setup";
if (isSettings || isTray || isChromeSetup) {
  // The main window's App starts its own theming; these windows have none.
  void startTheming(bridge, query.get("theme")).catch((error) =>
    ui.report(String(error)),
  );
}

const root = isSettings
  ? SettingsWindow
  : isTray
    ? TrayPopover
    : isChromeSetup
      ? ChromeSetup
      : App;

export default mount(root, {
  target,
  context: createContext(bridge, store, ui),
});
