import { describe, expect, it } from "vitest";
import { CHROME_ALLOWANCE, MIN_FIT_HEIGHT, cappedHeight } from "./fitting";

describe("cappedHeight", () => {
  it("leaves a height that fits alone", () => {
    expect(cappedHeight(300, 800)).toBe(300);
  });
  it("keeps room for the titlebar and a margin on a short screen", () => {
    expect(cappedHeight(776, 800)).toBe(800 - CHROME_ALLOWANCE);
  });
  it("never goes below a usable minimum", () => {
    expect(cappedHeight(776, 200)).toBe(MIN_FIT_HEIGHT);
  });
  it("takes a screen of unknown height as no limit", () => {
    expect(cappedHeight(776, null)).toBe(776);
    expect(cappedHeight(776, Number.NaN)).toBe(776);
  });
});
