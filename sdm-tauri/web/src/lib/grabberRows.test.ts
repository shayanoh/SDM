import { describe, expect, it } from "vitest";
import type {
  GrabberLinkRow,
  GrabberMediaRow,
  GrabberRow,
  GrabberView,
} from "../bindings";
import {
  EMPTY_VIEW,
  canPick,
  heldBackText,
  isAddable,
  linkBadge,
  mediaBadge,
  pickerLabel,
  rowsByPackage,
  sizeText,
  splitOptions,
  stageLabel,
} from "./grabberRows";

const link = (over: Partial<GrabberLinkRow> = {}): GrabberRow => ({
  kind: "link",
  id: "l1",
  filename: "a.zip",
  finalUrl: "https://e.com/a.zip",
  stage: "done",
  verdict: { kind: "online" },
  sizeBytes: 1024 * 1024,
  isDuplicate: false,
  removable: true,
  ...over,
});

const media = (over: Partial<GrabberMediaRow> = {}): GrabberRow => ({
  kind: "media",
  id: "m1",
  title: "A video",
  sourceUrl: "https://youtu.be/x",
  state: "resolved",
  stateText: null,
  summary: "Direct · 720p · h264 · mp4",
  options: [],
  isDuplicate: false,
  sizeBytes: null,
  ...over,
});

const asLink = (row: GrabberRow) => row as GrabberLinkRow & { kind: "link" };
const asMedia = (row: GrabberRow) => row as GrabberMediaRow & { kind: "media" };

describe("linkBadge", () => {
  it("reads each verdict as the SwiftUI text and color role", () => {
    const cases: [GrabberLinkRow["verdict"], string, string][] = [
      [{ kind: "online" }, "online", "online"],
      [{ kind: "offline" }, "offline", "offline"],
      [{ kind: "checkFailed" }, "check failed", "failed"],
      [{ kind: "faulty", reason: "HTML page" }, "HTML page", "faulty"],
    ];
    for (const [verdict, text, role] of cases)
      expect(linkBadge(asLink(link({ verdict })))).toMatchObject({
        text,
        role,
        spinner: false,
      });
  });

  it("shows the stage with a spinner while there is no verdict", () => {
    for (const stage of ["queued", "probing", "sniffing", "done"] as const)
      expect(linkBadge(asLink(link({ verdict: null, stage })))).toEqual({
        text: stageLabel(stage),
        role: "secondary",
        spinner: true,
      });
  });
});

describe("mediaBadge", () => {
  it("covers every state", () => {
    const text = (state: GrabberMediaRow["state"], extra = {}) =>
      mediaBadge(asMedia(media({ state, ...extra })));
    expect(text("resolving")).toMatchObject({
      text: "resolving",
      spinner: true,
    });
    expect(text("resolved")).toMatchObject({
      text: "Direct · 720p · h264 · mp4",
      role: "online",
    });
    expect(text("resolved", { summary: null }).text).toBe("resolved");
    expect(text("unselected")).toMatchObject({
      text: "choose a format",
      role: "faulty",
    });
    expect(text("unsupported").text).toBe("not supported yet");
    expect(text("needsYtDlp").text).toBe("requires yt-dlp");
    expect(text("needsFfmpeg").text).toBe("requires ffmpeg");
  });

  it("shows the first line of a failure and the whole reason on hover", () => {
    const badge = mediaBadge(
      asMedia(
        media({ state: "failed", stateText: "ERROR: gone\nmore detail" }),
      ),
    );
    expect(badge).toMatchObject({
      text: "ERROR: gone",
      role: "offline",
      title: "ERROR: gone\nmore detail",
    });
  });
});

describe("the format picker", () => {
  const option = (id: string, matchesPreferences: boolean) => ({
    id,
    label: id,
    matchesPreferences,
    isWholesale: false,
    selected: false,
  });

  it("is offered only with options in a pickable state", () => {
    const options = [option("a", true)];
    expect(canPick(asMedia(media({ options, state: "resolved" })))).toBe(true);
    expect(canPick(asMedia(media({ options, state: "unselected" })))).toBe(
      true,
    );
    expect(canPick(asMedia(media({ options, state: "needsFfmpeg" })))).toBe(
      true,
    );
    expect(canPick(asMedia(media({ options, state: "resolving" })))).toBe(
      false,
    );
    expect(canPick(asMedia(media({ options: [], state: "resolved" })))).toBe(
      false,
    );
  });

  it("labels the button by state", () => {
    expect(pickerLabel(asMedia(media()))).toEqual({
      text: "Direct · 720p · h264 · mp4",
      role: "online",
    });
    expect(pickerLabel(asMedia(media({ state: "needsFfmpeg" })))).toEqual({
      text: "requires ffmpeg",
      role: "faulty",
    });
    expect(pickerLabel(asMedia(media({ state: "unselected" })))).toEqual({
      text: "choose a format",
      role: "faulty",
    });
  });

  it("puts matching options first", () => {
    const { matching, nonMatching } = splitOptions([
      option("a", false),
      option("b", true),
    ]);
    expect(matching.map((o) => o.id)).toEqual(["b"]);
    expect(nonMatching.map((o) => o.id)).toEqual(["a"]);
  });
});

describe("text helpers", () => {
  it("formats sizes in binary units and hides unknown ones", () => {
    expect(sizeText(1024 * 1024)).toBe("1 MB");
    expect(sizeText(5 * 1024 * 1024 * 1024)).toBe("5 GB");
    expect(sizeText(null)).toBe("");
    expect(sizeText(0)).toBe("");
  });

  it("pluralizes the held-back message", () => {
    expect(heldBackText(1)).toBe("1 item held back — choose a format first");
    expect(heldBackText(3)).toBe("3 items held back — choose a format first");
  });
});

describe("rowsByPackage", () => {
  it("lists rows in no package first, then each package's own rows in order", () => {
    const view: GrabberView = {
      ...EMPTY_VIEW,
      rows: [
        link({ id: "a" }),
        link({ id: "b" }),
        media({ id: "m" }),
        link({ id: "loose" }),
      ],
      packages: [
        { id: "p1", name: "One", note: null, rowIds: ["b", "a"] },
        { id: "p2", name: "Two", note: null, rowIds: ["m", "gone"] },
      ],
    };
    const { loose, packages } = rowsByPackage(view);
    expect(loose.map((r) => r.id)).toEqual(["loose"]);
    expect(packages.map((p) => p.rows.map((r) => r.id))).toEqual([
      ["b", "a"],
      ["m"],
    ]);
  });
});

describe("isAddable", () => {
  it("accepts online or faulty links and resolved media only", () => {
    expect(isAddable(link())).toBe(true);
    expect(isAddable(link({ verdict: { kind: "faulty", reason: "x" } }))).toBe(
      true,
    );
    expect(isAddable(link({ verdict: { kind: "offline" } }))).toBe(false);
    expect(isAddable(link({ verdict: { kind: "checkFailed" } }))).toBe(false);
    expect(isAddable(link({ verdict: null }))).toBe(false);
    expect(isAddable(media())).toBe(true);
    expect(isAddable(media({ state: "unselected" }))).toBe(false);
  });
});
