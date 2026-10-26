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

describe("SelectionModel keyboard movement", () => {
  it("starts at the first row going down and the last row going up", () => {
    const down = model();
    down.move(1, false);
    expect(keys(down)).toEqual(["p1"]);
    const up = model();
    up.move(-1, false);
    expect(keys(up)).toEqual(["d"]);
  });

  it("moves the single selection one row, stopping at the ends", () => {
    const s = model();
    s.click("a");
    s.move(1, false);
    expect(keys(s)).toEqual(["b"]);
    s.move(-1, false);
    s.move(-1, false);
    s.move(-1, false);
    expect(keys(s)).toEqual(["p1"]);
    s.click("d");
    s.move(1, false);
    expect(keys(s)).toEqual(["d"]);
  });

  it("extends from the anchor with shift", () => {
    const s = model();
    s.click("b");
    s.move(1, true);
    s.move(1, true);
    expect(keys(s)).toEqual(["b", "c", "p2"]);
    s.move(-1, true);
    expect(keys(s)).toEqual(["b", "p2"]);
  });

  it("reports the row it moved to", () => {
    const s = model();
    s.click("a");
    expect(s.move(1, false)).toBe("b");
    const empty = new SelectionModel();
    expect(empty.move(1, false)).toBeNull();
  });
});
