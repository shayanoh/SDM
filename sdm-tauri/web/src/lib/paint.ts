/** Resolves after the browser has painted what the page just rendered (two frames). */
export function afterPaint(): Promise<void> {
  return new Promise((resolve) => {
    if (typeof requestAnimationFrame !== "function") return resolve();
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
  });
}
