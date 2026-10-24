import { fireEvent, screen, within } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import type {
  FormatOptionDto,
  GrabberLinkRow,
  GrabberMediaRow,
  GrabberView,
} from "../bindings";
import { EMPTY_VIEW } from "../lib/grabberRows";
import { MockBridge } from "../lib/mockBridge";
import { DownloadStore } from "../lib/store.svelte";
import { UiState } from "../lib/ui.svelte";
import { wireSelectionPruning } from "../lib/wiring";
import { makeApp, renderIn, type App } from "../test/render";
import FormatPicker from "./FormatPicker.svelte";
import GrabberHeader from "./GrabberHeader.svelte";
import LinkRow from "./LinkRow.svelte";
import LinkgrabberView from "./LinkgrabberView.svelte";
import MediaRow from "./MediaRow.svelte";
import AddLinksSheet from "./AddLinksSheet.svelte";
import RenamePackagePrompt from "./RenamePackagePrompt.svelte";

const noop = () => {};

const view = (over: Partial<GrabberView> = {}): GrabberView => ({
  ...EMPTY_VIEW,
  ...over,
  summary: { ...EMPTY_VIEW.summary, ...over.summary },
});

const headerProps = (v: GrabberView) => ({
  view: v,
  onAddAll: noop,
  onCancel: noop,
  onRecheckAll: noop,
  onClear: noop,
});

describe("GrabberHeader", () => {
  it("shows the counts and the four verdict readouts", () => {
    renderIn(
      GrabberHeader,
      makeApp(),
      headerProps(
        view({
          checkedCount: 3,
          totalCount: 10,
          summary: {
            online: 5,
            faulty: 2,
            offline: 1,
            failed: 4,
            isChecking: false,
            recheckable: 4,
          },
        }),
      ),
    );
    expect(screen.getByText("3 / 10 checked")).toBeTruthy();
    for (const text of ["Online 5", "Faulty 2", "Offline 1", "Check failed 4"])
      expect(screen.getByText(text)).toBeTruthy();
  });

  it("disables the bulk buttons with no packages and Recheck All with nothing to recheck", () => {
    renderIn(GrabberHeader, makeApp(), headerProps(view()));
    const disabled = (name: string) =>
      (screen.getByRole("button", { name }) as HTMLButtonElement).disabled;
    expect(disabled("Add all to downloads")).toBe(true);
    expect(disabled("Add all and start")).toBe(true);
    expect(disabled("Recheck All")).toBe(true);
    expect(disabled("Clear")).toBe(true);
  });

  it("swaps Recheck All for Cancel Checks while checking", async () => {
    let cancelled = 0;
    renderIn(GrabberHeader, makeApp(), {
      ...headerProps(
        view({
          packages: [{ id: "p", name: "P", note: null, rowIds: [] }],
          summary: { ...EMPTY_VIEW.summary, isChecking: true },
        }),
      ),
      onCancel: () => cancelled++,
    });
    expect(screen.queryByRole("button", { name: "Recheck All" })).toBeNull();
    await fireEvent.click(
      screen.getByRole("button", { name: "Cancel Checks" }),
    );
    expect(cancelled).toBe(1);
  });

  it("sends the add-all choices", async () => {
    const starts: boolean[] = [];
    renderIn(GrabberHeader, makeApp(), {
      ...headerProps(
        view({ packages: [{ id: "p", name: "P", note: null, rowIds: [] }] }),
      ),
      onAddAll: (start: boolean) => starts.push(start),
    });
    await fireEvent.click(
      screen.getByRole("button", { name: "Add all to downloads" }),
    );
    await fireEvent.click(
      screen.getByRole("button", { name: "Add all and start" }),
    );
    expect(starts).toEqual([false, true]);
  });

  it("shows the held-back and tools lines", () => {
    renderIn(
      GrabberHeader,
      makeApp(),
      headerProps(
        view({
          heldBack: 2,
          tools: { text: "Fetching ffmpeg…", isError: false },
        }),
      ),
    );
    expect(
      screen.getByText("2 items held back — choose a format first"),
    ).toBeTruthy();
    expect(screen.getByRole("status").textContent).toContain(
      "Fetching ffmpeg…",
    );
  });
});

const link = (over: Partial<GrabberLinkRow> = {}): GrabberLinkRow => ({
  id: "l1",
  filename: "movie.mkv",
  finalUrl: "https://e.com/movie.mkv",
  stage: "done",
  verdict: { kind: "online" },
  sizeBytes: 5 * 1024 * 1024,
  isDuplicate: false,
  removable: true,
  ...over,
});

describe("LinkRow", () => {
  it("shows the name with the address on hover, the verdict and the size", () => {
    renderIn(LinkRow, makeApp(), { row: link(), onRemove: noop });
    expect(screen.getByText("movie.mkv").getAttribute("title")).toBe(
      "https://e.com/movie.mkv",
    );
    expect(screen.getByText("online")).toBeTruthy();
    expect(screen.getByText("5 MB")).toBeTruthy();
  });

  it("shows the stage and no trash button while still checking", () => {
    renderIn(LinkRow, makeApp(), {
      row: link({ verdict: null, stage: "probing", removable: false }),
      onRemove: noop,
    });
    expect(screen.getByText("probing")).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("tags a duplicate and removes by id", async () => {
    const removed: string[] = [];
    renderIn(LinkRow, makeApp(), {
      row: link({ isDuplicate: true }),
      onRemove: (id: string) => removed.push(id),
    });
    expect(screen.getByText("duplicate")).toBeTruthy();
    await fireEvent.click(screen.getByRole("button", { name: /Remove/ }));
    expect(removed).toEqual(["l1"]);
  });
});

const option = (
  id: string,
  matches: boolean,
  selected = false,
): FormatOptionDto => ({
  id,
  label: `label ${id}`,
  matchesPreferences: matches,
  isWholesale: false,
  selected,
});

const media = (over: Partial<GrabberMediaRow> = {}): GrabberMediaRow => ({
  id: "m1",
  title: "A Video",
  sourceUrl: "https://youtu.be/x",
  state: "unselected",
  stateText: null,
  summary: null,
  options: [option("a", true), option("b", false)],
  isDuplicate: false,
  sizeBytes: null,
  ...over,
});

describe("MediaRow and the format picker", () => {
  it("offers the picker, matching formats first, then a divider and warned ones", async () => {
    const picked: string[][] = [];
    renderIn(MediaRow, makeApp(), {
      row: media(),
      onPick: (id: string, optionId: string) => picked.push([id, optionId]),
    });
    await fireEvent.click(
      screen.getByRole("button", { name: /choose a format/ }),
    );
    const menu = screen.getByRole("menu");
    const labels = within(menu)
      .getAllByRole("menuitem")
      .map((item) => item.textContent?.trim());
    expect(labels).toEqual(["label a", "⚠ label b"]);
    expect(within(menu).getAllByRole("separator")).toHaveLength(1);
    await fireEvent.click(within(menu).getByText("⚠ label b"));
    expect(picked).toEqual([["m1", "b"]]);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("shows a resolved summary in the picker and a plain badge when there is nothing to pick", () => {
    const { unmount } = renderIn(MediaRow, makeApp(), {
      row: media({ state: "resolved", summary: "Direct · 720p · h264 · mp4" }),
      onPick: noop,
    });
    expect(
      screen.getByRole("button", { name: /Direct · 720p · h264 · mp4/ }),
    ).toBeTruthy();
    unmount();
    renderIn(MediaRow, makeApp(), {
      row: media({ state: "needsYtDlp", options: [] }),
      onPick: noop,
    });
    expect(screen.getByText("requires yt-dlp")).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("shows a failure's first line with the whole reason on hover", () => {
    renderIn(MediaRow, makeApp(), {
      row: media({
        state: "failed",
        options: [],
        stateText: "ERROR: gone\nlong detail",
      }),
      onPick: noop,
    });
    const badge = screen.getByText("ERROR: gone");
    expect(badge.getAttribute("title")).toBe("ERROR: gone\nlong detail");
  });

  it("FormatPicker closes on Escape", async () => {
    renderIn(FormatPicker, makeApp(), {
      options: [option("a", true)],
      label: "pick",
      role: "faulty",
      onPick: noop,
    });
    await fireEvent.click(screen.getByRole("button", { name: /pick/ }));
    expect(screen.getByRole("menu")).toBeTruthy();
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
  });
});

describe("AddLinksSheet and RenamePackagePrompt", () => {
  it("keeps Add disabled for blank text", async () => {
    renderIn(AddLinksSheet, makeApp(), { onAdd: noop, onCancel: noop });
    const add = screen.getByRole("button", {
      name: "Add",
    }) as HTMLButtonElement;
    expect(add.disabled).toBe(true);
    await fireEvent.input(screen.getByLabelText("Links"), {
      target: { value: "   \n " },
    });
    expect(add.disabled).toBe(true);
  });

  it("closes first, then hands the pasted text over", async () => {
    const order: string[] = [];
    renderIn(AddLinksSheet, makeApp(), {
      onAdd: (text: string) => order.push(`add:${text}`),
      onCancel: () => order.push("close"),
    });
    await fireEvent.input(screen.getByLabelText("Links"), {
      target: { value: "https://e.com/a.zip" },
    });
    await fireEvent.click(screen.getByRole("button", { name: "Add" }));
    expect(order).toEqual(["close", "add:https://e.com/a.zip"]);
  });

  it("Cancel adds nothing", async () => {
    const added: string[] = [];
    let closed = 0;
    renderIn(AddLinksSheet, makeApp(), {
      onAdd: (text: string) => added.push(text),
      onCancel: () => closed++,
    });
    await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(closed).toBe(1);
    expect(added).toEqual([]);
  });

  it("rename starts with the current name and rejects an empty one", async () => {
    const renamed: string[] = [];
    renderIn(RenamePackagePrompt, makeApp(), {
      name: "Old",
      onRename: (n: string) => renamed.push(n),
      onCancel: noop,
    });
    const field = screen.getByLabelText("Name") as HTMLInputElement;
    expect(field.value).toBe("Old");
    await fireEvent.input(field, { target: { value: "  " } });
    expect(
      (screen.getByRole("button", { name: "Rename" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    await fireEvent.input(field, { target: { value: " New " } });
    await fireEvent.click(screen.getByRole("button", { name: "Rename" }));
    expect(renamed).toEqual(["New"]);
  });
});

/** The whole view over a mock bridge that behaves like the controller. */
function liveApp(text?: string): App & { mock: MockBridge } {
  const mock = new MockBridge({
    seed: [],
    ...(text !== undefined ? { grabber: { text } } : {}),
  });
  const ui = new UiState();
  const store = new DownloadStore(() => {});
  wireSelectionPruning(store, ui);
  void mock.attach((patch) => store.apply(patch));
  return { bridge: mock as never, store, ui, mock };
}

const settle = { timeout: 5000 };

describe("LinkgrabberView", () => {
  it("shows the empty state with the download folder still reachable", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      renderIn(LinkgrabberView, liveApp());
      expect(screen.getByText("No links yet.")).toBeTruthy();
      expect(await screen.findByLabelText("Download folder")).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });

  it("adds pasted links through the sheet and watches them get checked", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      renderIn(LinkgrabberView, liveApp());
      await fireEvent.click(screen.getByRole("button", { name: /Add Links/ }));
      await fireEvent.input(screen.getByLabelText("Links"), {
        target: { value: "https://e.com/a.zip\nhttps://e.com/offline/b.zip" },
      });
      await fireEvent.click(screen.getByRole("button", { name: "Add" }));
      expect(await screen.findByText("a.zip")).toBeTruthy();
      expect(await screen.findByText("online", {}, settle)).toBeTruthy();
      expect(await screen.findByText("offline", {}, settle)).toBeTruthy();
      expect(await screen.findByText("2 / 2 checked", {}, settle)).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });

  it("adds a selection from the context menu and the rows leave the grabber", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const app = liveApp("https://e.com/a.zip");
      renderIn(LinkgrabberView, app);
      await screen.findByText("online", {}, settle);
      const row = screen.getByText("a.zip").closest("[role=option]")!;
      await fireEvent.contextMenu(row);
      const menu = screen.getByRole("menu");
      await fireEvent.click(within(menu).getByText("Add and Start"));
      await vi.waitFor(() => expect(app.store.items.size).toBe(1), settle);
      await vi.waitFor(
        () => expect(screen.queryByText("a.zip")).toBeNull(),
        settle,
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it("disables Add in the menu for an offline row", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      renderIn(LinkgrabberView, liveApp("https://e.com/offline/b.zip"));
      await screen.findByText("offline", {}, settle);
      await fireEvent.contextMenu(
        screen.getByText("b.zip").closest("[role=option]")!,
      );
      const menu = screen.getByRole("menu");
      expect(
        within(menu)
          .getByText("Add to Downloads")
          .closest("[role=menuitem]")!
          .getAttribute("aria-disabled"),
      ).toBe("true");
    } finally {
      vi.useRealTimers();
    }
  });

  it("package header buttons hand the package off", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const app = liveApp(
        "https://e.com/Show.S01E01.mkv https://e.com/Show.S01E02.mkv",
      );
      renderIn(LinkgrabberView, app);
      await screen.findByText("2 / 2 checked", {}, settle);
      await fireEvent.click(
        screen.getByRole("button", { name: "Add to downloads" }),
      );
      await vi.waitFor(() => expect(app.store.items.size).toBe(2), settle);
    } finally {
      vi.useRealTimers();
    }
  });

  it("renames a package from its menu", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      renderIn(LinkgrabberView, liveApp("https://e.com/solo.zip"));
      await screen.findByText("1 / 1 checked", {}, settle);
      const header = screen.getByText("solo").closest("[data-package-id]")!;
      await fireEvent.contextMenu(header);
      await fireEvent.click(
        within(screen.getByRole("menu")).getByText("Rename…"),
      );
      const field = screen.getByLabelText("Name");
      await fireEvent.input(field, { target: { value: "Fresh" } });
      await fireEvent.click(screen.getByRole("button", { name: "Rename" }));
      expect(await screen.findByText("Fresh", {}, settle)).toBeTruthy();
    } finally {
      vi.useRealTimers();
    }
  });

  it("ingests text dropped on the view but ignores a drag of its own rows", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const app = liveApp();
      const { container } = renderIn(LinkgrabberView, app);
      const root = container.querySelector(".grabber")!;
      const transfer = (types: string[], data: Record<string, string>) => ({
        types,
        getData: (type: string) => data[type] ?? "",
      });
      await fireEvent.drop(root, {
        dataTransfer: transfer(
          ["application/x-sdm-grabber-row", "text/plain"],
          {
            "text/plain": "https://e.com/own.zip",
          },
        ),
      });
      expect(app.store.grabber.totalCount).toBe(0);
      await fireEvent.drop(root, {
        dataTransfer: transfer(["text/uri-list"], {
          "text/uri-list": "https://e.com/dropped.zip",
        }),
      });
      await vi.waitFor(
        () => expect(app.store.grabber.totalCount).toBe(1),
        settle,
      );
    } finally {
      vi.useRealTimers();
    }
  });
});
