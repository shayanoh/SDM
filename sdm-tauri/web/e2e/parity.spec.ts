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

test("04 item context menu and 05 package menu", async ({ page }) => {
  await page.goto("/?theme=deep-purple");
  await page.waitForTimeout(2500);
  await page
    .locator('[data-row-id][data-kind="item"]')
    .nth(1)
    .click({ button: "right" });
  await page.waitForTimeout(300);
  await page.screenshot(shot("04-context-menu-item"));
  await page.keyboard.press("Escape");
  await page
    .locator('[data-row-id][data-kind="package"]')
    .first()
    .click({ button: "right" });
  await page.waitForTimeout(300);
  await page.screenshot(shot("05-context-menu-package"));
});

test("02 03 06 details panel", async ({ page }) => {
  await page.goto("/?theme=deep-purple");
  await page.waitForTimeout(2500);
  const items = page.locator('[data-row-id][data-kind="item"]');
  await items.first().click();
  await page.waitForTimeout(600);
  await page.screenshot(shot("02-item-selected-details"));
  // Drag the handle up to the 380 px the reference 03 shows.
  const handle = page.getByRole("separator", {
    name: "Resize the details panel",
  });
  const box = (await handle.boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + 3);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2, box.y - 230, { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(400);
  await page.screenshot(shot("03-details-panel-tall"));
  await items.nth(2).click({ modifiers: ["Shift"] });
  await page.waitForTimeout(600);
  await page.screenshot(shot("06-multi-select-details"));
});

test("07 delete sheet", async ({ page }) => {
  await page.goto("/?theme=deep-purple");
  await page.waitForTimeout(2500);
  const items = page.locator('[data-row-id][data-kind="item"]');
  await items.first().click();
  await items.nth(2).click({ modifiers: ["Shift"] });
  await page.getByRole("button", { name: /^Remove and Delete File/ }).click();
  await page.waitForTimeout(500);
  await page.screenshot(shot("07-delete-sheet"));
});
