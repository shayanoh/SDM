import type {
  EngineStats,
  ItemFigures,
  ItemStateDto,
  Patch,
  StructureItem,
  StructurePackage,
} from "../bindings";

/** Wire-shaped test data, with every field a test is unlikely to care about filled in. */
export function structureItem(
  id: string,
  over: Partial<StructureItem> = {},
): StructureItem {
  return {
    id,
    url: `https://e.com/${id}`,
    filename: `${id}.bin`,
    state: { kind: "running" },
    isEnabled: true,
    isResumable: true,
    isAssembling: false,
    assembly: "none",
    fileMissing: false,
    metadata: null,
    ...over,
  };
}

export function figures(
  id: string,
  over: Partial<ItemFigures> = {},
): ItemFigures {
  return {
    id,
    completed: [],
    completedBytes: 0,
    totalBytes: 100,
    activeSegments: 1,
    configuredSegments: 4,
    bytesPerSecond: 0,
    speedHistory: [],
    retryHoldSeconds: null,
    failedAttemptCount: null,
    remainingAttempts: null,
    lastFailureReason: null,
    checkpointFailure: null,
    ...over,
  };
}

export const stats = (over: Partial<EngineStats> = {}): EngineStats => ({
  bytesPerSecond: 0,
  history: [],
  runningCount: 0,
  queuedCount: 0,
  overallFraction: 0,
  ringCompletedBytes: 0,
  ringTotalBytes: 0,
  waitingItemCount: 0,
  waitingPackageCount: 0,
  ...over,
});

export const patch = (seq: number, over: Partial<Patch> = {}): Patch => ({
  seq,
  full: false,
  structure: null,
  items: [],
  packages: [],
  stats: null,
  histories: [],
  ...over,
});

export type PackageSpec = {
  id: string;
  name?: string;
  items: {
    id: string;
    state?: ItemStateDto;
    over?: Partial<StructureItem>;
    fig?: Partial<ItemFigures>;
  }[];
};

export function structurePackages(specs: PackageSpec[]): StructurePackage[] {
  return specs.map((spec) => ({
    id: spec.id,
    name: spec.name ?? spec.id,
    priority: 2,
    folder: `/dl/${spec.name ?? spec.id}`,
    items: spec.items.map((i) =>
      structureItem(i.id, {
        ...(i.state ? { state: i.state } : {}),
        ...i.over,
      }),
    ),
  }));
}

/** A full patch for `specs`, with figures for every item and package. */
export function fullPatchOf(seq: number, specs: PackageSpec[]): Patch {
  return patch(seq, {
    full: true,
    structure: { packages: structurePackages(specs) },
    items: specs.flatMap((s) => s.items.map((i) => figures(i.id, i.fig))),
    packages: specs.map((s) => ({
      id: s.id,
      completedBytes: 0,
      totalBytes: 100 * s.items.length,
      bytesPerSecond: 0,
      speedHistory: [],
      etaSeconds: 0,
    })),
    stats: stats(),
  });
}
