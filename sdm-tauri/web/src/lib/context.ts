import { getContext } from "svelte";
import type { Bridge } from "./bridge";
import { BridgeError } from "./bridge";
import type { DownloadStore } from "./store.svelte";
import type { UiState } from "./ui.svelte";

const BRIDGE = "bridge";
const STORE = "store";
const UI = "ui";

export function createContext(
  bridge: Bridge,
  store: DownloadStore,
  ui: UiState,
): Map<string, unknown> {
  return new Map<string, unknown>([
    [BRIDGE, bridge],
    [STORE, store],
    [UI, ui],
  ]);
}

function required<T>(key: string): T {
  const value = getContext<T | undefined>(key);
  if (value === undefined) throw new Error(`missing "${key}" context`);
  return value;
}

export const useBridge = () => required<Bridge>(BRIDGE);
export const useStore = () => required<DownloadStore>(STORE);
export const useUi = () => required<UiState>(UI);

/** The text to show for anything a bridge call threw. */
export function errorMessage(error: unknown): string {
  if (error instanceof BridgeError) return error.message;
  if (error instanceof Error) return error.message;
  return String(error);
}
