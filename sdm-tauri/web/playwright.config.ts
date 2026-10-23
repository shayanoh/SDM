import { defineConfig } from "@playwright/test";

// The UI runs against the in-memory MockBridge in a plain browser.
export default defineConfig({
  testDir: "e2e",
  use: { baseURL: "http://localhost:1420" },
  webServer: {
    command: "pnpm dev",
    port: 1420,
    reuseExistingServer: true,
  },
});
