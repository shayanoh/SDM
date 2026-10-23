import { describe, expect, it } from "vitest";
import { fullPatchOf, patch, structurePackages } from "../test/fixtures";
import { DownloadStore } from "./store.svelte";
import { UiState } from "./ui.svelte";
import { wireSelectionPruning } from "./wiring";

describe("UiState", () => {
  it("starts on Downloads with the panel open at its default height", () => {
    const ui = new UiState();
    expect(ui.tab).toBe("downloads");
    expect(ui.panelVisible).toBe(true);
    expect(ui.panelHeight).toBe(150);
    expect(ui.modal).toBeNull();
    expect(ui.menu).toBeNull();
  });

  it("changing the tab clears the selection and closes the menu", () => {
    const ui = new UiState();
    ui.selection.setOrder(["a", "b"]);
    ui.selection.click("a");
    ui.menu = { x: 1, y: 2, targetId: "a" };
    ui.setTab("completed");
    expect(ui.tab).toBe("completed");
    expect(ui.selected.size).toBe(0);
    expect(ui.menu).toBeNull();
  });

  it("choosing the same tab again keeps the selection", () => {
    const ui = new UiState();
    ui.selection.setOrder(["a"]);
    ui.selection.click("a");
    ui.setTab("downloads");
    expect(ui.selected.has("a")).toBe(true);
  });

  it("clamps the panel height to 100 to 600 and ignores nonsense", () => {
    const ui = new UiState();
    ui.setPanelHeight(40);
    expect(ui.panelHeight).toBe(100);
    ui.setPanelHeight(900);
    expect(ui.panelHeight).toBe(600);
    ui.setPanelHeight(333);
    ui.setPanelHeight(NaN);
    expect(ui.panelHeight).toBe(333);
  });

  it("remembers collapsed packages per tab", () => {
    const ui = new UiState();
    ui.toggleCollapsed("downloads", "p1");
    expect(ui.isCollapsed("downloads", "p1")).toBe(true);
    expect(ui.isCollapsed("completed", "p1")).toBe(false);
    ui.toggleCollapsed("downloads", "p1");
    expect(ui.isCollapsed("downloads", "p1")).toBe(false);
  });

  it("selection changes are visible through `selected`", () => {
    const ui = new UiState();
    ui.selection.setOrder(["a", "b"]);
    ui.selection.click("b");
    expect([...ui.selected]).toEqual(["b"]);
  });
});

describe("wireSelectionPruning", () => {
  it("drops a selected item once a structure patch no longer has it", () => {
    const store = new DownloadStore(() => {});
    const ui = new UiState();
    wireSelectionPruning(store, ui);
    store.apply(fullPatchOf(1, [{ id: "p", items: [{ id: "a" }, { id: "b" }] }]));
    ui.selection.setOrder(["p", "a", "b"]);
    ui.selection.click("a");
    ui.selection.toggle("b");

    store.apply(
      patch(2, {
        structure: { packages: structurePackages([{ id: "p", items: [{ id: "b" }] }]) },
      }),
    );

    expect([...ui.selected]).toEqual(["b"]);
  });

  it("drops a selected package that was removed", () => {
    const store = new DownloadStore(() => {});
    const ui = new UiState();
    wireSelectionPruning(store, ui);
    store.apply(
      fullPatchOf(1, [
        { id: "p1", items: [{ id: "a" }] },
        { id: "p2", items: [{ id: "b" }] },
      ]),
    );
    ui.selection.setOrder(["p1", "a", "p2", "b"]);
    ui.selection.click("p2");
    store.apply(
      patch(2, { structure: { packages: structurePackages([{ id: "p1", items: [{ id: "a" }] }]) } }),
    );
    expect(ui.selected.size).toBe(0);
  });
});
