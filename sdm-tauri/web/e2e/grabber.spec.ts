import { expect, test } from "@playwright/test";

test.use({ viewport: { width: 1300, height: 960 } });

const open = async (page: import("@playwright/test").Page, query = "") => {
  await page.goto(`/?theme=deep-purple${query}`);
  await page.getByRole("button", { name: /Linkgrabber/ }).click();
};

test("paste links, watch them get checked, pick a format and add everything", async ({
  page,
}) => {
  await open(page);
  await expect(page.getByText("No links yet.")).toBeVisible();

  await page.getByRole("button", { name: /Add Links/ }).click();
  await page
    .getByRole("textbox", { name: "Links" })
    .fill(
      [
        "https://files.example.com/Show.S01E01.mkv",
        "https://files.example.com/Show.S01E02.mkv",
        "https://files.example.com/offline/gone.zip",
        "https://youtu.be/big-1",
        "https://youtu.be/ok-2",
      ].join("\n"),
    );
  await page.getByRole("button", { name: "Add", exact: true }).click();

  await expect(page.getByText("5 / 5 checked")).toBeVisible({ timeout: 8000 });
  await expect(page.getByText("Online 2")).toBeVisible();
  await expect(page.getByText("Offline 1")).toBeVisible();

  // The unselected video offers a picker; choosing a format resolves it.
  await page.getByRole("button", { name: /choose a format/ }).click();
  await page.getByRole("menuitem").first().click();
  await expect(
    page.getByRole("button", { name: /choose a format/ }),
  ).toHaveCount(0);

  await page.getByRole("button", { name: "Add all and start" }).click();
  await expect(page.getByText("Online 0")).toBeVisible();
  // A package hand-off sends every link in it, as the SwiftUI app does.
  await expect(page.getByText("No links yet.")).toBeVisible();

  await page
    .getByRole("button", { name: /Downloads/ })
    .first()
    .click();
  // New packages join the end of the reference list; the list is windowed.
  await page
    .getByRole("listbox", { name: "Downloads" })
    .evaluate((list) => (list.scrollTop = list.scrollHeight));
  await expect(page.getByText("Show.S01E01.mkv")).toBeVisible();
});

test("a row dragged onto another package header moves there", async ({
  page,
}) => {
  await open(page);
  await page.getByRole("button", { name: /Add Links/ }).click();
  await page
    .getByRole("textbox", { name: "Links" })
    .fill("https://a.example.com/one.zip\nhttps://a.example.com/two.zip");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByText("2 / 2 checked")).toBeVisible({ timeout: 8000 });

  const row = page.locator("[data-row-id]").filter({ hasText: "one.zip" });
  const target = page.locator("[data-package-id]").filter({ hasText: "two" });
  await row.dragTo(target);
  await expect(page.locator("[data-package-id]")).toHaveCount(1);
});

test("text dropped on the view is ingested", async ({ page }) => {
  await open(page);
  await page.locator(".grabber").dispatchEvent("drop", {
    dataTransfer: await page.evaluateHandle(() => {
      const data = new DataTransfer();
      data.setData("text/plain", "https://drop.example.com/dropped.zip");
      return data;
    }),
  });
  await expect(page.getByText("dropped.zip")).toBeVisible({ timeout: 8000 });
});

test("the context menu and the package menu open", async ({ page }) => {
  await open(page, "&grabber=demo&ffmpeg=off");
  await expect(page.getByText("11 / 11 checked")).toBeVisible({
    timeout: 10000,
  });
  await page
    .locator("[data-row-id]")
    .filter({ hasText: "ubuntu-24.04.iso" })
    .click({ button: "right" });
  await expect(
    page.getByRole("menuitem", { name: "Add and Start" }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await page.locator("[data-package-id]").first().click({ button: "right" });
  await page.getByRole("menuitem", { name: /Merge into/ }).hover();
  await expect(page.getByRole("menu")).toHaveCount(2);
});
