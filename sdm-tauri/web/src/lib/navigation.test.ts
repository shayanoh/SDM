import { describe, expect, it } from "vitest";
import { MockBridge } from "./mockBridge";
import { wireNavigation } from "./navigation";
import { UiState } from "./ui.svelte";

describe("wireNavigation", () => {
  it("switches to the tab the backend names", () => {
    const bridge = new MockBridge();
    const ui = new UiState();
    wireNavigation(bridge, ui);
    bridge.emitNavigate("completed");
    expect(ui.tab).toBe("completed");
    bridge.emitNavigate("linkgrabber");
    expect(ui.tab).toBe("linkgrabber");
  });

  it("ignores a route that is not a tab", () => {
    const bridge = new MockBridge();
    const ui = new UiState();
    wireNavigation(bridge, ui);
    bridge.emitNavigate("settings");
    expect(ui.tab).toBe("downloads");
  });

  it("stops listening once stopped", () => {
    const bridge = new MockBridge();
    const ui = new UiState();
    const stop = wireNavigation(bridge, ui);
    stop();
    bridge.emitNavigate("completed");
    expect(ui.tab).toBe("downloads");
  });
});
