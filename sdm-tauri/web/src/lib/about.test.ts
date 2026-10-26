import { describe, expect, it } from "vitest";
import type { BinariesStatusDto, ChromeExtensionDto } from "../bindings";
import { componentRows } from "./about";

const binaries = (
  extra: Partial<BinariesStatusDto> = {},
): BinariesStatusDto => ({
  downloading: false,
  ytDlpVersion: "2026.10.21",
  latestKnown: null,
  ffmpegVersion: "1789931890_9.0.2",
  qjsVersion: "v0.17.0",
  lastError: null,
  ...extra,
});

const chrome = (
  extra: Partial<ChromeExtensionDto> = {},
): ChromeExtensionDto => ({
  state: "connected",
  installedVersion: "2.0.1",
  bundledVersion: "2.0.1",
  cookiesSyncedSecondsAgo: 5,
  extensionFolder: "/x",
  chromeInstalled: true,
  ...extra,
});

describe("componentRows", () => {
  it("lists the three tools and the extension with their versions", () => {
    expect(componentRows(binaries(), chrome())).toEqual([
      { name: "yt-dlp", value: "2026.10.21", missing: false },
      { name: "FFmpeg", value: "1789931890_9.0.2", missing: false },
      { name: "QuickJS", value: "v0.17.0", missing: false },
      { name: "Chrome extension", value: "2.0.1", missing: false },
    ]);
  });

  it("says downloading while a tool is on its way, not installed otherwise", () => {
    const busy = componentRows(
      binaries({ ffmpegVersion: null, qjsVersion: null, downloading: true }),
      chrome(),
    );
    expect(busy.slice(1, 3).map((row) => row.value)).toEqual([
      "downloading…",
      "downloading…",
    ]);
    const idle = componentRows(binaries({ ffmpegVersion: null }), chrome());
    expect(idle[1]).toEqual({
      name: "FFmpeg",
      value: "not installed",
      missing: true,
    });
  });

  it("shows the extension as not connected until it has reported a version", () => {
    const rows = componentRows(binaries(), chrome({ installedVersion: null }));
    expect(rows[3]).toEqual({
      name: "Chrome extension",
      value: "not connected",
      missing: true,
    });
  });

  it("copes with statuses that have not arrived yet", () => {
    const rows = componentRows(null, null);
    expect(rows.every((row) => row.missing)).toBe(true);
    expect(rows).toHaveLength(4);
  });
});
