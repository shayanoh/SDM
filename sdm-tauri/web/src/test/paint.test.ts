import { afterEach, describe, expect, it, vi } from "vitest";
import { afterPaint, nextFrame } from "../lib/paint";

describe("paint waits", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("resolves when no frame ever comes (a hidden webview)", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("requestAnimationFrame", () => 0);
    let done = false;
    void afterPaint().then(() => (done = true));
    await vi.advanceTimersByTimeAsync(500);
    expect(done).toBe(true);
  });

  it("resolves on the frame, before the cap", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("requestAnimationFrame", (cb: () => void) => {
      setTimeout(cb, 5);
      return 0;
    });
    let done = false;
    void nextFrame(1000).then(() => (done = true));
    await vi.advanceTimersByTimeAsync(10);
    expect(done).toBe(true);
  });
});
