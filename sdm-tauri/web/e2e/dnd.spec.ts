import { expect, test } from "@playwright/test";

const names = (page: import("@playwright/test").Page) =>
  page.locator('[data-kind="item"] .name').allTextContents();

test("dragging an item above another reorders its package", async ({
  page,
}) => {
  await page.goto("/");
  const mint = page.locator('[data-kind="item"]', { hasText: "linuxmint-22" });
  const debian = page.locator('[data-kind="item"]', {
    hasText: "debian-12.9.0",
  });
  await expect(mint).toBeVisible();

  await mint.dragTo(debian, { targetPosition: { x: 200, y: 6 } });

  const order = (await names(page)).map((n) => n.trim());
  expect(order.slice(0, 5)).toEqual([
    "ubuntu-24.04.2-desktop-amd64.iso",
    "linuxmint-22-cinnamon-64bit.iso",
    "debian-12.9.0-amd64-netinst.iso",
    "Fedora-Workstation-Live-41.iso",
    "archlinux-2026.10.01-x86_64.iso",
  ]);
});

test("dragging an item onto another package's header moves it there", async ({
  page,
}) => {
  await page.goto("/");
  const mint = page.locator('[data-kind="item"]', { hasText: "linuxmint-22" });
  const talks = page.locator('[data-kind="package"]', {
    hasText: "Conference Talks",
  });
  await mint.dragTo(talks, { targetPosition: { x: 200, y: 10 } });
  // Linux ISOs lost it; Conference Talks gained it at the end.
  await expect(
    page.locator('[data-kind="item"]', { hasText: "linuxmint-22" }),
  ).toHaveCount(1);
  const all = (await names(page)).map((n) => n.trim());
  const mintAt = all.indexOf("linuxmint-22-cinnamon-64bit.iso");
  const closingAt = all.indexOf("Closing Remarks.mp4");
  expect(mintAt).toBeGreaterThan(closingAt);
});
