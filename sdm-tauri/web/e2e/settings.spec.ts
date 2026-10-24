import { expect, test } from "@playwright/test";

/**
 * Screenshots of the Settings window in the mock, one per tab, at the width of
 * the SwiftUI window with the Deep Purple theme. They land in
 * test-results/settings/ to be compared by eye with
 * docs/superpowers/specs/assets/2026-10-24-swiftui-settings/: fonts and icons
 * differ by design.
 */
test.use({ viewport: { width: 560, height: 900 } });

const TABS = [
  ["Appearance", "01-appearance"],
  ["Downloads", "02-downloads"],
  ["Linkgrabber", "03-linkgrabber"],
  ["Media Sites", "04-media-sites"],
  ["Notifications", "05-notifications"],
] as const;

test("each tab, next to its SwiftUI reference", async ({ page }) => {
  await page.goto("/?window=settings&theme=deep-purple");
  await page.getByRole("tab", { name: "Appearance" }).waitFor();
  await page.waitForTimeout(300);
  for (const [name, file] of TABS) {
    await page.getByRole("tab", { name }).click();
    await page.waitForTimeout(150);
    await page.locator(".window").screenshot({
      path: `test-results/settings/${file}.png`,
    });
  }
});

test("tabs switch and the last codec box cannot be unchecked", async ({
  page,
}) => {
  await page.goto("/?window=settings");
  await page.getByRole("tab", { name: "Media Sites" }).click();
  await page.getByLabel("AV1").uncheck();
  await page.getByLabel("VP9").uncheck();
  await expect(page.getByLabel("H.264")).toBeDisabled();
  await page.getByRole("tab", { name: "Notifications" }).click();
  await expect(page.getByLabel("Download finished")).toBeChecked();
});
