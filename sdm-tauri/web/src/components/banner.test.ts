import { fireEvent, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import { makeApp, renderIn } from "../test/render";
import Banner from "./Banner.svelte";

describe("Banner", () => {
  it("shows a reported error and goes away when dismissed", async () => {
    const app = makeApp();
    renderIn(Banner, app);
    expect(screen.queryByRole("alert")).toBeNull();

    app.ui.report("that is not a valid web address");
    expect((await screen.findByRole("alert")).textContent).toContain(
      "valid web address",
    );

    await fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
