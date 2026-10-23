import { describe, expect, it } from "vitest";
import {
  runningAverage,
  sparklinePoints,
  unionRanges,
  windowed,
} from "./graphs";

describe("windowed", () => {
  it("keeps the newest samples when there are too many", () => {
    expect(windowed([1, 2, 3, 4, 5], 3)).toEqual([3, 4, 5]);
  });

  it("pads the left with zeros so the graph does not stretch while filling", () => {
    expect(windowed([7, 8], 5)).toEqual([0, 0, 0, 7, 8]);
  });

  it("is empty for no width", () => {
    expect(windowed([1, 2], 0)).toEqual([]);
    expect(windowed([1, 2], -3)).toEqual([]);
    expect(windowed([1, 2], NaN)).toEqual([]);
  });

  it("rounds a fractional width", () => {
    expect(windowed([1, 2, 3, 4], 2.6)).toEqual([2, 3, 4]);
  });
});

describe("sparklinePoints", () => {
  it("is empty when there is nothing to draw", () => {
    expect(sparklinePoints([], 10, 10)).toBe("");
    expect(sparklinePoints([5], 1, 10)).toBe("");
    expect(sparklinePoints([0, 0, 0], 3, 10)).toBe("");
  });

  it("scales y to the series' own maximum, top is the maximum", () => {
    // Width 3, three samples: x = 0, 5, 10 across width 10... step = width / (n - 1).
    expect(sparklinePoints([0, 5, 10], 3, 20)).toBe("0,20 1.5,10 3,0");
  });

  it("left-pads a short series with zeros", () => {
    expect(sparklinePoints([10], 3, 10)).toBe("0,10 1.5,10 3,0");
  });

  it("ignores negative and non-finite samples", () => {
    expect(sparklinePoints([NaN, -4, 10], 3, 10)).toBe("0,10 1.5,10 3,0");
  });
});

describe("runningAverage", () => {
  it("is zero until a full window has been seen, then averages the window", () => {
    expect(runningAverage([1, 2, 3, 4], 2)).toEqual([0, 1.5, 2.5, 3.5]);
  });

  it("is empty for no values", () => {
    expect(runningAverage([], 10)).toEqual([]);
  });

  it("stays zero when the series is shorter than the window", () => {
    expect(runningAverage([5, 5], 10)).toEqual([0, 0]);
  });
});

describe("unionRanges", () => {
  it("offsets each part and merges overlapping and touching ranges", () => {
    const merged = unionRanges([
      {
        ranges: [
          { start: 0, end: 10 },
          { start: 20, end: 30 },
        ],
        offset: 0,
      },
      {
        ranges: [
          { start: 0, end: 5 },
          { start: 8, end: 12 },
        ],
        offset: 100,
      },
      { ranges: [{ start: 0, end: 10 }], offset: 10 },
    ]);
    expect(merged).toEqual([
      { start: 0, end: 30 },
      { start: 100, end: 105 },
      { start: 108, end: 112 },
    ]);
  });

  it("is empty for no parts and skips malformed ranges", () => {
    expect(unionRanges([])).toEqual([]);
    expect(
      unionRanges([
        {
          ranges: [
            { start: 5, end: 5 },
            { start: NaN, end: 3 },
          ],
          offset: 0,
        },
      ]),
    ).toEqual([]);
  });
});
