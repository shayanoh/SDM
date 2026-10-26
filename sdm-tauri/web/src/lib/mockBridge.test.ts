import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  Patch,
  SettingsDto,
  StructureItem,
  StructurePackage,
} from "../bindings";
import { BridgeError } from "./bridge";
import { MockBridge, type MockSeed } from "./mockBridge";
import { defaultSettings } from "./settingsModel";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

const MB = 1024 * 1024;

/** A full settings object: every field is required on the wire. */
function full(changes: Partial<SettingsDto>): SettingsDto {
  return { ...defaultSettings(), downloadFolder: null, ...changes };
}

async function attached(seed?: MockSeed[]) {
  const bridge = new MockBridge(seed === undefined ? {} : { seed });
  const patches: Patch[] = [];
  await bridge.attach((patch) => patches.push(patch));
  return { bridge, patches };
}

function packages(patches: Patch[]): StructurePackage[] {
  return [...patches].reverse().find((p) => p.structure)!.structure!.packages;
}

function items(patches: Patch[]): StructureItem[] {
  return packages(patches).flatMap((p) => p.items);
}

const named = (patches: Patch[], name: string) =>
  items(patches).find((i) => i.filename === name)!;

const idOf = (patches: Patch[], name: string) => named(patches, name).id;

const pkgOf = (patches: Patch[], name: string) =>
  packages(patches).find((p) => p.items.some((i) => i.filename === name))!;

const seed: MockSeed[] = [
  {
    name: "Alpha",
    items: [
      { name: "a1.bin", size: 100 * MB, state: "stopped", fraction: 0.4 },
      { name: "a2.bin", size: 50 * MB, state: "stopped" },
    ],
  },
  {
    name: "Beta",
    items: [
      { name: "b1.bin", size: 80 * MB, state: "completed" },
      { name: "b2.bin", size: 30 * MB, state: "failed", failure: "HTTP 404" },
    ],
  },
  {
    name: "Gamma",
    items: [{ name: "g1.bin", size: 10 * MB, state: "stopped" }],
  },
];

describe("MockBridge basics", () => {
  it("emits a full patch first", async () => {
    const { patches } = await attached();
    expect(patches).toHaveLength(1);
    expect(patches[0].full).toBe(true);
    expect(patches[0].structure).not.toBeNull();
    expect(patches[0].stats).not.toBeNull();
  });

  it("increments seq by exactly one per patch", async () => {
    const { patches } = await attached();
    vi.advanceTimersByTime(1000);
    expect(patches.length).toBeGreaterThan(2);
    for (let i = 1; i < patches.length; i++) {
      expect(patches[i].seq).toBe((patches[i - 1].seq + 1) >>> 0);
    }
  });

  it("rejects invalid urls with an invalidArgument error", async () => {
    const { bridge } = await attached([]);
    for (const bad of ["nope", "", "ftp://x/y"]) {
      const error = await bridge.addUrl(bad).catch((e) => e);
      expect(error).toBeInstanceOf(BridgeError);
      expect((error as BridgeError).error.kind).toBe("invalidArgument");
    }
  });

  it("puts a new url in a package named after the file", async () => {
    const { bridge, patches } = await attached([]);
    await bridge.addUrl("https://example.com/files/new.bin");
    expect(pkgOf(patches, "new.bin").name).toBe("new.bin");
    expect(pkgOf(patches, "new.bin").folder).toContain("new.bin");
  });

  it("completes a running item eventually", async () => {
    const { bridge, patches } = await attached([]);
    await bridge.addUrl("https://example.com/quick.bin");
    vi.advanceTimersByTime(10 * 60 * 1000);
    expect(named(patches, "quick.bin").state.kind).toBe("completed");
  });

  it("runs at most three items at once and queues the rest", async () => {
    const many: MockSeed[] = [
      {
        name: "Many",
        items: Array.from({ length: 5 }, (_, n) => ({
          name: `m${n}.bin`,
          size: 4000 * MB,
          state: "running" as const,
        })),
      },
    ];
    const { patches } = await attached(many);
    const states = items(patches).map((i) => i.state.kind);
    expect(states.filter((s) => s === "running")).toHaveLength(3);
    expect(states.filter((s) => s === "queued")).toHaveLength(2);
    const stats = patches[0].stats!;
    expect([stats.runningCount, stats.queuedCount]).toEqual([3, 2]);
  });

  it("rejects a malformed id and ignores a well-formed unknown one", async () => {
    const { bridge } = await attached(seed);
    const error = await bridge.startItems(["nope"]).catch((e) => e);
    expect((error as BridgeError).error.kind).toBe("invalidArgument");
    await bridge.startItems(["00000000-0000-4000-8000-0000000000ff"]);
  });
});

describe("MockBridge item actions", () => {
  it("starts and stops items", async () => {
    const { bridge, patches } = await attached(seed);
    const id = idOf(patches, "a1.bin");
    await bridge.startItems([id]);
    expect(named(patches, "a1.bin").state.kind).toBe("running");
    await bridge.stopItems([id]);
    expect(named(patches, "a1.bin").state.kind).toBe("stopped");
  });

  it("does not start a disabled item", async () => {
    const { bridge, patches } = await attached(seed);
    const id = idOf(patches, "a1.bin");
    await bridge.setItemsEnabled([id], false);
    expect(named(patches, "a1.bin").isEnabled).toBe(false);
    await bridge.startItems([id]);
    expect(named(patches, "a1.bin").state.kind).toBe("stopped");
    await bridge.setItemsEnabled([id], true);
    expect(named(patches, "a1.bin").isEnabled).toBe(true);
  });

  it("disabling a running item stops it", async () => {
    const { bridge, patches } = await attached(seed);
    const id = idOf(patches, "a1.bin");
    await bridge.startItems([id]);
    await bridge.setItemsEnabled([id], false);
    expect(named(patches, "a1.bin").state.kind).toBe("stopped");
  });

  it("retries a failed item", async () => {
    const { bridge, patches } = await attached(seed);
    expect(named(patches, "b2.bin").state.kind).toBe("failed");
    await bridge.retryItems([idOf(patches, "b2.bin")]);
    expect(["queued", "running"]).toContain(
      named(patches, "b2.bin").state.kind,
    );
  });

  it("resets a stopped item but skips a running one", async () => {
    const { bridge, patches } = await attached(seed);
    const stopped = idOf(patches, "a1.bin");
    const running = idOf(patches, "a2.bin");
    await bridge.startItems([running]);
    await bridge.resetItems([stopped, running]);
    const last = [...patches].reverse().find((p) => p.items.length)!;
    expect(last.items.find((f) => f.id === stopped)!.completedBytes).toBe(0);
    expect(named(patches, "a2.bin").state.kind).toBe("running");
  });

  it("removes items and drops the package they emptied", async () => {
    const { bridge, patches } = await attached(seed);
    await bridge.removeItems([idOf(patches, "g1.bin")], false);
    expect(packages(patches).map((p) => p.name)).toEqual(["Alpha", "Beta"]);
  });

  it("removes a whole package", async () => {
    const { bridge, patches } = await attached(seed);
    await bridge.removePackage(pkgOf(patches, "a1.bin").id, false);
    expect(packages(patches).map((p) => p.name)).toEqual(["Beta", "Gamma"]);
  });

  it("answers an empty id list with no patch", async () => {
    const { bridge, patches } = await attached(seed);
    const before = patches.length;
    await bridge.startItems([]);
    await bridge.removeItems([], true);
    expect(patches).toHaveLength(before);
  });
});

describe("MockBridge organizing", () => {
  it("moves items into a package at an index and drops the emptied one", async () => {
    const { bridge, patches } = await attached(seed);
    const alpha = pkgOf(patches, "a1.bin").id;
    await bridge.moveItems([idOf(patches, "g1.bin")], alpha, 1);
    expect(pkgOf(patches, "a1.bin").items.map((i) => i.filename)).toEqual([
      "a1.bin",
      "g1.bin",
      "a2.bin",
    ]);
    expect(packages(patches).map((p) => p.name)).toEqual(["Alpha", "Beta"]);
  });

  it("appends when no index is given", async () => {
    const { bridge, patches } = await attached(seed);
    await bridge.moveItems(
      [idOf(patches, "g1.bin")],
      pkgOf(patches, "a1.bin").id,
    );
    expect(pkgOf(patches, "a1.bin").items.at(-1)!.filename).toBe("g1.bin");
  });

  it("refuses to move a running item without an error", async () => {
    const { bridge, patches } = await attached(seed);
    const id = idOf(patches, "g1.bin");
    await bridge.startItems([id]);
    await bridge.moveItems([id], pkgOf(patches, "a1.bin").id);
    expect(pkgOf(patches, "g1.bin").name).toBe("Gamma");
  });

  it("moves items to a new trimmed package and rejects a blank name", async () => {
    const { bridge, patches } = await attached(seed);
    const id = idOf(patches, "a1.bin");
    for (const blank of ["", "   "]) {
      const error = await bridge
        .moveItemsToNewPackage([id], blank)
        .catch((e) => e);
      expect((error as BridgeError).error.kind).toBe("invalidArgument");
    }
    await bridge.moveItemsToNewPackage([id], "  Fresh ");
    expect(pkgOf(patches, "a1.bin").name).toBe("Fresh");
  });

  it("reorders items and packages", async () => {
    const { bridge, patches } = await attached(seed);
    const alpha = pkgOf(patches, "a1.bin");
    await bridge.reorderItems([idOf(patches, "a2.bin")], alpha.id);
    expect(pkgOf(patches, "a1.bin").items.map((i) => i.filename)).toEqual([
      "a2.bin",
      "a1.bin",
    ]);
    const ids = packages(patches)
      .map((p) => p.id)
      .reverse();
    await bridge.reorderPackages(ids);
    expect(packages(patches).map((p) => p.name)).toEqual([
      "Gamma",
      "Beta",
      "Alpha",
    ]);
  });
});

describe("MockBridge deletion, open and details", () => {
  it("describes the files a deletion would trash", async () => {
    const { bridge, patches } = await attached(seed);
    const info = await bridge.describeDeletion({
      kind: "package",
      id: pkgOf(patches, "a1.bin").id,
    });
    expect(info.title).toBe("Remove From List And Delete Files?");
    expect(info.names).toEqual(["a1.bin.incomplete"]);
    expect(info.totalBytes).toBe(0.4 * 100 * MB);
  });

  it("uses the plain title when nothing is on disk", async () => {
    const { bridge, patches } = await attached(seed);
    const info = await bridge.describeDeletion({
      kind: "items",
      ids: [idOf(patches, "a2.bin")],
    });
    expect(info).toEqual({
      title: "Remove From List?",
      totalBytes: 0,
      names: [],
    });
  });

  it("answers an unknown package and a bad id", async () => {
    const { bridge } = await attached(seed);
    const info = await bridge.describeDeletion({
      kind: "package",
      id: "00000000-0000-4000-8000-0000000000ff",
    });
    expect(info.title).toBe("Delete Package From Disk?");
    const error = await bridge
      .describeDeletion({ kind: "items", ids: ["nope"] })
      .catch((e) => e);
    expect((error as BridgeError).error.kind).toBe("invalidArgument");
  });

  it("opens only finished files and records reveals", async () => {
    const { bridge, patches } = await attached(seed);
    await bridge.openItem(idOf(patches, "a1.bin"));
    await bridge.openItem(idOf(patches, "b1.bin"));
    await bridge.revealItem(idOf(patches, "a1.bin"));
    await bridge.revealPackage(pkgOf(patches, "g1.bin").id);
    expect(bridge.opened).toEqual([idOf(patches, "b1.bin")]);
    expect(bridge.revealed).toEqual([
      idOf(patches, "a1.bin"),
      pkgOf(patches, "g1.bin").id,
    ]);
  });

  it("sends the full history only for items with an open panel", async () => {
    const { bridge, patches } = await attached(seed);
    const id = idOf(patches, "a1.bin");
    await bridge.startItems([id]);
    await bridge.setDetailItems("panel", [id]);
    vi.advanceTimersByTime(1000);
    expect(patches.some((p) => p.histories.some((h) => h.id === id))).toBe(
      true,
    );
    await bridge.setDetailItems("panel", []);
    const before = patches.length;
    vi.advanceTimersByTime(1000);
    expect(patches.slice(before).every((p) => p.histories.length === 0)).toBe(
      true,
    );
  });

  it("serves the default and the deep purple theme", async () => {
    const { bridge } = await attached(seed);
    expect(await bridge.themeCss()).toContain("--accent: #0A84FF");
    expect(await bridge.themeCss("deep-purple")).toContain("--accent: #B084F5");
    const error = await bridge.themeCss("nope").catch((e) => e);
    expect((error as BridgeError).error.kind).toBe("invalidArgument");
  });
});

describe("MockBridge vibrancy", () => {
  it("has no window effect unless asked", async () => {
    expect(await new MockBridge({ seed: [] }).vibrancyActive()).toBe(false);
    expect(
      await new MockBridge({ seed: [], vibrancy: true }).vibrancyActive(),
    ).toBe(true);
  });
});

describe("the settings in the mock", () => {
  it("starts from the SwiftUI defaults with a download folder", async () => {
    const bridge = new MockBridge();
    const settings = await bridge.getSettings();
    expect(settings.maxConcurrent).toBe(3);
    expect(settings.themeId).toBe("system");
    expect(settings.downloadFolder).toBe("/Users/mock/Downloads");
  });

  it("takes a valid update and keeps the folder when none is sent", async () => {
    const bridge = new MockBridge();
    await bridge.updateSettings(full({ maxConcurrent: 7 }));
    const settings = await bridge.getSettings();
    expect(settings.maxConcurrent).toBe(7);
    expect(settings.downloadFolder).toBe("/Users/mock/Downloads");
  });

  it("rejects an out-of-range value and changes nothing", async () => {
    const bridge = new MockBridge();
    const before = await bridge.getSettings();
    await expect(
      bridge.updateSettings(full({ maxConcurrent: 99 })),
    ).rejects.toThrow(BridgeError);
    await expect(
      bridge.updateSettings(full({ allowOpus: false, allowAac: false })),
    ).rejects.toThrow(BridgeError);
    expect(await bridge.getSettings()).toEqual(before);
  });

  it("lists its themes and previews one to every listener", async () => {
    const bridge = new MockBridge();
    const ids = (await bridge.listThemes()).map((t) => t.id);
    expect(ids).toContain("light");
    expect(ids).toContain("dark");
    const seen: (string | null)[] = [];
    const stop = bridge.onThemePreview((id) => seen.push(id));
    await bridge.broadcastTheme("deep-purple");
    await bridge.broadcastTheme(null);
    stop();
    await bridge.broadcastTheme("dark");
    expect(seen).toEqual(["deep-purple", null]);
  });

  it("counts opens and checks, and reports the tools it was given", async () => {
    const bridge = new MockBridge();
    await bridge.openSettings();
    await bridge.checkBinariesNow();
    expect(bridge.settingsOpens).toBe(1);
    expect(bridge.binaryChecks).toBe(1);
  });
});
