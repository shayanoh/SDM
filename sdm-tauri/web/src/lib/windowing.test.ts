import { describe, expect, it } from "vitest";
import { buildOffsets, visibleRange } from "./windowing";

const even = (count: number, height = 20) =>
  buildOffsets(Array.from({ length: count }, () => height));

describe("buildOffsets", () => {
  it("is the running sum of the heights, starting at zero", () => {
    expect(buildOffsets([53, 61, 61])).toEqual([0, 53, 114, 175]);
    expect(buildOffsets([])).toEqual([0]);
  });

  it("counts a bad height as zero", () => {
    expect(buildOffsets([10, NaN, -5, 10])).toEqual([0, 10, 10, 10, 20]);
  });
});

describe("visibleRange", () => {
  it("covers the viewport plus overscan", () => {
    expect(
      visibleRange({
        offsets: even(1000),
        scrollTop: 0,
        viewport: 100,
        overscan: 2,
      }),
    ).toEqual({ start: 0, end: 7 });
    expect(
      visibleRange({
        offsets: even(1000),
        scrollTop: 400,
        viewport: 100,
        overscan: 2,
      }),
    ).toEqual({ start: 18, end: 27 });
  });

  it("follows rows of different heights", () => {
    // Rows span [0,53) [53,114) [114,175): a 100 px viewport at the top
    // shows rows 0 and 1; scrolled to 60 it shows rows 1 and 2.
    const offsets = buildOffsets([53, 61, 61]);
    expect(
      visibleRange({ offsets, scrollTop: 0, viewport: 100, overscan: 0 }),
    ).toEqual({
      start: 0,
      end: 2,
    });
    expect(
      visibleRange({ offsets, scrollTop: 60, viewport: 100, overscan: 0 }),
    ).toEqual({
      start: 1,
      end: 3,
    });
  });

  it("clamps an overscrolled position to the last rows", () => {
    expect(
      visibleRange({
        offsets: even(10),
        scrollTop: 9999,
        viewport: 100,
        overscan: 2,
      }),
    ).toEqual({ start: 8, end: 10 });
  });

  it("handles an empty list and bad input", () => {
    expect(
      visibleRange({ offsets: [0], scrollTop: 0, viewport: 100, overscan: 2 }),
    ).toEqual({
      start: 0,
      end: 0,
    });
    expect(
      visibleRange({ offsets: [], scrollTop: 0, viewport: 100, overscan: 2 }),
    ).toEqual({
      start: 0,
      end: 0,
    });
    for (const bad of [-50, NaN, Infinity]) {
      const range = visibleRange({
        offsets: even(5),
        scrollTop: bad,
        viewport: 100,
        overscan: 0,
      });
      expect(range.start).toBe(0);
    }
    expect(
      visibleRange({
        offsets: even(5),
        scrollTop: 0,
        viewport: NaN,
        overscan: 0,
      }),
    ).toEqual({ start: 0, end: 0 });
  });

  it("pads with overscan and clamps it to the list", () => {
    expect(
      visibleRange({
        offsets: even(5),
        scrollTop: 0,
        viewport: 20,
        overscan: 10,
      }),
    ).toEqual({ start: 0, end: 5 });
  });

  it("finds the window in a 5,000 row list quickly", () => {
    const offsets = even(5000, 61);
    const started = performance.now();
    for (let i = 0; i < 1000; i++)
      visibleRange({ offsets, scrollTop: i * 300, viewport: 700, overscan: 6 });
    expect(performance.now() - started).toBeLessThan(200);
  });
});
