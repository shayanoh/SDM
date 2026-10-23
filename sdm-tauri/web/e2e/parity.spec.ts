import { test } from "@playwright/test";

/**
 * Screenshots of the mock in the states the SwiftUI reference screenshots
 * show (docs/superpowers/specs/assets/2026-10-23-swiftui-main-window/), at
 * the same 1300 x 960 window size with the Deep Purple theme. They land in
 * test-results/parity/ to be compared by eye: fonts and icons differ by design.
 */
test.use({ viewport: { width: 1300, height: 960 } });

const shot = (name: string) => ({ path: `test-results/parity/${name}.png` });

test("01 downloads running", async ({ page }) => {
  await page.goto("/?theme=deep-purple");
  await page.waitForTimeout(3500);
  await page.screenshot(shot("01-downloads-running"));
});
