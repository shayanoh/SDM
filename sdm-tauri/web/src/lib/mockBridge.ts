import type {
  BinariesStatusDto,
  EngineStats,
  ItemFigures,
  ItemStateDto,
  PackageFigures,
  Patch,
  SettingsDto,
  Structure,
} from "../bindings";
import { BridgeError, type Bridge } from "./bridge";

const TICK_MS = 200;
const HISTORY = 64;

interface MockItem {
  id: string;
  url: string;
  filename: string;
  state: ItemStateDto;
  total: number;
  done: number;
  rate: number;
  history: number[];
}

let nextId = 0;
const newId = () =>
  `00000000-0000-4000-8000-${String(++nextId).padStart(12, "0")}`;

function lastSegment(url: URL): string {
  const segment = url.pathname.split("/").filter(Boolean).at(-1) ?? "";
  const decoded = decodeURIComponent(segment).replace(/[\\/]/g, "_");
  return decoded && decoded !== ".." && decoded !== "." ? decoded : "download";
}

function parseUrl(text: string): URL {
  const invalid = (message: string) =>
    new BridgeError({ kind: "invalidArgument", argument: "url", message });
  const trimmed = text.trim();
  if (!trimmed) throw invalid("the address is empty");
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    throw invalid("that is not a valid web address");
  }
  if (url.protocol !== "http:" && url.protocol !== "https:")
    throw invalid("only http and https addresses can be downloaded");
  if (!url.hostname) throw invalid("the address has no host");
  return url;
}

const MB = 1024 * 1024;

/** An in-memory backend so the UI runs in a plain browser. */
export class MockBridge implements Bridge {
  #items: MockItem[] = [];
  #listener: ((patch: Patch) => void) | null = null;
  #seq = 0;
  #timer: ReturnType<typeof setInterval> | null = null;
  #settings: SettingsDto = { downloadFolder: "/Users/mock/Downloads/SDM" };

  constructor() {
    this.#items = [
      this.#make("https://example.com/files/ubuntu.iso", 4000 * MB, 90 * MB),
      this.#make("https://example.com/files/dataset.zip", 800 * MB, 25 * MB),
      this.#make("https://example.com/files/notes.pdf", 12 * MB, 1 * MB),
    ];
    this.#items[2].state = { kind: "stopped" };
  }

  #make(url: string, total: number, rate: number): MockItem {
    return {
      id: newId(),
      url,
      filename: lastSegment(new URL(url)),
      state: { kind: "running" },
      total,
      done: 0,
      rate,
      history: [],
    };
  }

  async attach(onPatch: (patch: Patch) => void) {
    this.#listener = onPatch;
    this.#emit(true, true);
    this.#timer ??= setInterval(() => this.#tick(), TICK_MS);
  }

  async fullState() {
    this.#emit(true, true);
  }

  async addUrl(url: string, _packageName?: string) {
    const parsed = parseUrl(url);
    this.#items.push(this.#make(parsed.href, 200 * MB, 20 * MB));
    this.#emit(false, true);
  }

  async startItem(id: string) {
    this.#set(id, { kind: "running" });
  }

  async stopItem(id: string) {
    this.#set(id, { kind: "stopped" });
  }

  async retryItem(id: string) {
    const item = this.#find(id);
    if (item) item.done = 0;
    this.#set(id, { kind: "running" });
  }

  async removeItem(id: string, _deleteFile: boolean) {
    this.#items = this.#items.filter((item) => item.id !== id);
    this.#emit(false, true);
  }

  async pauseAll() {
    for (const item of this.#items)
      if (item.state.kind === "running") item.state = { kind: "stopped" };
    this.#emit(false, true);
  }

  async resumeAll() {
    for (const item of this.#items)
      if (item.state.kind === "stopped") item.state = { kind: "running" };
    this.#emit(false, true);
  }

  async setListVisible(visible: boolean) {
    if (visible) this.#emit(true, true);
  }

  async setDetailItems(_owner: string, _ids: string[]) {}

  async getSettings() {
    return { ...this.#settings };
  }

  async updateSettings(settings: SettingsDto) {
    const folder = settings.downloadFolder.trim();
    if (!folder)
      throw new BridgeError({
        kind: "invalidArgument",
        argument: "downloadFolder",
        message: "the folder is empty",
      });
    this.#settings = { downloadFolder: folder };
  }

  async binariesStatus(): Promise<BinariesStatusDto> {
    return {
      ytDlpVersion: "mock",
      latestKnown: null,
      ffmpegVersion: null,
      qjsVersion: null,
      lastError: null,
    };
  }

  async themeCss(_themeId?: string) {
    return `:root {
  --surface-primary: #1c1c1e;
  --surface-secondary: #2c2c2e;
  --surface-tertiary: #3a3a3c;
  --text-primary: #f2f2f7;
  --text-secondary: #aeaeb2;
  --text-tertiary: #8e8e93;
  --accent: #0a84ff;
  --border: #48484a;
  --status-failed: #ff453a;
  --progress-fill: #0a84ff;
  --completed-segment-fill: #30d158;
  --graph-stroke: #64d2ff;
  --selection-tint: #0a84ff33;
  color-scheme: dark;
}
`;
  }

  #find(id: string) {
    return this.#items.find((item) => item.id === id);
  }

  #set(id: string, state: ItemStateDto) {
    const item = this.#find(id);
    if (item) item.state = state;
    this.#emit(false, true);
  }

  #tick() {
    let structural = false;
    for (const item of this.#items) {
      const running = item.state.kind === "running";
      const speed = running ? item.rate : 0;
      if (running) {
        item.done = Math.min(
          item.total,
          item.done + (item.rate * TICK_MS) / 1000,
        );
        if (item.done >= item.total) {
          item.state = { kind: "completed" };
          item.rate = 0;
          structural = true;
        }
      }
      item.history = [...item.history, speed].slice(-HISTORY);
    }
    this.#emit(false, structural);
  }

  #structure(): Structure {
    return {
      packages: this.#items.map((item) => ({
        id: `p-${item.id}`,
        name: item.filename,
        priority: 2,
        items: [
          {
            id: item.id,
            url: item.url,
            filename: item.filename,
            state: item.state,
            isEnabled: item.state.kind !== "stopped",
            isResumable: true,
            isAssembling: false,
            fileMissing: false,
            metadata: null,
          },
        ],
      })),
    };
  }

  #figures(item: MockItem): ItemFigures {
    const running = item.state.kind === "running";
    return {
      id: item.id,
      completed: item.done > 0 ? [{ start: 0, end: item.done }] : [],
      completedBytes: item.done,
      totalBytes: item.total,
      activeSegments: running ? 4 : 0,
      configuredSegments: 8,
      bytesPerSecond: running ? item.rate : 0,
      speedHistory: item.history,
      retryHoldSeconds: null,
      failedAttemptCount: null,
      lastFailureReason: null,
      checkpointFailure: null,
    };
  }

  #packageFigures(item: MockItem): PackageFigures {
    const running = item.state.kind === "running";
    const speed = running ? item.rate : 0;
    return {
      id: `p-${item.id}`,
      completedBytes: item.done,
      totalBytes: item.total,
      bytesPerSecond: speed,
      speedHistory: item.history,
      etaSeconds: speed > 0 ? (item.total - item.done) / speed : 0,
    };
  }

  #stats(): EngineStats {
    const running = this.#items.filter((i) => i.state.kind === "running");
    const total = this.#items.reduce((sum, i) => sum + i.total, 0);
    const done = this.#items.reduce((sum, i) => sum + i.done, 0);
    return {
      bytesPerSecond: running.reduce((sum, i) => sum + i.rate, 0),
      history: [],
      runningCount: running.length,
      queuedCount: 0,
      overallFraction: total > 0 ? done / total : 0,
      ringCompletedBytes: done,
      ringTotalBytes: total,
      waitingItemCount: 0,
      waitingPackageCount: 0,
    };
  }

  #emit(full: boolean, structural: boolean) {
    if (!this.#listener) return;
    this.#seq = (this.#seq + 1) >>> 0;
    this.#listener({
      seq: this.#seq,
      full,
      structure: full || structural ? this.#structure() : null,
      items: this.#items.map((item) => this.#figures(item)),
      packages: this.#items.map((item) => this.#packageFigures(item)),
      stats: this.#stats(),
      histories: [],
    });
  }
}
