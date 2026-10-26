import { describe, expect, it } from "vitest";
import {
  AUTO_CLEAR_LABELS,
  cookieChoices,
  isLastChecked,
  orderThemes,
  resolutionLabel,
  snapResolution,
} from "./settingsModel";

describe("snapResolution", () => {
  it("keeps a value that is on the ladder", () => {
    expect(snapResolution(1080)).toBe(1080);
  });
  it("snaps down onto the highest rung that fits", () => {
    expect(snapResolution(1000)).toBe(720);
    expect(snapResolution(5000)).toBe(1920);
  });
  it("floors at the lowest rung", () => {
    expect(snapResolution(300)).toBe(480);
  });
});

describe("resolutionLabel", () => {
  it("groups thousands like the SwiftUI picker", () => {
    expect(resolutionLabel(1080)).toBe("1,080p");
    expect(resolutionLabel(720)).toBe("720p");
  });
});

describe("orderThemes", () => {
  it("puts Light and Dark first, then the rest by name", () => {
    const ordered = orderThemes([
      { id: "deep-purple", name: "Deep Purple" },
      { id: "dark", name: "Dark" },
      { id: "catppuccin-latte", name: "Catppuccin Latte" },
      { id: "light", name: "Light" },
    ]);
    expect(ordered.map((t) => t.id)).toEqual([
      "light",
      "dark",
      "catppuccin-latte",
      "deep-purple",
    ]);
  });
});

describe("isLastChecked", () => {
  it("is true only for the single checked box", () => {
    expect(isLastChecked([true, false, false], 0)).toBe(true);
    expect(isLastChecked([true, true, false], 0)).toBe(false);
    expect(isLastChecked([true, false, false], 1)).toBe(false);
  });
});

describe("cookieChoices", () => {
  it("offers no Safari off macOS and starts with None", () => {
    const values = cookieChoices("windows").map((c) => c.value);
    expect(values).toEqual(["none", "chrome", "firefox", "edge", "brave"]);
    expect(cookieChoices("linux").map((c) => c.value)).toEqual(values);
  });
  it("puts Safari second on macOS, as the SwiftUI order does", () => {
    const choices = cookieChoices("mac");
    expect(choices.map((c) => c.value).slice(0, 2)).toEqual(["none", "safari"]);
    expect(choices[0].label).toBe("None");
    expect(choices[1].label).toBe("Safari");
  });
});

describe("AUTO_CLEAR_LABELS", () => {
  it("matches the SwiftUI labels", () => {
    expect(AUTO_CLEAR_LABELS.fifteenMinutes).toBe("After 15 minutes");
    expect(AUTO_CLEAR_LABELS.oneDay).toBe("After 1 day");
    expect(AUTO_CLEAR_LABELS.never).toBe("Never");
  });
});
