import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import ChromeSetup from "../components/ChromeSetup.svelte";
import { createContext } from "../lib/context";
import { MockBridge } from "../lib/mockBridge";
import { DownloadStore } from "../lib/store.svelte";
import { UiState } from "../lib/ui.svelte";

function open(setup: (bridge: MockBridge) => void = () => {}) {
  const bridge = new MockBridge();
  setup(bridge);
  const onClose = vi.fn();
  const clipboard = { writeText: vi.fn(async () => {}) };
  const context = createContext(
    bridge,
    new DownloadStore(() => {}),
    new UiState(),
  );
  render(ChromeSetup, { context, props: { onClose, clipboard } });
  return { bridge, onClose, clipboard };
}

describe("the Chrome extension setup window", () => {
  it("prepares the folder when it opens and shows the four steps", async () => {
    const { bridge } = open();
    await screen.findByText("Install the Chrome Extension");
    await waitFor(() => expect(bridge.chromePrepares).toBe(1));
    expect(screen.getByText("Open Chrome's Extensions page")).toBeTruthy();
    expect(screen.getByText("Enable Developer mode")).toBeTruthy();
    expect(screen.getByText("Choose Load unpacked")).toBeTruthy();
    expect(screen.getByText("Select the SDM extension folder")).toBeTruthy();
    expect(await screen.findByText("Extension not connected.")).toBeTruthy();
  });

  it("opens Chrome's extensions page, shows the folder and copies its path", async () => {
    const { bridge, clipboard } = open();
    await screen.findByText("Install the Chrome Extension");
    await fireEvent.click(
      screen.getByRole("button", { name: "Open Chrome Extensions" }),
    );
    await fireEvent.click(screen.getByRole("button", { name: "Show Folder" }));
    await fireEvent.click(screen.getByRole("button", { name: "Copy Path" }));
    expect(bridge.chromeExtensionsOpens).toBe(1);
    expect(bridge.chromeFolderReveals).toBe(1);
    await waitFor(() =>
      expect(clipboard.writeText).toHaveBeenCalledWith(
        bridge.chromeStatus.extensionFolder,
      ),
    );
  });

  it("cannot open Chrome when it is not installed", async () => {
    open((bridge) => {
      bridge.chromeStatus = { ...bridge.chromeStatus, chromeInstalled: false };
    });
    await screen.findByText("Install the Chrome Extension");
    const button = screen.getByRole("button", {
      name: "Open Chrome Extensions",
    }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
    expect(
      screen.getByText("Google Chrome was not found on this computer."),
    ).toBeTruthy();
  });

  it("shows the all-set view once the bundled version reports in", async () => {
    open((bridge) => {
      bridge.chromeStatus = {
        ...bridge.chromeStatus,
        state: "connected",
        installedVersion: "2.0.0",
        bundledVersion: "2.0.0",
        cookiesSyncedSecondsAgo: 5,
      };
    });
    expect(await screen.findByText("You're all set!")).toBeTruthy();
    expect(
      screen.getByText(/SDM Chrome Extension 2\.0\.0 is installed/),
    ).toBeTruthy();
  });

  it("is not all set while a connected extension has sent no cookies", async () => {
    open((bridge) => {
      bridge.chromeStatus = {
        ...bridge.chromeStatus,
        state: "connected",
        installedVersion: "2.0.0",
        bundledVersion: "2.0.0",
        cookiesSyncedSecondsAgo: null,
      };
    });
    expect(await screen.findByText(/no cookies have arrived/)).toBeTruthy();
    expect(screen.queryByText("You're all set!")).toBeNull();
  });

  it("closes from the done button", async () => {
    const { onClose } = open();
    await screen.findByText("Install the Chrome Extension");
    await fireEvent.click(
      screen.getByRole("button", { name: "I've installed the extension" }),
    );
    expect(onClose).toHaveBeenCalled();
  });
});
