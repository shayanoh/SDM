import type {
  GrabberLinkRow,
  GrabberMediaRow,
  GrabberPackage,
  GrabberRow,
  GrabberView,
  FormatOptionDto,
  LinkStageDto,
} from "../bindings";
import { formatBytes } from "./format";

/** Which theme role colors a piece of text (`--status-*` or the secondary text). */
export type TextRole = "online" | "offline" | "failed" | "faulty" | "secondary";

export interface Badge {
  text: string;
  role: TextRole;
  spinner: boolean;
  /** The full text on hover, when `text` is only its first line. */
  title?: string;
}

export const EMPTY_VIEW: GrabberView = {
  checkedCount: 0,
  totalCount: 0,
  summary: {
    online: 0,
    faulty: 0,
    offline: 0,
    failed: 0,
    isChecking: false,
    recheckable: 0,
  },
  rows: [],
  packages: [],
  heldBack: null,
  tools: null,
};

const STAGE_LABELS: Record<LinkStageDto, string> = {
  queued: "queued",
  probing: "probing",
  sniffing: "sniffing",
  done: "done",
};

export const stageLabel = (stage: LinkStageDto): string => STAGE_LABELS[stage];

/** The verdict badge of a link row: the reason for a faulty one, the stage while still checking. */
export function linkBadge(row: GrabberLinkRow): Badge {
  const verdict = row.verdict;
  if (verdict === null)
    return { text: stageLabel(row.stage), role: "secondary", spinner: true };
  switch (verdict.kind) {
    case "online":
      return { text: "online", role: "online", spinner: false };
    case "offline":
      return { text: "offline", role: "offline", spinner: false };
    case "checkFailed":
      return { text: "check failed", role: "failed", spinner: false };
    case "faulty":
      return { text: verdict.reason, role: "faulty", spinner: false };
  }
}

/** What a media row shows when it has no picker to show. */
export function mediaBadge(row: GrabberMediaRow): Badge {
  switch (row.state) {
    case "resolving":
      return { text: "resolving", role: "secondary", spinner: true };
    case "resolved":
      return {
        text: row.summary ?? "resolved",
        role: "online",
        spinner: false,
      };
    case "unselected":
      return { text: "choose a format", role: "faulty", spinner: false };
    case "unsupported":
      return { text: "not supported yet", role: "faulty", spinner: false };
    case "needsYtDlp":
      return { text: "requires yt-dlp", role: "faulty", spinner: false };
    case "needsFfmpeg":
      return { text: "requires ffmpeg", role: "faulty", spinner: false };
    case "failed": {
      const reason = row.stateText ?? "failed";
      return {
        text: reason.split("\n")[0] || reason,
        role: "offline",
        spinner: false,
        title: reason,
      };
    }
  }
}

/** A media row shows its format picker instead of a badge when it has options to pick from. */
export const canPick = (row: GrabberMediaRow): boolean =>
  row.options.length > 0 &&
  (row.state === "resolved" ||
    row.state === "unselected" ||
    row.state === "needsFfmpeg");

/** The picker button's text. */
export function pickerLabel(row: GrabberMediaRow): {
  text: string;
  role: TextRole;
} {
  if (row.state === "resolved")
    return { text: row.summary ?? "resolved", role: "online" };
  if (row.state === "needsFfmpeg")
    return { text: "requires ffmpeg", role: "faulty" };
  return { text: "choose a format", role: "faulty" };
}

/** Matching options first, then the rest (the UI prefixes those with a warning sign). */
export function splitOptions(options: FormatOptionDto[]): {
  matching: FormatOptionDto[];
  nonMatching: FormatOptionDto[];
} {
  return {
    matching: options.filter((o) => o.matchesPreferences),
    nonMatching: options.filter((o) => !o.matchesPreferences),
  };
}

/** `1.5 GB`-style size, or nothing when unknown. */
export function sizeText(bytes: number | null): string {
  return bytes !== null && bytes > 0 ? formatBytes(bytes) : "";
}

export function heldBackText(count: number): string {
  return `${count} item${count === 1 ? "" : "s"} held back — choose a format first`;
}

export const rowId = (row: GrabberRow): string => row.id;

export interface PackageRows {
  pkg: GrabberPackage;
  rows: GrabberRow[];
}

/** Rows in no package first (as SwiftUI lists them), then each package with its own rows. */
export function rowsByPackage(view: GrabberView): {
  loose: GrabberRow[];
  packages: PackageRows[];
} {
  const byId = new Map(view.rows.map((row) => [row.id, row] as const));
  const packaged = new Set(view.packages.flatMap((pkg) => pkg.rowIds));
  const loose = view.rows.filter((row) => !packaged.has(row.id));
  const packages = view.packages.map((pkg) => ({
    pkg,
    rows: pkg.rowIds.flatMap((id) => {
      const row = byId.get(id);
      return row ? [row] : [];
    }),
  }));
  return { loose, packages };
}

/**
 * Whether a row can go to the downloads right now: an online or faulty link,
 * or a media row with a chosen format. (The controller makes the final call.)
 */
export function isAddable(row: GrabberRow): boolean {
  if (row.kind === "link")
    return row.verdict?.kind === "online" || row.verdict?.kind === "faulty";
  return row.state === "resolved";
}
