import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import ChromeSetup from "../components/ChromeSetup.svelte";
import { createContext } from "../lib/context";
import { MockBridge } from "../lib/mockBridge";
import type { Platform } from "../lib/platform";
import { DownloadStore } from "../lib/store.svelte";
import { UiState } from "../lib/ui.svelte";

function open(
  setup: (bridge: MockBridge) => void = () => {},
  platform: Platform = "mac",
) {
  const bridge = new MockBridge();
  setup(bridge);
  const onClose = vi.fn();
  const clipboard = { writeText: vi.fn(async () => {}) };
  const context = createContext(
    bridge,
    new DownloadStore(() => {}),
    new UiState(),
  );
  render(ChromeSetup, {
    context,
    props: { onClose, clipboard, platform, themeReady: Promise.resolve() },
  });
  return { bridge, onClose, clipboard };
}

describe("the Chrome extension setup window", () => {
  it("shows its window once, after its status is in", async () => {
    const { bridge } = open();
    await waitFor(() => expect(bridge.windowReadyCalls).toBe(1));
    await new Promise((done) => setTimeout(done, 1200));
    expect(bridge.windowReadyCalls).toBe(1);
  });

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

  describe.each(["windows", "linux"] as const)("on %s", (platform) => {
    // Chrome drops a chrome:// address handed to it by another program, so
    // the step asks the user to paste it instead of opening the page.
    it("shows the address to paste, with the instructions for it", async () => {
      open(() => {}, platform);
      await screen.findByText("Install the Chrome Extension");
      const box = screen.getByLabelText(
        "Extensions page address",
      ) as HTMLInputElement;
      expect(box.value).toBe("chrome://extensions");
      expect(box.readOnly).toBe(true);
      expect(
        screen.getByText(/open a new tab, paste this address/i),
      ).toBeTruthy();
      expect(
        screen.queryByRole("button", { name: "Open Chrome Extensions" }),
      ).toBeNull();
    });

    it("copies the address from its own button", async () => {
      const { bridge, clipboard } = open(() => {}, platform);
      await screen.findByText("Install the Chrome Extension");
      await fireEvent.click(
        screen.getByRole("button", { name: "Copy Address" }),
      );
      await waitFor(() =>
        expect(clipboard.writeText).toHaveBeenCalledWith("chrome://extensions"),
      );
      expect(bridge.chromeExtensionsOpens).toBe(0);
    });
  });

  it("keeps the Open Chrome Extensions button on the Mac", async () => {
    open(() => {}, "mac");
    await screen.findByText("Install the Chrome Extension");
    expect(
      screen.getByRole("button", { name: "Open Chrome Extensions" }),
    ).toBeTruthy();
    expect(screen.queryByLabelText("Extensions page address")).toBeNull();
  });
});
