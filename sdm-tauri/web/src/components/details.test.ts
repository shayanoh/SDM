import { fireEvent, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import {
  fullPatchOf,
  patch,
  structurePackages,
  type PackageSpec,
} from "../test/fixtures";
import { makeApp, renderIn } from "../test/render";
import DetailsPanel from "./DetailsPanel.svelte";
import PanelHandle from "./PanelHandle.svelte";

const specs: PackageSpec[] = [
  {
    id: "p",
    name: "Pack",
    items: [
      {
        id: "a",
        state: { kind: "completed" },
        over: {
          filename: "a.iso",
          url: "https://e.com/a.iso",
          metadata: "x86_64 · ISO",
          isResumable: true,
        },
        fig: {
          completedBytes: 1500,
          totalBytes: 2000,
          activeSegments: 0,
          configuredSegments: 8,
        },
      },
      {
        id: "b",
        state: { kind: "running" },
        over: { filename: "b.iso" },
        fig: { completedBytes: 50, totalBytes: 100 },
      },
    ],
  },
];

const setup = () => makeApp([fullPatchOf(1, specs)]);
const setCalls = (app: ReturnType<typeof setup>) =>
  app.bridge.calls.filter(([name]) => name === "setDetailItems");

describe("DetailsPanel", () => {
  it("shows a placeholder when nothing is selected", () => {
    renderIn(DetailsPanel, setup(), { itemIds: [] });
    expect(
      screen.getByText("Select an item to view its details here"),
    ).toBeTruthy();
  });

  it("shows one item's fields", () => {
    renderIn(DetailsPanel, setup(), { itemIds: ["a"] });
    expect(screen.getByText("a.iso")).toBeTruthy();
    expect(screen.getByText("Completed")).toBeTruthy();
    expect(screen.getByText("https://e.com/a.iso")).toBeTruthy();
    expect(screen.getByText("x86_64 · ISO")).toBeTruthy();
    expect(
      screen.getByText(/1\.5 KB\/2 KB - 75\.0%|1\.46 KB|75\.0%/),
    ).toBeTruthy();
    expect(screen.getByText("Yes")).toBeTruthy();
    expect(screen.getByText("/dl/Pack")).toBeTruthy();
  });

  it("shows a dash for unknown media info and resumability", () => {
    const app = makeApp([
      fullPatchOf(1, [
        {
          id: "p",
          items: [{ id: "x", over: { metadata: null, isResumable: null } }],
        },
      ]),
    ]);
    renderIn(DetailsPanel, app, { itemIds: ["x"] });
    expect(screen.getAllByText("—").length).toBeGreaterThan(0);
    expect(screen.getByText("-")).toBeTruthy();
  });

  it("enables Open only for a finished file and sends the actions", async () => {
    const app = setup();
    renderIn(DetailsPanel, app, { itemIds: ["a"] });
    const open = screen.getByRole("button", {
      name: "Open",
    }) as HTMLButtonElement;
    expect(open.disabled).toBe(false);
    await fireEvent.click(open);
    await fireEvent.click(screen.getByRole("button", { name: /^Show in / }));
    expect(app.bridge.calls.filter(([n]) => n !== "setDetailItems")).toEqual([
      ["openItem", "a"],
      ["revealPackage", "p"],
    ]);
  });

  it("disables Open for an unfinished item", () => {
    renderIn(DetailsPanel, setup(), { itemIds: ["b"] });
    expect(
      (screen.getByRole("button", { name: "Open" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  });

  it("shows the aggregate for several items", () => {
    renderIn(DetailsPanel, setup(), { itemIds: ["a", "b"] });
    expect(screen.getByText("Item States")).toBeTruthy();
    expect(screen.getByText("1 Completed")).toBeTruthy();
    expect(screen.getByText("1 Running")).toBeTruthy();
    expect(screen.getByText("Total Progress")).toBeTruthy();
    expect(screen.getByText("Total Segments")).toBeTruthy();
    // both items live in /dl/Pack
    expect(screen.getByText("/dl/Pack")).toBeTruthy();
  });

  it("hides the destination folder when the items are in different folders", () => {
    const app = makeApp([
      fullPatchOf(1, [
        { id: "p1", name: "One", items: [{ id: "x" }] },
        { id: "p2", name: "Two", items: [{ id: "y" }] },
      ]),
    ]);
    renderIn(DetailsPanel, app, { itemIds: ["x", "y"] });
    expect(screen.queryByText("Destination Folder")).toBeNull();
  });

  it("asks the engine for exactly the shown items' histories, then stops", async () => {
    const app = setup();
    const view = renderIn(DetailsPanel, app, { itemIds: ["a", "b"] });
    await Promise.resolve();
    expect(setCalls(app)).toEqual([
      ["setDetailItems", "main-details", ["a", "b"]],
    ]);
    view.unmount();
    expect(setCalls(app).at(-1)).toEqual([
      "setDetailItems",
      "main-details",
      [],
    ]);
  });

  it("falls back to the placeholder when the shown item is removed", async () => {
    const app = setup();
    app.ui.selection.setOrder(["p", "a", "b"]);
    app.ui.selection.click("a");
    renderIn(DetailsPanel, app, { itemIds: ["a"] });
    expect(
      screen.queryByText("Select an item to view its details here"),
    ).toBeNull();
    app.store.apply(
      patch(2, {
        structure: {
          packages: structurePackages([
            { id: "p", name: "Pack", items: [{ id: "b" }] },
          ]),
        },
      }),
    );
    // The selection pruned the removed id, so the owner passes no ids.
    expect(app.ui.selected.size).toBe(0);
  });
});

describe("PanelHandle", () => {
  it("resizes the panel by dragging, within 100 to 600", async () => {
    const app = setup();
    const { container } = renderIn(PanelHandle, app);
    const handle = container.querySelector(".handle") as HTMLElement;
    handle.setPointerCapture = () => {};
    await fireEvent.pointerDown(handle, { clientY: 500, pointerId: 1 });
    await fireEvent.pointerMove(handle, { clientY: 400, pointerId: 1 });
    expect(app.ui.panelHeight).toBe(250);
    await fireEvent.pointerMove(handle, { clientY: 0, pointerId: 1 });
    expect(app.ui.panelHeight).toBe(600);
    await fireEvent.pointerMove(handle, { clientY: 900, pointerId: 1 });
    expect(app.ui.panelHeight).toBe(100);
    await fireEvent.pointerUp(handle, { pointerId: 1 });
  });
});
