import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(
    page.getByText("ubuntu-24.04.2-desktop-amd64.iso").first(),
  ).toBeVisible();
});

test("the sidebar shows the global download speed", async ({ page }) => {
  await expect(page.getByText(/\d+(\.\d+)? [KMG]B\/s/).first()).toBeVisible();
});

test("a running row paints part of its progress bar", async ({ page }) => {
  const painted = async () =>
    page
      .locator('[data-state="running"] canvas')
      .first()
      .evaluate((canvas: HTMLCanvasElement) => {
        const context = canvas.getContext("2d");
        if (!context || canvas.width === 0) return false;
        const { data } = context.getImageData(0, 0, canvas.width, 1);
        const first = [data[0], data[1], data[2]].join();
        for (let x = 0; x < canvas.width; x++) {
          const at = x * 4;
          if ([data[at], data[at + 1], data[at + 2]].join() !== first)
            return true;
        }
        return false;
      });
  await expect.poll(painted, { timeout: 10_000 }).toBe(true);
});
