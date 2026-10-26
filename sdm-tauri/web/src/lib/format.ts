/** Fraction digits `ByteCountFormatter` shows per unit (binary count style). */
const UNITS: { name: string; digits: number }[] = [
  { name: "KB", digits: 0 },
  { name: "MB", digits: 1 },
  { name: "GB", digits: 2 },
  { name: "TB", digits: 2 },
];

/**
 * `ByteCountFormatter` in binary style, as the SwiftUI app prints sizes:
 * `Zero KB`, `512 bytes`, `4 MB`, `629.4 MB`, `5.59 GB`. Anything that is not
 * a positive finite number reads as `Zero KB`.
 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "Zero KB";
  const whole = Math.trunc(bytes);
  if (whole < 1024) return whole === 1 ? "1 byte" : `${whole} bytes`;
  let unit = 0;
  let value = bytes / 1024;
  while (unit < UNITS.length - 1 && value >= 1024) {
    value /= 1024;
    unit += 1;
  }
  let text = value.toFixed(UNITS[unit].digits);
  // A value that rounds up to 1024 belongs to the next unit.
  if (Number(text) >= 1024 && unit < UNITS.length - 1) {
    unit += 1;
    text = (value / 1024).toFixed(UNITS[unit].digits);
  }
  if (text.includes(".")) text = text.replace(/\.?0+$/, "");
  return `${text} ${UNITS[unit].name}`;
}

/** Bytes per second, as `3.7 MB/s`; `Zero KB/s` when idle. */
export function formatSpeed(bytesPerSecond: number): string {
  return `${formatBytes(bytesPerSecond)}/s`;
}

const plural = (count: number, word: string) =>
  `${count} ${word}${count > 1 ? "s" : ""}`;

/**
 * `formatTimeIntervalForEta` from `SDM/Helpers.swift`: a rough remaining time
 * in words. Empty when there is no estimate (zero, negative or not finite).
 */
export function formatEta(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds <= 0) return "";
  const whole = Math.trunc(seconds);
  const secs = whole % 60;
  const minutes = Math.trunc(seconds / 60) % 60;
  const hours = Math.trunc(seconds / 3600) % 24;
  const days = Math.trunc(seconds / 86400);
  if (days > 0) {
    let text = `About ${plural(days, "Day")}`;
    if (hours > 0) text += ` and ${plural(hours, "Hour")}`;
    return text;
  }
  if (hours > 0) {
    let text = `About ${plural(hours, "Hour")}`;
    if (minutes > 5) text += ` and ${Math.trunc(minutes / 5) * 5} Minutes`;
    return text;
  }
  if (minutes > 0) {
    let text = `About ${plural(minutes, "Minute")}`;
    if (minutes < 10 && secs > 15)
      text += ` and ${Math.trunc(secs / 5) * 5} Seconds`;
    return text;
  }
  if (secs > 0) return secs < 10 ? "A few seconds" : "Less than a minute";
  return "Soon";
}
