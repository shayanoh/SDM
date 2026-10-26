/** What a window's titlebar, border and a little breathing room take off a screen's height. */
export const CHROME_ALLOWANCE = 60;
/** The shortest content height worth giving a window. */
export const MIN_FIT_HEIGHT = 300;

/**
 * The content height to ask for: `natural`, unless the screen's usable height
 * (`available`, in points; `null` when unknown) cannot hold it with the
 * window's own chrome, in which case the page scrolls instead.
 */
export function cappedHeight(
  natural: number,
  available: number | null,
): number {
  if (available === null || !Number.isFinite(available)) return natural;
  return Math.min(
    natural,
    Math.max(MIN_FIT_HEIGHT, available - CHROME_ALLOWANCE),
  );
}
