import type { Bridge } from "./bridge";

/**
 * Asks the backend whether the window has a translucent backdrop (Mica or
 * Acrylic) and marks the page, so the CSS lets the sidebar and toolbar show
 * it. Any failure leaves the page opaque.
 */
export async function syncVibrancy(
  bridge: Pick<Bridge, "vibrancyActive">,
  root: HTMLElement = document.documentElement,
): Promise<void> {
  let active = false;
  try {
    active = await bridge.vibrancyActive();
  } catch {
    // An opaque window is the safe default.
  }
  if (active) root.dataset.vibrancy = "on";
  else delete root.dataset.vibrancy;
}
