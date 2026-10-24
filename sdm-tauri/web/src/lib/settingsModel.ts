import type { AutoClearInterval, SettingsDto } from "../bindings";
import type { Platform } from "./platform";

/** Every setting the Settings window edits (the download folder is not one). */
export type SettingsValues = Required<Omit<SettingsDto, "downloadFolder">>;

/** The SwiftUI app's defaults, which the Rust side also uses. */
export function defaultSettings(): SettingsValues {
  return {
    maxConcurrent: 3,
    segmentsPerItem: 8,
    globalMaxConnections: 32,
    maxConnectionsPerHost: 8,
    minSegmentSizeMb: 10,
    autoStartDownloadsOnLaunch: false,
    clipboardWatchingEnabled: true,
    autoAddAndStartOnGrab: false,
    deepSniffEnabled: true,
    autoClearInterval: "thirtyMinutes",
    ytMaxHeight: 1080,
    allowAv1: true,
    allowVp9: true,
    allowH264: true,
    allowMp4: true,
    allowWebm: true,
    allowOpus: true,
    allowAac: true,
    maxPlaylistVideos: 50,
    cookieSource: "none",
    ytDlpChannel: "stable",
    notifyDownloadFinished: true,
    notifyPackageFinished: true,
    notifyDownloadFailed: true,
    notifyLinksGrabbed: true,
    launchAtLogin: false,
    activationMode: "both",
    themeId: "system",
  };
}

/** The resolutions the Media Sites popup offers, highest first. */
export const RESOLUTION_CHOICES = [1920, 1440, 1080, 720, 540, 480] as const;

/** The highest rung of the ladder at or below `height` (the lowest if none). */
export function snapResolution(height: number): number {
  return (
    RESOLUTION_CHOICES.find((rung) => rung <= height) ??
    RESOLUTION_CHOICES[RESOLUTION_CHOICES.length - 1]
  );
}

/** "1,080p", as the SwiftUI picker shows it. */
export function resolutionLabel(height: number): string {
  return `${height.toLocaleString("en-US")}p`;
}

/** Light and Dark first, then everything else alphabetically by name. */
export function orderThemes<T extends { id: string; name: string }>(
  themes: readonly T[],
): T[] {
  const pick = (id: string) => themes.filter((theme) => theme.id === id);
  const rest = themes
    .filter((theme) => theme.id !== "light" && theme.id !== "dark")
    .sort((a, b) => a.name.localeCompare(b.name));
  return [...pick("light"), ...pick("dark"), ...rest];
}

/** Whether `group[index]` is the only box left checked. */
export function isLastChecked(
  group: readonly boolean[],
  index: number,
): boolean {
  return group[index] === true && group.filter(Boolean).length === 1;
}

export const AUTO_CLEAR_LABELS: Record<AutoClearInterval, string> = {
  fifteenMinutes: "After 15 minutes",
  thirtyMinutes: "After 30 minutes",
  oneHour: "After 1 hour",
  oneDay: "After 1 day",
  never: "Never",
};

export const AUTO_CLEAR_ORDER: AutoClearInterval[] = [
  "fifteenMinutes",
  "thirtyMinutes",
  "oneHour",
  "oneDay",
  "never",
];

/** The browsers cookies can come from, in the SwiftUI `CookieSource` order. */
export function cookieChoices(
  platform: Platform,
): { value: string; label: string }[] {
  const all = ["none", "safari", "chrome", "firefox", "edge", "brave"];
  return all
    .filter((value) => value !== "safari" || platform === "mac")
    .map((value) => ({
      value,
      label:
        value === "none" ? "None" : value[0].toUpperCase() + value.slice(1),
    }));
}
