import { describe, expect, it } from "vitest";
import { formatBytes, formatEta, formatSpeed } from "./format";

const KB = 1024;
const MB = KB ** 2;
const GB = KB ** 3;

describe("formatBytes (ByteCountFormatter, binary)", () => {
  it("says Zero KB for nothing and for anything that is not a size", () => {
    for (const bad of [0, -1, NaN, Infinity, -Infinity]) {
      expect(formatBytes(bad)).toBe("Zero KB");
    }
  });

  it("spells out small sizes in bytes", () => {
    expect(formatBytes(1)).toBe("1 byte");
    expect(formatBytes(512)).toBe("512 bytes");
    expect(formatBytes(1023)).toBe("1023 bytes");
  });

  it("uses no decimals for KB, one for MB, two for GB and TB", () => {
    expect(formatBytes(KB)).toBe("1 KB");
    expect(formatBytes(1536)).toBe("2 KB");
    expect(formatBytes(4 * MB)).toBe("4 MB");
    expect(formatBytes(629.4 * MB)).toBe("629.4 MB");
    expect(formatBytes(3.7 * MB)).toBe("3.7 MB");
    expect(formatBytes(1.3 * GB)).toBe("1.3 GB");
    expect(formatBytes(5.59 * GB)).toBe("5.59 GB");
    expect(formatBytes(12.07 * GB)).toBe("12.07 GB");
    expect(formatBytes(2.5 * KB ** 4)).toBe("2.5 TB");
  });

  it("rolls a value that rounds up to 1024 into the next unit", () => {
    expect(formatBytes(1024 * KB - 1)).toBe("1 MB");
  });
});

describe("formatSpeed", () => {
  it("is Zero KB/s when idle and appends /s otherwise", () => {
    expect(formatSpeed(0)).toBe("Zero KB/s");
    expect(formatSpeed(NaN)).toBe("Zero KB/s");
    expect(formatSpeed(3.7 * MB)).toBe("3.7 MB/s");
    expect(formatSpeed(13.3 * MB)).toBe("13.3 MB/s");
  });
});

describe("formatEta (formatTimeIntervalForEta)", () => {
  it("is empty when there is no estimate", () => {
    for (const none of [0, -5, NaN, Infinity]) expect(formatEta(none)).toBe("");
  });

  it("covers seconds", () => {
    expect(formatEta(0.4)).toBe("Soon");
    expect(formatEta(5)).toBe("A few seconds");
    expect(formatEta(30)).toBe("Less than a minute");
  });

  it("covers minutes, with seconds only under ten minutes and over fifteen seconds", () => {
    expect(formatEta(60)).toBe("About 1 Minute");
    expect(formatEta(125)).toBe("About 2 Minutes");
    expect(formatEta(205)).toBe("About 3 Minutes and 25 Seconds");
    expect(formatEta(9 * 60 + 40)).toBe("About 9 Minutes and 40 Seconds");
    expect(formatEta(1200 + 40)).toBe("About 20 Minutes");
  });

  it("covers hours and days", () => {
    expect(formatEta(3600)).toBe("About 1 Hour");
    expect(formatEta(3600 + 60 * 5)).toBe("About 1 Hour");
    expect(formatEta(3600 + 60 * 17)).toBe("About 1 Hour and 15 Minutes");
    expect(formatEta(2 * 3600)).toBe("About 2 Hours");
    expect(formatEta(86400)).toBe("About 1 Day");
    expect(formatEta(86400 + 3 * 3600)).toBe("About 1 Day and 3 Hours");
    expect(formatEta(3 * 86400)).toBe("About 3 Days");
  });
});
