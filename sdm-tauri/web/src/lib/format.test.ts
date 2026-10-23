import { describe, expect, it } from "vitest";
import { formatBytes, formatEta, formatSpeed } from "./format";

describe("formatBytes", () => {
  it("scales to the largest fitting unit", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(1024)).toBe("1 KB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5 MB");
  });
  it("shows a dash for negative, NaN and infinite input", () => {
    expect(formatBytes(-1)).toBe("—");
    expect(formatBytes(NaN)).toBe("—");
    expect(formatBytes(Infinity)).toBe("—");
  });
});

describe("formatSpeed", () => {
  it("adds a per-second suffix and dashes out zero", () => {
    expect(formatSpeed(1.5 * 1024 * 1024)).toBe("1.5 MB/s");
    expect(formatSpeed(0)).toBe("—");
    expect(formatSpeed(NaN)).toBe("—");
  });
});

describe("formatEta", () => {
  it("is empty when unknown", () => {
    expect(formatEta(0)).toBe("");
    expect(formatEta(NaN)).toBe("");
    expect(formatEta(-5)).toBe("");
  });
  it("shows the two most significant units", () => {
    expect(formatEta(45)).toBe("45s");
    expect(formatEta(75)).toBe("1m 15s");
    expect(formatEta(3700)).toBe("1h 1m");
  });
});
