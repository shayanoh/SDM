const cache = new Map<string, string>();

/**
 * A theme role's value (`--progress-fill`), read from the computed style once.
 * Canvases redraw several times a second per row, and asking the browser for
 * a computed style each time forces a style recalculation. A role that has no
 * value yet (the theme CSS is still loading) is not remembered.
 */
export function themeColor(element: Element, name: string): string | null {
  const known = cache.get(name);
  if (known !== undefined) return known;
  const value = getComputedStyle(element).getPropertyValue(name).trim();
  if (value === "") return null;
  cache.set(name, value);
  return value;
}

/** Forgets the cached values; call after the theme's CSS changes. */
export function resetThemeColors() {
  cache.clear();
}
