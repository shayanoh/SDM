import { describe, expect, it, vi } from "vitest";
import { SelectionModel } from "./selection";

const ORDER = ["p1", "a", "b", "p2", "c", "d"];

function model() {
  const selection = new SelectionModel();
  selection.setOrder(ORDER);
  return selection;
}

const keys = (selection: SelectionModel) => [...selection.selected].sort();

describe("SelectionModel", () => {
  it("click replaces the selection", () => {
    const s = model();
    s.click("a");
    s.click("c");
    expect(keys(s)).toEqual(["c"]);
  });

  it("toggle adds and removes one key", () => {
    const s = model();
    s.click("a");
    s.toggle("c");
    expect(keys(s)).toEqual(["a", "c"]);
    s.toggle("a");
    expect(keys(s)).toEqual(["c"]);
  });

  it("range selects from the anchor in either direction", () => {
    const s = model();
    s.click("b");
    s.range("d");
    expect(keys(s)).toEqual(["b", "c", "d", "p2"]);
    s.range("a");
    expect(keys(s)).toEqual(["a", "b"]);
  });

  it("range with no anchor behaves like click", () => {
    const s = model();
    s.range("c");
    expect(keys(s)).toEqual(["c"]);
  });

  it("selectAll takes exactly the keys it is given", () => {
    const s = model();
    s.click("p1");
    s.selectAll(["a", "b", "c", "d"]);
    expect(keys(s)).toEqual(["a", "b", "c", "d"]);
  });

  it("prune drops keys that no longer exist, and a dead anchor", () => {
    const s = model();
    s.click("a");
    s.toggle("c");
    s.prune(new Set(["c", "d"]));
    expect(keys(s)).toEqual(["c"]);
    s.range("d");
    expect(keys(s)).toEqual(["c", "d"]);
  });

  it("contextTarget keeps a selection that contains the row, in list order", () => {
    const s = model();
    s.click("c");
    s.toggle("a");
    expect(s.contextTarget("c")).toEqual(["a", "c"]);
    expect(keys(s)).toEqual(["a", "c"]);
  });

  it("contextTarget on an unselected row selects only that row", () => {
    const s = model();
    s.click("a");
    expect(s.contextTarget("d")).toEqual(["d"]);
    expect(keys(s)).toEqual(["d"]);
  });

  it("clear empties the selection and reports changes only when something changed", () => {
    const onChange = vi.fn();
    const s = new SelectionModel(onChange);
    s.setOrder(ORDER);
    s.clear();
    expect(onChange).not.toHaveBeenCalled();
    s.click("a");
    s.click("a");
    expect(onChange).toHaveBeenCalledTimes(1);
    s.clear();
    expect(onChange).toHaveBeenCalledTimes(2);
    expect(keys(s)).toEqual([]);
  });
});
