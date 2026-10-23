import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Patch } from "../bindings";
import { BridgeError } from "./bridge";
import { MockBridge } from "./mockBridge";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

async function attached(bridge = new MockBridge()) {
  const patches: Patch[] = [];
  await bridge.attach((patch) => patches.push(patch));
  return { bridge, patches };
}

describe("MockBridge", () => {
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
    const { bridge } = await attached();
    for (const bad of ["nope", "", "ftp://x/y"]) {
      const error = await bridge.addUrl(bad).catch((e) => e);
      expect(error).toBeInstanceOf(BridgeError);
      expect((error as BridgeError).error.kind).toBe("invalidArgument");
    }
  });

  it("names a new item after the last url segment", async () => {
    const { bridge, patches } = await attached();
    await bridge.addUrl("https://example.com/files/new.bin");
    const structure = patches.at(-1)?.structure;
    const names = structure?.packages.flatMap((p) =>
      p.items.map((i) => i.filename),
    );
    expect(names).toContain("new.bin");
  });

  it("completes a running item eventually", async () => {
    const { bridge, patches } = await attached();
    await bridge.addUrl("https://example.com/quick.bin");
    vi.advanceTimersByTime(10 * 60 * 1000);
    const structure = [...patches]
      .reverse()
      .find((p) => p.structure)!.structure!;
    const item = structure.packages
      .flatMap((p) => p.items)
      .find((i) => i.filename === "quick.bin")!;
    expect(item.state.kind).toBe("completed");
  });
});
