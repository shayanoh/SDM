import { describe, expect, it } from "vitest";
import type { ItemStateDto } from "../bindings";
import {
  statusText,
  summarize,
  type DetailItem,
  type StatusInput,
} from "./details";

const item = (over: Partial<DetailItem> = {}): DetailItem => ({
  state: { kind: "running" },
  completed: [],
  completedBytes: 0,
  totalBytes: 100,
  bytesPerSecond: 0,
  activeSegments: 0,
  configuredSegments: 0,
  speedHistory: [],
  detailHistory: null,
  isRetrying: false,
  ...over,
});

describe("summarize (multipleItemData)", () => {
  it("sums sizes, speed and segments and averages the fractions", () => {
    const s = summarize([
      item({
        completedBytes: 50,
        totalBytes: 100,
        bytesPerSecond: 10,
        activeSegments: 2,
        configuredSegments: 8,
      }),
      item({
        completedBytes: 30,
        totalBytes: 60,
        bytesPerSecond: 5,
        activeSegments: 1,
        configuredSegments: 4,
      }),
    ]);
    expect(s.downloadedBytes).toBe(80);
    expect(s.totalBytes).toBe(160);
    expect(s.bytesPerSecond).toBe(15);
    expect(s.activeSegments).toBe(3);
    expect(s.configuredSegments).toBe(12);
    expect(s.averageFraction).toBeCloseTo(0.5);
  });

  it("offsets each item's ranges by the sizes before it and merges the union", () => {
    const s = summarize([
      item({ totalBytes: 100, completed: [{ start: 0, end: 100 }] }),
      item({
        totalBytes: 50,
        completed: [
          { start: 0, end: 20 },
          { start: 30, end: 40 },
        ],
      }),
    ]);
    expect(s.completed).toEqual([
      { start: 0, end: 120 },
      { start: 130, end: 140 },
    ]);
  });

  it("skips an item whose size is zero or unknown, but still counts it in the average", () => {
    const s = summarize([
      item({ completedBytes: 50, totalBytes: 100 }),
      item({ totalBytes: null }),
      item({ totalBytes: 0 }),
    ]);
    expect(s.totalBytes).toBe(100);
    expect(s.downloadedBytes).toBe(50);
    expect(s.averageFraction).toBeCloseTo(0.5 / 3);
  });

  it("takes the first history as is and adds only running items' histories, aligned at the end", () => {
    const s = summarize([
      item({ state: { kind: "stopped" }, speedHistory: [1, 2, 3] }),
      item({ speedHistory: [10, 10] }),
      item({ state: { kind: "stopped" }, speedHistory: [100, 100, 100] }),
    ]);
    expect(s.speedHistory).toEqual([1, 12, 13]);
  });

  it("prefers the full history the panel asked for", () => {
    const s = summarize([
      item({ speedHistory: [1], detailHistory: [4, 5, 6] }),
    ]);
    expect(s.speedHistory).toEqual([4, 5, 6]);
  });

  it("pads the first history when a later running one is longer", () => {
    const s = summarize([
      item({ speedHistory: [1, 1] }),
      item({ speedHistory: [5, 5, 5] }),
    ]);
    expect(s.speedHistory).toEqual([5, 6, 6]);
  });

  it("counts states, splitting retrying out of queued", () => {
    const states: [ItemStateDto, boolean][] = [
      [{ kind: "completed" }, false],
      [{ kind: "running" }, false],
      [{ kind: "running" }, false],
      [{ kind: "queued" }, true],
      [{ kind: "queued" }, false],
      [{ kind: "stopped" }, false],
      [{ kind: "failed", reason: "r" }, false],
    ];
    const s = summarize(
      states.map(([state, isRetrying]) => item({ state, isRetrying })),
    );
    expect(s.states).toEqual({
      completed: 1,
      running: 2,
      retrying: 1,
      queued: 1,
      stopped: 1,
      failed: 1,
    });
  });

  it("is all zeros for no items", () => {
    const s = summarize([]);
    expect(s.totalBytes).toBe(0);
    expect(s.averageFraction).toBe(0);
    expect(s.speedHistory).toEqual([]);
  });
});

const status = (over: Partial<StatusInput> = {}): StatusInput => ({
  state: { kind: "running" },
  isEnabled: true,
  isAssembling: false,
  isRetrying: false,
  fileMissing: false,
  failedAttemptCount: null,
  remainingAttempts: null,
  lastFailureReason: null,
  retryHoldSeconds: null,
  ...over,
});

describe("statusText (statusDescription)", () => {
  it("names each state and tones failures and a missing file", () => {
    expect(statusText(status({ state: { kind: "queued" } }))).toEqual({
      text: "Queued",
      tone: "normal",
    });
    expect(statusText(status({ state: { kind: "stopped" } })).text).toBe(
      "Stopped",
    );
    expect(statusText(status()).text).toBe("Running");
    expect(statusText(status({ state: { kind: "completed" } })).text).toBe(
      "Completed",
    );
    expect(
      statusText(status({ state: { kind: "completed" }, fileMissing: true })),
    ).toEqual({
      text: "Completed — file missing on disk",
      tone: "faulty",
    });
    expect(
      statusText(status({ state: { kind: "failed", reason: "HTTP 404" } })),
    ).toEqual({
      text: "Failed — HTTP 404",
      tone: "failed",
    });
  });

  it("spells out a retry in full", () => {
    expect(
      statusText(
        status({
          state: { kind: "queued" },
          isRetrying: true,
          failedAttemptCount: 2,
          remainingAttempts: 1,
          retryHoldSeconds: 7,
          lastFailureReason: "reset by peer",
        }),
      ),
    ).toEqual({
      text: "Retrying — attempt 2 of 3, retrying in 7s\nLast failure: reset by peer",
      tone: "faulty",
    });
    expect(
      statusText(status({ state: { kind: "queued" }, isRetrying: true })).text,
    ).toBe("Retrying — retrying soon");
  });

  it("says Assembling… and marks a disabled item", () => {
    expect(statusText(status({ isAssembling: true })).text).toBe("Assembling…");
    expect(
      statusText(status({ isAssembling: true, isEnabled: false })).text,
    ).toBe("Assembling… (disabled)");
    expect(
      statusText(status({ state: { kind: "stopped" }, isEnabled: false })).text,
    ).toBe("Stopped (disabled)");
  });
});
