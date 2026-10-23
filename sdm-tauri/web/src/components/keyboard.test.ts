import { fireEvent } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import { fullPatchOf, type PackageSpec } from "../test/fixtures";
import { makeApp, renderIn } from "../test/render";
import Shell from "./Shell.svelte";

const done = { kind: "completed" as const };

const specs: PackageSpec[] = [
  {
    id: "p",
    name: "Pack",
    items: [
      { id: "a", state: done },
      { id: "b", state: { kind: "stopped" } },
      { id: "c", state: done },
    ],
  },
];

function setup(over: PackageSpec[] = specs) {
  const app = makeApp([fullPatchOf(1, over)]);
  const view = renderIn(Shell, app);
  return { app, ...view };
}

/** Calls the window made for the user, without the details panel's bookkeeping. */
const calls = (app: ReturnType<typeof setup>["app"]) =>
  app.bridge.calls.filter(([name]) => name !== "setDetailItems");

const press = (init: KeyboardEventInit & { key: string }) =>
  fireEvent.keyDown(window, init);

describe("keyboard shortcuts in the window", () => {
  it("Ctrl+A selects every item of a 5,000 item list, rendered or not", async () => {
    const big: PackageSpec[] = [
      {
        id: "big",
        items: Array.from({ length: 5000 }, (_, n) => ({ id: `i${n}` })),
      },
    ];
    const { app } = setup(big);
    await press({ key: "a", ctrlKey: true });
    expect(app.ui.selected.size).toBe(5000);
    expect(app.ui.selected.has("big")).toBe(false);
  });

  it("Enter opens every selected finished file", async () => {
    const { app } = setup();
    app.ui.selection.setOrder(["p", "a", "b", "c"]);
    app.ui.selection.selectAll(["a", "b", "c"]);
    await press({ key: "Enter" });
    expect(calls(app)).toEqual([
      ["openItem", "a"],
      ["openItem", "c"],
    ]);
  });

  it("Delete removes the selection from the list and clears it", async () => {
    const { app } = setup();
    app.ui.selection.setOrder(["p", "a", "b", "c"]);
    app.ui.selection.selectAll(["a", "b"]);
    await press({ key: "Delete" });
    expect(calls(app)).toEqual([["removeItems", ["a", "b"], false]]);
    expect(app.ui.selected.size).toBe(0);
  });

  it("Ctrl+Delete asks before deleting files", async () => {
    const { app } = setup();
    app.ui.selection.setOrder(["p", "a", "b", "c"]);
    app.ui.selection.selectAll(["a"]);
    await press({ key: "Delete", ctrlKey: true });
    await Promise.resolve();
    await Promise.resolve();
    expect(calls(app).map(([name]) => name)).toEqual(["describeDeletion"]);
    expect(app.ui.modal?.kind).toBe("delete");
  });

  it("Delete and Enter with nothing selected do nothing", async () => {
    const { app } = setup();
    await press({ key: "Delete" });
    await press({ key: "Enter" });
    await press({ key: "Delete", ctrlKey: true });
    expect(calls(app)).toEqual([]);
    expect(app.ui.modal).toBeNull();
  });

  it("is ignored while a modal is open", async () => {
    const { app } = setup();
    app.ui.selection.setOrder(["p", "a", "b", "c"]);
    app.ui.selection.selectAll(["a", "b"]);
    app.ui.modal = { kind: "pause", ids: ["a"] };
    await press({ key: "Delete" });
    await press({ key: "a", ctrlKey: true });
    expect(calls(app)).toEqual([]);
  });

  it("is ignored while a text field has focus", async () => {
    const { app } = setup();
    app.ui.selection.setOrder(["p", "a", "b", "c"]);
    app.ui.selection.selectAll(["a"]);
    const input = document.createElement("input");
    document.body.append(input);
    input.focus();
    await fireEvent.keyDown(input, { key: "Delete" });
    input.remove();
    expect(calls(app)).toEqual([]);
  });

  it("arrow keys move the selection through the rows", async () => {
    const { app } = setup();
    await press({ key: "ArrowDown" });
    expect([...app.ui.selected]).toEqual(["p"]);
    await press({ key: "ArrowDown" });
    expect([...app.ui.selected]).toEqual(["a"]);
    await press({ key: "ArrowDown", shiftKey: true });
    expect([...app.ui.selected].sort()).toEqual(["a", "b"]);
  });
});
