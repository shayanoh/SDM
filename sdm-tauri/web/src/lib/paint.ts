/**
 * A webview that is hidden does not render, so `requestAnimationFrame` may
 * never fire in it. Every wait for a frame is capped by this, or a window
 * created hidden would never report ready (and only the native fallback
 * would show it).
 */
export const FRAME_TIMEOUT_MS = 50;

/** Resolves on the next frame, or after `FRAME_TIMEOUT_MS` if none comes. */
export function nextFrame(timeoutMs: number = FRAME_TIMEOUT_MS): Promise<void> {
  return new Promise((resolve) => {
    if (typeof requestAnimationFrame !== "function") return resolve();
    let settled = false;
    const done = () => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve();
    };
    const timer = setTimeout(done, Math.max(0, timeoutMs));
    requestAnimationFrame(done);
  });
}

/** Resolves after the browser has painted what the page just rendered (two frames, each capped). */
export async function afterPaint(): Promise<void> {
  await nextFrame();
  await nextFrame();
}
