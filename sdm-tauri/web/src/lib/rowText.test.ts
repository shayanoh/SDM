import { describe, expect, it } from "vitest";
import type { ItemStateDto } from "../bindings";
import {
  averageRecentSpeed,
  describeItem,
  firstLine,
  itemEta,
  statusTone,
  type DescribeInput,
} from "./rowText";

const input = (over: Partial<DescribeInput> = {}): DescribeInput => ({
  state: { kind: "running" },
  isEnabled: true,
  isAssembling: false,
  isRetrying: false,
  remainingAttempts: null,
  lastFailureReason: null,
  ...over,
});

const st = (kind: ItemStateDto["kind"], reason = "boom"): ItemStateDto =>
  kind === "failed" ? { kind, reason } : ({ kind } as ItemStateDto);

describe("describeItem (ItemRowBody.describe)", () => {
  it("names each state", () => {
    expect(describeItem(input({ state: st("queued") }), null)).toBe("Queued");
    expect(describeItem(input({ state: st("stopped") }), null)).toBe("Stopped");
    expect(describeItem(input({ state: st("running") }), null)).toBe("Running");
    expect(describeItem(input({ state: st("completed") }), null)).toBe(
      "Completed",
    );
  });

  it("gives the whole failure reason; the row clamps it to two lines", () => {
    expect(
      describeItem(input({ state: st("failed", "HTTP 404\nNot Found") }), null),
    ).toBe("Failed — HTTP 404\nNot Found");
  });

  it("describes a retry with its hold, attempts left and the first line of why", () => {
    const retrying = input({
      state: st("queued"),
      isRetrying: true,
      remainingAttempts: 2,
      lastFailureReason: "Connection reset\nby peer",
    });
    expect(describeItem(retrying, 5)).toBe(
      "Retrying in 5s · 2 attempts left — Connection reset",
    );
    expect(describeItem({ ...retrying, remainingAttempts: 1 }, 5)).toContain(
      "1 attempt left",
    );
    expect(
      describeItem(
        { ...retrying, remainingAttempts: null, lastFailureReason: null },
        null,
      ),
    ).toBe("Retrying soon");
  });

  it("says Assembling… for an assembling item", () => {
    expect(describeItem(input({ isAssembling: true }), null)).toBe(
      "Assembling…",
    );
  });

  it("marks a disabled item", () => {
    expect(
      describeItem(input({ state: st("stopped"), isEnabled: false }), null),
    ).toBe("Stopped (disabled)");
    expect(
      describeItem(input({ state: st("failed", "x"), isEnabled: false }), null),
    ).toBe("Failed — x (disabled)");
  });
});

describe("firstLine", () => {
  it("is the first non-empty line, or the text itself", () => {
    expect(firstLine("a\nb")).toBe("a");
    expect(firstLine("\n\nb\nc")).toBe("b");
    expect(firstLine("")).toBe("");
    expect(firstLine("single")).toBe("single");
  });
});

describe("statusTone", () => {
  it("is failed for a failed item, faulty while retrying, otherwise secondary", () => {
    expect(statusTone(input({ state: st("failed") }))).toBe("failed");
    expect(statusTone(input({ state: st("queued"), isRetrying: true }))).toBe(
      "faulty",
    );
    expect(statusTone(input({ state: st("running") }))).toBe("secondary");
  });
});

describe("averageRecentSpeed", () => {
  it("falls back when there is no history", () => {
    expect(averageRecentSpeed([], 7)).toBe(7);
  });

  it("averages the trailing fifty samples", () => {
    const history = [...Array(10).fill(1000), ...Array(50).fill(100)];
    expect(averageRecentSpeed(history, 0)).toBe(100);
    expect(averageRecentSpeed([2, 4], 0)).toBe(3);
  });
});

describe("itemEta", () => {
  const base = {
    state: st("running"),
    totalBytes: 1000,
    completedBytes: 400,
    speedHistory: Array(50).fill(100),
    bytesPerSecond: 100,
  };

  it("divides what is left by the recent average speed", () => {
    expect(itemEta(base, 0)).toBe(6);
  });

  it("lets a queued item with no speed of its own borrow the global speed", () => {
    const queued = {
      ...base,
      state: st("queued"),
      speedHistory: [],
      bytesPerSecond: 0,
    };
    expect(itemEta(queued, 200)).toBe(3);
    expect(itemEta({ ...queued, state: st("running") }, 200)).toBe(0);
  });

  it("is zero when it cannot be estimated", () => {
    expect(itemEta({ ...base, totalBytes: null }, 100)).toBe(0);
    expect(itemEta({ ...base, totalBytes: 0 }, 100)).toBe(0);
    expect(itemEta({ ...base, speedHistory: [], bytesPerSecond: 0 }, 100)).toBe(
      0,
    );
  });
});
