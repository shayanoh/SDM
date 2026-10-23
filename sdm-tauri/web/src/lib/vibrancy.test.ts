import { describe, expect, it } from "vitest";
import { stubBridge } from "../test/render";
import { syncVibrancy } from "./vibrancy";

describe("syncVibrancy", () => {
  it("marks the page when the window has a translucent backdrop", async () => {
    const bridge = stubBridge();
    bridge.answers.vibrancyActive = true;
    const root = document.createElement("html");
    await syncVibrancy(bridge, root);
    expect(root.dataset.vibrancy).toBe("on");
  });

  it("leaves the page opaque when there is none", async () => {
    const bridge = stubBridge();
    bridge.answers.vibrancyActive = false;
    const root = document.createElement("html");
    root.dataset.vibrancy = "on";
    await syncVibrancy(bridge, root);
    expect(root.dataset.vibrancy).toBeUndefined();
  });

  it("leaves the page opaque when the question fails", async () => {
    const bridge = stubBridge();
    bridge.failures.vibrancyActive = new Error("no");
    const root = document.createElement("html");
    await syncVibrancy(bridge, root);
    expect(root.dataset.vibrancy).toBeUndefined();
  });
});
