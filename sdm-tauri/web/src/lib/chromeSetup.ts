import type { ChromeExtensionDto } from "../bindings";

/** "Cookies synced 3 min ago", for the age of the cookie file. */
export function cookiesAge(seconds: number | null): string {
  if (seconds === null) return "No cookies synced yet";
  if (seconds < 60) return "Cookies synced just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `Cookies synced ${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `Cookies synced ${hours} h ago`;
  return `Cookies synced ${Math.floor(hours / 24)} d ago`;
}

/** The one line that says where the extension stands. */
export function statusLine(dto: ChromeExtensionDto): string {
  if (dto.state === "connected") {
    // Chrome can keep running an older copy of the extension's script (it
    // reports the new version from the manifest but never syncs): the cookie
    // file never appearing is how that shows.
    if (dto.cookiesSyncedSecondsAgo === null) {
      return `Connected (extension ${dto.installedVersion ?? "?"}), but no cookies have arrived. Press Reload on SDM at chrome://extensions; if that does not help, remove the extension and load it again.`;
    }
    return `Connected (extension ${dto.installedVersion ?? "?"}). ${cookiesAge(dto.cookiesSyncedSecondsAgo)}.`;
  }
  if (!dto.chromeInstalled)
    return "Google Chrome was not found on this computer.";
  if (dto.state === "outdated") {
    return "Update available: open chrome://extensions and press Reload on SDM.";
  }
  return "Extension not connected.";
}
