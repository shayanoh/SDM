export type Platform = "windows" | "linux" | "mac";

/** The OS the window runs on, from the user agent (the webview reports the host OS). */
export function detectPlatform(
  userAgent: string = globalThis.navigator?.userAgent ?? "",
): Platform {
  if (/Macintosh|Mac OS X/i.test(userAgent)) return "mac";
  if (/Windows/i.test(userAgent)) return "windows";
  return "linux";
}

/** The file manager's name, as the menus spell it. */
export const fileManagerName = (platform: Platform): string =>
  ({ windows: "Explorer", linux: "File Manager", mac: "Finder" })[platform];
