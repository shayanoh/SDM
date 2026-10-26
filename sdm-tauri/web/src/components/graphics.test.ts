import { render } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import BandwidthGraph from "./BandwidthGraph.svelte";
import Icon from "./Icon.svelte";
import SegmentedBar from "./SegmentedBar.svelte";
import Sparkline from "./Sparkline.svelte";
import StateIcon from "./StateIcon.svelte";

describe("graphics render without throwing", () => {
  it("SegmentedBar for no total, a zero total and ranges", () => {
    for (const total of [null, 0, 1000]) {
      const { container, unmount } = render(SegmentedBar, {
        completed: total ? [{ start: 0, end: 500 }] : [],
        total,
      });
      expect(container.querySelector("canvas")).not.toBeNull();
      unmount();
    }
  });

  it("Sparkline for empty, short and long series", () => {
    for (const samples of [[], [3], Array.from({ length: 200 }, (_, i) => i)]) {
      const { container, unmount } = render(Sparkline, {
        samples,
        width: 48,
        height: 16,
      });
      expect(container.querySelector("svg")).not.toBeNull();
      unmount();
    }
  });

  it("Sparkline draws a polyline only when there is something to draw", () => {
    const empty = render(Sparkline, { samples: [], width: 48, height: 16 });
    expect(empty.container.querySelector("polyline")).toBeNull();
    empty.unmount();
    const some = render(Sparkline, {
      samples: [1, 2, 3],
      width: 48,
      height: 16,
    });
    expect(some.container.querySelector("polyline")).not.toBeNull();
  });

  it("BandwidthGraph for an empty history", () => {
    const { container } = render(BandwidthGraph, { history: [] });
    expect(container.querySelector("canvas")).not.toBeNull();
  });

  it("Icon renders an svg for a known name", () => {
    const { container } = render(Icon, { name: "eye", size: 16 });
    expect(container.querySelector("svg")).not.toBeNull();
  });

  it("StateIcon draws a ring for a running item and none for a completed one", () => {
    const running = render(StateIcon, {
      state: { kind: "running" },
      fraction: 0.4,
      isRetrying: false,
      isAssembling: false,
    });
    expect(running.container.querySelector("[data-ring]")).not.toBeNull();
    running.unmount();
    const done = render(StateIcon, {
      state: { kind: "completed" },
      fraction: 1,
      isRetrying: false,
      isAssembling: false,
    });
    expect(done.container.querySelector("[data-ring]")).toBeNull();
  });
});
