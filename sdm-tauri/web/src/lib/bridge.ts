import type {
  Patch,
  SettingsDto,
  BinariesStatusDto,
  AppError,
} from "../bindings";

/** Every call the UI makes into the backend. Two implementations exist. */
export interface Bridge {
  /** Starts the patch stream; the first patch is always full. */
  attach(onPatch: (patch: Patch) => void): Promise<void>;
  fullState(): Promise<void>;
  addUrl(url: string, packageName?: string): Promise<void>;
  startItem(id: string): Promise<void>;
  stopItem(id: string): Promise<void>;
  retryItem(id: string): Promise<void>;
  removeItem(id: string, deleteFile: boolean): Promise<void>;
  pauseAll(): Promise<void>;
  resumeAll(): Promise<void>;
  setListVisible(visible: boolean): Promise<void>;
  setDetailItems(owner: string, ids: string[]): Promise<void>;
  getSettings(): Promise<SettingsDto>;
  updateSettings(settings: SettingsDto): Promise<void>;
  binariesStatus(): Promise<BinariesStatusDto>;
  themeCss(themeId?: string): Promise<string>;
}

/** What a rejected bridge call throws. */
export class BridgeError extends Error {
  constructor(public readonly error: AppError) {
    super(error.message);
  }
}
