import { cappedHeight } from "./fitting";

/** What the page asks of its own native window. Every call is a no-op in a plain browser. */

const inTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function closeWindow(): Promise<void> {
  if (!inTauri()) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().close();
}

let fitting: Promise<void> = Promise.resolve();

const nextFrame = () =>
  new Promise<void>((done) => requestAnimationFrame(() => done()));

/**
 * Resizes the window so the page has `width` x `height` points to itself.
 * Calls run one after another, so a stale size can never land last. The size
 * is then checked against what the page actually got (`innerHeight`):
 * platforms disagree on whether a window's size includes its titlebar, and
 * the shortfall is made up in a second step. The window is centered after
 * every fit so a tall tab stays on screen (`center: false` for a window that
 * is anchored somewhere, like the tray popover).
 */
export function fitWindow(
  width: number,
  height: number,
  options: { center?: boolean } = {},
): Promise<void> {
  if (!inTauri() || !(height > 0)) return Promise.resolve();
  // A failed step must not freeze every later fit, hence the catch first.
  fitting = fitting
    .catch(() => {})
    .then(async () => {
      const { getCurrentWindow, currentMonitor, LogicalSize } =
        await import("@tauri-apps/api/window");
      const window = getCurrentWindow();
      // A tall page on a short screen scrolls instead of running off it.
      const monitor = await currentMonitor();
      const available = monitor
        ? monitor.workArea.size.height / monitor.scaleFactor
        : null;
      const target = Math.ceil(cappedHeight(height, available));
      await window.setSize(new LogicalSize(width, target));
      await nextFrame();
      const shortfall = target - globalThis.innerHeight;
      if (shortfall > 1) {
        await window.setSize(new LogicalSize(width, target + shortfall));
        await nextFrame();
      }
      // A taller tab must not push the window's bottom off the screen.
      if (options.center ?? true) await window.center();
    });
  return fitting.catch(() => {});
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
