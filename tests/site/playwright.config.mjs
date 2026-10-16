import { defineConfig, devices } from "@playwright/test";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { target } from "../settings/paths.mjs";

const port = process.env.SITE_TEST_PORT ?? "1431";
if (!/^\d+$/.test(port) || Number(port) < 1 || Number(port) > 65535) {
  throw new Error("SITE_TEST_PORT must be an integer from 1 to 65535.");
}
const baseURL = `http://127.0.0.1:${port}`;

export default defineConfig({
  testDir: ".",
  testMatch: "*.spec.mjs",
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [["list"]],
  outputDir: join(target, "site-browser"),
  use: { baseURL },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
  ],
  webServer: {
    command: "node tests/site/serve.mjs",
    cwd: fileURLToPath(new URL("../../", import.meta.url)),
    env: { SITE_TEST_PORT: port },
    url: baseURL,
    reuseExistingServer: false,
  },
});
