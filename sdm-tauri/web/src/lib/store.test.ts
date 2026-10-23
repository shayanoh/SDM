import { EMPTY_VIEW } from "./grabberRows";
import { describe, expect, it, vi } from "vitest";
import { DownloadStore } from "./store.svelte";
import type { Patch } from "../bindings";

const item = (id: string, over: Partial<any> = {}) => ({
  id,
  url: "https://e.com/f",
  filename: "f.bin",
  state: { kind: "running" as const },
  isEnabled: true,
  isResumable: true,
  isAssembling: false,
  assembly: "none" as const,
  fileMissing: false,
  metadata: null,
  ...over,
});
const figures = (id: string, over: Partial<any> = {}) => ({
  id,
  completed: [],
  completedBytes: 0,
  totalBytes: 100,
  activeSegments: 1,
  configuredSegments: 4,
  bytesPerSecond: 0,
  speedHistory: [],
  retryHoldSeconds: null,
  failedAttemptCount: null,
  remainingAttempts: null,
  lastFailureReason: null,
  checkpointFailure: null,
  ...over,
});
const stats = () => ({
  bytesPerSecond: 0,
  history: [],
  runningCount: 0,
  queuedCount: 0,
  overallFraction: 0,
  ringCompletedBytes: 0,
  ringTotalBytes: 0,
  waitingItemCount: 0,
  waitingPackageCount: 0,
});
const patch = (seq: number, over: Partial<Patch> = {}): Patch => ({
  seq,
  full: false,
  structure: null,
  items: [],
  packages: [],
  stats: null,
  histories: [],
  grabber: null,
  ...over,
});
const fullPatch = (seq: number, ids: string[]): Patch =>
  patch(seq, {
    full: true,
    structure: {
      packages: [
        {
          id: "p1",
          name: "P",
          priority: 2,
          folder: "/dl/P",
          items: ids.map((id) => item(id)),
        },
      ],
    },
    items: ids.map((id) => figures(id)),
    packages: [
      {
        id: "p1",
        completedBytes: 0,
        totalBytes: 100,
        bytesPerSecond: 0,
        speedHistory: [],
        etaSeconds: 0,
      },
    ],
    stats: stats(),
  });

describe("DownloadStore", () => {
  it("builds rows from a full patch", () => {
    const store = new DownloadStore(() => {});
    store.apply(fullPatch(5, ["a", "b"]));
    expect(store.order).toEqual([{ id: "p1", itemIds: ["a", "b"] }]);
    expect(store.items.get("a")?.filename).toBe("f.bin");
    expect(store.items.get("b")?.totalBytes).toBe(100);
  });

  it("applies figures to existing models in place", () => {
    const store = new DownloadStore(() => {});
    store.apply(fullPatch(1, ["a"]));
    const model = store.items.get("a")!;
    store.apply(
      patch(2, {
        items: [figures("a", { completedBytes: 40, bytesPerSecond: 7 })],
      }),
    );
    expect(store.items.get("a")).toBe(model); // same object: rows keep their identity
    expect(model.completedBytes).toBe(40);
    expect(model.bytesPerSecond).toBe(7);
  });

  it("drops items missing from a new structure", () => {
    const store = new DownloadStore(() => {});
    store.apply(fullPatch(1, ["a", "b"]));
    store.apply(
      patch(2, {
        structure: {
          packages: [
            {
              id: "p1",
              name: "P",
              priority: 2,
              folder: "/dl/P",
              items: [item("a")],
            },
          ],
        },
      }),
    );
    expect(store.items.has("b")).toBe(false);
    expect(store.order[0].itemIds).toEqual(["a"]);
  });

  it("asks for a resync on a gap and ignores patches until a full one arrives", () => {
    const resync = vi.fn();
    const store = new DownloadStore(resync);
    store.apply(fullPatch(1, ["a"]));
    store.apply(patch(3, { items: [figures("a", { completedBytes: 99 })] })); // seq 2 missing
    expect(resync).toHaveBeenCalledTimes(1);
    expect(store.items.get("a")?.completedBytes).toBe(0);
    store.apply(patch(4)); // still waiting for the full patch: no second request
    expect(resync).toHaveBeenCalledTimes(1);
    store.apply(fullPatch(9, ["a", "c"]));
    expect(store.items.has("c")).toBe(true);
    store.apply(patch(10, { items: [figures("c", { completedBytes: 5 })] }));
    expect(store.items.get("c")?.completedBytes).toBe(5);
  });

  it("wraps the sequence number at 2^32", () => {
    const resync = vi.fn();
    const store = new DownloadStore(resync);
    store.apply(fullPatch(4294967295, ["a"]));
    store.apply(patch(0, { items: [figures("a", { completedBytes: 1 })] }));
    expect(resync).not.toHaveBeenCalled();
    expect(store.items.get("a")?.completedBytes).toBe(1);
  });

  it("requests a resync when a non-full patch arrives before any full one", () => {
    const resync = vi.fn();
    const store = new DownloadStore(resync);
    store.apply(patch(7));
    expect(resync).toHaveBeenCalledTimes(1);
  });

  it("a full patch clears everything first", () => {
    const store = new DownloadStore(() => {});
    store.apply(fullPatch(1, ["a", "b"]));
    store.apply(fullPatch(2, ["c"]));
    expect([...store.items.keys()]).toEqual(["c"]);
  });

  it("keeps histories only for the ids in a patch", () => {
    const store = new DownloadStore(() => {});
    store.apply(fullPatch(1, ["a"]));
    store.apply(
      patch(2, { histories: [{ id: "a", speedHistory: [1, 2, 3] }] }),
    );
    expect(store.items.get("a")?.detailHistory).toEqual([1, 2, 3]);
  });
  it("reads null figures (a non-finite f64 on the wire) as zero", () => {
    const store = new DownloadStore(() => {});
    store.apply(fullPatch(1, ["a"]));
    store.apply(
      patch(2, {
        items: [
          figures("a", {
            completedBytes: null,
            bytesPerSecond: null,
            speedHistory: [1, null, 3],
            completed: [{ start: null, end: 5 }],
          }),
        ],
        packages: [
          {
            id: "p1",
            completedBytes: null,
            totalBytes: null,
            bytesPerSecond: null,
            speedHistory: [null],
            etaSeconds: null,
          },
        ],
      }),
    );
    const model = store.items.get("a")!;
    expect(model.completedBytes).toBe(0);
    expect(model.bytesPerSecond).toBe(0);
    expect(model.speedHistory).toEqual([1, 0, 3]);
    expect(model.completed).toEqual([{ start: 0, end: 5 }]);
    const pkg = store.packages.get("p1")!;
    expect(pkg.etaSeconds).toBe(0);
    expect(pkg.totalBytes).toBe(0);
  });
});

describe("the link grabber view", () => {
  const view = { ...EMPTY_VIEW, totalCount: 3 };

  it("starts empty and is replaced whole by a patch that carries it", () => {
    const store = new DownloadStore(() => {});
    expect(store.grabber).toEqual(EMPTY_VIEW);
    store.apply(patch(1, { full: true, grabber: view }));
    expect(store.grabber.totalCount).toBe(3);
  });

  it("keeps the last view when a patch carries none", () => {
    const store = new DownloadStore(() => {});
    store.apply(patch(1, { full: true, grabber: view }));
    store.apply(patch(2));
    expect(store.grabber.totalCount).toBe(3);
    store.apply(patch(3, { grabber: { ...view, totalCount: 5 } }));
    expect(store.grabber.totalCount).toBe(5);
  });
});
