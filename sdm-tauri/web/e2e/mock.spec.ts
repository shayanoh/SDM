import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(
    page.getByText("ubuntu-24.04.2-desktop-amd64.iso").first(),
  ).toBeVisible();
});

test("the header shows a download speed", async ({ page }) => {
  await expect(page.getByTestId("total-speed")).not.toHaveText("—");
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

test("a bad address shows a banner and a good one adds a row", async ({
  page,
}) => {
  const input = page.getByLabel("Download address");
  await input.fill("nope");
  await input.press("Enter");
  await expect(page.getByRole("alert")).toContainText("valid web address");

  await input.fill("https://example.com/files/new.bin");
  await input.press("Enter");
  // The list is windowed and the new package lands last.
  await page
    .locator(".scroller")
    .evaluate((element) => (element.scrollTop = element.scrollHeight));
  await expect(page.getByText("new.bin").first()).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
});
