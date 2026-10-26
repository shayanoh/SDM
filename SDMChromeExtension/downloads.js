// Pure helpers for deciding which downloads SDM takes over. No chrome.* calls
// in here, so they run under `node --test`.

// A download the user just started has a start time within moments of the
// event. At browser startup Chrome raises `downloads.onCreated` again for
// downloads of earlier sessions (cancelled, interrupted, resumable ones), and
// those must be left alone.
export const FRESH_MS = 30000;

/** The address to download: where a redirect ended up, else the one clicked. */
export function downloadUrl(download) {
  return download.finalUrl || download.url || "";
}

/** True for a plain web address SDM can fetch. */
export function isWebUrl(url) {
  return typeof url === "string" && /^https?:\/\//i.test(url);
}

/**
 * True when `download` was started by the user just now: still in progress,
 * and begun no longer than `FRESH_MS` ago (nor, allowing for a clock that
 * disagrees a little, that far ahead).
 */
export function isFreshDownload(download, now) {
  if (download.state !== undefined && download.state !== "in_progress") {
    return false;
  }
  const started = Date.parse(download.startTime);
  if (Number.isNaN(started)) {
    return false;
  }
  return Math.abs(now - started) <= FRESH_MS;
}

/** True when SDM should take `download` over. */
export function shouldCapture(download, now) {
  return isWebUrl(downloadUrl(download)) && isFreshDownload(download, now);
}
