import { describe, expect, it } from "vitest";
import { BridgeError } from "./bridge";
import {
  addToDownloads,
  ingestDropped,
  runPackageMenu,
  runRowMenu,
} from "./grabberActions";
import { makeApp } from "../test/render";

function context() {
  const app = makeApp();
  const written: string[] = [];
  app.bridge.answers.grabberCopyUrl = "https://e.com/a.zip";
  return {
    app,
    written,
    ctx: {
      bridge: app.bridge,
      ui: app.ui,
      clipboard: {
        writeText: async (text: string) => {
          written.push(text);
        },
      },
    },
  };
}

const pkg = { id: "p1", name: "One", note: null, rowIds: [] };

describe("runRowMenu", () => {
  it("rechecks, removes and adds the given rows", async () => {
    const { app, ctx } = context();
    await runRowMenu("recheck", ["a", "b"], ctx);
    await runRowMenu("remove", ["a"], ctx);
    await runRowMenu("add", ["a"], ctx);
    await runRowMenu("addStart", ["a"], ctx);
    expect(app.bridge.calls).toEqual([
      ["grabberRecheck", ["a", "b"]],
      ["grabberRemoveLinks", ["a"]],
      ["grabberAddToDownloads", { kind: "rows", ids: ["a"] }, false],
      ["grabberAddToDownloads", { kind: "rows", ids: ["a"] }, true],
    ]);
  });

  it("copies the address of exactly one row", async () => {
    const { written, ctx, app } = context();
    await runRowMenu("copy", ["a"], ctx);
    expect(written).toEqual(["https://e.com/a.zip"]);
    await runRowMenu("copy", ["a", "b"], ctx);
    expect(written).toHaveLength(1);
    expect(
      app.bridge.calls.filter(([m]) => m === "grabberCopyUrl"),
    ).toHaveLength(1);
  });

  it("shows a failure in the banner instead of throwing", async () => {
    const { app, ctx } = context();
    app.bridge.failures.grabberRecheck = new BridgeError({
      kind: "failed",
      message: "no grabber",
    });
    await runRowMenu("recheck", ["a"], ctx);
    expect(app.ui.error).toBe("no grabber");
  });
});

describe("runPackageMenu", () => {
  it("asks the caller to prompt for a rename", async () => {
    const { app, ctx } = context();
    expect(await runPackageMenu("rename", pkg, ctx)).toBe("rename");
    expect(app.bridge.calls).toEqual([]);
  });

  it("splits and merges", async () => {
    const { app, ctx } = context();
    await runPackageMenu("split", pkg, ctx);
    await runPackageMenu("merge:p2", pkg, ctx);
    expect(app.bridge.calls).toEqual([
      ["grabberSplitPackage", "p1"],
      ["grabberMergePackages", "p1", "p2"],
    ]);
  });
});

describe("the bulk actions", () => {
  it("hands a target to the bridge", async () => {
    const { app, ctx } = context();
    await addToDownloads({ kind: "all" }, true, ctx);
    await addToDownloads({ kind: "package", id: "p1" }, false, ctx);
    expect(app.bridge.calls).toEqual([
      ["grabberAddToDownloads", { kind: "all" }, true],
      ["grabberAddToDownloads", { kind: "package", id: "p1" }, false],
    ]);
  });

  it("ingests dropped text but ignores a blank drop", async () => {
    const { app, ctx } = context();
    await ingestDropped("  \n ", ctx);
    await ingestDropped("https://e.com/a.zip", ctx);
    expect(app.bridge.calls).toEqual([
      ["grabberIngestText", "https://e.com/a.zip"],
    ]);
  });
});
