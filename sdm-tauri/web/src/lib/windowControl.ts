/** What the page asks of its own native window. Every call is a no-op in a plain browser. */

const inTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function closeWindow(): Promise<void> {
  if (!inTauri()) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().close();
}

/** Resizes the window's content area to `width` x `height` points. */
export async function fitWindow(width: number, height: number): Promise<void> {
  if (!inTauri() || !(height > 0)) return;
  const { getCurrentWindow, LogicalSize } =
    await import("@tauri-apps/api/window");
  await getCurrentWindow().setSize(new LogicalSize(width, Math.ceil(height)));
}

/** Runs `handler` when the user closes the window by its titlebar; returns the way to stop. */
export function onCloseRequested(handler: () => void): () => void {
  if (!inTauri()) return () => {};
  let stop: (() => void) | undefined;
  let cancelled = false;
  void import("@tauri-apps/api/window").then(async ({ getCurrentWindow }) => {
    const unlisten = await getCurrentWindow().onCloseRequested(() => handler());
    if (cancelled) unlisten();
    else stop = unlisten;
  });
  return () => {
    cancelled = true;
    stop?.();
  };
}
