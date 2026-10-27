import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { EXTENSION_VERSION } from "./version.js";

test("version.js matches manifest.json (bump both together)", () => {
  const manifest = JSON.parse(
    readFileSync(new URL("./manifest.json", import.meta.url), "utf8"),
  );
  assert.equal(EXTENSION_VERSION, manifest.version);
});
