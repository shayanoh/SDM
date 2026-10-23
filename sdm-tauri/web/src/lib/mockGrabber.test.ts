import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  GrabberLinkRow,
  GrabberMediaRow,
  GrabberView,
  Patch,
} from "../bindings";
import { BridgeError } from "./bridge";
import { MockBridge } from "./mockBridge";

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

async function attached(grabber?: { text?: string; ffmpeg?: boolean }) {
  const bridge = new MockBridge({ seed: [], ...(grabber ? { grabber } : {}) });
  const patches: Patch[] = [];
  await bridge.attach((patch) => patches.push(patch));
  const view = (): GrabberView =>
    [...patches].reverse().find((p) => p.grabber)!.grabber!;
  const settle = () => vi.advanceTimersByTime(200 * 10);
  return { bridge, patches, view, settle };
}

const links = (view: GrabberView) =>
  view.rows.filter((r) => r.kind === "link") as (GrabberLinkRow & {
    kind: "link";
  })[];
const media = (view: GrabberView) =>
  view.rows.filter((r) => r.kind === "media") as (GrabberMediaRow & {
    kind: "media";
  })[];

describe("the mock grabber", () => {
  it("sends the view on the full patch even when empty", async () => {
    const { view } = await attached();
    expect(view().totalCount).toBe(0);
  });

  it("probes pasted links through the stages to a verdict", async () => {
    const { bridge, view, settle } = await attached();
    await bridge.grabberIngestText(
      "get https://e.com/a.zip and https://e.com/offline/b.zip then https://e.com/fail/c.zip https://e.com/faulty/d.zip",
    );
    expect(links(view()).every((l) => l.stage === "queued")).toBe(true);
    expect(view().summary.isChecking).toBe(true);
    settle();
    const verdicts = links(view()).map((l) => l.verdict?.kind);
    expect(verdicts).toEqual(["online", "offline", "checkFailed", "faulty"]);
    expect(view().summary).toMatchObject({
      online: 1,
      offline: 1,
      failed: 1,
      faulty: 1,
      isChecking: false,
      recheckable: 1,
    });
    expect(view().checkedCount).toBe(4);
  });

  it("ignores text without links and links it has already", async () => {
    const { bridge, view } = await attached();
    await bridge.grabberIngestText("no links here");
    await bridge.grabberIngestText("https://e.com/a.zip https://e.com/a.zip");
    await bridge.grabberIngestText("https://e.com/a.zip");
    expect(view().totalCount).toBe(1);
  });

  it("resolves media rows by what their address says", async () => {
    const { bridge, view, settle } = await attached({ ffmpeg: false });
    await bridge.grabberIngestText(
      "https://youtu.be/ok1 https://youtu.be/big1 https://youtu.be/mux1 https://youtu.be/gone1",
    );
    settle();
    expect(media(view()).map((m) => m.state)).toEqual([
      "resolved",
      "unselected",
      "needsFfmpeg",
      "failed",
    ]);
    expect(media(view())[3].stateText).toContain("Video unavailable");
  });

  it("picks a format, and refuses a stale option", async () => {
    const { bridge, view, settle } = await attached();
    await bridge.grabberIngestText("https://youtu.be/big1");
    settle();
    const row = media(view())[0];
    await bridge.grabberSetFormat(row.id, row.options[1].id);
    expect(media(view())[0].state).toBe("resolved");
    expect(media(view())[0].options.filter((o) => o.selected)).toHaveLength(1);
    await expect(
      bridge.grabberSetFormat(row.id, "nope"),
    ).rejects.toBeInstanceOf(BridgeError);
  });

  it("hands a package off, holds back unresolved media and clears the message next time", async () => {
    const { bridge, view, settle, patches } = await attached();
    await bridge.grabberIngestText(
      "https://e.com/a.zip https://youtu.be/ok1 https://youtu.be/big1",
    );
    settle();
    await bridge.grabberAddToDownloads({ kind: "all" }, false);
    expect(view().heldBack).toBe(1);
    expect(view().totalCount).toBe(1);
    const downloads = [...patches]
      .reverse()
      .find((p) => p.structure)!
      .structure!.packages.flatMap((p) => p.items);
    expect(downloads.map((i) => i.filename).sort()).toEqual(
      ["a.zip", "A Title [ok1].mp4"].sort(),
    );
    expect(downloads.every((i) => i.state.kind === "stopped")).toBe(true);

    const row = media(view())[0];
    await bridge.grabberSetFormat(row.id, row.options[0].id);
    await bridge.grabberAddToDownloads({ kind: "all" }, true);
    expect(view().heldBack).toBeNull();
    expect(view().totalCount).toBe(0);
  });

  it("hands off only addable rows of a selection", async () => {
    const { bridge, view, settle } = await attached();
    await bridge.grabberIngestText(
      "https://e.com/a.zip https://e.com/offline/b.zip",
    );
    settle();
    const ids = links(view()).map((l) => l.id);
    await bridge.grabberAddToDownloads({ kind: "rows", ids }, false);
    expect(links(view()).map((l) => l.filename)).toEqual(["b.zip"]);
  });

  it("renames, moves, merges, splits and removes packages by their current ids", async () => {
    const { bridge, view, settle } = await attached();
    await bridge.grabberIngestText(
      "https://e.com/Show.S01E01.mkv https://e.com/Show.S01E02.mkv https://e.com/other.zip",
    );
    settle();
    expect(view().packages.map((p) => p.name)).toEqual(["Show", "other"]);
    await bridge.grabberRenamePackage(view().packages[0].id, "  Renamed ");
    expect(view().packages.map((p) => p.name)).toContain("Renamed");
    await expect(
      bridge.grabberRenamePackage(view().packages[0].id, "  "),
    ).rejects.toBeInstanceOf(BridgeError);
    const other = view().packages.find((p) => p.name === "other")!;
    const renamed = view().packages.find((p) => p.name === "Renamed")!;
    await bridge.grabberMoveLink(other.rowIds[0], renamed.id);
    expect(view().packages).toHaveLength(1);
    await bridge.grabberSplitPackage(view().packages[0].id);
    expect(view().packages).toHaveLength(3);
    const [first, second] = view().packages;
    await bridge.grabberMergePackages(first.id, second.id);
    expect(view().packages).toHaveLength(2);
    await bridge.grabberRemovePackage(view().packages[0].id);
    expect(view().packages).toHaveLength(1);
    await bridge.grabberClear();
    expect(view().totalCount).toBe(0);
    await expect(
      bridge.grabberRemovePackage("pkg:nope"),
    ).rejects.toBeInstanceOf(BridgeError);
  });

  it("rechecks failed rows, and cancel settles the waiting ones as failed", async () => {
    const { bridge, view, settle } = await attached();
    await bridge.grabberIngestText("https://e.com/fail/c.zip");
    settle();
    expect(view().summary.recheckable).toBe(1);
    await bridge.grabberRecheckFailed();
    expect(links(view())[0].verdict).toBeNull();
    await bridge.grabberCancelChecks();
    settle();
    expect(links(view())[0].verdict?.kind).toBe("checkFailed");
    expect(view().summary.isChecking).toBe(false);
  });

  it("copies a link's address and rejects an unknown id", async () => {
    const { bridge, view } = await attached();
    await bridge.grabberIngestText("https://e.com/a.zip");
    expect(await bridge.grabberCopyUrl(links(view())[0].id)).toBe(
      "https://e.com/a.zip",
    );
    await expect(bridge.grabberCopyUrl("nope")).rejects.toBeInstanceOf(
      BridgeError,
    );
  });

  it("marks links that are already downloads as duplicates", async () => {
    const { bridge, view, settle } = await attached();
    await bridge.addUrl("https://e.com/a.zip");
    await bridge.grabberIngestText("https://e.com/a.zip");
    settle();
    expect(links(view())[0].isDuplicate).toBe(true);
  });
});
