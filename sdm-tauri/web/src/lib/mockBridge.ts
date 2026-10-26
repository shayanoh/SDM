import type {
  BinariesStatusDto,
  ChromeExtensionDto,
  DeletionInfo,
  DeletionTarget,
  EngineStats,
  ItemFigures,
  ItemStateDto,
  PackageFigures,
  Patch,
  SettingsDto,
  Structure,
  GrabberView,
  HandoffTarget,
  TrayStatus,
} from "../bindings";
import { BridgeError, type Bridge } from "./bridge";
import { DEMO_TEXT, MockGrabber } from "./mockGrabber";
import { RESOLUTION_CHOICES, defaultSettings } from "./settingsModel";

const TICK_MS = 200;
/** Samples an item's row sparkline gets (matches the engine's trailing window). */
const ROW_HISTORY = 64;
/** Samples kept for the details panel and the global graph. */
const FULL_HISTORY = 300;
const SEGMENTS = 8;
const MAX_RUNNING = 3;

const MB = 1_000_000;
const GB = 1_000_000_000;

export interface MockSeedItem {
  name: string;
  size: number;
  state: "running" | "queued" | "stopped" | "completed" | "failed";
  /** How much of the file is already on disk, 0 to 1. */
  fraction?: number;
  enabled?: boolean;
  resumable?: boolean | null;
  failure?: string;
  metadata?: string;
}

export interface MockSeed {
  name: string;
  items: MockSeedItem[];
}

export interface MockOptions {
  seed?: MockSeed[];
  /** How many items download at once (the engine's default is three). */
  maxRunning?: number;
  /** Pretend the window has a Mica/Acrylic backdrop (`?vibrancy=on` in the browser). */
  vibrancy?: boolean;
  /** Link-grabber options: pre-load links, and whether ffmpeg counts as installed. */
  grabber?: { text?: string; ffmpeg?: boolean };
}

/** The five packages the SwiftUI reference screenshots were taken with. */
export const REFERENCE_SEED: MockSeed[] = [
  {
    name: "Linux ISOs",
    items: [
      {
        name: "ubuntu-24.04.2-desktop-amd64.iso",
        size: 6 * GB,
        state: "running",
        fraction: 0.31,
        resumable: true,
        metadata: "x86_64 · ISO",
      },
      {
        name: "debian-12.9.0-amd64-netinst.iso",
        size: 660 * MB,
        state: "running",
        fraction: 0.62,
        resumable: true,
      },
      {
        name: "Fedora-Workstation-Live-41.iso",
        size: 2300 * MB,
        state: "running",
        fraction: 0.08,
        resumable: true,
      },
      {
        name: "archlinux-2026.10.01-x86_64.iso",
        size: 1100 * MB,
        state: "queued",
      },
      {
        name: "linuxmint-22-cinnamon-64bit.iso",
        size: 2900 * MB,
        state: "stopped",
        fraction: 0.45,
        resumable: true,
      },
    ],
  },
  {
    name: "Conference Talks",
    items: [
      {
        name: "Keynote - The Road Ahead.1080p.WEB-DL.H.264.mp4",
        size: 1400 * MB,
        state: "running",
        fraction: 0.5,
        resumable: true,
        metadata: "1080p · WEB-DL · H.264",
      },
      {
        name: "Rust in Production.1080p.WEB-DL.H.264.mp4",
        size: 900 * MB,
        state: "queued",
        metadata: "1080p · WEB-DL · H.264",
      },
      {
        name: "Lightning Talks Day 2.720p.mp4",
        size: 480 * MB,
        state: "stopped",
        enabled: false,
        fraction: 0.2,
        resumable: true,
        metadata: "720p · H.264",
      },
      {
        name: "Panel - Open Source Funding.1080p.mp4",
        size: 1200 * MB,
        state: "failed",
        failure: "HTTP 404 Not Found",
        metadata: "1080p · H.264",
      },
      {
        name: "Closing Remarks.mp4",
        size: 300 * MB,
        state: "stopped",
        enabled: false,
      },
    ],
  },
  {
    name: "Old Archive Dump",
    items: [
      {
        name: "scan-batch-0042.zip",
        size: 750 * MB,
        state: "running",
        fraction: 0.15,
        resumable: false,
      },
      {
        name: "website-mirror-2019.tar.gz",
        size: 3200 * MB,
        state: "failed",
        failure: "Connection reset by peer",
        fraction: 0.77,
        resumable: true,
      },
    ],
  },
  {
    name: "Season 2",
    items: Array.from({ length: 6 }, (_, n) => ({
      name: `Show.S02E0${n + 1}.1080p.WEB.x264.mkv`,
      size: 1400 * MB,
      state: "queued" as const,
      metadata: "1080p · WEB · x264",
    })),
  },
  {
    name: "Holiday Photos",
    items: [
      {
        name: "IMG_2041.jpg",
        size: 4_200_000,
        state: "completed",
        resumable: true,
      },
      {
        name: "IMG_2042.jpg",
        size: 3_900_000,
        state: "completed",
        resumable: true,
      },
      {
        name: "Trip Notes.pdf",
        size: 1_100_000,
        state: "completed",
        resumable: true,
      },
      {
        name: "Wallpaper Pack.zip",
        size: 48_000_000,
        state: "completed",
        resumable: true,
      },
    ],
  },
];

interface MockItem {
  id: string;
  url: string;
  filename: string;
  state: ItemStateDto;
  enabled: boolean;
  resumable: boolean | null;
  total: number;
  /** Bytes done in each of the `SEGMENTS` equal slices of the file. */
  segments: number[];
  rate: number;
  history: number[];
  metadata: string | null;
}

interface MockPackage {
  id: string;
  name: string;
  items: MockItem[];
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
let nextId = 0;
const newId = () =>
  `00000000-0000-4000-8000-${String(++nextId).padStart(12, "0")}`;

const invalid = (argument: string, message: string) =>
  new BridgeError({ kind: "invalidArgument", argument, message });

const SETTING_RANGES: [keyof SettingsDto, number, number][] = [
  ["maxConcurrent", 1, 20],
  ["segmentsPerItem", 1, 64],
  ["globalMaxConnections", 1, 256],
  ["maxConnectionsPerHost", 1, 64],
  ["minSegmentSizeMb", 1, 100],
  ["maxPlaylistVideos", 10, 200],
];

/** The checks `SettingsStore::update` makes in Rust. */
function validateSettings(settings: SettingsDto) {
  for (const [key, min, max] of SETTING_RANGES) {
    const value = settings[key] as number;
    if (!(value >= min && value <= max))
      throw invalid(key, `must be between ${min} and ${max}`);
  }
  if (!RESOLUTION_CHOICES.includes(settings.ytMaxHeight as 1080))
    throw invalid("ytMaxHeight", "not a resolution on offer");
  if (!(settings.allowAv1 || settings.allowVp9 || settings.allowH264))
    throw invalid("allowAv1", "keep at least one video codec");
  if (!(settings.allowMp4 || settings.allowWebm))
    throw invalid("allowMp4", "keep at least one container");
  if (!(settings.allowOpus || settings.allowAac))
    throw invalid("allowOpus", "keep at least one audio codec");
}

function parseIds(ids: string[], argument = "id"): string[] {
  const seen = new Set<string>();
  for (const id of ids) {
    if (!UUID.test(id.trim()))
      throw invalid(argument, "that is not a valid item id");
    seen.add(id.trim().toLowerCase());
  }
  return [...seen];
}

function lastSegment(url: URL): string {
  const segment = url.pathname.split("/").filter(Boolean).at(-1) ?? "";
  const decoded = decodeURIComponent(segment).replace(/[\\/]/g, "_");
  return decoded && decoded !== ".." && decoded !== "." ? decoded : "download";
}

function parseUrl(text: string): URL {
  const trimmed = text.trim();
  if (!trimmed) throw invalid("url", "the address is empty");
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    throw invalid("url", "that is not a valid web address");
  }
  if (url.protocol !== "http:" && url.protocol !== "https:")
    throw invalid("url", "only http and https addresses can be downloaded");
  if (!url.hostname) throw invalid("url", "the address has no host");
  return url;
}

const done = (item: MockItem) => item.segments.reduce((a, b) => a + b, 0);

/** The completed byte ranges, with neighbouring slices merged. */
function rangesOf(item: MockItem): { start: number; end: number }[] {
  const size = item.total / SEGMENTS;
  const out: { start: number; end: number }[] = [];
  item.segments.forEach((bytes, k) => {
    if (bytes <= 0) return;
    const start = k * size;
    const end = start + bytes;
    const last = out.at(-1);
    if (last && Math.abs(last.end - start) < 1e-6) last.end = end;
    else out.push({ start, end });
  });
  return out;
}

function fillSequentially(item: MockItem, bytes: number) {
  const size = item.total / SEGMENTS;
  let left = Math.min(bytes, item.total);
  item.segments = item.segments.map(() => {
    const take = Math.min(size, left);
    left -= take;
    return take;
  });
}

const rateFor = (total: number) =>
  Math.min(12 * MB, Math.max(1.5 * MB, total / 80));

interface Palette {
  isDark: boolean;
  surfacePrimary: string;
  surfaceSecondary: string;
  surfaceTertiary: string;
  textPrimary: string;
  textSecondary: string;
  textTertiary: string;
  accent: string;
  border: string;
  statusOnline: string;
  statusFaulty: string;
  statusOffline: string;
  statusFailed: string;
  progressFill: string;
  completedSegmentFill: string;
  activeHeadTint: string;
  graphStroke: string;
  graphAverageStroke: string;
  sidebarBackground: string;
  selectionTint: string;
}

// Copies of the bundled theme files (sdm-core/resources/themes), so the mock
// paints like the real thing without reaching outside the web project.
const PALETTES: Record<string, Palette> = {
  dark: {
    isDark: true,
    surfacePrimary: "#1C1C1E",
    surfaceSecondary: "#2C2C2E",
    surfaceTertiary: "#3A3A3C",
    textPrimary: "#FFFFFF",
    textSecondary: "#DCDCE1",
    textTertiary: "#C4C4CA",
    accent: "#0A84FF",
    border: "#48484A",
    statusOnline: "#30D158",
    statusFaulty: "#FF9F0A",
    statusOffline: "#8E8E93",
    statusFailed: "#FF453A",
    progressFill: "#0A84FF",
    completedSegmentFill: "#32D74B",
    activeHeadTint: "#64D2FF",
    graphStroke: "#0A84FF",
    graphAverageStroke: "#5E5CE6",
    sidebarBackground: "#000000",
    selectionTint: "#0A84FF",
  },
  light: {
    isDark: false,
    surfacePrimary: "#FFFFFF",
    surfaceSecondary: "#F5F5F7",
    surfaceTertiary: "#E8E8ED",
    textPrimary: "#17171A",
    textSecondary: "#47474B",
    textTertiary: "#5F5F63",
    accent: "#007AFF",
    border: "#D2D2D7",
    statusOnline: "#34C759",
    statusFaulty: "#FF9500",
    statusOffline: "#8E8E93",
    statusFailed: "#FF3B30",
    progressFill: "#007AFF",
    completedSegmentFill: "#30D158",
    activeHeadTint: "#32ADE6",
    graphStroke: "#007AFF",
    graphAverageStroke: "#5E5CE6",
    sidebarBackground: "#F5F5F7",
    selectionTint: "#0A84FF",
  },
  "deep-purple": {
    isDark: true,
    surfacePrimary: "#1A0F2E",
    surfaceSecondary: "#241640",
    surfaceTertiary: "#332057",
    textPrimary: "#FFFFFF",
    textSecondary: "#DCDCE1",
    textTertiary: "#C4C4CA",
    accent: "#B084F5",
    border: "#332057",
    statusOnline: "#4ADE9C",
    statusFaulty: "#F5C563",
    statusOffline: "#8875A8",
    statusFailed: "#FF6B8A",
    progressFill: "#B084F5",
    completedSegmentFill: "#4ADE9C",
    activeHeadTint: "#7FE0D8",
    graphStroke: "#B084F5",
    graphAverageStroke: "#E894D4",
    sidebarBackground: "#1A0F2E",
    selectionTint: "#B084F5",
  },
};

function paletteCss(p: Palette): string {
  const roles: [string, string][] = [
    ["surface-primary", p.surfacePrimary],
    ["surface-secondary", p.surfaceSecondary],
    ["surface-tertiary", p.surfaceTertiary],
    ["text-primary", p.textPrimary],
    ["text-secondary", p.textSecondary],
    ["text-tertiary", p.textTertiary],
    ["accent", p.accent],
    ["border", p.border],
    ["status-online", p.statusOnline],
    ["status-faulty", p.statusFaulty],
    ["status-offline", p.statusOffline],
    ["status-failed", p.statusFailed],
    ["progress-fill", p.progressFill],
    ["completed-segment-fill", p.completedSegmentFill],
    ["active-head-tint", p.activeHeadTint],
    ["graph-stroke", p.graphStroke],
    ["graph-average-stroke", p.graphAverageStroke],
    ["sidebar-background", p.sidebarBackground],
    ["selection-tint", p.selectionTint],
  ];
  const body = roles
    .map(([name, value]) => `  --${name}: ${value};`)
    .join("\n");
  return `:root {\n${body}\n  color-scheme: ${p.isDark ? "dark" : "light"};\n}\n`;
}

/** An in-memory backend so the UI runs in a plain browser. It follows the
 * engine's rules (three at a time, moves refuse running items, a package that
 * loses its last item disappears) closely enough to build the UI against. */
/** `?tray=running`: three downloads, four waiting links. */
export function demoTrayStatus(): TrayStatus {
  const MB = 1024 * 1024;
  return {
    runningCount: 3,
    nonResumableCount: 0,
    bytesPerSecond: 12.4 * MB,
    history: Array.from(
      { length: 64 },
      (_, i) => (6 + 5 * Math.sin(i / 5)) * MB,
    ),
    overallFraction: 0.4,
    items: [
      {
        name: "ubuntu-24.04-desktop-amd64.iso",
        fraction: 0.62,
        bytesPerSecond: 8.1 * MB,
      },
      {
        name: "Lecture 04 - Memory.mp4",
        fraction: 0.18,
        bytesPerSecond: 3.9 * MB,
      },
      { name: "dataset-v2.tar.zst", fraction: 0.91, bytesPerSecond: 0.4 * MB },
    ],
    linksWaiting: 4,
  };
}

export class MockBridge implements Bridge {
  /** Item ids `openItem` opened (finished files only). */
  readonly opened: string[] = [];
  /** Item or package ids `revealItem` / `revealPackage` was asked about. */
  readonly revealed: string[] = [];

  #packages: MockPackage[] = [];
  #listener: ((patch: Patch) => void) | null = null;
  #seq = 0;
  #timer: ReturnType<typeof setInterval> | null = null;
  #settings: SettingsDto = {
    ...defaultSettings(),
    downloadFolder: "/Users/mock/Downloads",
  };
  #themeListeners = new Set<(themeId: string | null) => void>();
  /** How often `openSettings` and `checkBinariesNow` were called. */
  settingsOpens = 0;
  binaryChecks = 0;
  /** What `chromeExtensionStatus` reports, and how often the Chrome setup calls were made. */
  chromeStatus: ChromeExtensionDto = {
    state: "notConnected",
    installedVersion: null,
    bundledVersion: "2.0.0",
    cookiesSyncedSecondsAgo: null,
    extensionFolder:
      "/Users/mock/Library/Application Support/SDM/ChromeExtension",
    chromeInstalled: true,
  };
  chromePrepares = 0;
  chromeExtensionsOpens = 0;
  chromeFolderReveals = 0;
  chromeSetupOpens = 0;
  /** What `binariesStatus` reports; tests and the browser preview change it. */
  binaries: BinariesStatusDto = {
    bundled: false,
    downloading: false,
    ytDlpVersion: "2026.10.21",
    latestKnown: null,
    ffmpegVersion: null,
    qjsVersion: null,
    lastError: null,
  };
  #globalHistory: number[] = [];
  #detail = new Map<string, Set<string>>();
  #vibrancy: boolean;
  #maxRunning: number;
  #grabber: MockGrabber;
  #lastGrabber = "";

  constructor(options: MockOptions = {}) {
    this.#vibrancy = options.vibrancy ?? false;
    this.#maxRunning = options.maxRunning ?? MAX_RUNNING;
    this.#grabber = new MockGrabber(
      (name, _note, items, start) => this.#addPackage(name, items, start),
      options.grabber?.ffmpeg ?? true,
    );
    if (options.grabber?.text !== undefined)
      this.#grabber.ingestText(
        options.grabber.text === "demo" ? DEMO_TEXT : options.grabber.text,
      );
    for (const seed of options.seed ?? REFERENCE_SEED) {
      this.#packages.push({
        id: newId(),
        name: seed.name,
        items: seed.items.map((s) => this.#fromSeed(s)),
      });
    }
    this.#schedule();
  }

  #fromSeed(seed: MockSeedItem): MockItem {
    const item: MockItem = {
      id: newId(),
      url: `https://example.com/files/${encodeURIComponent(seed.name)}`,
      filename: seed.name,
      state: { kind: "queued" },
      enabled: seed.enabled ?? true,
      resumable: seed.resumable ?? null,
      total: seed.size,
      segments: Array(SEGMENTS).fill(0),
      rate: rateFor(seed.size),
      history: [],
      metadata: seed.metadata ?? null,
    };
    if (seed.state === "completed") fillSequentially(item, item.total);
    else if (seed.fraction) fillSequentially(item, item.total * seed.fraction);
    item.state =
      seed.state === "failed"
        ? { kind: "failed", reason: seed.failure ?? "Failed" }
        : // Whether it runs is the scheduler's call (three at a time).
          { kind: seed.state === "running" ? "queued" : seed.state };
    return item;
  }

  /** The grabber hands a package over: its items join the list as queued or stopped. */
  #addPackage(
    name: string,
    items: { url: string; filename: string; size: number | null }[],
    start: boolean,
  ) {
    this.#packages.push({
      id: newId(),
      name,
      items: items.map((handed) => {
        const item = this.#fromSeed({
          name: handed.filename,
          size: handed.size ?? 200 * MB,
          state: start ? "queued" : "stopped",
        });
        item.url = handed.url;
        return item;
      }),
    });
    this.#schedule();
  }

  /** Publishes the grabber (or any other change) and refreshes the knowledge of known downloads. */
  #afterGrabber() {
    this.#grabber.setKnownUrls(this.#allItems().map((item) => item.url));
    this.#emit(false, true);
  }

  // MARK: Bridge

  async attach(onPatch: (patch: Patch) => void) {
    this.#listener = onPatch;
    this.#emit(true);
    this.#timer ??= setInterval(() => this.#tick(), TICK_MS);
  }

  async fullState() {
    this.#emit(true);
  }

  async addUrl(url: string, packageName?: string) {
    const parsed = parseUrl(url);
    const filename = lastSegment(parsed);
    const item = this.#fromSeed({
      name: filename,
      size: 200 * MB,
      state: "queued",
    });
    item.url = parsed.href;
    this.#packages.push({
      id: newId(),
      name: packageName?.trim() || filename,
      items: [item],
    });
    this.#schedule();
    this.#emit(false, true);
  }

  async startItems(ids: string[]) {
    this.#each(ids, (item) => {
      if (item.state.kind === "stopped" && item.enabled)
        item.state = { kind: "queued" };
    });
  }

  async stopItems(ids: string[]) {
    this.#each(ids, (item) => this.#stop(item));
  }

  async retryItems(ids: string[]) {
    this.#each(ids, (item) => {
      if (item.state.kind !== "failed") return;
      item.state = { kind: "queued" };
      item.enabled = true;
    });
  }

  async retryMuxItems(ids: string[]) {
    // The mock has no muxed items; nothing to retry.
    this.#each(ids, () => {});
  }

  async resetItems(ids: string[]) {
    this.#each(ids, (item) => {
      if (item.state.kind === "running") return;
      const wasQueued = item.state.kind === "queued";
      item.segments = Array(SEGMENTS).fill(0);
      item.resumable = null;
      item.state = { kind: item.enabled && wasQueued ? "queued" : "stopped" };
    });
  }

  async setItemsEnabled(ids: string[], enabled: boolean) {
    this.#each(ids, (item) => {
      item.enabled = enabled;
      if (!enabled) this.#stop(item);
    });
  }

  async removeItems(ids: string[], _deleteFiles: boolean) {
    const wanted = new Set(parseIds(ids));
    if (wanted.size === 0) return;
    for (const pkg of this.#packages)
      pkg.items = pkg.items.filter((item) => !wanted.has(item.id));
    for (const open of this.#detail.values())
      for (const id of wanted) open.delete(id);
    this.#dropEmptyPackages();
    this.#schedule();
    this.#emit(false, true);
  }

  async removePackage(id: string, _deleteFiles: boolean) {
    const [wanted] = parseIds([id]);
    const pkg = this.#packages.find((p) => p.id === wanted);
    if (pkg) {
      const gone = new Set(pkg.items.map((i) => i.id));
      for (const open of this.#detail.values())
        for (const item of gone) open.delete(item);
      this.#packages = this.#packages.filter((p) => p !== pkg);
    }
    this.#schedule();
    this.#emit(false, true);
  }

  async moveItems(ids: string[], packageId: string, atIndex?: number) {
    const wanted = parseIds(ids);
    const [target] = parseIds([packageId]);
    if (wanted.length === 0) return;
    this.#move(wanted, target, atIndex);
    this.#emit(false, true);
  }

  async moveItemsToNewPackage(ids: string[], name: string) {
    const wanted = parseIds(ids);
    const trimmed = name.trim();
    if (!trimmed) throw invalid("name", "the package name is empty");
    if (wanted.length === 0) return;
    const pkg: MockPackage = { id: newId(), name: trimmed, items: [] };
    this.#packages.push(pkg);
    if (!this.#move(wanted, pkg.id, 0)) this.#packages.pop();
    this.#emit(false, true);
  }

  async reorderItems(ids: string[], packageId: string) {
    const wanted = parseIds(ids);
    const [target] = parseIds([packageId]);
    const pkg = this.#packages.find((p) => p.id === target);
    if (pkg) {
      const named = wanted
        .map((id) => pkg.items.find((i) => i.id === id))
        .filter((i): i is MockItem => i !== undefined);
      pkg.items = [...named, ...pkg.items.filter((i) => !named.includes(i))];
    }
    this.#emit(false, true);
  }

  async reorderPackages(ids: string[]) {
    const wanted = parseIds(ids);
    const named = wanted
      .map((id) => this.#packages.find((p) => p.id === id))
      .filter((p): p is MockPackage => p !== undefined);
    this.#packages = [
      ...named,
      ...this.#packages.filter((p) => !named.includes(p)),
    ];
    this.#emit(false, true);
  }

  async describeDeletion(target: DeletionTarget): Promise<DeletionInfo> {
    let chosen: MockItem[];
    if (target.kind === "package") {
      const [id] = parseIds([target.id]);
      const pkg = this.#packages.find((p) => p.id === id);
      if (!pkg)
        return { title: "Delete Package From Disk?", totalBytes: 0, names: [] };
      chosen = pkg.items;
    } else {
      const wanted = new Set(parseIds(target.ids));
      chosen = this.#packages
        .flatMap((p) => p.items)
        .filter((i) => wanted.has(i.id));
    }
    const names: string[] = [];
    let totalBytes = 0;
    for (const item of chosen) {
      if (item.state.kind === "completed") {
        names.push(item.filename);
        totalBytes += item.total;
      } else if (done(item) > 0) {
        names.push(`${item.filename}.incomplete`);
        totalBytes += done(item);
      }
    }
    names.sort();
    return {
      title:
        names.length === 0
          ? "Remove From List?"
          : "Remove From List And Delete Files?",
      totalBytes,
      names,
    };
  }

  async openItem(id: string) {
    const [wanted] = parseIds([id]);
    const item = this.#allItems().find((i) => i.id === wanted);
    if (item?.state.kind === "completed") this.opened.push(item.id);
  }

  async revealItem(id: string) {
    const [wanted] = parseIds([id]);
    if (this.#allItems().some((i) => i.id === wanted))
      this.revealed.push(wanted);
  }

  async revealPackage(id: string) {
    const [wanted] = parseIds([id]);
    if (this.#packages.some((p) => p.id === wanted)) this.revealed.push(wanted);
  }

  async pauseAll() {
    for (const item of this.#allItems()) this.#stop(item);
    this.#emit(false, true);
  }

  async resumeAll() {
    for (const item of this.#allItems())
      if (item.state.kind === "stopped" && item.enabled)
        item.state = { kind: "queued" };
    this.#schedule();
    this.#emit(false, true);
  }

  async setListVisible(visible: boolean) {
    if (visible) this.#emit(true);
  }

  async setDetailItems(owner: string, ids: string[]) {
    const parsed = parseIds(ids);
    if (parsed.length === 0) this.#detail.delete(owner);
    else this.#detail.set(owner, new Set(parsed));
    this.#emit(false);
  }

  async getSettings() {
    return { ...this.#settings };
  }

  async updateSettings(settings: SettingsDto) {
    // Like the Rust store: every field is required, and a null folder keeps
    // the stored one.
    const next: SettingsDto = {
      ...settings,
      downloadFolder: this.#settings.downloadFolder,
    };
    if (settings.downloadFolder != null) {
      const folder = settings.downloadFolder.trim();
      if (!folder) throw invalid("downloadFolder", "the folder is empty");
      next.downloadFolder = folder;
    }
    validateSettings(next);
    this.#settings = next;
  }

  async binariesStatus(): Promise<BinariesStatusDto> {
    return { ...this.binaries };
  }

  async checkBinariesNow() {
    this.binaryChecks += 1;
  }

  async listThemes() {
    return Object.keys(PALETTES).map((id) => ({
      id,
      name: id
        .split("-")
        .map((word) => word[0].toUpperCase() + word.slice(1))
        .join(" "),
    }));
  }

  async openSettings() {
    this.settingsOpens += 1;
  }

  async chromeExtensionStatus(): Promise<ChromeExtensionDto> {
    return { ...this.chromeStatus };
  }

  async prepareChromeExtension() {
    this.chromePrepares += 1;
    return this.chromeStatus.extensionFolder;
  }

  async openChromeExtensions() {
    this.chromeExtensionsOpens += 1;
  }

  async revealChromeExtensionFolder() {
    this.chromeFolderReveals += 1;
  }

  async openChromeSetup() {
    this.chromeSetupOpens += 1;
  }

  async broadcastTheme(themeId: string | null) {
    for (const listener of [...this.#themeListeners]) listener(themeId);
  }

  #trayStatus: TrayStatus = {
    runningCount: 0,
    nonResumableCount: 0,
    bytesPerSecond: 0,
    history: [],
    overallFraction: 0,
    items: [],
    linksWaiting: 0,
  };
  #trayListeners = new Set<(status: TrayStatus) => void>();
  /** How many times the popover asked to quit. */
  quitRequests = 0;

  async trayStatus() {
    return this.#trayStatus;
  }

  onTrayStatus(handler: (status: TrayStatus) => void) {
    this.#trayListeners.add(handler);
    return () => void this.#trayListeners.delete(handler);
  }

  /** What the backend does on a tick while the popover is open. */
  setTrayStatus(status: TrayStatus) {
    this.#trayStatus = status;
    for (const handler of this.#trayListeners) handler(status);
  }

  async requestQuit() {
    this.quitRequests += 1;
  }

  #navigateListeners = new Set<(route: string) => void>();

  async showMain(route?: string) {
    if (route !== undefined) this.emitNavigate(route);
  }

  onNavigate(handler: (route: string) => void) {
    this.#navigateListeners.add(handler);
    return () => void this.#navigateListeners.delete(handler);
  }

  /** What the backend does when a notification is clicked. */
  emitNavigate(route: string) {
    for (const handler of this.#navigateListeners) handler(route);
  }

  onThemePreview(handler: (themeId: string | null) => void) {
    this.#themeListeners.add(handler);
    return () => void this.#themeListeners.delete(handler);
  }

  async vibrancyActive() {
    return this.#vibrancy;
  }

  async themeCss(themeId?: string) {
    const id = themeId ?? "dark";
    const palette = PALETTES[id];
    if (!palette) throw invalid("themeId", `no theme named ${id}`);
    return paletteCss(palette);
  }

  // MARK: Internals

  #allItems(): MockItem[] {
    return this.#packages.flatMap((p) => p.items);
  }

  /** Applies `change` to each existing item named in `ids`, then reschedules and publishes. */
  #each(ids: string[], change: (item: MockItem) => void) {
    const wanted = new Set(parseIds(ids));
    if (wanted.size === 0) return;
    for (const item of this.#allItems()) if (wanted.has(item.id)) change(item);
    this.#schedule();
    this.#emit(false, true);
  }

  #stop(item: MockItem) {
    if (item.state.kind === "running" || item.state.kind === "queued") {
      item.state = { kind: "stopped" };
      item.rate = rateFor(item.total);
    }
  }

  #dropEmptyPackages() {
    this.#packages = this.#packages.filter((p) => p.items.length > 0);
  }

  /** The engine's batch move: all or nothing, and a running item refuses it. */
  #move(ids: string[], packageId: string, atIndex?: number): boolean {
    const target = this.#packages.find((p) => p.id === packageId);
    if (!target) return false;
    const byId = new Map(this.#allItems().map((i) => [i.id, i]));
    const moving = ids.map((id) => byId.get(id));
    if (moving.some((i) => !i || i.state.kind === "running")) return false;
    const batch = moving as MockItem[];
    const index = atIndex ?? target.items.length;
    const before = target.items
      .slice(0, Math.min(index, target.items.length))
      .filter((i) => batch.includes(i)).length;
    for (const pkg of this.#packages)
      pkg.items = pkg.items.filter((i) => !batch.includes(i));
    const at = Math.max(0, Math.min(index - before, target.items.length));
    target.items.splice(at, 0, ...batch);
    this.#dropEmptyPackages();
    return true;
  }

  /** Starts queued, enabled items while there is a free slot. */
  #schedule() {
    const items = this.#allItems();
    let running = items.filter((i) => i.state.kind === "running").length;
    for (const item of items) {
      if (running >= this.#maxRunning) break;
      if (item.state.kind === "queued" && item.enabled) {
        item.state = { kind: "running" };
        running += 1;
      }
    }
  }

  #tick() {
    let structural = false;
    let globalSpeed = 0;
    for (const item of this.#allItems()) {
      const running = item.state.kind === "running";
      const speed = running ? item.rate * (0.9 + Math.random() * 0.2) : 0;
      if (running) {
        const open = item.segments
          .map((bytes, k) => ({ bytes, k }))
          .filter(({ bytes }) => bytes < item.total / SEGMENTS - 1e-6);
        const share = (speed * TICK_MS) / 1000 / Math.max(1, open.length);
        for (const { k } of open)
          item.segments[k] = Math.min(
            item.total / SEGMENTS,
            item.segments[k] + share,
          );
        if (done(item) >= item.total - 1e-6) {
          fillSequentially(item, item.total);
          item.state = { kind: "completed" };
          structural = true;
        }
      }
      globalSpeed += speed;
      item.history = [...item.history, speed].slice(-FULL_HISTORY);
    }
    this.#globalHistory = [...this.#globalHistory, globalSpeed].slice(
      -FULL_HISTORY,
    );
    this.#grabber.tick();
    if (structural) this.#schedule();
    this.#emit(false, structural);
  }

  #structure(): Structure {
    return {
      packages: this.#packages.map((pkg) => ({
        id: pkg.id,
        name: pkg.name,
        priority: 2,
        folder: `${this.#settings.downloadFolder}/${pkg.name}`,
        items: pkg.items.map((item) => ({
          id: item.id,
          url: item.url,
          filename: item.filename,
          state: item.state,
          isEnabled: item.enabled,
          isResumable: item.resumable,
          isAssembling: false,
          assembly: "none" as const,
          fileMissing: false,
          metadata: item.metadata,
        })),
      })),
    };
  }

  #figures(item: MockItem): ItemFigures {
    const running = item.state.kind === "running";
    const open = item.segments.filter(
      (b) => b < item.total / SEGMENTS - 1e-6,
    ).length;
    return {
      id: item.id,
      completed: rangesOf(item),
      completedBytes: done(item),
      totalBytes: item.total,
      activeSegments: running ? open : 0,
      configuredSegments: SEGMENTS,
      bytesPerSecond: running ? (item.history.at(-1) ?? 0) : 0,
      speedHistory: item.history.slice(-ROW_HISTORY),
      retryHoldSeconds: null,
      failedAttemptCount: null,
      remainingAttempts: null,
      lastFailureReason: null,
      checkpointFailure: null,
    };
  }

  #packageFigures(pkg: MockPackage): PackageFigures {
    const completedBytes = pkg.items.reduce((sum, i) => sum + done(i), 0);
    const totalBytes = pkg.items.reduce((sum, i) => sum + i.total, 0);
    const speed = pkg.items.reduce(
      (sum, i) =>
        sum + (i.state.kind === "running" ? (i.history.at(-1) ?? 0) : 0),
      0,
    );
    const history: number[] = [];
    for (let n = 1; n <= ROW_HISTORY; n++)
      history.unshift(
        pkg.items.reduce((sum, i) => sum + (i.history.at(-n) ?? 0), 0),
      );
    const remaining = pkg.items
      .filter((i) => i.state.kind === "running" || i.state.kind === "queued")
      .reduce((sum, i) => sum + (i.total - done(i)), 0);
    return {
      id: pkg.id,
      completedBytes,
      totalBytes,
      bytesPerSecond: speed,
      speedHistory: history,
      etaSeconds: speed > 0 ? remaining / speed : 0,
    };
  }

  #stats(): EngineStats {
    const items = this.#allItems();
    const running = items.filter((i) => i.state.kind === "running");
    const live = items.filter((i) => i.state.kind !== "failed");
    const total = live.reduce((sum, i) => sum + i.total, 0);
    const completed = live.reduce((sum, i) => sum + done(i), 0);
    return {
      bytesPerSecond: running.reduce(
        (sum, i) => sum + (i.history.at(-1) ?? 0),
        0,
      ),
      history: this.#globalHistory,
      runningCount: running.length,
      queuedCount: items.filter((i) => i.state.kind === "queued").length,
      overallFraction: total > 0 ? completed / total : 0,
      ringCompletedBytes: completed,
      ringTotalBytes: total,
      waitingItemCount: 0,
      waitingPackageCount: 0,
    };
  }

  // MARK: Link grabber

  async grabberIngestText(text: string) {
    this.#grabber.ingestText(text);
    this.#afterGrabber();
  }
  async grabberRemoveLinks(ids: string[]) {
    this.#grabber.removeLinks(ids);
    this.#afterGrabber();
  }
  async grabberRemovePackage(id: string) {
    this.#grabber.removePackage(id);
    this.#afterGrabber();
  }
  async grabberClear() {
    this.#grabber.clear();
    this.#afterGrabber();
  }
  async grabberMoveLink(id: string, packageId: string) {
    this.#grabber.moveLink(id, packageId);
    this.#afterGrabber();
  }
  async grabberRenamePackage(id: string, name: string) {
    this.#grabber.renamePackage(id, name);
    this.#afterGrabber();
  }
  async grabberMergePackages(source: string, destination: string) {
    this.#grabber.mergePackages(source, destination);
    this.#afterGrabber();
  }
  async grabberSplitPackage(id: string) {
    this.#grabber.splitPackage(id);
    this.#afterGrabber();
  }
  async grabberRecheck(ids: string[]) {
    this.#grabber.recheck(ids);
    this.#afterGrabber();
  }
  async grabberRecheckFailed() {
    this.#grabber.recheckFailed();
    this.#afterGrabber();
  }
  async grabberCancelChecks() {
    this.#grabber.cancelChecks();
    this.#afterGrabber();
  }
  async grabberSetFormat(id: string, optionId: string) {
    this.#grabber.setFormat(id, optionId);
    this.#afterGrabber();
  }
  async grabberCopyUrl(id: string) {
    return this.#grabber.copyUrl(id);
  }
  async grabberAddToDownloads(target: HandoffTarget, start: boolean) {
    this.#grabber.addToDownloads(target, start);
    this.#afterGrabber();
  }

  #grabberIfChanged(full: boolean): GrabberView | null {
    this.#grabber.setKnownUrls(this.#allItems().map((item) => item.url));
    const view = this.#grabber.view();
    const text = JSON.stringify(view);
    if (!full && text === this.#lastGrabber) return null;
    this.#lastGrabber = text;
    return view;
  }

  #emit(full: boolean, structural = false) {
    if (!this.#listener) return;
    this.#seq = (this.#seq + 1) >>> 0;
    const open = new Set([...this.#detail.values()].flatMap((ids) => [...ids]));
    const all = this.#allItems();
    this.#listener({
      seq: this.#seq,
      full,
      structure: full || structural ? this.#structure() : null,
      items: all.map((item) => this.#figures(item)),
      packages: this.#packages.map((pkg) => this.#packageFigures(pkg)),
      stats: this.#stats(),
      histories: all
        .filter((item) => open.has(item.id))
        .map((item) => ({ id: item.id, speedHistory: item.history })),
      grabber: this.#grabberIfChanged(full),
    });
  }
}

/** `count` items spread over ten packages, for load checks (`?simulate=500:50`). */
export function loadSeed(count: number): MockSeed[] {
  const packages = 10;
  return Array.from({ length: packages }, (_, p) => ({
    name: `Simulated ${p + 1}`,
    items: Array.from({ length: Math.ceil(count / packages) }, (_, n) => ({
      name: `file-${p * Math.ceil(count / packages) + n}.bin`,
      size: 64 * MB,
      state: "running" as const,
      fraction: 0.01,
      resumable: true,
    })),
  }));
}
