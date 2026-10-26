import { describe, expect, it } from "vitest";
import { MockBridge } from "./mockBridge";
import { firstThemePaint, startTheming } from "./theme";

describe("firstThemePaint", () => {
  it("settles once the theme is in the page, not before", async () => {
    let settled = false;
    void firstThemePaint.then(() => (settled = true));
    await Promise.resolve();
    expect(settled).toBe(false);
    await startTheming(new MockBridge(), "light");
    await firstThemePaint;
    expect(document.getElementById("theme")?.textContent).toContain(
      "--surface-primary",
    );
    expect(settled).toBe(true);
  });
});
