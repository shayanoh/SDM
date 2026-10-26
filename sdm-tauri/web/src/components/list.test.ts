import { fireEvent, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import { fullPatchOf, type PackageSpec } from "../test/fixtures";
import { makeApp, renderIn } from "../test/render";
import { completedView } from "../lib/views";
import PackageList from "./PackageList.svelte";

const done = { kind: "completed" as const };

const specs: PackageSpec[] = [
  { id: "p1", name: "Alpha", items: [{ id: "a" }, { id: "b" }, { id: "c" }] },
  { id: "p2", name: "Beta", items: [{ id: "d" }] },
];

function listApp(over: PackageSpec[] = specs) {
  const app = makeApp([fullPatchOf(1, over)]);
  const view = { packages: app.store.order };
  return { app, view };
}

const row = (container: Element, id: string) =>
  container.querySelector(`[data-row-id="${id}"]`) as HTMLElement;

const props = (view: { packages: { id: string; itemIds: string[] }[] }) => ({
  view,
  tab: "downloads" as const,
  allowsReordering: true,
  showsLiveAggregates: true,
});

describe("PackageList rows", () => {
  it("renders only the rows near the viewport of a 5,000 item list", () => {
    const many: PackageSpec[] = [
      {
        id: "big",
        items: Array.from({ length: 5000 }, (_, n) => ({ id: `i${n}` })),
      },
    ];
    const { app, view } = listApp(many);
    const { container } = renderIn(PackageList, app, props(view));
    const rendered = container.querySelectorAll("[data-row-id]").length;
    expect(rendered).toBeGreaterThan(0);
    expect(rendered).toBeLessThan(60);
  });

  it("lists each package header followed by its items", () => {
    const { app, view } = listApp();
    const { container } = renderIn(PackageList, app, props(view));
    const ids = [...container.querySelectorAll("[data-row-id]")].map((e) =>
      e.getAttribute("data-row-id"),
    );
    expect(ids).toEqual(["p1", "a", "b", "c", "p2", "d"]);
  });

  it("shows the empty message for no downloads", () => {
    const app = makeApp([]);
    renderIn(PackageList, app, props({ packages: [] }));
    expect(screen.getByText(/No downloads/)).toBeTruthy();
  });
});

describe("PackageList selection", () => {
  it("click selects one row, ctrl-click toggles, shift-click selects a range", async () => {
    const { app, view } = listApp();
    const { container, ui } = renderIn(PackageList, app, props(view));

    await fireEvent.click(row(container, "a"));
    expect([...ui.selected]).toEqual(["a"]);
    await fireEvent.click(row(container, "c"), { ctrlKey: true });
    expect([...ui.selected].sort()).toEqual(["a", "c"]);
    // The anchor is the last plain or ctrl click ("c"): shift-click reaches back to "a".
    await fireEvent.click(row(container, "a"), { shiftKey: true });
    expect([...ui.selected].sort()).toEqual(["a", "b", "c"]);
    expect(row(container, "b").getAttribute("aria-selected")).toBe("true");
  });

  it("the meta key toggles like ctrl (macOS)", async () => {
    const { app, view } = listApp();
    const { container, ui } = renderIn(PackageList, app, props(view));
    await fireEvent.click(row(container, "a"));
    await fireEvent.click(row(container, "b"), { metaKey: true });
    expect([...ui.selected].sort()).toEqual(["a", "b"]);
  });

  it("clicking a package header selects the package; its chevron only collapses it", async () => {
    const { app, view } = listApp();
    const { container, ui } = renderIn(PackageList, app, props(view));

    await fireEvent.click(row(container, "p1"));
    expect([...ui.selected]).toEqual(["p1"]);

    ui.selection.clear();
    await fireEvent.click(
      screen.getByRole("button", { name: "Collapse Alpha" }),
    );
    expect(ui.selected.size).toBe(0);
    expect(row(container, "a")).toBeNull();
    expect(row(container, "p1")).not.toBeNull();

    await fireEvent.click(screen.getByRole("button", { name: "Expand Alpha" }));
    expect(row(container, "a")).not.toBeNull();
  });

  it("right-click on an unselected row selects it and opens the menu there", async () => {
    const { app, view } = listApp();
    const { container, ui } = renderIn(PackageList, app, props(view));
    await fireEvent.click(row(container, "a"));
    await fireEvent.contextMenu(row(container, "b"), {
      clientX: 30,
      clientY: 40,
    });
    expect([...ui.selected]).toEqual(["b"]);
    expect(ui.menu).toEqual({ x: 30, y: 40, targetId: "b" });
  });

  it("right-click on a selected row keeps the whole selection", async () => {
    const { app, view } = listApp();
    const { container, ui } = renderIn(PackageList, app, props(view));
    await fireEvent.click(row(container, "a"));
    await fireEvent.click(row(container, "b"), { ctrlKey: true });
    await fireEvent.contextMenu(row(container, "b"), {
      clientX: 1,
      clientY: 2,
    });
    expect([...ui.selected].sort()).toEqual(["a", "b"]);
  });
});

describe("item rows", () => {
  it("shows zero and unknown sizes without NaN", () => {
    const { app, view } = listApp([
      {
        id: "p",
        items: [
          { id: "zero", fig: { totalBytes: 0, completedBytes: 0 } },
          { id: "unknown", fig: { totalBytes: null, completedBytes: 0 } },
        ],
      },
    ]);
    const { container } = renderIn(PackageList, app, props(view));
    expect(container.textContent).not.toContain("NaN");
    expect(container.textContent).not.toContain("Infinity");
    expect(row(container, "zero").textContent).toContain("Zero KB / Zero KB");
    expect(row(container, "unknown").textContent).toContain(
      "Zero KB / Zero KB",
    );
  });

  it("strikes through a disabled item and fades the row", () => {
    const { app, view } = listApp([
      {
        id: "p",
        items: [
          { id: "x", state: { kind: "stopped" }, over: { isEnabled: false } },
        ],
      },
    ]);
    const { container } = renderIn(PackageList, app, props(view));
    expect(row(container, "x").textContent).toContain("Stopped (disabled)");
    expect(row(container, "x").querySelector(".disabled")).not.toBeNull();
  });

  it("badges a non-resumable item and a missing file", () => {
    const { app, view } = listApp([
      {
        id: "p",
        items: [
          { id: "nr", over: { isResumable: false } },
          { id: "gone", state: done, over: { fileMissing: true } },
          { id: "ok", over: { isResumable: null } },
        ],
      },
    ]);
    const { container } = renderIn(PackageList, app, props(view));
    expect(row(container, "nr").textContent).toContain("not resumable");
    expect(row(container, "gone").textContent).toContain("file missing");
    expect(row(container, "ok").textContent).not.toContain("not resumable");
  });

  it("shows an ETA only while running or queued", () => {
    const history = Array(50).fill(100);
    const { app, view } = listApp([
      {
        id: "p",
        items: [
          {
            id: "run",
            fig: {
              totalBytes: 100000,
              completedBytes: 40000,
              speedHistory: history,
              bytesPerSecond: 100,
            },
          },
          {
            id: "fin",
            state: done,
            fig: {
              totalBytes: 100000,
              completedBytes: 100000,
              speedHistory: history,
            },
          },
        ],
      },
    ]);
    const { container } = renderIn(PackageList, app, props(view));
    expect(row(container, "run").textContent).toContain(
      "ETA: About 10 Minutes",
    );
    expect(row(container, "fin").textContent).not.toContain("ETA");
  });

  it("shows a failed item's reason in the status line", () => {
    const { app, view } = listApp([
      {
        id: "p",
        items: [{ id: "f", state: { kind: "failed", reason: "HTTP 404" } }],
      },
    ]);
    const { container } = renderIn(PackageList, app, props(view));
    expect(row(container, "f").textContent).toContain("Failed — HTTP 404");
  });

  it("truncates very long and right-to-left names inside the row", () => {
    const { app, view } = listApp([
      {
        id: "p",
        items: [
          { id: "long", over: { filename: "x".repeat(400) + ".iso" } },
          { id: "rtl", over: { filename: "خلاصه پرونده.pdf" } },
        ],
      },
    ]);
    const { container } = renderIn(PackageList, app, props(view));
    const name = row(container, "long").querySelector(".name") as HTMLElement;
    expect(name.textContent).toContain("x".repeat(400));
    expect(
      row(container, "rtl").querySelector(".name")!.getAttribute("dir"),
    ).toBe("auto");
  });
});

describe("package headers", () => {
  it("show the package's live figures on Downloads", () => {
    const { app, view } = listApp();
    app.store.packages.get("p1")!.completedBytes = 3 * 1024 * 1024;
    app.store.packages.get("p1")!.totalBytes = 10 * 1024 * 1024;
    const { container } = renderIn(PackageList, app, props(view));
    expect(row(container, "p1").textContent).toContain("3 MB / 10 MB");
  });

  it("sum only the completed items on the Completed tab", () => {
    const { app } = listApp([
      {
        id: "p",
        name: "Mixed",
        items: [
          { id: "a", state: done, fig: { completedBytes: 50, totalBytes: 50 } },
          {
            id: "b",
            state: { kind: "running" },
            fig: { completedBytes: 10, totalBytes: 500 },
          },
          { id: "c", state: done, fig: { completedBytes: 70, totalBytes: 70 } },
        ],
      },
    ]);
    const view = completedView(app.store.order, app.store.items);
    const { container } = renderIn(PackageList, app, {
      view,
      tab: "completed" as const,
      allowsReordering: false,
      showsLiveAggregates: false,
    });
    expect(row(container, "p").textContent).toContain("120 bytes / 120 bytes");
    expect(row(container, "p").textContent).toContain("Zero KB/s");
    expect(row(container, "b")).toBeNull();
  });
});
