import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import SettingsWindow from "../components/settings/SettingsWindow.svelte";
import { BridgeError } from "../lib/bridge";
import { createContext } from "../lib/context";
import { MockBridge } from "../lib/mockBridge";
import type { Platform } from "../lib/platform";
import { DownloadStore } from "../lib/store.svelte";
import { UiState } from "../lib/ui.svelte";

async function open(platform: Platform = "windows") {
  const bridge = new MockBridge();
  const onClose = vi.fn();
  const context = createContext(
    bridge,
    new DownloadStore(() => {}),
    new UiState(),
  );
  render(SettingsWindow, { context: context, props: { platform, onClose } });
  // The stored settings arrive asynchronously.
  await screen.findByRole("tab", { name: "Appearance" });
  await waitFor(() =>
    expect(screen.queryByTestId("settings-loading")).toBeNull(),
  );
  return { bridge, onClose };
}

const tab = (name: string) => screen.getByRole("tab", { name });

describe("the Settings window", () => {
  it("shows the five tabs in order", async () => {
    await open();
    expect(
      screen.getAllByRole("tab").map((t) => t.textContent?.trim()),
    ).toEqual([
      "Appearance",
      "Downloads",
      "Linkgrabber",
      "Media Sites",
      "Notifications",
    ]);
  });

  it("shows the five stepped fields on Downloads with their defaults", async () => {
    await open();
    await fireEvent.click(tab("Downloads"));
    const values = screen
      .getAllByRole("spinbutton")
      .map((field) => (field as HTMLInputElement).value);
    expect(values).toEqual(["3", "8", "32", "8", "10"]);
    expect(screen.getByLabelText("Max concurrent downloads")).toBeTruthy();
    expect(screen.getByLabelText("Minimum segment size (MB)")).toBeTruthy();
  });

  it("sends one update and closes on OK", async () => {
    const { bridge, onClose } = await open();
    const update = vi.spyOn(bridge, "updateSettings");
    await fireEvent.click(tab("Downloads"));
    await fireEvent.input(screen.getByLabelText("Max concurrent downloads"), {
      target: { value: "5" },
    });
    await fireEvent.click(screen.getByRole("button", { name: "OK" }));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(update).toHaveBeenCalledTimes(1);
    const sent = update.mock.calls[0][0];
    expect(sent.maxConcurrent).toBe(5);
    expect(sent.downloadFolder).toBeNull();
    expect((await bridge.getSettings()).maxConcurrent).toBe(5);
  });

  it("sends nothing and closes on Cancel", async () => {
    const { bridge, onClose } = await open();
    const update = vi.spyOn(bridge, "updateSettings");
    await fireEvent.click(tab("Downloads"));
    await fireEvent.input(screen.getByLabelText("Max concurrent downloads"), {
      target: { value: "9" },
    });
    await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(update).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledTimes(1);
    expect((await bridge.getSettings()).maxConcurrent).toBe(3);
  });

  it("presses OK on Enter and Cancel on Escape", async () => {
    const { bridge, onClose } = await open();
    const update = vi.spyOn(bridge, "updateSettings");
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(update).not.toHaveBeenCalled();
    await fireEvent.keyDown(window, { key: "Enter" });
    await waitFor(() => expect(update).toHaveBeenCalledTimes(1));
  });

  it("keeps the window open and says why when the update is rejected", async () => {
    const { bridge, onClose } = await open();
    vi.spyOn(bridge, "updateSettings").mockRejectedValue(
      new BridgeError({
        kind: "invalidArgument",
        argument: "maxConcurrent",
        message: "must be between 1 and 20",
      }),
    );
    await fireEvent.click(screen.getByRole("button", { name: "OK" }));
    expect(await screen.findByRole("alert")).toHaveProperty(
      "textContent",
      "must be between 1 and 20",
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it("disables the last checked codec box", async () => {
    await open();
    await fireEvent.click(tab("Media Sites"));
    await fireEvent.click(screen.getByLabelText("AV1"));
    await fireEvent.click(screen.getByLabelText("VP9"));
    expect((screen.getByLabelText("H.264") as HTMLInputElement).disabled).toBe(
      true,
    );
    expect((screen.getByLabelText("AV1") as HTMLInputElement).disabled).toBe(
      false,
    );
  });

  it("shows the Dock / Menu Bar section on macOS only", async () => {
    await open("windows");
    expect(screen.queryByText("Dock / Menu Bar")).toBeNull();
  });

  it("shows the Dock / Menu Bar section on macOS", async () => {
    await open("mac");
    expect(screen.getAllByText("Dock / Menu Bar").length).toBeGreaterThan(0);
  });

  it("never offers a download folder", async () => {
    await open();
    for (const name of [
      "Appearance",
      "Downloads",
      "Linkgrabber",
      "Media Sites",
    ]) {
      await fireEvent.click(tab(name));
      expect(screen.queryByLabelText(/folder/i)).toBeNull();
    }
  });

  it("says (bundled) for the tools when the app ships them", async () => {
    const bridge = new MockBridge();
    bridge.binaries = {
      ...bridge.binaries,
      bundled: true,
      ffmpegVersion: "N-1",
    };
    render(SettingsWindow, {
      context: createContext(
        bridge,
        new DownloadStore(() => {}),
        new UiState(),
      ),
      props: { platform: "mac", onClose: () => {} },
    });
    await fireEvent.click(
      await screen.findByRole("tab", { name: "Media Sites" }),
    );
    expect((await screen.findAllByText(/\(bundled\)/)).length).toBe(3);
  });

  it("shows a download status instead when the tools are downloaded", async () => {
    const bridge = new MockBridge();
    bridge.binaries = {
      ...bridge.binaries,
      bundled: false,
      downloading: true,
      ffmpegVersion: null,
    };
    render(SettingsWindow, {
      context: createContext(
        bridge,
        new DownloadStore(() => {}),
        new UiState(),
      ),
      props: { platform: "windows", onClose: () => {} },
    });
    await fireEvent.click(
      await screen.findByRole("tab", { name: "Media Sites" }),
    );
    expect((await screen.findAllByText("downloading…")).length).toBe(3);
    expect(screen.queryByText(/\(bundled\)/)).toBeNull();
  });

  it("runs a check and saves the channel at once, without sending other edits", async () => {
    const { bridge } = await open();
    await fireEvent.click(tab("Downloads"));
    await fireEvent.input(screen.getByLabelText("Max concurrent downloads"), {
      target: { value: "7" },
    });
    await fireEvent.click(tab("Media Sites"));
    await fireEvent.change(screen.getByLabelText("Update channel"), {
      target: { value: "nightly" },
    });
    await waitFor(() => expect(bridge.binaryChecks).toBe(1));
    const stored = await bridge.getSettings();
    expect(stored.ytDlpChannel).toBe("nightly");
    expect(stored.maxConcurrent).toBe(3);
  });

  it("previews a theme everywhere and takes it back on Cancel", async () => {
    const { bridge } = await open();
    const broadcast = vi.spyOn(bridge, "broadcastTheme");
    await fireEvent.change(screen.getByLabelText("Theme"), {
      target: { value: "dark" },
    });
    expect(broadcast).toHaveBeenLastCalledWith("dark");
    await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(broadcast).toHaveBeenLastCalledWith(null);
  });

  it("lets a number be typed digit by digit even when a prefix is out of range", async () => {
    const { bridge } = await open();
    await fireEvent.click(tab("Media Sites"));
    const field = screen.getByLabelText(
      "Maximum playlist videos",
    ) as HTMLInputElement;
    // "1" is below the minimum of 10; typing on to "15" must still end at 15.
    await fireEvent.input(field, { target: { value: "1" } });
    expect(field.value).toBe("1");
    await fireEvent.input(field, { target: { value: "15" } });
    expect(field.value).toBe("15");
    await fireEvent.click(screen.getByRole("button", { name: "OK" }));
    await waitFor(async () =>
      expect((await bridge.getSettings()).maxPlaylistVideos).toBe(15),
    );
  });

  it("clamps what was typed when the field is left or Enter is pressed", async () => {
    const { bridge } = await open();
    await fireEvent.click(tab("Media Sites"));
    const field = screen.getByLabelText(
      "Maximum playlist videos",
    ) as HTMLInputElement;
    await fireEvent.input(field, { target: { value: "1" } });
    await fireEvent.blur(field);
    expect(field.value).toBe("10");
    await fireEvent.input(field, { target: { value: "999" } });
    await fireEvent.keyDown(field, { key: "Enter" });
    expect(field.value).toBe("200");
    await waitFor(async () =>
      expect((await bridge.getSettings()).maxPlaylistVideos).toBe(200),
    );
  });
});
