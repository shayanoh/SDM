import type { ItemStateDto } from "../bindings";
import { unionRanges, type Range } from "./graphs";

/** What the multi-item summary reads from each selected item. */
export interface DetailItem {
  state: ItemStateDto;
  completed: Range[];
  completedBytes: number;
  totalBytes: number | null;
  bytesPerSecond: number;
  activeSegments: number;
  configuredSegments: number;
  speedHistory: number[];
  /** The full history the panel asked the engine for, once it arrives. */
  detailHistory: number[] | null;
  isRetrying: boolean;
}

export interface StateCounts {
  completed: number;
  running: number;
  retrying: number;
  queued: number;
  stopped: number;
  failed: number;
}

export interface Summary {
  states: StateCounts;
  speedHistory: number[];
  bytesPerSecond: number;
  completed: Range[];
  totalBytes: number;
  downloadedBytes: number;
  averageFraction: number;
  activeSegments: number;
  configuredSegments: number;
}

/** Adds two histories aligned on their newest sample, padding the shorter with zeros on the left. */
function addHistories(a: number[], b: number[]): number[] {
  const length = Math.max(a.length, b.length);
  const padA = length - a.length;
  const padB = length - b.length;
  return Array.from(
    { length },
    (_, i) => (a[i - padA] ?? 0) + (b[i - padB] ?? 0),
  );
}

/**
 * Port of `BottomPanel.multipleItemData`: the totals shown for several selected
 * items. An item with no known size is left out of the sums (but counted in the
 * average, as the SwiftUI app divides by the selection size); the first history
 * seeds the speed graph and only running items' histories are added to it.
 */
export function summarize(items: DetailItem[]): Summary {
  let speedHistory: number[] = [];
  let bytesPerSecond = 0;
  const parts: { ranges: Range[]; offset: number }[] = [];
  let downloadedBytes = 0;
  let totalBytes = 0;
  let sumFraction = 0;
  let activeSegments = 0;
  let configuredSegments = 0;

  for (const item of items) {
    const bytes = item.totalBytes ?? 0;
    if (!(bytes > 0)) continue;
    parts.push({ ranges: item.completed, offset: totalBytes });
    downloadedBytes += item.completedBytes;
    totalBytes += bytes;
    bytesPerSecond += item.bytesPerSecond;
    sumFraction += item.completedBytes / bytes;
    activeSegments += item.activeSegments;
    configuredSegments += item.configuredSegments;
    const history = item.detailHistory ?? item.speedHistory;
    if (speedHistory.length === 0 && history.length > 0)
      speedHistory = [...history];
    else if (history.length > 0 && item.state.kind === "running")
      speedHistory = addHistories(speedHistory, history);
  }

  const states: StateCounts = {
    completed: 0,
    running: 0,
    retrying: 0,
    queued: 0,
    stopped: 0,
    failed: 0,
  };
  for (const item of items) {
    if (item.isRetrying) states.retrying += 1;
    else states[item.state.kind] += 1;
  }

  return {
    states,
    speedHistory,
    bytesPerSecond,
    completed: unionRanges(parts),
    totalBytes,
    downloadedBytes,
    averageFraction: items.length > 0 ? sumFraction / items.length : 0,
    activeSegments,
    configuredSegments,
  };
}

export interface StatusInput {
  state: ItemStateDto;
  isEnabled: boolean;
  isAssembling: boolean;
  isRetrying: boolean;
  fileMissing: boolean;
  failedAttemptCount: number | null;
  remainingAttempts: number | null;
  lastFailureReason: string | null;
  retryHoldSeconds: number | null;
}

/** Port of `BottomPanel.statusDescription` and `statusColor`: the full status, retry detail included. */
export function statusText(item: StatusInput): {
  text: string;
  tone: "normal" | "failed" | "faulty";
} {
  if (item.isAssembling)
    return {
      text: item.isEnabled ? "Assembling…" : "Assembling… (disabled)",
      tone: "normal",
    };
  let base: string;
  if (item.isRetrying) {
    const when =
      item.retryHoldSeconds === null
        ? "retrying soon"
        : `retrying in ${item.retryHoldSeconds}s`;
    const attempt =
      item.failedAttemptCount === null
        ? ""
        : `attempt ${item.failedAttemptCount} of ${item.failedAttemptCount + (item.remainingAttempts ?? 0)}, `;
    const why =
      item.lastFailureReason === null
        ? ""
        : `\nLast failure: ${item.lastFailureReason}`;
    base = `Retrying — ${attempt}${when}${why}`;
  } else {
    switch (item.state.kind) {
      case "queued":
        base = "Queued";
        break;
      case "stopped":
        base = "Stopped";
        break;
      case "running":
        base = "Running";
        break;
      case "completed":
        base = item.fileMissing
          ? "Completed — file missing on disk"
          : "Completed";
        break;
      case "failed":
        base = `Failed — ${item.state.reason}`;
        break;
    }
  }
  const tone =
    item.state.kind === "failed"
      ? "failed"
      : item.isRetrying || (item.state.kind === "completed" && item.fileMissing)
        ? "faulty"
        : "normal";
  return { text: item.isEnabled ? base : `${base} (disabled)`, tone };
}
