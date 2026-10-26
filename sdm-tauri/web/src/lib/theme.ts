import type { Bridge } from "./bridge";
import { resetThemeColors } from "./colors";
import { resolveThemeId } from "./themeSelection";

type ThemeBridge = Pick<
  Bridge,
  "getSettings" | "listThemes" | "themeCss" | "onThemePreview"
>;

let markFirstPaint: () => void = () => {};

/**
 * Settles once the first theme is in the page. A window that was created
 * hidden waits for this before it asks to be shown, so it never appears
 * unstyled.
 */
export const firstThemePaint: Promise<void> = new Promise((resolve) => {
  markFirstPaint = resolve;
});

/**
 * Paints the stored theme into `<style id="theme">` and keeps it current: a
 * `theme-preview` from the Settings window swaps it (`null` goes back to the
 * stored one) and, while the selection is `system`, an OS light/dark change
 * does too. `override` (the `?theme=` query) stands in for the stored value.
 * Returns the way to stop.
 */
export async function startTheming(
  bridge: ThemeBridge,
  override?: string | null,
): Promise<() => void> {
  const media =
    typeof matchMedia === "function"
      ? matchMedia("(prefers-color-scheme: dark)")
      : null;
  const known = (await bridge.listThemes()).map((theme) => theme.id);
  let stored = override ?? (await bridge.getSettings()).themeId ?? "system";
  let shown = stored;

  const style =
    document.getElementById("theme") ?? document.createElement("style");
  style.id = "theme";
  if (!style.isConnected) document.head.append(style);

  async function paint() {
    const id = resolveThemeId(shown, media?.matches ?? true, known);
    style.textContent = await bridge.themeCss(id);
    resetThemeColors();
  }
  await paint();
  markFirstPaint();

  const stopPreview = bridge.onThemePreview(async (themeId) => {
    if (themeId === null) {
      if (override == null)
        stored = (await bridge.getSettings()).themeId ?? "system";
      shown = stored;
    } else {
      shown = themeId;
    }
    await paint();
  });
  const onSystemChange = () => {
    if (shown === "system") void paint();
  };
  media?.addEventListener("change", onSystemChange);
  return () => {
    stopPreview();
    media?.removeEventListener("change", onSystemChange);
  };
}
