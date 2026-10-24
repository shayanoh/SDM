import type { SettingsDto } from "../bindings";
import { defaultSettings, type SettingsValues } from "./settingsModel";

/**
 * The Settings window's buffered edits: a copy of the stored settings that
 * nothing writes back until OK. The download folder is deliberately not part
 * of it, so the window can never send one.
 */
export class SettingsDraft {
  values = $state<SettingsValues>(defaultSettings());
  #original: SettingsValues = defaultSettings();

  constructor(stored: SettingsDto) {
    this.reset(stored);
  }

  /** Takes `stored` as both the starting point and the "unchanged" reference. */
  reset(stored: SettingsDto) {
    const { downloadFolder: _folder, ...rest } = stored;
    const next: SettingsValues = {
      ...defaultSettings(),
      ...stripUndefined(rest),
    };
    this.#original = next;
    this.values = { ...next };
  }

  get dirty(): boolean {
    return (Object.keys(this.#original) as (keyof SettingsValues)[]).some(
      (key) => this.values[key] !== this.#original[key],
    );
  }

  set<K extends keyof SettingsValues>(key: K, value: SettingsValues[K]) {
    this.values[key] = value;
  }

  /** What OK sends: every value, and no download folder. */
  toDto(): SettingsDto {
    return { ...this.values };
  }
}

function stripUndefined<T extends object>(value: T): Partial<T> {
  return Object.fromEntries(
    Object.entries(value).filter(([, v]) => v !== undefined),
  ) as Partial<T>;
}
