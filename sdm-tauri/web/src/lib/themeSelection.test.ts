import { describe, expect, it } from "vitest";
import { resolveThemeId } from "./themeSelection";

const known = ["light", "dark", "deep-purple"];

describe("resolveThemeId", () => {
  it("follows the OS for system", () => {
    expect(resolveThemeId("system", true, known)).toBe("dark");
    expect(resolveThemeId("system", false, known)).toBe("light");
  });
  it("uses a known theme as stored", () => {
    expect(resolveThemeId("deep-purple", false, known)).toBe("deep-purple");
  });
  it("falls back to system for an id that no longer exists", () => {
    expect(resolveThemeId("gone", true, known)).toBe("dark");
    expect(resolveThemeId("", false, known)).toBe("light");
  });
});
