import { defineConfig } from "@playwright/test";
import { fileURLToPath } from "node:url";

export default defineConfig({
  testDir: ".",
  testMatch: "*.spec.mjs",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [["list", { printSteps: true }]],
  outputDir: fileURLToPath(
    new URL("../../src-tauri/target/settings-browser", import.meta.url),
  ),
  use: {
    baseURL: "http://127.0.0.1:1421",
    browserName: "chromium",
  },
  webServer: {
    command:
      "npm exec vite preview -- --host 127.0.0.1 --port 1421 --strictPort",
    cwd: fileURLToPath(new URL("../../", import.meta.url)),
    url: "http://127.0.0.1:1421",
    reuseExistingServer: false,
  },
});
