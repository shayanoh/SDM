import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vitest/config";

// https://vite.dev/config/
export default defineConfig({
  plugins: [svelte()],
  // Matches `devUrl` in src-tauri/tauri.conf.json.
  server: { port: 1420, strictPort: true },
  test: { environment: "jsdom", include: ["src/**/*.test.ts"] },
});
