import type {
  FormatOptionDto,
  GrabberLinkRow,
  GrabberMediaRow,
  GrabberPackage,
  GrabberRow,
  GrabberView,
  HandoffTarget,
  LinkStageDto,
  MediaStateDto,
  VerdictDto,
} from "../bindings";
import { EMPTY_VIEW, isAddable } from "./grabberRows";
import { BridgeError } from "./bridge";

const MB = 1_000_000;

/** What the mock hands to the download list. */
export interface HandedOffItem {
  url: string;
  filename: string;
  size: number | null;
}

export type AddToEngine = (
  name: string,
  note: string | null,
  items: HandedOffItem[],
  start: boolean,
) => void;

interface Entry {
  id: string;
  url: string;
  kind: "link" | "media";
  stage: LinkStageDto;
  verdict: VerdictDto | null;
  size: number | null;
  /** Media only. */
  state?: MediaStateDto;
  stateText?: string | null;
  title?: string;
  options?: FormatOptionDto[];
  /** Ticks until a resolving media row resolves. */
  resolveIn?: number;
  override?: string;
  addedAt: number;
}

const newId = () => crypto.randomUUID();

const isMedia = (url: string): boolean => {
  try {
    const host = new URL(url).hostname;
    return host === "youtu.be" || host.endsWith("youtube.com");
  } catch {
    return false;
  }
};

const fileName = (url: string): string => {
  try {
    const path = new URL(url).pathname.split("/").filter(Boolean);
    return decodeURIComponent(path[path.length - 1] ?? "download");
  } catch {
    return "download";
  }
};

const stem = (name: string): string => name.replace(/\.[^./]+$/, "");

/** The package an entry falls into without a manual override. */
function clusterName(entry: Entry): string {
  const name =
    entry.kind === "media" ? (entry.title ?? "video") : fileName(entry.url);
  const series = /^(.*?)[. _-]S\d{1,2}E\d{1,2}/i.exec(name);
  return series ? series[1].replace(/[._]+$/, "") || stem(name) : stem(name);
}

/** A hash-derived size so the same URL always reads the same. */
function sizeFor(url: string): number {
  let h = 0;
  for (const c of url) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return (20 + (h % 900)) * MB;
}

function verdictFor(url: string): VerdictDto {
  if (url.includes("offline")) return { kind: "offline" };
  if (url.includes("fail")) return { kind: "checkFailed" };
  if (url.includes("faulty")) return { kind: "faulty", reason: "HTML page" };
  return { kind: "online" };
}

const URL_PATTERN = /https?:\/\/[^\s<>"']+/g;

function options(selected: string | null): FormatOptionDto[] {
  const make = (
    id: string,
    label: string,
    matches: boolean,
  ): FormatOptionDto => ({
    id,
    label,
    matchesPreferences: matches,
    isWholesale: false,
    selected: id === selected,
  });
  return [
    make("22+-", "720p · avc1 · mp4 · 48 MB", true),
    make("18+-", "360p · avc1 · mp4 · 21 MB", true),
    make("313+-", "2160p · vp9 · webm · 320 MB", false),
  ];
}

/** The mock link grabber: a stand-in for the controller's session, with rules
 * the real one follows closely enough to build the view against. URLs steer
 * the outcome: `offline`, `fail`, `faulty` in the address pick the verdict,
 * and YouTube links resolve (`ok…`), wait for a format (`big…`), need ffmpeg
 * (`mux…`) or fail (anything else). */
export class MockGrabber {
  #entries: Entry[] = [];
  #held: number | null = null;
  #clock = 0;
  #known = new Set<string>();
  #cancel = false;

  constructor(
    private readonly addToEngine: AddToEngine,
    private readonly ffmpeg = true,
  ) {}

  /** Numbers of the engine's known download addresses, for duplicate marking. */
  setKnownUrls(urls: Iterable<string>) {
    this.#known = new Set(urls);
  }

  ingestText(text: string) {
    const seen = new Set(this.#entries.map((e) => e.url));
    for (const raw of text.match(URL_PATTERN) ?? []) {
      const url = raw.replace(/[.,;)]+$/, "");
      if (seen.has(url)) continue;
      seen.add(url);
      const media = isMedia(url);
      this.#entries.push({
        id: newId(),
        url,
        kind: media ? "media" : "link",
        stage: "queued",
        verdict: null,
        size: null,
        state: media ? "resolving" : undefined,
        resolveIn: media ? 3 : undefined,
        title: media ? fileName(url) : undefined,
        addedAt: this.#clock,
      });
    }
    this.#cancel = false;
  }

  /** Advances every probe and resolve by one step. */
  tick() {
    this.#clock += 1;
    for (const entry of this.#entries) {
      if (entry.kind === "link") {
        if (entry.stage === "done") continue;
        if (this.#cancel) {
          entry.stage = "done";
          entry.verdict = { kind: "checkFailed" };
          continue;
        }
        const order: LinkStageDto[] = ["queued", "probing", "sniffing", "done"];
        entry.stage = order[order.indexOf(entry.stage) + 1];
        if (entry.stage === "done") {
          entry.verdict = verdictFor(entry.url);
          entry.size =
            entry.verdict.kind === "offline" ? null : sizeFor(entry.url);
        }
      } else if (entry.state === "resolving") {
        if (this.#cancel) {
          entry.state = "failed";
          entry.stateText = "Cancelled";
        } else if ((entry.resolveIn = (entry.resolveIn ?? 1) - 1) <= 0) {
          this.#resolve(entry);
        }
      }
    }
    if (this.#cancel && !this.#entries.some((e) => this.#isChecking(e)))
      this.#cancel = false;
  }

  #isChecking(e: Entry): boolean {
    return e.kind === "link" ? e.stage !== "done" : e.state === "resolving";
  }

  #resolve(entry: Entry) {
    const path = new URL(entry.url).pathname;
    entry.title = "A Title";
    if (path.startsWith("/ok")) {
      entry.state = "resolved";
      entry.options = options("22+-");
    } else if (path.startsWith("/big")) {
      entry.state = "unselected";
      entry.options = options(null);
    } else if (path.startsWith("/mux")) {
      entry.state = this.ffmpeg ? "unselected" : "needsFfmpeg";
      entry.options = options(null);
    } else {
      entry.state = "failed";
      entry.stateText = "ERROR: Video unavailable\nThis video is not available";
    }
  }

  // MARK: Edits

  removeLinks(ids: string[]) {
    const gone = new Set(ids);
    this.#entries = this.#entries.filter((e) => !gone.has(e.id));
  }

  clear() {
    this.#entries = [];
    this.#held = null;
  }

  #packages(): { name: string; ids: string[] }[] {
    const order: string[] = [];
    const byName = new Map<string, string[]>();
    for (const entry of this.#entries) {
      const name = entry.override ?? clusterName(entry);
      if (!byName.has(name)) {
        byName.set(name, []);
        order.push(name);
      }
      byName.get(name)!.push(entry.id);
    }
    return order.map((name) => ({ name, ids: byName.get(name)! }));
  }

  /** Package ids are made fresh from the name, as the session's are by reclustering. */
  #packageId(name: string): string {
    return `pkg:${name}`;
  }

  #packageByIdOrThrow(id: string) {
    const found = this.#packages().find((p) => this.#packageId(p.name) === id);
    if (!found) throw this.#invalid("id", "that package is no longer there");
    return found;
  }

  #invalid(argument: string, message: string) {
    return new BridgeError({ kind: "invalidArgument", argument, message });
  }

  removePackage(id: string) {
    this.removeLinks(this.#packageByIdOrThrow(id).ids);
  }

  moveLink(id: string, packageId: string) {
    const target = this.#packageByIdOrThrow(packageId);
    const entry = this.#entries.find((e) => e.id === id);
    if (entry) entry.override = target.name;
  }

  renamePackage(id: string, name: string) {
    const trimmed = name.trim();
    if (!trimmed) throw this.#invalid("name", "the name is empty");
    for (const entryId of this.#packageByIdOrThrow(id).ids) {
      const entry = this.#entries.find((e) => e.id === entryId);
      if (entry) entry.override = trimmed;
    }
  }

  mergePackages(source: string, destination: string) {
    const to = this.#packageByIdOrThrow(destination);
    this.renamePackage(source, to.name);
  }

  splitPackage(id: string) {
    for (const entryId of this.#packageByIdOrThrow(id).ids) {
      const entry = this.#entries.find((e) => e.id === entryId);
      if (!entry) continue;
      entry.override =
        entry.kind === "media"
          ? (entry.title ?? "video")
          : stem(fileName(entry.url));
    }
  }

  recheck(ids: string[]) {
    const wanted = new Set(ids);
    for (const entry of this.#entries)
      if (wanted.has(entry.id)) this.#restart(entry);
  }

  recheckFailed() {
    for (const entry of this.#entries) {
      const failedLink =
        entry.kind === "link" && entry.verdict?.kind === "checkFailed";
      const failedMedia =
        entry.kind === "media" &&
        (entry.state === "failed" ||
          entry.state === "needsYtDlp" ||
          entry.state === "needsFfmpeg");
      if (failedLink || failedMedia) this.#restart(entry);
    }
  }

  #restart(entry: Entry) {
    if (entry.kind === "link") {
      entry.stage = "queued";
      entry.verdict = null;
    } else {
      entry.state = "resolving";
      entry.resolveIn = 3;
      entry.options = undefined;
    }
  }

  cancelChecks() {
    this.#cancel = true;
  }

  setFormat(id: string, optionId: string) {
    const entry = this.#entries.find((e) => e.id === id && e.kind === "media");
    const option = entry?.options?.find((o) => o.id === optionId);
    if (!entry || !option)
      throw this.#invalid("optionId", "that format is no longer offered");
    entry.options = options(optionId);
    entry.state = "resolved";
  }

  copyUrl(id: string): string {
    const entry = this.#entries.find((e) => e.id === id);
    if (!entry)
      throw this.#invalid("id", "that link is no longer in the grabber");
    return entry.url;
  }

  addToDownloads(target: HandoffTarget, start: boolean) {
    const packages = this.#packages();
    const handOff = (
      name: string,
      ids: string[],
      onlyAddable: boolean,
    ): number => {
      const rows = ids
        .map((id) => this.#entries.find((e) => e.id === id))
        .filter((e): e is Entry => !!e);
      const sendable = rows.filter((e) =>
        onlyAddable
          ? isAddable(this.#row(e))
          : e.kind === "link" || e.state === "resolved",
      );
      const heldBack = rows.filter(
        (e) => e.kind === "media" && e.state !== "resolved",
      ).length;
      if (sendable.length > 0) {
        this.addToEngine(
          name,
          null,
          sendable.map((e) => ({
            url: e.url,
            filename:
              e.kind === "media"
                ? `${e.title} [${fileName(e.url)}].mp4`
                : fileName(e.url),
            size: e.size,
          })),
          start,
        );
        this.removeLinks(sendable.map((e) => e.id));
      }
      return onlyAddable ? 0 : heldBack;
    };
    if (target.kind === "rows") {
      const wanted = new Set(target.ids);
      for (const pkg of packages) {
        handOff(
          pkg.name,
          pkg.ids.filter((id) => wanted.has(id)),
          true,
        );
      }
    } else if (target.kind === "package") {
      const pkg = this.#packageByIdOrThrow(target.id);
      const held = handOff(pkg.name, pkg.ids, false);
      this.#held = held > 0 ? held : null;
    } else {
      let held = 0;
      for (const pkg of packages) held += handOff(pkg.name, pkg.ids, false);
      this.#held = held > 0 ? held : null;
    }
  }

  // MARK: View

  #row(e: Entry): GrabberRow {
    const duplicate = this.#known.has(e.url);
    if (e.kind === "link") {
      const row: GrabberLinkRow = {
        id: e.id,
        filename: fileName(e.url),
        finalUrl: e.url,
        stage: e.stage,
        verdict: e.verdict,
        sizeBytes: e.size,
        isDuplicate: duplicate,
        removable: e.verdict !== null,
      };
      return { kind: "link", ...row };
    }
    const resolved = e.state === "resolved";
    const picked = e.options?.find((o) => o.selected);
    const row: GrabberMediaRow = {
      id: e.id,
      title: e.title ?? "video",
      sourceUrl: e.url,
      state: e.state ?? "resolving",
      stateText: e.state === "failed" ? (e.stateText ?? null) : null,
      summary: resolved
        ? `Direct · ${picked?.label.split(" · ").slice(0, 3).join(" · ") ?? "resolved"}`
        : null,
      options:
        e.state === "resolved" ||
        e.state === "unselected" ||
        e.state === "needsFfmpeg"
          ? (e.options ?? [])
          : [],
      isDuplicate: duplicate,
      sizeBytes: resolved ? 48 * MB : null,
    };
    return { kind: "media", ...row };
  }

  view(): GrabberView {
    if (this.#entries.length === 0 && this.#held === null) return EMPTY_VIEW;
    const rows = this.#entries.map((e) => this.#row(e));
    const links = rows.filter((r) => r.kind === "link") as (GrabberLinkRow & {
      kind: "link";
    })[];
    const packages: GrabberPackage[] = this.#packages().map((p) => ({
      id: this.#packageId(p.name),
      name: p.name,
      note: null,
      rowIds: [
        ...p.ids.filter((id) => links.some((l) => l.id === id)),
        ...p.ids.filter((id) => !links.some((l) => l.id === id)),
      ],
    }));
    const count = (kind: VerdictDto["kind"]) =>
      links.filter((l) => l.verdict?.kind === kind).length;
    const recheckableMedia = this.#entries.filter(
      (e) =>
        e.kind === "media" &&
        (e.state === "failed" ||
          e.state === "needsYtDlp" ||
          e.state === "needsFfmpeg"),
    ).length;
    const checked = this.#entries.filter((e) => !this.#isChecking(e)).length;
    return {
      checkedCount: checked,
      totalCount: this.#entries.length,
      summary: {
        online: count("online"),
        faulty: count("faulty"),
        offline: count("offline"),
        failed: count("checkFailed"),
        isChecking: this.#entries.some((e) => this.#isChecking(e)),
        recheckable: count("checkFailed") + recheckableMedia,
      },
      rows,
      packages,
      heldBack: this.#held,
      tools: null,
    };
  }
}

/** The fixture `?grabber=demo` and the parity screenshots use: every verdict, media state and a package. */
export const DEMO_TEXT = [
  "https://files.example.com/Show.S01E01.1080p.mkv",
  "https://files.example.com/Show.S01E02.1080p.mkv",
  "https://files.example.com/Show.S01E03.1080p.mkv",
  "https://files.example.com/ubuntu-24.04.iso",
  "https://files.example.com/offline/report.pdf",
  "https://files.example.com/faulty/page.zip",
  "https://files.example.com/fail/timeout.bin",
  "https://youtu.be/ok-1",
  "https://youtu.be/big-2",
  "https://youtu.be/mux-3",
  "https://youtu.be/gone-4",
].join("\n");
