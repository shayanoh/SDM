import { fireEvent, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import {
  fullPatchOf,
  patch,
  structurePackages,
  type PackageSpec,
} from "../test/fixtures";
import { makeApp, renderIn } from "../test/render";
import ContextMenu from "./ContextMenu.svelte";
import Toolbar from "./Toolbar.svelte";

const specs: PackageSpec[] = [
  {
    id: "p",
    name: "Pack",
    items: [
      { id: "stop", state: { kind: "stopped" } },
      { id: "fin", state: { kind: "completed" } },
    ],
  },
];

const setup = () => {
  const app = makeApp([fullPatchOf(1, specs)]);
  return { app, view: { packages: app.store.order } };
};

describe("Toolbar", () => {
  it("has one labelled button per entry, Info Panel last", () => {
    const { app, view } = setup();
    renderIn(Toolbar, app, { view });
    const buttons = screen.getAllByRole("button");
    expect(buttons).toHaveLength(14);
    expect(buttons.at(-1)!.getAttribute("aria-label")).toBe("Info Panel");
  });

  it("greys out what the selection cannot do and acts on the selection", async () => {
    const { app, view } = setup();
    renderIn(Toolbar, app, { view });
    const start = screen.getByRole("button", {
      name: /^Start Download/,
    }) as HTMLButtonElement;
    expect(start.disabled).toBe(true);

    app.ui.selection.setOrder(["p", "stop", "fin"]);
    app.ui.selection.click("stop");
    await Promise.resolve();
    await fireEvent.click(
      await screen.findByRole("button", { name: "Start Download" }),
    );
    expect(app.bridge.calls).toEqual([["startItems", ["stop"]]]);
  });

  it("toggles the details panel", async () => {
    const { app, view } = setup();
    renderIn(Toolbar, app, { view });
    await fireEvent.click(screen.getByRole("button", { name: "Info Panel" }));
    expect(app.ui.panelVisible).toBe(false);
  });

  it("a selected package stands for its items", async () => {
    const { app, view } = setup();
    renderIn(Toolbar, app, { view });
    app.ui.selection.setOrder(["p", "stop", "fin"]);
    app.ui.selection.click("p");
    await fireEvent.click(
      await screen.findByRole("button", { name: "Start Downloads" }),
    );
    expect(app.bridge.calls).toEqual([["startItems", ["stop"]]]);
  });
});

describe("ContextMenu", () => {
  it("shows nothing until opened", () => {
    const { app, view } = setup();
    renderIn(ContextMenu, app, { view });
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("lists the item menu and runs an entry, then closes", async () => {
    const { app, view } = setup();
    renderIn(ContextMenu, app, { view });
    app.ui.selection.setOrder(["p", "stop", "fin"]);
    app.ui.selection.click("stop");
    app.ui.menu = { x: 10, y: 10, targetId: "stop" };
    await fireEvent.click(
      await screen.findByRole("menuitem", { name: /Start Download/ }),
    );
    expect(app.bridge.calls).toEqual([["startItems", ["stop"]]]);
    expect(app.ui.menu).toBeNull();
  });

  it("does not run a disabled entry", async () => {
    const { app, view } = setup();
    renderIn(ContextMenu, app, { view });
    app.ui.selection.setOrder(["p", "stop", "fin"]);
    app.ui.selection.click("stop");
    app.ui.menu = { x: 10, y: 10, targetId: "stop" };
    const pause = await screen.findByRole("menuitem", {
      name: /Pause Download/,
    });
    await fireEvent.click(pause);
    expect(app.bridge.calls).toEqual([]);
  });

  it("opens the package menu on a header", async () => {
    const { app, view } = setup();
    renderIn(ContextMenu, app, { view });
    app.ui.menu = { x: 5, y: 5, targetId: "p" };
    expect(
      await screen.findByRole("menuitem", { name: "Sort Items by Name" }),
    ).toBeTruthy();
    expect(
      screen.queryByRole("menuitem", { name: /Start Download/ }),
    ).toBeNull();
  });

  it("closes on Escape and when its row disappears", async () => {
    const { app, view } = setup();
    renderIn(ContextMenu, app, { view });
    app.ui.menu = { x: 5, y: 5, targetId: "fin" };
    await screen.findByRole("menu");
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(app.ui.menu).toBeNull();

    app.ui.menu = { x: 5, y: 5, targetId: "fin" };
    await screen.findByRole("menu");
    app.store.apply(
      patch(2, {
        structure: {
          packages: structurePackages([
            { id: "p", name: "Pack", items: [{ id: "stop" }] },
          ]),
        },
      }),
    );
    await Promise.resolve();
    await Promise.resolve();
    expect(app.ui.menu).toBeNull();
  });
});
