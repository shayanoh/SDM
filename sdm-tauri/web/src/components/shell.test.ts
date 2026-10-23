import { fireEvent, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import { fullPatchOf, stats } from "../test/fixtures";
import { makeApp, renderIn } from "../test/render";
import BottomBar from "./BottomBar.svelte";
import OverviewBlock from "./OverviewBlock.svelte";
import Sidebar from "./Sidebar.svelte";

const queued = { kind: "queued" as const };
const stopped = { kind: "stopped" as const };
const running = { kind: "running" as const };
const done = { kind: "completed" as const };

function appWith(
  items: {
    id: string;
    state: typeof queued | typeof stopped | typeof running | typeof done;
  }[],
  statsOver = {},
) {
  const patch = fullPatchOf(1, [{ id: "p", items }]);
  patch.stats = stats(statsOver);
  return makeApp([patch]);
}

describe("Sidebar", () => {
  it("shows running/(running+queued) on the Downloads row while something runs", () => {
    renderIn(
      Sidebar,
      appWith([{ id: "a", state: running }], {
        runningCount: 3,
        queuedCount: 11,
      }),
    );
    expect(screen.getByText("3/14")).toBeTruthy();
  });

  it("hides the badge and ring when nothing runs", () => {
    const view = renderIn(
      Sidebar,
      appWith([{ id: "a", state: queued }], { queuedCount: 4 }),
    );
    expect(screen.queryByText(/\d+\/\d+/)).toBeNull();
    expect(
      view.container.querySelector("[data-testid=downloads-ring]"),
    ).toBeNull();
  });

  it("selects a tab on click", async () => {
    const view = renderIn(Sidebar, appWith([]));
    await fireEvent.click(screen.getByText("Completed"));
    expect(view.ui.tab).toBe("completed");
    await fireEvent.click(screen.getByText("Linkgrabber"));
    expect(view.ui.tab).toBe("linkgrabber");
  });

  it("marks the selected row", async () => {
    renderIn(Sidebar, appWith([]));
    const row = screen.getByText("Downloads").closest("button")!;
    expect(row.getAttribute("aria-current")).toBe("page");
    expect(
      screen
        .getByText("Completed")
        .closest("button")!
        .getAttribute("aria-current"),
    ).toBeNull();
  });
});

describe("OverviewBlock", () => {
  it("says there are no running downloads when none run", () => {
    renderIn(OverviewBlock, appWith([{ id: "a", state: stopped }]));
    expect(screen.getByText("No running downloads")).toBeTruthy();
    expect(screen.getByText("0 active")).toBeTruthy();
  });

  it("shows the global speed and the active count while downloads run", () => {
    renderIn(
      OverviewBlock,
      appWith([{ id: "a", state: running }], {
        runningCount: 3,
        bytesPerSecond: 3.7 * 1024 * 1024,
      }),
    );
    expect(screen.getByText("3.7 MB/s")).toBeTruthy();
    expect(screen.getByText("3 active")).toBeTruthy();
    expect(screen.queryByText("No running downloads")).toBeNull();
  });
});

describe("BottomBar", () => {
  const view = (ids: string[]) => ({ packages: [{ id: "p", itemIds: ids }] });

  it("offers Pause All while something is downloadable and not stopped", async () => {
    const app = appWith([
      { id: "a", state: running },
      { id: "b", state: stopped },
    ]);
    renderIn(BottomBar, app, {
      view: view(["a", "b"]),
      showsPauseResume: true,
      showsLiveAggregates: true,
    });
    await fireEvent.click(screen.getByRole("button", { name: /Pause All/ }));
    expect(app.bridge.calls).toEqual([["pauseAll"]]);
  });

  it("flips to Resume All when every downloadable item is stopped", async () => {
    const app = appWith([
      { id: "a", state: stopped },
      { id: "b", state: done },
    ]);
    renderIn(BottomBar, app, {
      view: view(["a", "b"]),
      showsPauseResume: true,
      showsLiveAggregates: true,
    });
    await fireEvent.click(screen.getByRole("button", { name: /Resume All/ }));
    expect(app.bridge.calls).toEqual([["resumeAll"]]);
  });

  it("disables the button when nothing is downloadable", () => {
    renderIn(BottomBar, appWith([{ id: "a", state: done }]), {
      view: view(["a"]),
      showsPauseResume: true,
      showsLiveAggregates: true,
    });
    expect(
      (
        screen.getByRole("button", {
          name: /(Pause|Resume) All/,
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
  });

  it("has no button and an idle speed on the Completed tab", () => {
    renderIn(
      BottomBar,
      appWith([{ id: "a", state: done }], { bytesPerSecond: 5e6 }),
      {
        view: view(["a"]),
        showsPauseResume: false,
        showsLiveAggregates: false,
      },
    );
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.getByText("Zero KB/s")).toBeTruthy();
    expect(screen.getByText("1 packages, 1 items")).toBeTruthy();
  });

  it("shows the live speed on the Downloads tab", () => {
    renderIn(
      BottomBar,
      appWith([{ id: "a", state: running }], {
        bytesPerSecond: 3.7 * 1024 * 1024,
      }),
      {
        view: view(["a"]),
        showsPauseResume: true,
        showsLiveAggregates: true,
      },
    );
    expect(screen.getByText("3.7 MB/s")).toBeTruthy();
  });
});
