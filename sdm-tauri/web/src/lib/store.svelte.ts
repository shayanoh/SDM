import { SvelteMap } from "svelte/reactivity";
import type {
  EngineStats,
  ItemFigures,
  ItemStateDto,
  Patch,
  PackageFigures,
  StructureItem,
} from "../bindings";
import { averageRecentSpeed } from "./rowText";

/** The wire turns a non-finite `f64` into `null`; the UI reads that as zero. */
const num = (value: number | null): number => value ?? 0;
const nums = (values: (number | null)[]): number[] =>
  values.some((v) => v === null) ? values.map(num) : (values as number[]);
const ranges = (values: ItemFigures["completed"]) =>
  values.some((r) => r.start === null || r.end === null)
    ? values.map((r) => ({ start: num(r.start), end: num(r.end) }))
    : (values as { start: number; end: number }[]);

/** One row's data. Fields are `$state`, so a row re-renders only for what it reads. */
export class ItemModel {
  id: string;
  url = $state("");
  filename = $state("");
  state = $state.raw<ItemStateDto>({ kind: "queued" });
  isEnabled = $state(true);
  isResumable = $state<boolean | null>(null);
  isAssembling = $state(false);
  assembly = $state<"none" | "mux">("none");
  fileMissing = $state(false);
  metadata = $state<string | null>(null);

  completed = $state.raw<{ start: number; end: number }[]>([]);
  completedBytes = $state(0);
  totalBytes = $state<number | null>(null);
  activeSegments = $state(0);
  configuredSegments = $state(0);
  bytesPerSecond = $state(0);
  speedHistory = $state.raw<number[]>([]);
  retryHoldSeconds = $state<number | null>(null);
  failedAttemptCount = $state<number | null>(null);
  remainingAttempts = $state<number | null>(null);
  lastFailureReason = $state<string | null>(null);
  checkpointFailure = $state<string | null>(null);
  detailHistory = $state.raw<number[] | null>(null);

  /** Queued after a failure that still has attempts left (the engine sets a failed-attempt count). */
  get isRetrying(): boolean {
    return this.state.kind === "queued" && this.failedAttemptCount !== null;
  }

  constructor(id: string) {
    this.id = id;
  }

  applyStructure(s: StructureItem) {
    this.url = s.url;
    this.filename = s.filename;
    this.state = s.state;
    this.isEnabled = s.isEnabled;
    this.isResumable = s.isResumable;
    this.isAssembling = s.isAssembling;
    this.assembly = s.assembly;
    this.fileMissing = s.fileMissing;
    this.metadata = s.metadata;
  }

  applyFigures(f: ItemFigures) {
    this.completed = ranges(f.completed);
    this.completedBytes = num(f.completedBytes);
    this.totalBytes = f.totalBytes;
    this.activeSegments = f.activeSegments;
    this.configuredSegments = f.configuredSegments;
    this.bytesPerSecond = num(f.bytesPerSecond);
    this.speedHistory = nums(f.speedHistory);
    this.retryHoldSeconds = f.retryHoldSeconds;
    this.failedAttemptCount = f.failedAttemptCount;
    this.remainingAttempts = f.remainingAttempts;
    this.lastFailureReason = f.lastFailureReason;
    this.checkpointFailure = f.checkpointFailure;
  }
}

export class PackageModel {
  id: string;
  name = $state("");
  priority = $state(2);
  /** The package's absolute destination folder. */
  folder = $state("");
  completedBytes = $state(0);
  totalBytes = $state(0);
  bytesPerSecond = $state(0);
  speedHistory = $state.raw<number[]>([]);
  etaSeconds = $state(0);

  constructor(id: string) {
    this.id = id;
  }

  applyFigures(f: PackageFigures) {
    this.completedBytes = num(f.completedBytes);
    this.totalBytes = num(f.totalBytes);
    this.bytesPerSecond = num(f.bytesPerSecond);
    this.speedHistory = nums(f.speedHistory);
    this.etaSeconds = num(f.etaSeconds);
  }
}

export interface PackageOrder {
  id: string;
  itemIds: string[];
}

const next = (seq: number) => (seq + 1) >>> 0;

/** The UI's copy of the download list, kept current by applying patches. */
export class DownloadStore {
  items = new SvelteMap<string, ItemModel>();
  packages = new SvelteMap<string, PackageModel>();
  order = $state.raw<PackageOrder[]>([]);
  stats = $state.raw<EngineStats | null>(null);

  /** Called after every structure change, so owners can drop what no longer exists. */
  onStructure: (() => void) | undefined;

  #expected: number | null = null;
  #resyncRequested = false;
  #resync: () => void;

  constructor(resync: () => void) {
    this.#resync = resync;
  }

  apply(patch: Patch) {
    if (patch.full) {
      this.items.clear();
      this.packages.clear();
      this.order = [];
      this.#resyncRequested = false;
    } else if (this.#expected === null || patch.seq !== this.#expected) {
      // A gap (or no full patch yet): ignore until a full patch arrives.
      if (!this.#resyncRequested) {
        this.#resyncRequested = true;
        this.#resync();
      }
      return;
    }
    this.#expected = next(patch.seq);

    if (patch.structure) this.#applyStructure(patch.structure);
    for (const f of patch.items) this.items.get(f.id)?.applyFigures(f);
    for (const f of patch.packages) this.packages.get(f.id)?.applyFigures(f);
    if (patch.stats) this.stats = patch.stats;
    for (const h of patch.histories) {
      const model = this.items.get(h.id);
      if (model) model.detailHistory = nums(h.speedHistory);
    }
  }

  #applyStructure(structure: Patch["structure"] & object) {
    const liveItems = new Set<string>();
    const livePackages = new Set<string>();
    const order: PackageOrder[] = [];
    for (const pkg of structure.packages) {
      livePackages.add(pkg.id);
      let pm = this.packages.get(pkg.id);
      if (!pm) this.packages.set(pkg.id, (pm = new PackageModel(pkg.id)));
      pm.name = pkg.name;
      pm.priority = pkg.priority;
      pm.folder = pkg.folder;
      const itemIds: string[] = [];
      for (const s of pkg.items) {
        liveItems.add(s.id);
        itemIds.push(s.id);
        let im = this.items.get(s.id);
        if (!im) this.items.set(s.id, (im = new ItemModel(s.id)));
        im.applyStructure(s);
      }
      order.push({ id: pkg.id, itemIds });
    }
    for (const id of [...this.items.keys()])
      if (!liveItems.has(id)) this.items.delete(id);
    for (const id of [...this.packages.keys()])
      if (!livePackages.has(id)) this.packages.delete(id);
    this.order = order;
    this.onStructure?.();
  }

  /** The global download rate averaged over the last ten seconds (a queued item's ETA uses it). */
  get globalRecentSpeed(): number {
    const stats = this.stats;
    if (!stats) return 0;
    return averageRecentSpeed(nums(stats.history), num(stats.bytesPerSecond));
  }

  /** The package holding `itemId`, if any. */
  packageOfItem(itemId: string): PackageModel | undefined {
    const order = this.order.find((pkg) => pkg.itemIds.includes(itemId));
    return order ? this.packages.get(order.id) : undefined;
  }

  /** Every package id and item id currently in the list. */
  liveKeys(): Set<string> {
    return new Set([...this.packages.keys(), ...this.items.keys()]);
  }
}
