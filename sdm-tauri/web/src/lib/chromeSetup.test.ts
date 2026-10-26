import { describe, expect, it } from "vitest";
import type { ChromeExtensionDto } from "../bindings";
import { cookiesAge, statusLine } from "./chromeSetup";

const dto = (over: Partial<ChromeExtensionDto> = {}): ChromeExtensionDto => ({
  state: "notConnected",
  installedVersion: null,
  bundledVersion: "2.0.0",
  cookiesSyncedSecondsAgo: null,
  extensionFolder: "/data/SDM/ChromeExtension",
  chromeInstalled: true,
  ...over,
});

describe("cookiesAge", () => {
  it.each([
    [null, "No cookies synced yet"],
    [0, "Cookies synced just now"],
    [59, "Cookies synced just now"],
    [60, "Cookies synced 1 min ago"],
    [180, "Cookies synced 3 min ago"],
    [3600, "Cookies synced 1 h ago"],
    [7300, "Cookies synced 2 h ago"],
    [172800, "Cookies synced 2 d ago"],
  ])("%s -> %s", (seconds, expected) => {
    expect(cookiesAge(seconds)).toBe(expected);
  });
});

describe("statusLine", () => {
  it("says connected with the version and the cookie age", () => {
    expect(
      statusLine(
        dto({
          state: "connected",
          installedVersion: "2.0.0",
          cookiesSyncedSecondsAgo: 180,
        }),
      ),
    ).toBe("Connected (extension 2.0.0). Cookies synced 3 min ago.");
  });

  it("tells a connected extension that has never synced to reload", () => {
    expect(
      statusLine(dto({ state: "connected", installedVersion: "2.0.0" })),
    ).toBe(
      "Connected (extension 2.0.0), but no cookies have arrived. Press Reload on SDM at chrome://extensions; if that does not help, remove the extension and load it again.",
    );
  });

  it("asks for a reload when an older version reports", () => {
    expect(
      statusLine(dto({ state: "outdated", installedVersion: "1.0.1" })),
    ).toBe(
      "Update available: open chrome://extensions and press Reload on SDM.",
    );
  });

  it("says not connected, and notices a missing Chrome first", () => {
    expect(statusLine(dto())).toBe("Extension not connected.");
    expect(statusLine(dto({ chromeInstalled: false }))).toBe(
      "Google Chrome was not found on this computer.",
    );
  });
});
