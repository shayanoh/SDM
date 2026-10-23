import { describe, expect, it } from "vitest";
import type { ItemStateDto } from "../bindings";
import {
  allDownloadableStopped,
  canDisable,
  canEnable,
  canOpen,
  canReset,
  canRetry,
  canRetryMux,
  canStart,
  canStop,
  downloadable,
  needsPauseConfirmation,
  type RowItem,
} from "./eligibility";

const STATES: ItemStateDto[] = [
  { kind: "stopped" },
  { kind: "queued" },
  { kind: "running" },
  { kind: "completed" },
  { kind: "failed", reason: "boom" },
];

const item = (over: Partial<RowItem> = {}): RowItem => ({
  state: { kind: "stopped" },
  isEnabled: true,
  isResumable: true,
  fileMissing: false,
  assembly: "none",
  ...over,
});

/** The predicate's answer for every state, enabled and disabled. */
function table(predicate: (item: RowItem) => boolean) {
  return Object.fromEntries(
    STATES.flatMap((state) =>
      [true, false].map((isEnabled) => [
        `${state.kind}${isEnabled ? "" : " (disabled)"}`,
        predicate(item({ state, isEnabled })),
      ]),
    ),
  );
}

describe("eligibility, ported from PackagesListView.swift", () => {
  it("start: only a stopped, enabled item", () => {
    expect(table(canStart)).toEqual({
      stopped: true,
      "stopped (disabled)": false,
      queued: false,
      "queued (disabled)": false,
      running: false,
      "running (disabled)": false,
      completed: false,
      "completed (disabled)": false,
      failed: false,
      "failed (disabled)": false,
    });
  });

  it("stop: running or queued, enabled or not", () => {
    const answers = table(canStop);
    expect(Object.keys(answers).filter((key) => answers[key])).toEqual([
      "queued",
      "queued (disabled)",
      "running",
      "running (disabled)",
    ]);
  });

  it("enable: exactly the disabled items", () => {
    expect(canEnable(item({ isEnabled: false }))).toBe(true);
    expect(canEnable(item({ isEnabled: true }))).toBe(false);
  });

  it("disable: an enabled item that is not completed", () => {
    const answers = table(canDisable);
    expect(Object.keys(answers).filter((key) => answers[key])).toEqual([
      "stopped",
      "queued",
      "running",
      "failed",
    ]);
  });

  it("retry: only a failed item; retry mux: only a failed mux item", () => {
    for (const state of STATES) {
      const failed = state.kind === "failed";
      expect(canRetry(item({ state }))).toBe(failed);
      expect(canRetryMux(item({ state, assembly: "mux" }))).toBe(failed);
      expect(canRetryMux(item({ state, assembly: "none" }))).toBe(false);
    }
  });

  it("open: a completed item whose file is there", () => {
    const done: ItemStateDto = { kind: "completed" };
    expect(canOpen(item({ state: done }))).toBe(true);
    expect(canOpen(item({ state: done, fileMissing: true }))).toBe(false);
    expect(canOpen(item())).toBe(false);
  });

  it("reset: anything not running", () => {
    for (const state of STATES)
      expect(canReset(item({ state }))).toBe(state.kind !== "running");
  });
});

describe("pause and the global button", () => {
  it("asks only when a running item is known not to resume", () => {
    const running: ItemStateDto = { kind: "running" };
    expect(
      needsPauseConfirmation([item({ state: running, isResumable: false })]),
    ).toBe(true);
    expect(
      needsPauseConfirmation([item({ state: running, isResumable: null })]),
    ).toBe(false);
    expect(
      needsPauseConfirmation([item({ state: running, isResumable: true })]),
    ).toBe(false);
    expect(
      needsPauseConfirmation([
        item({ state: { kind: "queued" }, isResumable: false }),
      ]),
    ).toBe(false);
    expect(needsPauseConfirmation([])).toBe(false);
  });

  it("counts queued, running and stopped items as downloadable", () => {
    const items = STATES.map((state) => item({ state }));
    expect(downloadable(items).map((i) => i.state.kind)).toEqual([
      "stopped",
      "queued",
      "running",
    ]);
  });

  it("is all-stopped only when every downloadable item is stopped", () => {
    expect(
      allDownloadableStopped([item(), item({ state: { kind: "completed" } })]),
    ).toBe(true);
    expect(
      allDownloadableStopped([item(), item({ state: { kind: "running" } })]),
    ).toBe(false);
    expect(allDownloadableStopped([])).toBe(true);
  });
});
