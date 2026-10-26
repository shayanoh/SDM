import type { BinariesStatusDto, ChromeExtensionDto } from "../bindings";

export interface ComponentRow {
  name: string;
  /** The version, or what stands in for it (`downloading…`, `not installed`). */
  value: string;
  /** True when `value` is not a version, so it can be shown dimmed. */
  missing: boolean;
}

function versionOrState(
  version: string | null | undefined,
  downloading: boolean,
): Pick<ComponentRow, "value" | "missing"> {
  if (version) return { value: version, missing: false };
  return {
    value: downloading ? "downloading…" : "not installed",
    missing: true,
  };
}

/**
 * The helper programs and the Chrome extension, with the version each is at.
 * A tool still being fetched says so; one that never arrived says `not installed`.
 */
export function componentRows(
  binaries: BinariesStatusDto | null,
  chrome: ChromeExtensionDto | null,
): ComponentRow[] {
  const downloading = binaries?.downloading ?? false;
  const rows: ComponentRow[] = [
    {
      name: "yt-dlp",
      ...versionOrState(binaries?.ytDlpVersion, downloading),
    },
    {
      name: "FFmpeg",
      ...versionOrState(binaries?.ffmpegVersion, downloading),
    },
    {
      name: "QuickJS",
      ...versionOrState(binaries?.qjsVersion, downloading),
    },
  ];
  const extension = chrome?.installedVersion ?? null;
  rows.push({
    name: "Chrome extension",
    value: extension ?? "not connected",
    missing: extension === null,
  });
  return rows;
}
