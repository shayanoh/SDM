import { describe, expect, it } from "vitest";
import type { GrabberRow, GrabberView } from "../bindings";
import { flatLines, selectableIds } from "./grabberList";
import { EMPTY_VIEW } from "./grabberRows";
import { GRABBER_HEADER_HEIGHT, GRABBER_ROW_HEIGHT } from "./layout";

const link = (id: string): GrabberRow => ({
  kind: "link",
  id,
  filename: id,
  finalUrl: `https://e.com/${id}`,
  stage: "done",
  verdict: { kind: "online" },
  sizeBytes: null,
  isDuplicate: false,
  removable: true,
});

const view: GrabberView = {
  ...EMPTY_VIEW,
  rows: [link("a"), link("b"), link("loose")],
  packages: [
    { id: "p", name: "P", note: null, rowIds: ["a", "b"] },
    { id: "q", name: "Q", note: null, rowIds: [] },
  ],
};

describe("flatLines", () => {
  it("lists loose rows, then each package header with its rows", () => {
    expect(flatLines(view).map((l) => l.key)).toEqual([
      "loose",
      "package:p",
      "a",
      "b",
      "package:q",
    ]);
  });

  it("gives headers and rows their two heights", () => {
    expect(flatLines(view).map((l) => l.height)).toEqual([
      GRABBER_ROW_HEIGHT,
      GRABBER_HEADER_HEIGHT,
      GRABBER_ROW_HEIGHT,
      GRABBER_ROW_HEIGHT,
      GRABBER_HEADER_HEIGHT,
    ]);
  });

  it("makes only rows selectable", () => {
    expect(selectableIds(flatLines(view))).toEqual(["loose", "a", "b"]);
  });

  it("is empty for an empty view", () => {
    expect(flatLines(EMPTY_VIEW)).toEqual([]);
  });
});
