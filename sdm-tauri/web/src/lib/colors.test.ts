import { afterEach, describe, expect, it } from "vitest";
import { resetThemeColors, themeColor } from "./colors";

afterEach(() => {
  resetThemeColors();
  document.documentElement.style.cssText = "";
});

describe("themeColor", () => {
  it("reads a role once and keeps it until reset", () => {
    document.documentElement.style.setProperty("--progress-fill", "#B084F5");
    expect(themeColor(document.documentElement, "--progress-fill")).toBe(
      "#B084F5",
    );
    document.documentElement.style.setProperty("--progress-fill", "#000000");
    expect(themeColor(document.documentElement, "--progress-fill")).toBe(
      "#B084F5",
    );
    resetThemeColors();
    expect(themeColor(document.documentElement, "--progress-fill")).toBe(
      "#000000",
    );
  });

  it("does not remember a role that has no value yet", () => {
    expect(themeColor(document.documentElement, "--graph-stroke")).toBeNull();
    document.documentElement.style.setProperty("--graph-stroke", "#123456");
    expect(themeColor(document.documentElement, "--graph-stroke")).toBe(
      "#123456",
    );
  });
});
