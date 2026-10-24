import type {
  Patch,
  SettingsDto,
  BinariesStatusDto,
  AppError,
  DeletionInfo,
  DeletionTarget,
  HandoffTarget,
  ThemeInfo,
} from "../bindings";

/** Every call the UI makes into the backend. Two implementations exist. */
export interface Bridge {
  /** Starts the patch stream; the first patch is always full. */
  attach(onPatch: (patch: Patch) => void): Promise<void>;
  fullState(): Promise<void>;
  addUrl(url: string, packageName?: string): Promise<void>;
  startItems(ids: string[]): Promise<void>;
  stopItems(ids: string[]): Promise<void>;
  retryItems(ids: string[]): Promise<void>;
  retryMuxItems(ids: string[]): Promise<void>;
  resetItems(ids: string[]): Promise<void>;
  setItemsEnabled(ids: string[], enabled: boolean): Promise<void>;
  removeItems(ids: string[], deleteFiles: boolean): Promise<void>;
  removePackage(id: string, deleteFiles: boolean): Promise<void>;
  /** `atIndex` is against the package's list before the move; omit to append. */
  moveItems(ids: string[], packageId: string, atIndex?: number): Promise<void>;
  moveItemsToNewPackage(ids: string[], name: string): Promise<void>;
  reorderItems(ids: string[], packageId: string): Promise<void>;
  reorderPackages(ids: string[]): Promise<void>;
  /** The files a "Remove and Delete" would trash, for the confirmation. */
  describeDeletion(target: DeletionTarget): Promise<DeletionInfo>;
  openItem(id: string): Promise<void>;
  revealItem(id: string): Promise<void>;
  revealPackage(id: string): Promise<void>;
  pauseAll(): Promise<void>;
  resumeAll(): Promise<void>;
  setListVisible(visible: boolean): Promise<void>;
  setDetailItems(owner: string, ids: string[]): Promise<void>;
  getSettings(): Promise<SettingsDto>;
  updateSettings(settings: SettingsDto): Promise<void>;
  binariesStatus(): Promise<BinariesStatusDto>;
  /** Looks for a newer yt-dlp now and waits for the check. */
  checkBinariesNow(): Promise<void>;
  /** The bundled themes, for the picker. */
  listThemes(): Promise<ThemeInfo[]>;
  /** Shows the Settings window, or focuses it if it is open. */
  openSettings(): Promise<void>;
  /** Previews a theme in every window; `null` restores the stored one. */
  broadcastTheme(themeId: string | null): Promise<void>;
  /** Shows the main window, on the tab `route` names (`downloads`, `completed`, `linkgrabber`). */
  showMain(route?: string): Promise<void>;
  /** Calls `handler` with the route of each request to switch tabs; returns the way to stop. */
  onNavigate(handler: (route: string) => void): () => void;
  /** Calls `handler` for each theme preview; returns the way to stop. */
  onThemePreview(handler: (themeId: string | null) => void): () => void;
  themeCss(themeId?: string): Promise<string>;
  /** Whether the window has a translucent backdrop (Mica or Acrylic). */
  vibrancyActive(): Promise<boolean>;

  // Link grabber. The view arrives as `Patch.grabber`; these only act.
  grabberIngestText(text: string): Promise<void>;
  grabberRemoveLinks(ids: string[]): Promise<void>;
  grabberRemovePackage(id: string): Promise<void>;
  grabberClear(): Promise<void>;
  grabberMoveLink(id: string, packageId: string): Promise<void>;
  grabberRenamePackage(id: string, name: string): Promise<void>;
  grabberMergePackages(source: string, destination: string): Promise<void>;
  grabberSplitPackage(id: string): Promise<void>;
  grabberRecheck(ids: string[]): Promise<void>;
  grabberRecheckFailed(): Promise<void>;
  grabberCancelChecks(): Promise<void>;
  grabberSetFormat(id: string, optionId: string): Promise<void>;
  grabberCopyUrl(id: string): Promise<string>;
  grabberAddToDownloads(target: HandoffTarget, start: boolean): Promise<void>;
}

/** What a rejected bridge call throws. */
export class BridgeError extends Error {
  constructor(public readonly error: AppError) {
    super(error.message);
  }
}
