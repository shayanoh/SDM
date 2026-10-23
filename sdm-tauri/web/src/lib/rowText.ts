import type { ItemStateDto } from "../bindings";

/** Trailing samples an ETA's speed is averaged over: ten seconds at five ticks a second. */
export const ETA_SAMPLES = 50;

/** What a row's status line is written from. */
export interface DescribeInput {
  state: ItemStateDto;
  isEnabled: boolean;
  isAssembling: boolean;
  isRetrying: boolean;
  remainingAttempts: number | null;
  lastFailureReason: string | null;
}

/** The first non-empty line of a possibly multi-line error; the row has room for one. */
export function firstLine(text: string): string {
  return text.split("\n").find((line) => line.length > 0) ?? text;
}

/**
 * Port of `ItemRowBody.describe`: the row's status text. A failure reason is
 * given whole (the row clamps it to two lines); a retry's last failure is cut
 * to its first line.
 */
export function describeItem(
  item: DescribeInput,
  retryHoldSeconds: number | null,
): string {
  if (item.isAssembling) return "Assembling…";
  let base: string;
  if (item.isRetrying) {
    const when =
      retryHoldSeconds === null
        ? "Retrying soon"
        : `Retrying in ${retryHoldSeconds}s`;
    const left =
      item.remainingAttempts === null
        ? ""
        : ` · ${item.remainingAttempts} attempt${item.remainingAttempts === 1 ? "" : "s"} left`;
    const why =
      item.lastFailureReason === null
        ? ""
        : ` — ${firstLine(item.lastFailureReason)}`;
    base = `${when}${left}${why}`;
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
        base = "Completed";
        break;
      case "failed":
        base = `Failed — ${item.state.reason}`;
        break;
    }
  }
  return item.isEnabled ? base : `${base} (disabled)`;
}

export type StatusTone = "failed" | "faulty" | "secondary";

/** Port of `ItemRowBody.statusColor`. */
export function statusTone(
  item: Pick<DescribeInput, "state" | "isRetrying">,
): StatusTone {
  if (item.state.kind === "failed") return "failed";
  if (item.isRetrying) return "faulty";
  return "secondary";
}

/** Mean of the trailing [`ETA_SAMPLES`] samples, or `fallback` when there is no history. */
export function averageRecentSpeed(
  history: number[],
  fallback: number,
): number {
  if (history.length === 0) return fallback;
  const window = history.slice(-ETA_SAMPLES);
  return window.reduce((sum, value) => sum + value, 0) / window.length;
}

/**
 * Seconds left for a row: what remains at the item's recent average speed. A
 * queued item with no speed yet borrows the global recent speed. Zero means no
 * estimate (the row shows nothing).
 */
export function itemEta(
  item: {
    state: ItemStateDto;
    totalBytes: number | null;
    completedBytes: number;
    speedHistory: number[];
    bytesPerSecond: number;
  },
  globalRecentSpeed: number,
): number {
  let speed = averageRecentSpeed(item.speedHistory, item.bytesPerSecond);
  if (speed === 0 && item.state.kind === "queued") speed = globalRecentSpeed;
  if (!(speed > 0) || item.totalBytes === null || !(item.totalBytes > 0))
    return 0;
  return (item.totalBytes - item.completedBytes) / speed;
}
