import { describe, expect, it, vi } from "vitest";
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

  it("applies a tab the backend asked for before the page was ready", async () => {
    const bridge = new MockBridge();
    bridge.pendingRoute = "linkgrabber";
    const ui = new UiState();
    wireNavigation(bridge, ui);
    await vi.waitFor(() => expect(ui.tab).toBe("linkgrabber"));
    expect(bridge.pendingRoute).toBeNull();
  });

  it("stays on the current tab when nothing was asked for", async () => {
    const bridge = new MockBridge();
    const ui = new UiState();
    wireNavigation(bridge, ui);
    await Promise.resolve();
    expect(ui.tab).toBe("downloads");
  });
});
