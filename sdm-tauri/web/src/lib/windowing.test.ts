import { describe, expect, it } from "vitest";
import { visibleRange } from "./windowing";

describe("visibleRange", () => {
  it("covers the viewport plus overscan", () => {
    expect(
      visibleRange({
        scrollTop: 0,
        viewport: 100,
        rowHeight: 20,
        count: 1000,
        overscan: 2,
      }),
    ).toEqual({ start: 0, end: 7 });
    expect(
      visibleRange({
        scrollTop: 400,
        viewport: 100,
        rowHeight: 20,
        count: 1000,
        overscan: 2,
      }),
    ).toEqual({ start: 18, end: 27 });
  });
  it("clamps to the list", () => {
    expect(
      visibleRange({
        scrollTop: 9999,
        viewport: 100,
        rowHeight: 20,
        count: 10,
        overscan: 2,
      }),
    ).toEqual({ start: 8, end: 10 });
  });
  it("handles empty lists and bad input", () => {
    expect(
      visibleRange({
        scrollTop: 0,
        viewport: 100,
        rowHeight: 20,
        count: 0,
        overscan: 2,
      }),
    ).toEqual({ start: 0, end: 0 });
    expect(
      visibleRange({
        scrollTop: -50,
        viewport: 100,
        rowHeight: 20,
        count: 5,
        overscan: 0,
      }),
    ).toEqual({ start: 0, end: 5 });
    expect(
      visibleRange({
        scrollTop: 0,
        viewport: 100,
        rowHeight: 0,
        count: 5,
        overscan: 0,
      }),
    ).toEqual({ start: 0, end: 0 });
    expect(
      visibleRange({
        scrollTop: NaN,
        viewport: 100,
        rowHeight: 20,
        count: 5,
        overscan: 0,
      }),
    ).toEqual({ start: 0, end: 5 });
  });
});
