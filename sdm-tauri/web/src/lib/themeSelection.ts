/**
 * The theme to paint for a stored selection: `system` (and any id that is
 * not in `known`, such as a theme that was removed) follows the OS.
 */
export function resolveThemeId(
  stored: string,
  prefersDark: boolean,
  known: readonly string[],
): string {
  if (stored !== "system" && known.includes(stored)) return stored;
  return prefersDark ? "dark" : "light";
}
