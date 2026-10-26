import { describe, expect, it } from "vitest";
import type { GrabberRow, GrabberView } from "../bindings";
import { EMPTY_VIEW } from "./grabberRows";
import { packageMenu, rowMenu } from "./grabberMenu";

const link = (id: string, verdict: "online" | "offline"): GrabberRow => ({
  kind: "link",
  id,
  filename: id,
  finalUrl: `https://e.com/${id}`,
  stage: "done",
  verdict: { kind: verdict },
  sizeBytes: null,
  isDuplicate: false,
  removable: true,
});

const view: GrabberView = {
  ...EMPTY_VIEW,
  rows: [link("a", "online"), link("b", "offline")],
  packages: [
    { id: "p1", name: "One", note: null, rowIds: ["a"] },
    { id: "p2", name: "Two", note: null, rowIds: ["b"] },
  ],
};

const find = (entries: ReturnType<typeof rowMenu>, id: string) =>
  entries.find((e) => e.kind === "item" && e.id === id);

describe("rowMenu", () => {
  it("is empty with nothing selected", () => {
    expect(rowMenu(view, [])).toEqual([]);
  });

  it("has the SwiftUI entries in order", () => {
    const labels = rowMenu(view, ["a"]).map((e) =>
      e.kind === "divider" ? "-" : e.label,
    );
    expect(labels).toEqual([
      "Recheck",
      "Copy URL",
      "-",
      "Add to Downloads",
      "Add and Start",
      "-",
      "Remove",
    ]);
  });

  it("copies a URL for exactly one row", () => {
    expect(find(rowMenu(view, ["a"]), "copy")).toMatchObject({
      disabled: false,
    });
    expect(find(rowMenu(view, ["a", "b"]), "copy")).toMatchObject({
      disabled: true,
    });
  });

  it("disables adding when nothing selected is addable", () => {
    expect(find(rowMenu(view, ["a"]), "add")).toMatchObject({
      disabled: false,
    });
    expect(find(rowMenu(view, ["a", "b"]), "addStart")).toMatchObject({
      disabled: false,
    });
    expect(find(rowMenu(view, ["b"]), "add")).toMatchObject({ disabled: true });
    expect(find(rowMenu(view, ["b"]), "addStart")).toMatchObject({
      disabled: true,
    });
  });

  it("marks Remove destructive", () => {
    expect(find(rowMenu(view, ["a"]), "remove")).toMatchObject({
      destructive: true,
    });
  });
});

describe("packageMenu", () => {
  it("lists the other packages under Merge into", () => {
    const entries = packageMenu(view, view.packages[0]);
    const merge = entries.find((e) => e.kind === "submenu");
    expect(merge).toMatchObject({ label: "Merge into" });
    expect(merge!.kind === "submenu" && merge!.entries).toEqual([
      { kind: "item", id: "merge:p2", label: "Two" },
    ]);
  });

  it("offers a disabled placeholder when there is no other package", () => {
    const alone = { ...view, packages: [view.packages[0]] };
    const merge = packageMenu(alone, view.packages[0]).find(
      (e) => e.kind === "submenu",
    )!;
    expect(merge.kind === "submenu" && merge.entries).toEqual([
      {
        kind: "item",
        id: "none",
        label: "No Other Packages",
        disabled: true,
      },
    ]);
  });

  it("has rename and split", () => {
    const ids = packageMenu(view, view.packages[0])
      .filter((e) => e.kind === "item")
      .map((e) => e.kind === "item" && e.id);
    expect(ids).toEqual(["rename", "split"]);
  });
});
