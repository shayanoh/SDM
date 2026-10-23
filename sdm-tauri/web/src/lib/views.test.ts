import { describe, expect, it } from "vitest";
import { fullPatchOf, type PackageSpec } from "../test/fixtures";
import { HEADER_HEIGHT, ITEM_HEIGHT } from "./layout";
import { DownloadStore } from "./store.svelte";
import {
  completedAggregate,
  completedView,
  flatRows,
  itemOrder,
  selectedItemIds,
} from "./views";

const done = { kind: "completed" as const };

const specs: PackageSpec[] = [
  {
    id: "p1",
    items: [
      { id: "a", state: done, fig: { completedBytes: 100, totalBytes: 100 } },
      { id: "b", state: { kind: "running" }, fig: { completedBytes: 40, totalBytes: 100 } },
    ],
  },
  { id: "p2", items: [{ id: "c", state: { kind: "queued" } }] },
  {
    id: "p3",
    items: [
      { id: "d", state: done, fig: { completedBytes: 50, totalBytes: 50 } },
      { id: "e", state: done, fig: { completedBytes: 70, totalBytes: 70 } },
    ],
  },
];

function storeOf() {
  const store = new DownloadStore(() => {});
  store.apply(fullPatchOf(1, specs));
  return store;
}

describe("completedView", () => {
  it("keeps only completed items and drops packages left with none", () => {
    const store = storeOf();
    expect(completedView(store.order, store.items)).toEqual({
      packages: [
        { id: "p1", itemIds: ["a"] },
        { id: "p3", itemIds: ["d", "e"] },
      ],
    });
  });

  it("is empty when nothing has completed", () => {
    const store = new DownloadStore(() => {});
    store.apply(fullPatchOf(1, [{ id: "p", items: [{ id: "x", state: { kind: "failed", reason: "r" } }] }]));
    expect(completedView(store.order, store.items).packages).toEqual([]);
  });
});

describe("completedAggregate", () => {
  it("sums only the items it is given", () => {
    const store = storeOf();
    expect(completedAggregate(["d", "e"], store.items)).toEqual({
      completedBytes: 120,
      totalBytes: 120,
    });
    expect(completedAggregate(["a"], store.items)).toEqual({
      completedBytes: 100,
      totalBytes: 100,
    });
    expect(completedAggregate([], store.items)).toEqual({ completedBytes: 0, totalBytes: 0 });
  });

  it("counts an item with an unknown size as zero bytes", () => {
    const store = new DownloadStore(() => {});
    store.apply(
      fullPatchOf(1, [
        { id: "p", items: [{ id: "x", state: done, fig: { totalBytes: null, completedBytes: 5 } }] },
      ]),
    );
    expect(completedAggregate(["x"], store.items)).toEqual({ completedBytes: 5, totalBytes: 0 });
  });
});

describe("flatRows", () => {
  it("lists each header then its items, with the two heights and zebra indices", () => {
    const store = storeOf();
    const rows = flatRows({ packages: store.order }, new Set());
    expect(rows.map((r) => `${r.kind}:${r.id}`)).toEqual([
      "package:p1",
      "item:a",
      "item:b",
      "package:p2",
      "item:c",
      "package:p3",
      "item:d",
      "item:e",
    ]);
    expect(rows[0]).toMatchObject({ height: HEADER_HEIGHT, index: 0, packageId: "p1" });
    expect(rows[2]).toMatchObject({ height: ITEM_HEIGHT, index: 1, packageId: "p1" });
    expect(rows[5]).toMatchObject({ kind: "package", index: 2 });
  });

  it("leaves out the items of a collapsed package", () => {
    const store = storeOf();
    const rows = flatRows({ packages: store.order }, new Set(["p1"]));
    expect(rows.map((r) => r.id)).toEqual(["p1", "p2", "c", "p3", "d", "e"]);
  });
});

describe("selectedItemIds", () => {
  it("expands a package into its items, in list order, without repeats", () => {
    const store = storeOf();
    const view = { packages: store.order };
    expect(selectedItemIds(new Set(["p3", "b", "d"]), view)).toEqual(["b", "d", "e"]);
    expect(selectedItemIds(new Set(["ghost"]), view)).toEqual([]);
  });

  it("lists every item in list order", () => {
    const store = storeOf();
    expect(itemOrder({ packages: store.order })).toEqual(["a", "b", "c", "d", "e"]);
  });
});
