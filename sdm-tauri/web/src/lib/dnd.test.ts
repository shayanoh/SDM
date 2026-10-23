import { describe, expect, it } from "vitest";
import {
  dropIndex,
  orderedItemIds,
  orderedPackageIds,
  reorderedPackageIds,
  type DragPayload,
} from "./dnd";

const items = ["i1", "i2", "i3", "i4"];
const packages = ["a", "b", "c"];

const item = (id: string) => ({ kind: "item" as const, id });
const pkg = (id: string) => ({ kind: "package" as const, id });

describe("drag payload ordering", () => {
  it("returns item ids in list order, not drag order", () => {
    const payload: DragPayload = [item("i3"), item("i1")];
    expect(orderedItemIds(payload, items)).toEqual(["i1", "i3"]);
  });

  it("refuses a payload that mixes items and packages", () => {
    const mixed: DragPayload = [item("i1"), pkg("a")];
    expect(orderedItemIds(mixed, items)).toBeNull();
    expect(orderedPackageIds(mixed, packages)).toBeNull();
  });

  it("refuses an empty payload and one naming nothing that exists", () => {
    expect(orderedItemIds([], items)).toBeNull();
    expect(orderedItemIds([item("ghost")], items)).toBeNull();
    expect(orderedPackageIds([], packages)).toBeNull();
    expect(orderedPackageIds([pkg("ghost")], packages)).toBeNull();
  });

  it("returns package ids in list order", () => {
    expect(orderedPackageIds([pkg("c"), pkg("a")], packages)).toEqual([
      "a",
      "c",
    ]);
  });
});

describe("drop positions", () => {
  it("inserts before the row on its top half and after it on the bottom half", () => {
    expect(dropIndex(3, 10, 40)).toBe(3);
    expect(dropIndex(3, 20, 40)).toBe(3);
    expect(dropIndex(3, 30, 40)).toBe(4);
  });

  it("moves packages before or after the target", () => {
    expect(reorderedPackageIds(packages, ["c"], "a", false)).toEqual([
      "c",
      "a",
      "b",
    ]);
    expect(reorderedPackageIds(packages, ["c"], "a", true)).toEqual([
      "a",
      "c",
      "b",
    ]);
    expect(reorderedPackageIds(packages, ["a", "b"], "c", true)).toEqual([
      "c",
      "a",
      "b",
    ]);
  });

  it("refuses to drop a package onto itself or an unknown target", () => {
    expect(reorderedPackageIds(packages, ["a", "b"], "b", false)).toBeNull();
    expect(reorderedPackageIds(packages, ["a"], "ghost", false)).toBeNull();
  });
});
