import { expect, test } from "@playwright/test";

/**
 * The tray popover in the mock: idle, then with downloads and waiting links.
 * The screenshots land in test-results/tray/ to be compared by eye with the
 * SwiftUI menu bar panel (280 points wide).
 */
test.use({ viewport: { width: 280, height: 420 } });

test("idle: no downloads, no links row", async ({ page }) => {
  await page.goto("/?window=tray&theme=deep-purple");
  await expect(page.getByText("0 active")).toBeVisible();
  await expect(page.getByText("Review")).toHaveCount(0);
  await page.waitForTimeout(200);
  await page.screenshot({ path: "test-results/tray/idle.png" });
});

test("running: three downloads and four waiting links", async ({ page }) => {
  await page.goto("/?window=tray&tray=running&theme=deep-purple");
  await expect(page.getByText("3 active")).toBeVisible();
  await expect(page.getByText("ubuntu-24.04-desktop-amd64.iso")).toBeVisible();
  await expect(page.getByText("4 links waiting")).toBeVisible();
  await page.getByRole("button", { name: "Quit" }).click();
  await page.waitForTimeout(200);
  await page.screenshot({ path: "test-results/tray/running.png" });
});
