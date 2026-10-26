import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import AboutWindow from "../components/AboutWindow.svelte";
import { createContext } from "../lib/context";
import { MockBridge } from "../lib/mockBridge";
import { DownloadStore } from "../lib/store.svelte";
import { UiState } from "../lib/ui.svelte";

function open(setup: (bridge: MockBridge) => void = () => {}) {
  const bridge = new MockBridge();
  setup(bridge);
  const onClose = vi.fn();
  render(AboutWindow, {
    context: createContext(bridge, new DownloadStore(() => {}), new UiState()),
    props: { onClose, themeReady: Promise.resolve() },
  });
  return { bridge, onClose };
}

describe("the About window", () => {
  it("shows the app, its version and the components", async () => {
    open((bridge) => {
      bridge.binaries = {
        ...bridge.binaries,
        ffmpegVersion: "1789931890_9.0.2",
        qjsVersion: "v0.17.0",
      };
      bridge.chromeStatus = {
        ...bridge.chromeStatus,
        state: "connected",
        installedVersion: "2.0.1",
      };
    });
    expect(await screen.findByText("Shayan's Download Manager")).toBeTruthy();
    expect(await screen.findByText("Version 4.0.0")).toBeTruthy();
    expect(await screen.findByText("2026.10.21")).toBeTruthy();
    expect(await screen.findByText("1789931890_9.0.2")).toBeTruthy();
    expect(await screen.findByText("v0.17.0")).toBeTruthy();
    expect(await screen.findByText("2.0.1")).toBeTruthy();
    expect(
      screen.getByText(/© 2026 Shayan Ostadhassan · MIT License/),
    ).toBeTruthy();
  });

  it("is shown once its text is in", async () => {
    const { bridge } = open();
    await waitFor(() => expect(bridge.windowReadyCalls).toBe(1));
    await new Promise((done) => setTimeout(done, 2200));
    expect(bridge.windowReadyCalls).toBe(1);
  });

  it("opens the home page", async () => {
    const { bridge } = open();
    await fireEvent.click(
      await screen.findByRole("button", { name: "github.com/shayanoh/SDM" }),
    );
    expect(bridge.homepageOpens).toBe(1);
  });

  it("shows the licenses on request, and Back (or Escape) returns", async () => {
    const { bridge, onClose } = open((bridge) => {
      bridge.licensesText = "THE LICENSES";
    });
    await fireEvent.click(
      await screen.findByRole("button", { name: /Open-source licenses/ }),
    );
    expect(await screen.findByText("THE LICENSES")).toBeTruthy();
    await fireEvent.click(screen.getByRole("button", { name: /Back/ }));
    expect(await screen.findByText("Components")).toBeTruthy();

    await fireEvent.click(
      screen.getByRole("button", { name: /Open-source licenses/ }),
    );
    await screen.findByText("THE LICENSES");
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(await screen.findByText("Components")).toBeTruthy();
    expect(onClose).not.toHaveBeenCalled();
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(bridge.aboutOpens).toBe(0);
  });
});
