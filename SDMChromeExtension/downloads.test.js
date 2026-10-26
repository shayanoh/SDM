import assert from "node:assert/strict";
import test from "node:test";

import {
  FRESH_MS,
  downloadUrl,
  isFreshDownload,
  isWebUrl,
  shouldCapture,
} from "./downloads.js";

const NOW = Date.parse("2026-10-27T10:00:00.000Z");
const ago = (ms) => new Date(NOW - ms).toISOString();
const download = (extra = {}) => ({
  url: "https://example.com/a.zip",
  state: "in_progress",
  startTime: ago(500),
  ...extra,
});

test("a download started a moment ago is taken", () => {
  assert.equal(shouldCapture(download(), NOW), true);
});

test("downloads of an earlier session are left alone", () => {
  const table = [
    download({ startTime: ago(FRESH_MS + 1000) }),
    download({ startTime: ago(3 * 24 * 3600 * 1000), state: "interrupted" }),
    download({ startTime: ago(2 * 3600 * 1000), state: "complete" }),
    download({ state: "interrupted" }),
  ];
  for (const item of table) {
    assert.equal(shouldCapture(item, NOW), false, JSON.stringify(item));
  }
});

test("an unreadable or missing start time is not fresh", () => {
  assert.equal(isFreshDownload(download({ startTime: "soon" }), NOW), false);
  assert.equal(isFreshDownload(download({ startTime: undefined }), NOW), false);
});

test("a clock a little ahead is tolerated, a far-off one is not", () => {
  assert.equal(isFreshDownload(download({ startTime: ago(-5000) }), NOW), true);
  assert.equal(
    isFreshDownload(download({ startTime: ago(-FRESH_MS - 1000) }), NOW),
    false,
  );
});

test("only web addresses are taken, and the final address wins", () => {
  assert.equal(isWebUrl("https://a.example/x"), true);
  assert.equal(isWebUrl("HTTP://a.example/x"), true);
  for (const url of [
    "blob:https://a.example/1",
    "data:text/plain,hi",
    "ftp://a/b",
    "",
    undefined,
  ]) {
    assert.equal(isWebUrl(url), false, String(url));
  }
  assert.equal(
    downloadUrl({
      url: "https://a.example/go",
      finalUrl: "https://cdn.example/f.zip",
    }),
    "https://cdn.example/f.zip",
  );
  assert.equal(
    downloadUrl({ url: "https://a.example/go", finalUrl: "" }),
    "https://a.example/go",
  );
  assert.equal(
    shouldCapture(download({ url: "blob:https://x/1" }), NOW),
    false,
  );
});
