import { describe, expect, it } from "vitest";
import { fullPatchOf, type PackageSpec } from "../test/fixtures";
import { makeApp, type App } from "../test/render";
import { runMenu, type ActionContext } from "./actions";
import type { MenuId } from "./menu";

const done = { kind: "completed" as const };

const specs: PackageSpec[] = [
  {
    id: "p",
    name: "Pack",
    items: [
      {
        id: "run",
        state: { kind: "running" },
        over: { filename: "file10.bin", url: "https://e/10" },
      },
      {
        id: "stop",
        state: { kind: "stopped" },
        over: { filename: "file2.bin", url: "https://e/2" },
      },
      {
        id: "off",
        state: { kind: "stopped" },
        over: { isEnabled: false, filename: "File1.bin" },
      },
      {
        id: "fail",
        state: { kind: "failed", reason: "r" },
        over: { filename: "fail.bin" },
      },
      { id: "fin", state: done, over: { filename: "fin.bin" } },
      {
        id: "nr",
        state: { kind: "running" },
        over: { isResumable: false, filename: "nr.bin" },
      },
    ],
  },
];

function ctx(ids: string[], app: App, extra: Partial<ActionContext> = {}) {
  const written: string[] = [];
  const context: ActionContext = {
    ids,
    items: app.store.items,
    bridge: app.bridge,
    ui: app.ui,
    clipboard: { writeText: async (text: string) => void written.push(text) },
    ...extra,
  };
  return { context, written };
}

const setup = () => makeApp([fullPatchOf(1, specs)]);
const call = async (
  id: MenuId,
  ids: string[],
  app: App,
  extra: Partial<ActionContext> = {},
) => {
  const { context, written } = ctx(ids, app, extra);
  await runMenu(id, context);
  return { calls: app.bridge.calls, written };
};

describe("runMenu sends only what applies", () => {
  it("start: only stopped, enabled items", async () => {
    const app = setup();
    const { calls } = await call("start", ["run", "stop", "off", "fin"], app);
    expect(calls).toEqual([["startItems", ["stop"]]]);
  });

  it("with nothing eligible no bridge call is made", async () => {
    const app = setup();
    for (const id of ["start", "enable", "retryMux", "open"] as MenuId[]) {
      await call(id, id === "enable" ? ["run"] : ["run"], app);
    }
    // "enable" for an enabled running item, "start" for a running one, ... nothing eligible.
    expect(app.bridge.calls).toEqual([]);
  });

  it("with an empty selection nothing happens at all", async () => {
    const app = setup();
    for (const id of [
      "start",
      "pause",
      "enable",
      "disable",
      "retry",
      "retryMux",
      "reset",
      "moveToNew",
      "removeFromList",
      "removeAndDelete",
      "copy",
      "open",
      "show",
    ] as MenuId[]) {
      await call(id, [], app);
    }
    expect(app.bridge.calls).toEqual([]);
    expect(app.ui.modal).toBeNull();
  });

  it("enable and disable", async () => {
    const app = setup();
    await call("enable", ["off", "run"], app);
    await call("disable", ["off", "run", "fin"], app);
    expect(app.bridge.calls).toEqual([
      ["setItemsEnabled", ["off"], true],
      ["setItemsEnabled", ["run"], false],
    ]);
  });

  it("retry, retry mux and reset", async () => {
    const app = setup();
    await call("retry", ["fail", "run"], app);
    await call("reset", ["run", "stop", "fin"], app);
    expect(app.bridge.calls).toEqual([
      ["retryItems", ["fail"]],
      ["resetItems", ["stop", "fin"]],
    ]);
    // none of the fixture's failed items is a mux item
    await call("retryMux", ["fail"], app);
    expect(app.bridge.calls).toHaveLength(2);
  });

  it("open sends each finished file; show reveals the first item", async () => {
    const app = setup();
    await call("open", ["fin", "stop"], app);
    await call("show", ["stop", "fin"], app);
    expect(app.bridge.calls).toEqual([
      ["openItem", "fin"],
      ["revealItem", "stop"],
    ]);
  });

  it("copy joins the urls with newlines and does not call the bridge", async () => {
    const app = setup();
    const { written, calls } = await call("copy", ["run", "stop"], app);
    expect(written).toEqual(["https://e/10\nhttps://e/2"]);
    expect(calls).toEqual([]);
  });
});

describe("pause", () => {
  it("stops the running and queued items directly when none is non-resumable", async () => {
    const app = setup();
    const { calls } = await call("pause", ["run", "stop"], app);
    expect(calls).toEqual([["stopItems", ["run"]]]);
    expect(app.ui.modal).toBeNull();
  });

  it("opens the confirmation for a non-resumable running item and calls nothing yet", async () => {
    const app = setup();
    const { calls } = await call("pause", ["run", "nr"], app);
    expect(calls).toEqual([]);
    expect(app.ui.modal).toEqual({ kind: "pause", ids: ["run", "nr"] });
  });
});

describe("modals and removal", () => {
  it("move to new package opens the name prompt for the selected items", async () => {
    const app = setup();
    await call("moveToNew", ["stop", "fin"], app);
    expect(app.ui.modal).toEqual({ kind: "newPackage", ids: ["stop", "fin"] });
    expect(app.bridge.calls).toEqual([]);
  });

  it("remove from list removes without deleting and clears the selection", async () => {
    const app = setup();
    app.ui.selection.setOrder(["stop", "fin"]);
    app.ui.selection.click("stop");
    await call("removeFromList", ["stop", "fin"], app);
    expect(app.bridge.calls).toEqual([["removeItems", ["stop", "fin"], false]]);
    expect(app.ui.selected.size).toBe(0);
  });

  it("remove and delete asks what would be trashed, then opens the confirmation", async () => {
    const app = setup();
    app.bridge.answers.describeDeletion = {
      title: "Remove From List And Delete Files?",
      totalBytes: 3,
      names: ["a"],
    };
    await call("removeAndDelete", ["fin"], app);
    expect(app.bridge.calls).toEqual([
      ["describeDeletion", { kind: "items", ids: ["fin"] }],
    ]);
    expect(app.ui.modal).toEqual({
      kind: "delete",
      target: { kind: "items", ids: ["fin"] },
      info: {
        title: "Remove From List And Delete Files?",
        totalBytes: 3,
        names: ["a"],
      },
    });
  });

  it("info toggles the details panel", async () => {
    const app = setup();
    await call("info", [], app);
    expect(app.ui.panelVisible).toBe(false);
    await call("info", [], app);
    expect(app.ui.panelVisible).toBe(true);
  });

  it("reports a bridge failure instead of throwing", async () => {
    const app = setup();
    app.bridge.failures.startItems = new Error("engine said no");
    await call("start", ["stop"], app);
    expect(app.ui.error).toBe("engine said no");
  });
});
