import { fireEvent } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import { fullPatchOf, type PackageSpec } from "../test/fixtures";
import { makeApp, renderIn } from "../test/render";
import PackageList from "./PackageList.svelte";

const specs: PackageSpec[] = [
  { id: "p1", name: "One", items: [{ id: "a" }, { id: "b" }, { id: "c" }] },
  { id: "p2", name: "Two", items: [{ id: "d" }, { id: "e" }] },
];

function setup(allowsReordering = true) {
  const app = makeApp([fullPatchOf(1, specs)]);
  const view = { packages: app.store.order };
  const rendered = renderIn(PackageList, app, {
    view,
    tab: "downloads" as const,
    allowsReordering,
    showsLiveAggregates: true,
  });
  return { app, ...rendered };
}

/** A stand-in for the browser's drag data store. */
function transfer() {
  const data = new Map<string, string>();
  return {
    data,
    effectAllowed: "uninitialized",
    dropEffect: "none",
    types: [] as string[],
    setData(type: string, value: string) {
      data.set(type, value);
      this.types = [...data.keys()];
    },
    getData: (type: string) => data.get(type) ?? "",
    setDragImage() {},
  };
}

const row = (container: Element, id: string) =>
  container.querySelector(`[data-row-id="${id}"]`) as HTMLElement;

/** Fires a drag event; jsdom has no DragEvent, so a MouseEvent carries the pointer position. */
function fire(
  element: Element,
  type: "dragstart" | "dragover" | "drop" | "dragend",
  dataTransfer: ReturnType<typeof transfer>,
  clientY = 0,
) {
  const event = new MouseEvent(type, {
    bubbles: true,
    cancelable: true,
    clientY,
  });
  Object.defineProperty(event, "dataTransfer", { value: dataTransfer });
  return fireEvent(element, event);
}

async function drag(
  container: Element,
  from: string,
  to: string,
  offsetY: number,
) {
  const dataTransfer = transfer();
  await fire(row(container, from), "dragstart", dataTransfer);
  await fire(row(container, to), "dragover", dataTransfer, offsetY);
  await fire(row(container, to), "drop", dataTransfer, offsetY);
  await fire(row(container, from), "dragend", dataTransfer);
}

const moves = (app: ReturnType<typeof setup>["app"]) =>
  app.bridge.calls.filter(
    ([name]) => name === "moveItems" || name === "reorderPackages",
  );

describe("dragging items", () => {
  it("drops before a row on its top half and after it on its bottom half", async () => {
    const { app, container } = setup();
    await drag(container, "a", "c", 10);
    await drag(container, "a", "c", 50);
    expect(moves(app)).toEqual([
      ["moveItems", ["a"], "p1", 2],
      ["moveItems", ["a"], "p1", 3],
    ]);
  });

  it("moves an item into another package at the row's index", async () => {
    const { app, container } = setup();
    await drag(container, "b", "d", 50);
    expect(moves(app)).toEqual([["moveItems", ["b"], "p2", 1]]);
  });

  it("drags the whole selection when the dragged row is selected, in list order", async () => {
    const { app, container } = setup();
    await fireEvent.click(row(container, "b"));
    await fireEvent.click(row(container, "a"), { ctrlKey: true });
    await drag(container, "b", "d", 10);
    expect(moves(app)).toEqual([["moveItems", ["a", "b"], "p2", 0]]);
  });

  it("drags only the row when it is not part of the selection", async () => {
    const { app, container } = setup();
    await fireEvent.click(row(container, "a"));
    await drag(container, "c", "d", 10);
    expect(moves(app)).toEqual([["moveItems", ["c"], "p2", 0]]);
  });

  it("appends to a package when dropped on its header", async () => {
    const { app, container } = setup();
    await drag(container, "a", "p2", 10);
    expect(moves(app)).toEqual([["moveItems", ["a"], "p2"]]);
  });
});

describe("dragging packages", () => {
  it("moves a package before or after the header it is dropped on", async () => {
    const { app, container } = setup();
    await drag(container, "p2", "p1", 10);
    await drag(container, "p1", "p2", 50);
    expect(moves(app)).toEqual([
      ["reorderPackages", ["p2", "p1"]],
      ["reorderPackages", ["p2", "p1"]],
    ]);
  });

  it("refuses to drop a selection of packages onto one of its own members", async () => {
    const { app, container } = setup();
    await fireEvent.click(row(container, "p1"));
    await fireEvent.click(row(container, "p2"), { ctrlKey: true });
    await drag(container, "p1", "p2", 10);
    expect(moves(app)).toEqual([]);
  });

  it("does not let a package be dropped onto an item row", async () => {
    const { app, container } = setup();
    await drag(container, "p2", "a", 10);
    expect(moves(app)).toEqual([]);
  });
});

describe("refusals", () => {
  it("ignores a payload that mixes items and packages", async () => {
    const { app, container } = setup();
    await fireEvent.click(row(container, "p1"));
    await fireEvent.click(row(container, "d"), { ctrlKey: true });
    await drag(container, "p1", "d", 10);
    await drag(container, "p1", "p2", 10);
    expect(moves(app)).toEqual([]);
  });

  it("is off on the Completed tab: rows are not draggable and drops do nothing", async () => {
    const { app, container } = setup(false);
    expect(row(container, "a").getAttribute("draggable")).toBe("false");
    await drag(container, "a", "d", 10);
    expect(moves(app)).toEqual([]);
  });

  it("ignores a drop that did not start in the list (a file from the desktop)", async () => {
    const { app, container } = setup();
    const dataTransfer = transfer();
    await fireEvent.dragOver(row(container, "a"), {
      dataTransfer,
      clientY: 10,
    });
    await fireEvent.drop(row(container, "a"), { dataTransfer, clientY: 10 });
    expect(moves(app)).toEqual([]);
  });
});

describe("the drop target", () => {
  it("is outlined while a drag is over it and cleared when the drag ends", async () => {
    const { container } = setup();
    const dataTransfer = transfer();
    await fireEvent.dragStart(row(container, "a"), { dataTransfer });
    await fireEvent.dragOver(row(container, "c"), {
      dataTransfer,
      clientY: 10,
    });
    expect(row(container, "c").classList.contains("drop-target")).toBe(true);
    await fireEvent.dragEnd(row(container, "a"), { dataTransfer });
    expect(row(container, "c").classList.contains("drop-target")).toBe(false);
  });

  it("is not outlined for a refused drop", async () => {
    const { container } = setup();
    const dataTransfer = transfer();
    await fireEvent.dragStart(row(container, "p1"), { dataTransfer });
    await fireEvent.dragOver(row(container, "a"), {
      dataTransfer,
      clientY: 10,
    });
    expect(row(container, "a").classList.contains("drop-target")).toBe(false);
  });
});
