import { describe, expect, it } from "vitest";
import type { ItemStateDto } from "../bindings";
import type { RowItem } from "./eligibility";
import { itemMenu, toolbarEntries, type MenuEntry } from "./menu";
import { detectPlatform } from "./platform";

const item = (state: ItemStateDto, over: Partial<RowItem> = {}): RowItem => ({
  state,
  isEnabled: true,
  isResumable: true,
  fileMissing: false,
  assembly: "none",
  ...over,
});

const running = item({ kind: "running" });
const stopped = item({ kind: "stopped" });
const done = item({ kind: "completed" });
const failedMux = item(
  { kind: "failed", reason: "ffmpeg" },
  { assembly: "mux" },
);
const disabled = item({ kind: "stopped" }, { isEnabled: false });

const win = { platform: "windows" as const };

const real = (entries: MenuEntry[]) =>
  entries.filter(
    (e): e is Exclude<MenuEntry, { id: "divider" }> => e.id !== "divider",
  );
const byId = (entries: MenuEntry[]) =>
  Object.fromEntries(real(entries).map((e) => [e.id, e]));

describe("item menu", () => {
  it("lists the SwiftUI entries in order with dividers after Copy, Pause and Reset", () => {
    const ids = itemMenu([running], win).map((e) => e.id);
    expect(ids).toEqual([
      "open",
      "show",
      "copy",
      "divider",
      "start",
      "pause",
      "divider",
      "enable",
      "disable",
      "retry",
      "retryMux",
      "reset",
      "divider",
      "moveToNew",
      "removeFromList",
      "removeAndDelete",
    ]);
  });

  it("uses singular labels for one item and plural for several", () => {
    const one = byId(itemMenu([running], win));
    expect(one.copy.label).toBe("Copy URL to Clipboard");
    expect(one.start.label).toBe("Start Download");
    expect(one.pause.label).toBe("Pause Download");
    expect(one.enable.label).toBe("Enable");
    expect(one.disable.label).toBe("Disable");
    expect(one.retry.label).toBe("Retry Failed Item");
    expect(one.reset.label).toBe("Reset Download");
    expect(one.removeAndDelete.label).toBe("Remove and Delete File");

    const many = byId(itemMenu([running, stopped], win));
    expect(many.copy.label).toBe("Copy URLs to Clipboard");
    expect(many.start.label).toBe("Start Downloads");
    expect(many.enable.label).toBe("Enable All");
    expect(many.disable.label).toBe("Disable All");
    expect(many.retry.label).toBe("Retry Failed Items");
    expect(many.retryMux.label).toBe("Retry Muxs");
    expect(many.removeAndDelete.label).toBe("Remove and Delete Files");
  });

  it("names the file manager per platform", () => {
    const label = (platform: "windows" | "linux" | "mac") =>
      byId(itemMenu([done], { platform })).show.label;
    expect(label("windows")).toBe("Show in Explorer");
    expect(label("linux")).toBe("Show in File Manager");
    expect(label("mac")).toBe("Show in Finder");
  });

  it("shows shortcuts with the platform's modifier", () => {
    const win_ = byId(itemMenu([done], { platform: "windows" }));
    expect(win_.open.shortcut).toBe("Enter");
    expect(win_.show.shortcut).toBe("Ctrl+Enter");
    expect(win_.removeFromList.shortcut).toBe("Del");
    expect(win_.removeAndDelete.shortcut).toBe("Ctrl+Del");
    const mac = byId(itemMenu([done], { platform: "mac" }));
    expect(mac.show.shortcut).toBe("⌘↩");
    expect(mac.removeAndDelete.shortcut).toBe("⌘⌫");
  });

  it("disables every entry that has nothing to act on, for an empty selection", () => {
    const entries = real(itemMenu([], win));
    expect(entries.every((e) => e.disabled)).toBe(true);
  });

  it("disables entries by what the selected items allow", () => {
    const e = byId(itemMenu([running, disabled, failedMux], win));
    expect(e.open.disabled).toBe(true); // several items
    expect(e.show.disabled).toBe(true);
    expect(e.copy.disabled).toBe(false);
    expect(e.start.disabled).toBe(true); // the stopped one is disabled
    expect(e.pause.disabled).toBe(false); // the running one
    expect(e.enable.disabled).toBe(false); // the disabled one
    expect(e.disable.disabled).toBe(false);
    expect(e.retry.disabled).toBe(false);
    expect(e.retryMux.disabled).toBe(false);
    expect(e.reset.disabled).toBe(false);
    expect(e.moveToNew.disabled).toBe(false);
    expect(e.removeFromList.disabled).toBe(false);
    expect(e.removeAndDelete.disabled).toBe(false);
  });

  it("allows Open only for exactly one finished file", () => {
    expect(byId(itemMenu([done], win)).open.disabled).toBe(false);
    expect(byId(itemMenu([stopped], win)).open.disabled).toBe(true);
    expect(byId(itemMenu([done, done], win)).open.disabled).toBe(true);
    expect(
      byId(itemMenu([item({ kind: "completed" }, { fileMissing: true })], win))
        .open.disabled,
    ).toBe(true);
  });

  it("allows Show for exactly one item", () => {
    expect(byId(itemMenu([stopped], win)).show.disabled).toBe(false);
    expect(byId(itemMenu([stopped, done], win)).show.disabled).toBe(true);
  });

  it("gives every entry an icon", () => {
    expect(real(itemMenu([running], win)).every((e) => e.icon)).toBe(true);
  });
});

describe("toolbar", () => {
  it("is the item menu without dividers, then Info Panel: 14 buttons", () => {
    const entries = toolbarEntries([running], win);
    expect(entries.map((e) => e.id)).toEqual([
      "open",
      "show",
      "copy",
      "start",
      "pause",
      "enable",
      "disable",
      "retry",
      "retryMux",
      "reset",
      "moveToNew",
      "removeFromList",
      "removeAndDelete",
      "info",
    ]);
    expect(real(entries)).toHaveLength(14);
  });

  it("keeps Info Panel enabled with nothing selected", () => {
    const entries = byId(toolbarEntries([], win));
    expect(entries.info.disabled).toBe(false);
    expect(entries.removeFromList.disabled).toBe(true);
  });
});

describe("detectPlatform", () => {
  it("reads the user agent", () => {
    expect(detectPlatform("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")).toBe(
      "windows",
    );
    expect(
      detectPlatform("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)"),
    ).toBe("mac");
    expect(detectPlatform("Mozilla/5.0 (X11; Linux x86_64)")).toBe("linux");
    expect(detectPlatform("")).toBe("linux");
  });
});
