import type { ItemStateDto } from "../bindings";

/** Exactly what the menu rules need to know about an item. */
export interface RowItem {
  state: ItemStateDto;
  isEnabled: boolean;
  isResumable: boolean | null;
  fileMissing: boolean;
  assembly: "none" | "mux";
}

/** Start: a stopped, enabled item (a disabled one must be enabled first). */
export const canStart = (item: RowItem) =>
  item.state.kind === "stopped" && item.isEnabled;

/** Stop: an item that is running or queued. */
export const canStop = (item: RowItem) =>
  item.state.kind === "running" || item.state.kind === "queued";

export const canEnable = (item: RowItem) => !item.isEnabled;

/** Disable: any enabled item that is not finished. */
export const canDisable = (item: RowItem) =>
  item.isEnabled && item.state.kind !== "completed";

export const canRetry = (item: RowItem) => item.state.kind === "failed";

/** Retry Mux: a muxed item whose parts downloaded but ffmpeg failed. */
export const canRetryMux = (item: RowItem) =>
  canRetry(item) && item.assembly === "mux";

/** Open: a finished file that is still on disk. */
export const canOpen = (item: RowItem) =>
  item.state.kind === "completed" && !item.fileMissing;

/** Reset: a running item has a worker writing to its file, so not that. */
export const canReset = (item: RowItem) => item.state.kind !== "running";

/**
 * Pausing a running item that cannot resume loses its progress for good, so
 * the user is asked first. `null` (not yet probed) does not count.
 */
export const needsPauseConfirmation = (items: RowItem[]) =>
  items.some(
    (item) => item.state.kind === "running" && item.isResumable === false,
  );

/** Items Pause All and Resume All act on. */
export const downloadable = (items: RowItem[]) =>
  items.filter(
    (item) =>
      item.state.kind === "queued" ||
      item.state.kind === "running" ||
      item.state.kind === "stopped",
  );

/** True when there is nothing left for Pause All to do. */
export const allDownloadableStopped = (items: RowItem[]) =>
  downloadable(items).every((item) => item.state.kind === "stopped");
