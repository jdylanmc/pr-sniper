import { defineConfig } from "@playwright/test";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { target } from "./paths.mjs";

const port = process.env.SETTINGS_TEST_PORT ?? "1421";
if (!/^\d+$/.test(port) || Number(port) < 1 || Number(port) > 65535) {
  throw new Error("SETTINGS_TEST_PORT must be an integer from 1 to 65535.");
}
const baseURL = `http://127.0.0.1:${port}`;

export default defineConfig({
  testDir: ".",
  testMatch: "*.spec.mjs",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [["list", { printSteps: true }]],
  outputDir: join(target, "settings-browser"),
  use: {
    baseURL,
    browserName: "chromium",
  },
  webServer: {
    command: `npm exec vite preview -- --host 127.0.0.1 --port ${port} --strictPort`,
    cwd: fileURLToPath(new URL("../../", import.meta.url)),
    url: baseURL,
    reuseExistingServer: false,
  },
});
