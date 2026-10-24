import { fireEvent, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import type { TrayStatus } from "../bindings";
import { formatSpeed } from "../lib/format";
import { makeApp, renderIn } from "../test/render";
import TrayPopover from "./TrayPopover.svelte";
import source from "./TrayPopover.svelte?raw";

const MB = 1024 * 1024;

function status(over: Partial<TrayStatus> = {}): TrayStatus {
  return {
    runningCount: 0,
    nonResumableCount: 0,
    bytesPerSecond: 0,
    history: [],
    overallFraction: 0,
    items: [],
    linksWaiting: 0,
    ...over,
  };
}

function running(n: number): TrayStatus {
  return status({
    runningCount: n,
    bytesPerSecond: 5 * MB,
    history: [1, 2, 3],
    items: Array.from({ length: n }, (_, i) => ({
      name: `file${i}.iso`,
      fraction: 0.5,
      bytesPerSecond: (i + 1) * MB,
    })),
  });
}

async function open(tray: TrayStatus) {
  const app = makeApp();
  app.bridge.answers.trayStatus = tray;
  renderIn(TrayPopover, app);
  await screen.findByText(`${tray.runningCount} active`);
  return app;
}

describe("TrayPopover", () => {
  it("shows an idle engine: zero speed, zero active, no downloads, no links row", async () => {
    await open(status());
    expect(screen.getByText(formatSpeed(0))).toBeTruthy();
    expect(screen.queryByText(/links? waiting/)).toBeNull();
    expect(screen.queryByText("Review")).toBeNull();
  });

  it("lists the running downloads with their speeds", async () => {
    await open(running(3));
    expect(screen.getByText(formatSpeed(5 * MB))).toBeTruthy();
    for (let i = 0; i < 3; i++) {
      expect(screen.getByText(`file${i}.iso`)).toBeTruthy();
      expect(screen.getByText(formatSpeed((i + 1) * MB))).toBeTruthy();
    }
  });

  it("lists at most five downloads", async () => {
    await open(running(8));
    expect(screen.getByText("file4.iso")).toBeTruthy();
    expect(screen.queryByText("file5.iso")).toBeNull();
  });

  it("shows waiting links with a Review button that opens the Linkgrabber", async () => {
    const app = await open(status({ linksWaiting: 4 }));
    expect(screen.getByText("4 links waiting")).toBeTruthy();
    await fireEvent.click(screen.getByText("Review"));
    expect(app.bridge.calls).toContainEqual(["showMain", "linkgrabber"]);
  });

  it("says 'link' for one waiting link", async () => {
    await open(status({ linksWaiting: 1 }));
    expect(screen.getByText("1 link waiting")).toBeTruthy();
  });

  it("Pause all, Open SDM and Quit call the backend", async () => {
    const app = await open(status());
    await fireEvent.click(screen.getByText("Pause all"));
    await fireEvent.click(screen.getByText("Open SDM"));
    await fireEvent.click(screen.getByText("Quit"));
    const names = app.bridge.calls.map(([name]) => name);
    expect(names).toContain("pauseAll");
    expect(names).toContain("showMain");
    expect(names).toContain("requestQuit");
  });

  it("takes every color from the theme, never a literal", () => {
    expect(source).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
    expect(source).not.toMatch(/rgba?\(/);
  });
});
